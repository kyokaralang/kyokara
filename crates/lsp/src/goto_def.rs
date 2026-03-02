//! `textDocument/definition` — jump to a symbol's definition site.

use std::sync::Arc;

use kyokara_parser::SyntaxKind;
use kyokara_syntax::SyntaxNode;
use kyokara_syntax::SyntaxToken;
use kyokara_syntax::ast::AstNode;
use kyokara_syntax::ast::nodes::{EffectDef, FnDef, LetBinding, Param, Pat, TypeDef};
use kyokara_syntax::ast::traits::HasName;
use text_size::TextSize;
use tower_lsp::lsp_types::{Location, Url};

use crate::db::FileAnalysis;
use crate::position::{self, SymbolAtPosition, text_range_to_lsp_range};

/// Find the definition location for the symbol at the given offset.
pub fn goto_definition(
    analysis: &Arc<FileAnalysis>,
    source: &str,
    offset: TextSize,
    uri: &Url,
) -> Option<Location> {
    let root = analysis.syntax_root();
    let symbol = position::symbol_at_offset_with_scope(
        &root,
        offset,
        &analysis.module_scope,
        &analysis.interner,
    );

    match symbol {
        SymbolAtPosition::Function { ref name, .. } => find_fn_def(&root, name, source, uri),
        SymbolAtPosition::Type { ref name, .. } => find_type_def(&root, name, source, uri),
        SymbolAtPosition::Capability { ref name, .. } => find_cap_def(&root, name, source, uri),
        SymbolAtPosition::Variant { ref name, .. } => find_variant_def(&root, name, source, uri),
        SymbolAtPosition::Local { ref name } => {
            find_local_def(analysis, &root, name, offset, source, uri)
        }
        _ => None,
    }
}

fn find_fn_def(root: &SyntaxNode, name: &str, source: &str, uri: &Url) -> Option<Location> {
    for node in root.descendants() {
        if let Some(fn_def) = FnDef::cast(node)
            && fn_def.name_token().is_some_and(|t| t.text() == name)
        {
            let range = text_range_to_lsp_range(fn_def.syntax().text_range(), source);
            return Some(Location::new(uri.clone(), range));
        }
    }
    None
}

fn find_type_def(root: &SyntaxNode, name: &str, source: &str, uri: &Url) -> Option<Location> {
    for node in root.descendants() {
        if let Some(type_def) = TypeDef::cast(node)
            && type_def.name_token().is_some_and(|t| t.text() == name)
        {
            let range = text_range_to_lsp_range(type_def.syntax().text_range(), source);
            return Some(Location::new(uri.clone(), range));
        }
    }
    None
}

fn find_cap_def(root: &SyntaxNode, name: &str, source: &str, uri: &Url) -> Option<Location> {
    for node in root.descendants() {
        if let Some(effect_def) = EffectDef::cast(node)
            && effect_def.name_token().is_some_and(|t| t.text() == name)
        {
            let range = text_range_to_lsp_range(effect_def.syntax().text_range(), source);
            return Some(Location::new(uri.clone(), range));
        }
    }
    None
}

fn find_variant_def(root: &SyntaxNode, name: &str, source: &str, uri: &Url) -> Option<Location> {
    for node in root.descendants() {
        if node.kind() == SyntaxKind::Variant {
            // The variant's name is the first Ident token child.
            let ident = node
                .children_with_tokens()
                .filter_map(|e| e.into_token())
                .find(|t| t.kind() == SyntaxKind::Ident);
            if ident.is_some_and(|t| t.text() == name) {
                let range = text_range_to_lsp_range(node.text_range(), source);
                return Some(Location::new(uri.clone(), range));
            }
        }
    }
    None
}

fn find_local_def(
    analysis: &FileAnalysis,
    root: &SyntaxNode,
    name: &str,
    cursor_offset: TextSize,
    source: &str,
    uri: &Url,
) -> Option<Location> {
    let range = find_local_def_range(analysis, root, name, cursor_offset)?;
    Some(Location::new(
        uri.clone(),
        text_range_to_lsp_range(range, source),
    ))
}

pub(crate) fn find_local_def_range(
    analysis: &FileAnalysis,
    root: &SyntaxNode,
    name: &str,
    cursor_offset: TextSize,
) -> Option<kyokara_span::TextRange> {
    let token = token_at_offset_prefer_ident(root, cursor_offset)?;
    let fn_def = token.parent_ancestors().find_map(FnDef::cast)?;
    let fn_name = fn_def.name_token()?.text().to_string();

    // Semantic local resolution first (handles lexical scope and nested shadowing).
    if let Some((_fn_idx, body)) = analysis.type_check.fn_bodies.iter().find(|(idx, _)| {
        analysis.item_tree.functions[**idx]
            .name
            .resolve(&analysis.interner)
            == fn_name
    }) {
        let mut best_expr = None;
        let mut best_span = TextSize::from(u32::MAX);
        for (expr_idx, range) in body.expr_source_map.iter() {
            if range.start() <= cursor_offset && cursor_offset < range.end() {
                let span = range.end() - range.start();
                if span <= best_span {
                    best_span = span;
                    best_expr = Some(expr_idx);
                }
            }
        }

        if let Some(expr_idx) = best_expr
            && let Some(interned_name) = find_interned_name_in_body(body, &analysis.interner, name)
            && let Some(resolved) =
                body.resolve_name_at(&analysis.module_scope, expr_idx, interned_name)
            && let Some((_, meta)) = resolved.local_binding
        {
            return Some(meta.decl_range);
        }
    }

    // Fallback for parameters / syntax-only cases.
    find_local_def_range_syntax(root, name, cursor_offset)
}

fn find_interned_name_in_body(
    body: &kyokara_hir::Body,
    interner: &kyokara_intern::Interner,
    name: &str,
) -> Option<kyokara_hir::Name> {
    for (_, scope_data) in body.scopes.scopes.iter() {
        for scope_name in scope_data.entries.keys() {
            if scope_name.resolve(interner) == name {
                return Some(*scope_name);
            }
        }
    }
    None
}

pub(crate) fn find_local_def_range_syntax(
    root: &SyntaxNode,
    name: &str,
    cursor_offset: TextSize,
) -> Option<kyokara_span::TextRange> {
    // Scope the search to the enclosing FnDef so we don't jump to bindings
    // in other functions.
    let token = token_at_offset_prefer_ident(root, cursor_offset)?;
    let search_root = token
        .parent_ancestors()
        .find_map(FnDef::cast)
        .map(|f| f.syntax().clone())
        .unwrap_or_else(|| root.clone());

    // Walk backwards from cursor to find the nearest LetBinding or Param
    // that introduces this name.
    let mut best: Option<(TextSize, kyokara_span::TextRange)> = None;

    for node in search_root.descendants() {
        let node_start = node.text_range().start();
        if node_start > cursor_offset {
            continue;
        }

        match node.kind() {
            SyntaxKind::LetBinding => {
                if let Some(let_b) = LetBinding::cast(node.clone()) {
                    if !cursor_within_decl_scope(let_b.syntax(), cursor_offset) {
                        continue;
                    }
                    if let Some(pat) = let_b.pat() {
                        for tok in pattern_binding_tokens(&pat) {
                            if tok.text() != name {
                                continue;
                            }
                            let tok_start = tok.text_range().start();
                            if tok_start > cursor_offset {
                                continue;
                            }
                            let range = tok.text_range();
                            match &best {
                                Some((prev_start, _)) if *prev_start < tok_start => {
                                    best = Some((tok_start, range));
                                }
                                None => best = Some((tok_start, range)),
                                _ => {}
                            }
                        }
                    }
                }
            }
            SyntaxKind::Param => {
                if let Some(param) = Param::cast(node.clone())
                    && let Some(name_tok) = param.name_token()
                    && name_tok.text() == name
                {
                    let range = name_tok.text_range();
                    match &best {
                        Some((prev_start, _)) if *prev_start < range.start() => {
                            best = Some((range.start(), range));
                        }
                        None => best = Some((range.start(), range)),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    best.map(|(_, range)| range)
}

fn cursor_within_decl_scope(binding_syntax: &SyntaxNode, cursor_offset: TextSize) -> bool {
    declaration_scope(binding_syntax)
        .is_some_and(|scope| scope.start() <= cursor_offset && cursor_offset < scope.end())
}

fn declaration_scope(node: &SyntaxNode) -> Option<kyokara_span::TextRange> {
    node.ancestors()
        .find(|anc| matches!(anc.kind(), SyntaxKind::BlockExpr | SyntaxKind::MatchArm))
        .map(|anc| anc.text_range())
}

fn token_at_offset_prefer_ident(root: &SyntaxNode, offset: TextSize) -> Option<SyntaxToken> {
    use rowan::TokenAtOffset;

    match root.token_at_offset(offset) {
        TokenAtOffset::Single(tok) => Some(tok),
        TokenAtOffset::Between(left, right) => {
            if left.kind() == SyntaxKind::Ident {
                Some(left)
            } else {
                Some(right)
            }
        }
        TokenAtOffset::None => None,
    }
}

fn pattern_binding_tokens(pat: &Pat) -> Vec<SyntaxToken> {
    let mut out = Vec::new();
    collect_pattern_binding_tokens(pat, &mut out);
    out
}

fn collect_pattern_binding_tokens(pat: &Pat, out: &mut Vec<SyntaxToken>) {
    match pat {
        Pat::Ident(ident) => {
            if let Some(path) = ident.path() {
                // A single-segment ident pattern is a local binding.
                let mut segments = path.segments();
                if let Some(first) = segments.next()
                    && segments.next().is_none()
                {
                    out.push(first);
                }
            } else if let Some(tok) = ident
                .syntax()
                .children_with_tokens()
                .filter_map(|it| it.into_token())
                .find(|t| t.kind() == SyntaxKind::Ident)
            {
                out.push(tok);
            }
        }
        Pat::Constructor(cons) => {
            for arg in cons.args() {
                collect_pattern_binding_tokens(&arg, out);
            }
        }
        Pat::Record(record) => {
            let path_range = record.path().map(|p| p.syntax().text_range());
            for tok in record.field_names() {
                // Exclude path segment(s) from bindings in `Point { x, y }`.
                if let Some(pr) = path_range {
                    let tr = tok.text_range();
                    if pr.start() <= tr.start() && tr.end() <= pr.end() {
                        continue;
                    }
                }
                out.push(tok);
            }
        }
        Pat::Wildcard(_) | Pat::Literal(_) => {}
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::db::FileAnalysis;

    fn test_uri() -> Url {
        Url::parse("file:///test.ky").unwrap()
    }

    #[test]
    fn goto_fn_definition() {
        let source = "fn foo() -> Int { 42 }\nfn bar() -> Int { foo() }";
        let result = kyokara_hir::check_file(source);
        let analysis = Arc::new(FileAnalysis::from_check_result(result, source.to_string()));
        // "foo" in the call at "foo()" on second line
        let offset = TextSize::from(source.find("foo()").unwrap() as u32);
        let loc = goto_definition(&analysis, source, offset, &test_uri());
        assert!(loc.is_some(), "should find fn definition");
        // Should point to the first fn foo
        assert_eq!(loc.unwrap().range.start.line, 0);
    }

    #[test]
    fn goto_type_definition() {
        let source = "type Color = Red | Blue\nfn pick() -> Color { Red }";
        let result = kyokara_hir::check_file(source);
        let analysis = Arc::new(FileAnalysis::from_check_result(result, source.to_string()));
        // "Color" in the return type
        let idx = source.rfind("Color").unwrap();
        let loc = goto_definition(&analysis, source, TextSize::from(idx as u32), &test_uri());
        assert!(loc.is_some(), "should find type definition");
        assert_eq!(loc.unwrap().range.start.line, 0);
    }

    #[test]
    fn goto_local_scoped_to_function() {
        // Both functions have a parameter `x`. Cursor on `x` in `g`'s body
        // should go to `g`'s param (line 1), not `f`'s param (line 0).
        let source = "fn f(x: Int) -> Int { x }\nfn g(x: Int) -> Int { x }";
        let result = kyokara_hir::check_file(source);
        let analysis = Arc::new(FileAnalysis::from_check_result(result, source.to_string()));
        // Find the last `x` (in g's body).
        let body_x = source.rfind('x').unwrap();
        let loc = goto_definition(
            &analysis,
            source,
            TextSize::from(body_x as u32),
            &test_uri(),
        );
        assert!(loc.is_some(), "should find local definition");
        assert_eq!(
            loc.unwrap().range.start.line,
            1,
            "should jump to g's param on line 1, not f's param on line 0"
        );
    }

    #[test]
    fn goto_local_definition_constructor_pattern_binding() {
        let source = "type Pair = Pair(Int, Int)\n\
                      fn main() -> Int {\n\
                        let Pair(x, y) = Pair(1, 2)\n\
                        x + y\n\
                      }";
        let result = kyokara_hir::check_file(source);
        let analysis = Arc::new(FileAnalysis::from_check_result(result, source.to_string()));
        let usage_x = source.rfind("x + y").unwrap();
        let loc = goto_definition(
            &analysis,
            source,
            TextSize::from(usage_x as u32),
            &test_uri(),
        );
        assert!(
            loc.is_some(),
            "should resolve constructor-pattern local binding"
        );
        assert_eq!(
            loc.unwrap().range.start.line,
            2,
            "should jump to `x` binder in let pattern on line 2"
        );
    }

    #[test]
    fn goto_local_definition_respects_lexical_shadowing() {
        let source = "fn main() -> Int {\n\
                        let x = 1;\n\
                        let y = x;\n\
                        {\n\
                          let x = 2;\n\
                          x\n\
                        };\n\
                        x\n\
                      }";
        let result = kyokara_hir::check_file(source);
        let analysis = Arc::new(FileAnalysis::from_check_result(result, source.to_string()));
        let usage_x = source.rfind('x').expect("final x usage");
        let root = analysis.syntax_root();
        let syntax_range = find_local_def_range_syntax(&root, "x", TextSize::from(usage_x as u32));
        assert!(
            syntax_range.is_some(),
            "expected syntax fallback to find outer x"
        );
        let loc = goto_definition(
            &analysis,
            source,
            TextSize::from(usage_x as u32),
            &test_uri(),
        );
        assert!(loc.is_some(), "expected local definition for final x");
        assert_eq!(
            loc.unwrap().range.start.line,
            1,
            "final x should resolve to outer let x on line 1"
        );
    }
}
