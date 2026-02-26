//! Position and offset conversion utilities.
//!
//! Bridges LSP `Position` (line/character) ↔ rowan `TextSize` (byte offset),
//! and provides a symbol classifier for tokens at a given offset.

use kyokara_parser::SyntaxKind;
use kyokara_syntax::SyntaxNode;
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
    None
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
        SyntaxKind::CapDef => {
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
    if parent_kind == SyntaxKind::Path {
        if let Some(grandparent) = parent.parent() {
            let gp_kind = grandparent.kind();
            match gp_kind {
                SyntaxKind::PathExpr | SyntaxKind::CallExpr => {
                    return SymbolAtPosition::Function {
                        name,
                        is_definition: false,
                    };
                }
                SyntaxKind::NameType => {
                    // Check if it's inside a WithClause or PipeClause (capability).
                    if let Some(ggp) = grandparent.parent() {
                        if matches!(ggp.kind(), SyntaxKind::WithClause | SyntaxKind::PipeClause) {
                            return SymbolAtPosition::Capability {
                                name,
                                is_definition: false,
                            };
                        }
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
                    if let Some(ggp) = grandparent.parent() {
                        if ggp.kind() == SyntaxKind::LetBinding {
                            return SymbolAtPosition::Local { name };
                        }
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
    }

    // Fallback: if we're inside a Path > IdentPat, it's probably a local.
    SymbolAtPosition::Local { name }
}

#[cfg(test)]
mod tests {
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
}
