//! Position and offset conversion utilities.
//!
//! Bridges LSP `Position` (line/character) ↔ rowan `TextSize` (byte offset),
//! and provides a symbol classifier for tokens at a given offset.

use kyokara_hir::ModuleScope;
use kyokara_intern::Interner;
use kyokara_parser::SyntaxKind;
use kyokara_syntax::SyntaxNode;
use rowan::TokenAtOffset;
use text_size::TextSize;
use tower_lsp::lsp_types::{Position, Range};

/// Convert an LSP `Position` (0-based line/char) to a byte offset.
pub fn lsp_position_to_offset(pos: Position, text: &str) -> Option<TextSize> {
    let mut offset = 0usize;
    for (i, line) in text.split('\n').enumerate() {
        if i == pos.line as usize {
            let target_units = pos.character as usize;
            // Count UTF-16 code units to find the byte offset.
            let mut units = 0usize;
            let mut byte_offset = 0usize;
            for ch in line.chars() {
                if units >= target_units {
                    break;
                }
                units += ch.len_utf16();
                byte_offset += ch.len_utf8();
            }
            return Some(TextSize::from((offset + byte_offset) as u32));
        }
        offset += line.len() + 1; // +1 for the '\n'
    }
    // Position is past the last line — clamp to end of file.
    // LSP clients can send this for cursors at EOF in files without
    // trailing newlines.
    Some(TextSize::from(text.len() as u32))
}

/// Convert a byte offset to an LSP `Position`.
pub fn offset_to_lsp_position(offset: TextSize, text: &str) -> Position {
    let offset: usize = offset.into();
    let offset = offset.min(text.len());
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, ch) in text.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += ch.len_utf16() as u32;
        }
    }
    Position::new(line, col)
}

/// Convert a rowan `TextRange` to an LSP `Range`.
pub fn text_range_to_lsp_range(range: kyokara_span::TextRange, text: &str) -> Range {
    Range::new(
        offset_to_lsp_position(range.start(), text),
        offset_to_lsp_position(range.end(), text),
    )
}

/// What kind of symbol sits at a given cursor position.
#[derive(Debug, Clone)]
pub enum SymbolAtPosition {
    Function { name: String, is_definition: bool },
    Type { name: String, is_definition: bool },
    Capability { name: String, is_definition: bool },
    Variant { name: String, is_definition: bool },
    Local { name: String },
    FieldAccess { field_name: String },
    Import { name: String },
    None,
}

/// Classify the symbol at a byte offset in the CST.
pub fn symbol_at_offset(root: &SyntaxNode, offset: TextSize) -> SymbolAtPosition {
    use rowan::TokenAtOffset;

    let token = match root.token_at_offset(offset) {
        TokenAtOffset::Single(t) => t,
        TokenAtOffset::Between(left, right) => {
            // Prefer the ident token.
            if left.kind() == SyntaxKind::Ident {
                left
            } else {
                right
            }
        }
        TokenAtOffset::None => return SymbolAtPosition::None,
    };

    if token.kind() != SyntaxKind::Ident {
        return SymbolAtPosition::None;
    }

    let name = token.text().to_string();

    let Some(parent) = token.parent() else {
        return SymbolAtPosition::None;
    };

    let parent_kind = parent.kind();

    // Definition sites.
    match parent_kind {
        SyntaxKind::FnDef => {
            return SymbolAtPosition::Function {
                name,
                is_definition: true,
            };
        }
        SyntaxKind::TypeDef => {
            return SymbolAtPosition::Type {
                name,
                is_definition: true,
            };
        }
        SyntaxKind::EffectDef => {
            return SymbolAtPosition::Capability {
                name,
                is_definition: true,
            };
        }
        SyntaxKind::Variant => {
            return SymbolAtPosition::Variant {
                name,
                is_definition: true,
            };
        }
        SyntaxKind::Param | SyntaxKind::LetBinding => {
            return SymbolAtPosition::Local { name };
        }
        SyntaxKind::FieldExpr => {
            return SymbolAtPosition::FieldAccess { field_name: name };
        }
        SyntaxKind::ImportDecl | SyntaxKind::ImportAlias => {
            return SymbolAtPosition::Import { name };
        }
        _ => {}
    }

    // Usage sites: ident inside a Path node.
    if parent_kind == SyntaxKind::Path
        && let Some(grandparent) = parent.parent()
    {
        let gp_kind = grandparent.kind();
        match gp_kind {
            SyntaxKind::PathExpr | SyntaxKind::CallExpr => {
                return SymbolAtPosition::Function {
                    name,
                    is_definition: false,
                };
            }
            SyntaxKind::NameType => {
                // Check if it's inside a WithClause (capability).
                if let Some(ggp) = grandparent.parent()
                    && ggp.kind() == SyntaxKind::WithClause
                {
                    return SymbolAtPosition::Capability {
                        name,
                        is_definition: false,
                    };
                }
                return SymbolAtPosition::Type {
                    name,
                    is_definition: false,
                };
            }
            SyntaxKind::RecordExpr | SyntaxKind::RecordPat => {
                return SymbolAtPosition::Type {
                    name,
                    is_definition: false,
                };
            }
            SyntaxKind::ConstructorPat => {
                return SymbolAtPosition::Variant {
                    name,
                    is_definition: false,
                };
            }
            SyntaxKind::IdentPat => {
                // Could be a zero-arg variant in a match arm, or a local.
                if let Some(ggp) = grandparent.parent()
                    && ggp.kind() == SyntaxKind::LetBinding
                {
                    return SymbolAtPosition::Local { name };
                }
                // In a match arm, treat as variant.
                return SymbolAtPosition::Variant {
                    name,
                    is_definition: false,
                };
            }
            SyntaxKind::ImportDecl => {
                return SymbolAtPosition::Import { name };
            }
            _ => {}
        }
    }

    // Fallback: if we're inside a Path > IdentPat, it's probably a local.
    SymbolAtPosition::Local { name }
}

/// Scope-aware variant of [`symbol_at_offset`].
///
/// For usage sites inside `PathExpr`/`CallExpr`, this consults the module
/// scope to decide whether the name refers to a function, constructor, or
/// local variable. Without this, every ident under `PathExpr` is classified
/// as `Function`, which misdirects hover/goto/references.
pub fn symbol_at_offset_with_scope(
    root: &SyntaxNode,
    offset: TextSize,
    module_scope: &ModuleScope,
    interner: &Interner,
) -> SymbolAtPosition {
    let base = symbol_at_offset(root, offset);

    // Only override the PathExpr/CallExpr case where the base classifier
    // unconditionally returns Function for usage sites.
    if let SymbolAtPosition::Function {
        ref name,
        is_definition: false,
    } = base
    {
        let token = match root.token_at_offset(offset) {
            TokenAtOffset::Single(tok) => Some(tok),
            TokenAtOffset::Between(left, right) => {
                if left.kind() == SyntaxKind::Ident {
                    Some(left)
                } else {
                    Some(right)
                }
            }
            TokenAtOffset::None => None,
        };
        if let Some(tok) = token
            && tok.kind() == SyntaxKind::Ident
            && tok.text() == name
            && crate::goto_def::find_local_def_range_syntax(root, name, offset).is_some()
        {
            return SymbolAtPosition::Local { name: name.clone() };
        }

        // Check if name is actually known at module level.
        let is_fn = module_scope
            .functions
            .keys()
            .any(|n| n.resolve(interner) == name);
        let is_ctor = module_scope
            .constructors
            .keys()
            .any(|n| n.resolve(interner) == name);

        if is_fn {
            return base; // Correctly a function call.
        }
        if is_ctor {
            return SymbolAtPosition::Variant {
                name: name.clone(),
                is_definition: false,
            };
        }
        // Not in module scope — it's a local variable reference.
        return SymbolAtPosition::Local { name: name.clone() };
    }

    base
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn offset_roundtrip() {
        let text = "fn foo() -> Int {\n  42\n}";
        let pos = Position::new(1, 2); // line 1, col 2 → "42"
        let offset = lsp_position_to_offset(pos, text).unwrap();
        let back = offset_to_lsp_position(offset, text);
        assert_eq!(pos, back);
    }

    #[test]
    fn offset_at_start() {
        let text = "hello\nworld";
        let pos = Position::new(0, 0);
        let offset = lsp_position_to_offset(pos, text).unwrap();
        assert_eq!(offset, TextSize::from(0));
    }

    #[test]
    fn offset_second_line() {
        let text = "hello\nworld";
        let pos = Position::new(1, 0);
        let offset = lsp_position_to_offset(pos, text).unwrap();
        assert_eq!(offset, TextSize::from(6));
    }

    #[test]
    fn emoji_position_to_offset() {
        // 😀 is U+1F600: 4 bytes in UTF-8, 2 code units in UTF-16.
        let text = "a😀b\ncd";
        // In UTF-16: a(1) + 😀(2) + b(1) = col 4 for end of first line.
        // Position(0, 3) means after a(1 unit) + 😀(2 units) = 3 units → byte offset 5 (1 + 4).
        let offset = lsp_position_to_offset(Position::new(0, 3), text).unwrap();
        assert_eq!(
            offset,
            TextSize::from(5),
            "col 3 in UTF-16 should be byte 5 (past a + 😀)"
        );
    }

    #[test]
    fn emoji_offset_to_position() {
        let text = "a😀b\ncd";
        // Byte offset 5 = after 'a'(1 byte) + '😀'(4 bytes) → col 3 in UTF-16.
        let pos = offset_to_lsp_position(TextSize::from(5), text);
        assert_eq!(pos, Position::new(0, 3));
    }

    #[test]
    fn emoji_roundtrip() {
        let text = "a😀b\ncd";
        let pos = Position::new(0, 3);
        let offset = lsp_position_to_offset(pos, text).unwrap();
        let back = offset_to_lsp_position(offset, text);
        assert_eq!(pos, back);
    }

    #[test]
    fn offset_at_eof_no_trailing_newline() {
        // File without trailing newline — cursor at end of last line.
        let text = "fn main() -> Int { 42 }";
        // Line 0, col 23 = one past the last character.
        let pos = Position::new(0, 23);
        let offset = lsp_position_to_offset(pos, text);
        assert_eq!(
            offset,
            Some(TextSize::from(23)),
            "cursor at EOF of no-newline file should resolve"
        );
    }

    #[test]
    fn offset_past_last_line_no_trailing_newline() {
        // File "hello\nworld" has lines 0 and 1. Line 2 doesn't exist.
        // LSP clients can send line 2, col 0 when cursor is at the very end.
        let text = "hello\nworld";
        let pos = Position::new(2, 0);
        let offset = lsp_position_to_offset(pos, text);
        // Should clamp to end of file, not return None.
        assert!(
            offset.is_some(),
            "position past last line should clamp to EOF, got None"
        );
    }

    #[test]
    fn offset_past_last_line_with_trailing_newline() {
        // File "hello\nworld\n" — split gives ["hello", "world", ""].
        // Line 2 is the empty string after the trailing newline.
        let text = "hello\nworld\n";
        let pos = Position::new(2, 0);
        let offset = lsp_position_to_offset(pos, text);
        assert_eq!(
            offset,
            Some(TextSize::from(12)),
            "line 2 col 0 in trailing-newline file should be byte 12"
        );
    }

    #[test]
    fn symbol_classifier_fn_def() {
        let source = "fn foo() -> Int { 42 }";
        let parse = kyokara_syntax::parse(source);
        let root = SyntaxNode::new_root(parse.green);
        let sym = symbol_at_offset(&root, TextSize::from(3)); // "foo"
        assert!(matches!(
            sym,
            SymbolAtPosition::Function {
                is_definition: true,
                ..
            }
        ));
    }

    #[test]
    fn symbol_classifier_type_def() {
        let source = "type Color = Red | Blue";
        let parse = kyokara_syntax::parse(source);
        let root = SyntaxNode::new_root(parse.green);
        let sym = symbol_at_offset(&root, TextSize::from(5)); // "Color"
        assert!(matches!(
            sym,
            SymbolAtPosition::Type {
                is_definition: true,
                ..
            }
        ));
    }

    #[test]
    fn symbol_classifier_local_in_path_expr() {
        // `x` in `x + 1` should be classified as Local, not Function.
        let source = "fn f(x: Int) -> Int { x + 1 }";
        let result = kyokara_hir::check_file(source);
        let root = SyntaxNode::new_root(result.green.clone());
        // Find the `x` in the body (after the `{ `).
        let body_x = source.rfind('x').unwrap();
        let sym = symbol_at_offset_with_scope(
            &root,
            TextSize::from(body_x as u32),
            &result.module_scope,
            &result.interner,
        );
        assert!(
            matches!(sym, SymbolAtPosition::Local { .. }),
            "expected Local, got {sym:?}"
        );
    }

    #[test]
    fn symbol_classifier_fn_call_still_function() {
        // `f` in `f()` should still be classified as Function.
        let source = "fn f() -> Int { 1 }\nfn g() -> Int { f() }";
        let result = kyokara_hir::check_file(source);
        let root = SyntaxNode::new_root(result.green.clone());
        // Find the `f` in `f()` on the second line.
        let call_f = source.rfind("f()").unwrap();
        let sym = symbol_at_offset_with_scope(
            &root,
            TextSize::from(call_f as u32),
            &result.module_scope,
            &result.interner,
        );
        assert!(
            matches!(
                sym,
                SymbolAtPosition::Function {
                    is_definition: false,
                    ..
                }
            ),
            "expected Function usage, got {sym:?}"
        );
    }

    #[test]
    fn symbol_classifier_shadowed_function_usage_is_local() {
        let source = "fn foo() -> Int { 1 }\n\
                      fn main() -> Int {\n\
                        let foo = fn() => 2\n\
                        foo()\n\
                      }";
        let result = kyokara_hir::check_file(source);
        let root = SyntaxNode::new_root(result.green.clone());
        let post_shadow = source.rfind("foo()").expect("shadowed call");
        let sym = symbol_at_offset_with_scope(
            &root,
            TextSize::from(post_shadow as u32),
            &result.module_scope,
            &result.interner,
        );
        assert!(
            matches!(sym, SymbolAtPosition::Local { ref name } if name == "foo"),
            "shadowed call should classify as local, got: {sym:?}"
        );
    }

    #[test]
    fn symbol_classifier_pre_shadow_function_usage_stays_function() {
        let source = "fn foo() -> Int { 1 }\n\
                      fn main() -> Int {\n\
                        foo()\n\
                        let foo = fn() => 2\n\
                        foo()\n\
                      }";
        let result = kyokara_hir::check_file(source);
        let root = SyntaxNode::new_root(result.green.clone());
        let pre_shadow = source.find("foo()").expect("pre-shadow call");
        let sym = symbol_at_offset_with_scope(
            &root,
            TextSize::from(pre_shadow as u32),
            &result.module_scope,
            &result.interner,
        );
        assert!(
            matches!(sym, SymbolAtPosition::Function { ref name, .. } if name == "foo"),
            "pre-shadow call should classify as function, got: {sym:?}"
        );
    }

    #[test]
    fn symbol_classifier_block_shadow_does_not_leak() {
        let source = "fn foo() -> Int { 1 }\n\
                      fn main() -> Int {\n\
                        {\n\
                          let foo = fn() => 2;\n\
                          foo()\n\
                        };\n\
                        foo()\n\
                      }";
        let result = kyokara_hir::check_file(source);
        let root = SyntaxNode::new_root(result.green.clone());
        let post_block = source.rfind("foo()").expect("post-block call");
        let sym = symbol_at_offset_with_scope(
            &root,
            TextSize::from(post_block as u32),
            &result.module_scope,
            &result.interner,
        );
        assert!(
            matches!(sym, SymbolAtPosition::Function { ref name, .. } if name == "foo"),
            "post-block call should classify as function, got: {sym:?}"
        );
    }
}
