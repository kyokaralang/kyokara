//! `textDocument/definition` — jump to a symbol's definition site.

use std::sync::Arc;

use kyokara_parser::SyntaxKind;
use kyokara_syntax::SyntaxNode;
use kyokara_syntax::ast::AstNode;
use kyokara_syntax::ast::nodes::{CapDef, FnDef, LetBinding, Param, TypeDef};
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
    let symbol = position::symbol_at_offset(&root, offset);

    match symbol {
        SymbolAtPosition::Function { ref name, .. } => find_fn_def(&root, name, source, uri),
        SymbolAtPosition::Type { ref name, .. } => find_type_def(&root, name, source, uri),
        SymbolAtPosition::Capability { ref name, .. } => find_cap_def(&root, name, source, uri),
        SymbolAtPosition::Variant { ref name, .. } => find_variant_def(&root, name, source, uri),
        SymbolAtPosition::Local { ref name } => find_local_def(&root, name, offset, source, uri),
        _ => None,
    }
}

fn find_fn_def(root: &SyntaxNode, name: &str, source: &str, uri: &Url) -> Option<Location> {
    for node in root.descendants() {
        if let Some(fn_def) = FnDef::cast(node) {
            if fn_def.name_token().is_some_and(|t| t.text() == name) {
                let range = text_range_to_lsp_range(fn_def.syntax().text_range(), source);
                return Some(Location::new(uri.clone(), range));
            }
        }
    }
    None
}

fn find_type_def(root: &SyntaxNode, name: &str, source: &str, uri: &Url) -> Option<Location> {
    for node in root.descendants() {
        if let Some(type_def) = TypeDef::cast(node) {
            if type_def.name_token().is_some_and(|t| t.text() == name) {
                let range = text_range_to_lsp_range(type_def.syntax().text_range(), source);
                return Some(Location::new(uri.clone(), range));
            }
        }
    }
    None
}

fn find_cap_def(root: &SyntaxNode, name: &str, source: &str, uri: &Url) -> Option<Location> {
    for node in root.descendants() {
        if let Some(cap_def) = CapDef::cast(node) {
            if cap_def.name_token().is_some_and(|t| t.text() == name) {
                let range = text_range_to_lsp_range(cap_def.syntax().text_range(), source);
                return Some(Location::new(uri.clone(), range));
            }
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
    root: &SyntaxNode,
    name: &str,
    cursor_offset: TextSize,
    source: &str,
    uri: &Url,
) -> Option<Location> {
    // Walk backwards from cursor to find the nearest LetBinding or Param
    // that introduces this name.
    let mut best: Option<(TextSize, kyokara_span::TextRange)> = None;

    for node in root.descendants() {
        let node_start = node.text_range().start();
        if node_start > cursor_offset {
            continue;
        }

        match node.kind() {
            SyntaxKind::LetBinding => {
                if let Some(let_b) = LetBinding::cast(node.clone()) {
                    // The pattern child contains the name.
                    if let Some(pat) = let_b.pat() {
                        let pat_text = pat.syntax().text().to_string();
                        if pat_text.trim() == name {
                            let range = let_b.syntax().text_range();
                            match &best {
                                Some((prev_start, _)) if *prev_start < node_start => {
                                    best = Some((node_start, range));
                                }
                                None => best = Some((node_start, range)),
                                _ => {}
                            }
                        }
                    }
                }
            }
            SyntaxKind::Param => {
                if let Some(param) = Param::cast(node.clone()) {
                    if param.name_token().is_some_and(|t| t.text() == name) {
                        let range = param.syntax().text_range();
                        match &best {
                            Some((prev_start, _)) if *prev_start < node_start => {
                                best = Some((node_start, range));
                            }
                            None => best = Some((node_start, range)),
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }

    best.map(|(_, range)| Location::new(uri.clone(), text_range_to_lsp_range(range, source)))
}

#[cfg(test)]
mod tests {
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
}
