//! `textDocument/completion` — scope-based symbol completion.

use std::sync::Arc;

use kyokara_hir::{TypeDefKind, display_ty_with_tree};
use kyokara_parser::SyntaxKind;
use kyokara_syntax::ast::AstNode;
use kyokara_syntax::ast::nodes::FnDef;
use kyokara_syntax::ast::traits::HasName;
use text_size::TextSize;
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, CompletionResponse};

use crate::db::FileAnalysis;

/// Compute completion candidates at the given offset.
pub fn completions(
    analysis: &Arc<FileAnalysis>,
    source: &str,
    offset: TextSize,
) -> Option<CompletionResponse> {
    let mut items = Vec::new();

    // Module scope: functions, types, caps, constructors.
    add_module_scope_completions(analysis, &mut items);

    // Builtin types.
    add_builtin_completions(&mut items);

    // Local scope: find enclosing function and add locals.
    add_local_completions(analysis, source, offset, &mut items);

    // Hole completion: if at a `_`, suggest matching locals.
    add_hole_completions(analysis, source, offset, &mut items);

    if items.is_empty() {
        None
    } else {
        Some(CompletionResponse::Array(items))
    }
}

fn add_module_scope_completions(analysis: &FileAnalysis, items: &mut Vec<CompletionItem>) {
    let interner = &analysis.interner;
    let scope = &analysis.module_scope;
    let tree = &analysis.item_tree;

    // Functions.
    for (name, idx) in &scope.functions {
        let label = name.resolve(interner).to_string();
        let fn_item = &tree.functions[*idx];
        let detail = {
            let params: Vec<String> = fn_item
                .params
                .iter()
                .map(|p| p.name.resolve(interner).to_string())
                .collect();
            let ret = fn_item.ret_type.as_ref().map(|_| " -> ...").unwrap_or("");
            format!("fn({params}){ret}", params = params.join(", "))
        };
        items.push(CompletionItem {
            label,
            kind: Some(CompletionItemKind::FUNCTION),
            detail: Some(detail),
            ..Default::default()
        });
    }

    // Types.
    for (name, idx) in &scope.types {
        let label = name.resolve(interner).to_string();
        let kind = match &tree.types[*idx].kind {
            TypeDefKind::Adt { .. } => CompletionItemKind::ENUM,
            TypeDefKind::Record { .. } => CompletionItemKind::STRUCT,
            TypeDefKind::Alias(_) => CompletionItemKind::CLASS,
        };
        items.push(CompletionItem {
            label,
            kind: Some(kind),
            ..Default::default()
        });
    }

    // Capabilities.
    for name in scope.caps.keys() {
        items.push(CompletionItem {
            label: name.resolve(interner).to_string(),
            kind: Some(CompletionItemKind::INTERFACE),
            ..Default::default()
        });
    }

    // Constructors.
    for name in scope.constructors.keys() {
        items.push(CompletionItem {
            label: name.resolve(interner).to_string(),
            kind: Some(CompletionItemKind::ENUM_MEMBER),
            ..Default::default()
        });
    }
}

fn add_builtin_completions(items: &mut Vec<CompletionItem>) {
    for name in &[
        "Int", "Float", "String", "Bool", "Char", "Unit", "Option", "Result", "List", "Map",
    ] {
        items.push(CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::CLASS),
            detail: Some("builtin".into()),
            ..Default::default()
        });
    }
}

fn add_local_completions(
    analysis: &FileAnalysis,
    _source: &str,
    offset: TextSize,
    items: &mut Vec<CompletionItem>,
) {
    let root = analysis.syntax_root();
    let interner = &analysis.interner;
    let tree = &analysis.item_tree;

    // Find enclosing FnDef.
    let token = root.token_at_offset(offset).left_biased();
    let Some(token) = token else { return };
    let Some(fn_def) = token.parent_ancestors().find_map(FnDef::cast) else {
        return;
    };
    let Some(fn_name_tok) = fn_def.name_token() else {
        return;
    };
    let fn_name_str = fn_name_tok.text().to_string();

    // Find the matching body + inference.
    for (fn_idx, body) in &analysis.type_check.fn_bodies {
        let fn_item = &tree.functions[*fn_idx];
        if fn_item.name.resolve(interner) != fn_name_str {
            continue;
        }
        let Some(_infer) = analysis.type_check.fn_results.get(fn_idx) else {
            continue;
        };

        // Find the scope at the cursor position.
        // Walk expr_source_map to find the closest expr before the cursor.
        let mut best_scope = None;
        let mut best_dist = u32::MAX;

        for (expr_idx, range) in body.expr_source_map.iter() {
            let start: u32 = range.start().into();
            let off: u32 = offset.into();
            if start <= off {
                let dist = off - start;
                if dist < best_dist {
                    best_dist = dist;
                    if let Some(scope_idx) = body.expr_scopes.get(expr_idx) {
                        best_scope = Some(*scope_idx);
                    }
                }
            }
        }

        // Walk the scope chain to collect locals.
        if let Some(scope_idx) = best_scope {
            let mut current = Some(scope_idx);
            while let Some(idx) = current {
                let scope_data = &body.scopes.scopes[idx];
                for name in scope_data.entries.keys() {
                    items.push(CompletionItem {
                        label: name.resolve(interner).to_string(),
                        kind: Some(CompletionItemKind::VARIABLE),
                        ..Default::default()
                    });
                }
                current = scope_data.parent;
            }
        }

        // Also add function parameters.
        for param in &fn_item.params {
            items.push(CompletionItem {
                label: param.name.resolve(interner).to_string(),
                kind: Some(CompletionItemKind::VARIABLE),
                detail: Some("parameter".into()),
                ..Default::default()
            });
        }

        break;
    }
}

fn add_hole_completions(
    analysis: &FileAnalysis,
    _source: &str,
    offset: TextSize,
    items: &mut Vec<CompletionItem>,
) {
    let interner = &analysis.interner;
    let tree = &analysis.item_tree;

    // Check if cursor is at a hole (`_`).
    let root = analysis.syntax_root();
    let token = root.token_at_offset(offset).left_biased();
    let is_hole =
        token.is_some_and(|t| t.parent().is_some_and(|p| p.kind() == SyntaxKind::HoleExpr));

    if !is_hole {
        return;
    }

    // Find matching HoleInfo from inference results.
    for infer in analysis.type_check.fn_results.values() {
        for hole in &infer.holes {
            let hole_start: u32 = hole.span.range.start().into();
            let hole_end: u32 = hole.span.range.end().into();
            let off: u32 = offset.into();

            if off >= hole_start && off <= hole_end {
                // Add available locals that match the expected type.
                for (local_name, local_ty) in &hole.available_locals {
                    let label = local_name.resolve(interner).to_string();
                    let ty_str = display_ty_with_tree(local_ty, interner, tree);
                    let type_match = hole
                        .expected_type
                        .as_ref()
                        .is_some_and(|expected| expected == local_ty);
                    let sort_text = if type_match {
                        Some("0".to_string()) // Sort exact matches first.
                    } else {
                        Some("1".to_string())
                    };
                    items.push(CompletionItem {
                        label,
                        kind: Some(CompletionItemKind::VARIABLE),
                        detail: Some(ty_str),
                        sort_text,
                        ..Default::default()
                    });
                }
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::db::FileAnalysis;

    #[test]
    fn completion_includes_functions() {
        let source = "fn foo() -> Int { 42 }\nfn bar() -> Int { 0 }";
        let result = kyokara_hir::check_file(source);
        let analysis = Arc::new(FileAnalysis::from_check_result(result, source.to_string()));
        let resp = completions(&analysis, source, TextSize::from(0));
        assert!(resp.is_some());
        let items = match resp.unwrap() {
            CompletionResponse::Array(items) => items,
            _ => panic!("expected array"),
        };
        assert!(items.iter().any(|i| i.label == "foo"));
        assert!(items.iter().any(|i| i.label == "bar"));
    }

    #[test]
    fn completion_includes_builtins() {
        let source = "fn foo() -> Int { 42 }";
        let result = kyokara_hir::check_file(source);
        let analysis = Arc::new(FileAnalysis::from_check_result(result, source.to_string()));
        let resp = completions(&analysis, source, TextSize::from(0));
        let items = match resp.unwrap() {
            CompletionResponse::Array(items) => items,
            _ => panic!("expected array"),
        };
        assert!(items.iter().any(|i| i.label == "Int"));
        assert!(items.iter().any(|i| i.label == "Option"));
    }
}
