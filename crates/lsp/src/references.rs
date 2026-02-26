//! `textDocument/references` — find all usages of a symbol.

use std::sync::Arc;

use kyokara_parser::SyntaxKind;
use kyokara_refactor::SymbolKind;
use kyokara_syntax::SyntaxNode;
use text_size::TextSize;
use tower_lsp::lsp_types::{Location, Url};

use crate::db::FileAnalysis;
use crate::position::{self, SymbolAtPosition, text_range_to_lsp_range};

/// Find all references to the symbol at the given offset.
pub fn find_references(
    analysis: &Arc<FileAnalysis>,
    source: &str,
    offset: TextSize,
    uri: &Url,
) -> Vec<Location> {
    let root = analysis.syntax_root();
    let symbol = position::symbol_at_offset(&root, offset);

    let (name, kind) = match &symbol {
        SymbolAtPosition::Function { name, .. } => (name.clone(), Some(SymbolKind::Function)),
        SymbolAtPosition::Type { name, .. } => (name.clone(), Some(SymbolKind::Type)),
        SymbolAtPosition::Capability { name, .. } => (name.clone(), Some(SymbolKind::Capability)),
        SymbolAtPosition::Variant { name, .. } => (name.clone(), Some(SymbolKind::Variant)),
        SymbolAtPosition::Local { name } => (name.clone(), None),
        _ => return Vec::new(),
    };

    if let Some(kind) = kind {
        find_symbol_references(&root, &name, kind, source, uri)
    } else {
        find_local_references(&root, &name, source, uri)
    }
}

/// Find references for module-level symbols using the same logic as the
/// refactor crate's rename.
fn find_symbol_references(
    root: &SyntaxNode,
    name: &str,
    kind: SymbolKind,
    source: &str,
    uri: &Url,
) -> Vec<Location> {
    let mut locations = Vec::new();

    for element in root.descendants_with_tokens() {
        let Some(token) = element.into_token() else {
            continue;
        };
        if token.kind() != SyntaxKind::Ident || token.text() != name {
            continue;
        }

        let Some(parent) = token.parent() else {
            continue;
        };

        if should_include_token(&parent, kind) {
            let range = text_range_to_lsp_range(token.text_range(), source);
            locations.push(Location::new(uri.clone(), range));
        }
    }

    locations
}

/// Determine if an ident token should be included as a reference for the
/// given symbol kind. Same logic as refactor crate's `should_rename_token`.
fn should_include_token(parent: &SyntaxNode, kind: SymbolKind) -> bool {
    let parent_kind = parent.kind();

    // Definition sites.
    match kind {
        SymbolKind::Function if parent_kind == SyntaxKind::FnDef => return true,
        SymbolKind::Type if parent_kind == SyntaxKind::TypeDef => return true,
        SymbolKind::Capability if parent_kind == SyntaxKind::CapDef => return true,
        SymbolKind::Variant if parent_kind == SyntaxKind::Variant => return true,
        _ => {}
    }

    // Usage sites: ident inside a Path node.
    if parent_kind == SyntaxKind::Path {
        let Some(grandparent) = parent.parent() else {
            return false;
        };
        let gp_kind = grandparent.kind();
        return match kind {
            SymbolKind::Function => {
                matches!(gp_kind, SyntaxKind::PathExpr | SyntaxKind::CallExpr)
            }
            SymbolKind::Type => matches!(
                gp_kind,
                SyntaxKind::NameType | SyntaxKind::RecordExpr | SyntaxKind::RecordPat
            ),
            SymbolKind::Capability => {
                if gp_kind == SyntaxKind::NameType {
                    if let Some(ggp) = grandparent.parent() {
                        return matches!(
                            ggp.kind(),
                            SyntaxKind::WithClause | SyntaxKind::PipeClause
                        );
                    }
                }
                false
            }
            SymbolKind::Variant => {
                if matches!(gp_kind, SyntaxKind::ConstructorPat | SyntaxKind::PathExpr) {
                    return true;
                }
                if gp_kind == SyntaxKind::IdentPat {
                    if let Some(ggp) = grandparent.parent() {
                        return ggp.kind() != SyntaxKind::LetBinding;
                    }
                    return true;
                }
                false
            }
        };
    }

    false
}

/// Find references for local variables (all ident tokens matching the name
/// within the same function body).
fn find_local_references(root: &SyntaxNode, name: &str, source: &str, uri: &Url) -> Vec<Location> {
    let mut locations = Vec::new();

    for element in root.descendants_with_tokens() {
        let Some(token) = element.into_token() else {
            continue;
        };
        if token.kind() != SyntaxKind::Ident || token.text() != name {
            continue;
        }
        let range = text_range_to_lsp_range(token.text_range(), source);
        locations.push(Location::new(uri.clone(), range));
    }

    locations
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::FileAnalysis;

    fn test_uri() -> Url {
        Url::parse("file:///test.ky").unwrap()
    }

    #[test]
    fn find_fn_references() {
        let source = "fn foo() -> Int { 42 }\nfn bar() -> Int { foo() }";
        let result = kyokara_hir::check_file(source);
        let analysis = Arc::new(FileAnalysis::from_check_result(result, source.to_string()));
        // Cursor on "foo" definition
        let offset = TextSize::from(3);
        let refs = find_references(&analysis, source, offset, &test_uri());
        // Should find at least 2: definition + usage
        assert!(
            refs.len() >= 2,
            "expected >=2 references, got {}",
            refs.len()
        );
    }
}
