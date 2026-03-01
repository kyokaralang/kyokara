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
    // Dot-completion: if cursor is after `module.` or `Type.`, only show members.
    if let Some(dot_items) = try_dot_completion(analysis, offset) {
        return if dot_items.is_empty() {
            None
        } else {
            Some(CompletionResponse::Array(dot_items))
        };
    }

    let mut items = Vec::new();

    // Module scope: functions, types, caps, constructors, synthetic modules.
    add_module_scope_completions(analysis, &mut items);

    // Builtin types.
    add_builtin_completions(&mut items);

    // Local scope: find enclosing function and add locals.
    add_local_completions(analysis, source, offset, &mut items);

    // Hole completion: if at a `_`, suggest matching locals.
    add_hole_completions(analysis, source, offset, &mut items);

    let items = dedup_completion_items(items);

    if items.is_empty() {
        None
    } else {
        Some(CompletionResponse::Array(items))
    }
}

fn dedup_completion_items(items: Vec<CompletionItem>) -> Vec<CompletionItem> {
    let mut deduped = Vec::new();
    let mut by_label = std::collections::HashMap::<String, usize>::new();

    for item in items {
        if let Some(&idx) = by_label.get(&item.label) {
            if should_replace_completion(&deduped[idx], &item) {
                deduped[idx] = item;
            }
            continue;
        }
        by_label.insert(item.label.clone(), deduped.len());
        deduped.push(item);
    }

    deduped
}

fn should_replace_completion(existing: &CompletionItem, new_item: &CompletionItem) -> bool {
    if existing.sort_text.is_none() && new_item.sort_text.is_some() {
        return true;
    }
    if let (Some(existing_sort), Some(new_sort)) = (&existing.sort_text, &new_item.sort_text)
        && new_sort < existing_sort
    {
        return true;
    }
    existing.kind != Some(CompletionItemKind::VARIABLE)
        && new_item.kind == Some(CompletionItemKind::VARIABLE)
}

/// If the cursor is right after `module_name.` or `TypeName.`, return member completions.
fn try_dot_completion(
    analysis: &Arc<FileAnalysis>,
    offset: TextSize,
) -> Option<Vec<CompletionItem>> {
    let root = analysis.syntax_root();
    let interner = &analysis.interner;
    let scope = &analysis.module_scope;
    let tree = &analysis.item_tree;

    // Find the token at or just before the cursor.
    let token = root.token_at_offset(offset).left_biased()?;

    // Walk up to find a FieldExpr parent — the cursor is on the field name or just after the dot.
    let field_expr = token
        .parent_ancestors()
        .find(|n| n.kind() == SyntaxKind::FieldExpr)?;

    // Get the base expression (first child that's an expr node).
    let base_node = field_expr
        .children()
        .find(|n| n.kind() == SyntaxKind::PathExpr)?;

    // The base should be a single-segment path (e.g., `io`, `List`).
    let path_node = base_node
        .children()
        .find(|n| n.kind() == SyntaxKind::Path)?;
    let ident = path_node
        .children_with_tokens()
        .filter_map(|c| c.into_token())
        .find(|t| t.kind() == SyntaxKind::Ident)?;
    let base_name = ident.text().to_string();

    let mut items = Vec::new();

    // Check synthetic modules: io.println, math.min, fs.read_file, etc.
    for (mod_name, mod_fns) in &scope.synthetic_modules {
        if !scope.imported_modules.contains(mod_name) {
            continue;
        }
        if mod_name.resolve(interner) == base_name {
            for (fn_name, fn_idx) in mod_fns {
                let fn_item = &tree.functions[*fn_idx];
                let params: Vec<String> = fn_item
                    .params
                    .iter()
                    .map(|p| p.name.resolve(interner).to_string())
                    .collect();
                let ret = fn_item.ret_type.as_ref().map(|_| " -> ...").unwrap_or("");
                let detail = format!("fn({params}){ret}", params = params.join(", "));
                items.push(CompletionItem {
                    label: fn_name.resolve(interner).to_string(),
                    kind: Some(CompletionItemKind::FUNCTION),
                    detail: Some(detail),
                    ..Default::default()
                });
            }
            return Some(items);
        }
    }

    // Check static methods: List.new, Map.new, etc.
    for ((ty_name, method_name), fn_idx) in &scope.static_methods {
        if ty_name.resolve(interner) == base_name {
            let fn_item = &tree.functions[*fn_idx];
            let params: Vec<String> = fn_item
                .params
                .iter()
                .map(|p| p.name.resolve(interner).to_string())
                .collect();
            let ret = fn_item.ret_type.as_ref().map(|_| " -> ...").unwrap_or("");
            let detail = format!("fn({params}){ret}", params = params.join(", "));
            items.push(CompletionItem {
                label: method_name.resolve(interner).to_string(),
                kind: Some(CompletionItemKind::FUNCTION),
                detail: Some(detail),
                ..Default::default()
            });
        }
    }

    if items.is_empty() {
        None // Not a module or type with static methods — fall through to normal completion.
    } else {
        Some(items)
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

    // Synthetic modules (io, math, fs) — only when explicitly imported.
    for mod_name in &scope.imported_modules {
        if !scope.synthetic_modules.contains_key(mod_name) {
            continue;
        }
        items.push(CompletionItem {
            label: mod_name.resolve(interner).to_string(),
            kind: Some(CompletionItemKind::MODULE),
            detail: Some("module".into()),
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
    use rowan::TokenAtOffset;

    let interner = &analysis.interner;
    let tree = &analysis.item_tree;

    // Check if cursor is at a hole (`_`).
    let root = analysis.syntax_root();
    let token = match root.token_at_offset(offset) {
        TokenAtOffset::Single(tok) => Some(tok),
        TokenAtOffset::Between(left, right) => {
            if left
                .parent()
                .is_some_and(|p| p.kind() == SyntaxKind::HoleExpr)
            {
                Some(left)
            } else if right
                .parent()
                .is_some_and(|p| p.kind() == SyntaxKind::HoleExpr)
            {
                Some(right)
            } else {
                Some(left)
            }
        }
        TokenAtOffset::None => None,
    };
    let is_hole = token
        .as_ref()
        .and_then(|t| t.parent())
        .is_some_and(|p| p.kind() == SyntaxKind::HoleExpr);
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

    fn completion_items(
        analysis: &Arc<FileAnalysis>,
        source: &str,
        offset: TextSize,
    ) -> Vec<CompletionItem> {
        match completions(analysis, source, offset) {
            Some(CompletionResponse::Array(items)) => items,
            Some(other) => panic!("expected array completion response, got: {other:?}"),
            None => Vec::new(),
        }
    }

    #[test]
    fn completion_includes_functions() {
        let source = "fn foo() -> Int { 42 }\nfn bar() -> Int { 0 }";
        let result = kyokara_hir::check_file(source);
        let analysis = Arc::new(FileAnalysis::from_check_result(result, source.to_string()));
        let items = completion_items(&analysis, source, TextSize::from(0));
        assert!(items.iter().any(|i| i.label == "foo"));
        assert!(items.iter().any(|i| i.label == "bar"));
    }

    #[test]
    fn completion_includes_builtins() {
        let source = "fn foo() -> Int { 42 }";
        let result = kyokara_hir::check_file(source);
        let analysis = Arc::new(FileAnalysis::from_check_result(result, source.to_string()));
        let items = completion_items(&analysis, source, TextSize::from(0));
        assert!(items.iter().any(|i| i.label == "Int"));
        assert!(items.iter().any(|i| i.label == "Option"));
    }

    #[test]
    fn completion_includes_locals_in_function_scope() {
        let source = "fn main() -> Int {\n\
                        let value = 1\n\
                        value\n\
                      }";
        let result = kyokara_hir::check_file(source);
        let analysis = Arc::new(FileAnalysis::from_check_result(result, source.to_string()));
        let offset = TextSize::from(source.rfind("value").expect("value usage offset") as u32);
        let items = completion_items(&analysis, source, offset);
        assert!(
            items.iter().any(|i| i.label == "value"),
            "expected local binding in completion items: {items:?}"
        );
    }

    #[test]
    fn completion_shadowed_name_is_not_duplicated() {
        let source = "fn main(x: Int) -> Int {\n\
                        let x = 1\n\
                        x\n\
                      }";
        let result = kyokara_hir::check_file(source);
        let analysis = Arc::new(FileAnalysis::from_check_result(result, source.to_string()));
        let offset = TextSize::from(source.rfind('x').expect("x usage offset") as u32);
        let items = completion_items(&analysis, source, offset);
        let x_count = items.iter().filter(|i| i.label == "x").count();
        assert_eq!(
            x_count, 1,
            "shadowed name should appear once in completion list, got items: {items:?}"
        );
    }

    #[test]
    fn completion_excludes_unimported_synthetic_modules() {
        let source = "fn main() -> Int { 0 }";
        let result = kyokara_hir::check_file(source);
        let analysis = Arc::new(FileAnalysis::from_check_result(result, source.to_string()));
        let items = completion_items(&analysis, source, TextSize::from(0));
        assert!(
            !items.iter().any(|i| i.label == "io"),
            "did not expect 'io' module in completions without import: {items:?}"
        );
        assert!(
            !items.iter().any(|i| i.label == "math"),
            "did not expect 'math' module in completions without import: {items:?}"
        );
        assert!(
            !items.iter().any(|i| i.label == "fs"),
            "did not expect 'fs' module in completions without import: {items:?}"
        );
    }

    #[test]
    fn completion_includes_only_imported_synthetic_modules() {
        let source = "import io\nimport math\nfn main() -> Unit { io.println(\"hi\") }";
        let result = kyokara_hir::check_file(source);
        let analysis = Arc::new(FileAnalysis::from_check_result(result, source.to_string()));
        let items = completion_items(&analysis, source, TextSize::from(0));
        assert!(
            items.iter().any(|i| i.label == "io"),
            "expected imported 'io' module in completions: {items:?}"
        );
        assert!(
            items.iter().any(|i| i.label == "math"),
            "expected imported 'math' module in completions: {items:?}"
        );
        assert!(
            !items.iter().any(|i| i.label == "fs"),
            "did not expect unimported 'fs' module in completions: {items:?}"
        );
    }

    #[test]
    fn completion_dot_after_module_shows_members() {
        let source = "import io\nfn main() -> Unit { io.println(\"hi\") }";
        let result = kyokara_hir::check_file(source);
        let analysis = Arc::new(FileAnalysis::from_check_result(result, source.to_string()));
        // Cursor on "println" — inside FieldExpr with base "io".
        let offset = TextSize::from(source.find("println").expect("println offset") as u32);
        let items = completion_items(&analysis, source, offset);
        assert!(
            items.iter().any(|i| i.label == "println"),
            "expected 'println' in io dot-completion: {items:?}"
        );
        assert!(
            items.iter().any(|i| i.label == "print"),
            "expected 'print' in io dot-completion: {items:?}"
        );
        // Should NOT include top-level items like Int, Option, etc.
        assert!(
            !items.iter().any(|i| i.label == "Int"),
            "dot-completion should not include builtins: {items:?}"
        );
    }

    #[test]
    fn completion_dot_after_unimported_module_hides_members() {
        let source = "fn main() -> Unit { io.println(\"hi\") }";
        let result = kyokara_hir::check_file(source);
        let analysis = Arc::new(FileAnalysis::from_check_result(result, source.to_string()));
        let offset = TextSize::from(source.find("println").expect("println offset") as u32);
        let items = completion_items(&analysis, source, offset);
        assert!(
            !items.iter().any(|i| i.label == "println"),
            "did not expect 'println' in dot completion for unimported module: {items:?}"
        );
        assert!(
            !items.iter().any(|i| i.label == "print"),
            "did not expect 'print' in dot completion for unimported module: {items:?}"
        );
    }

    #[test]
    fn completion_dot_after_type_shows_static_methods() {
        // Use Int return type to avoid parser issues with List[Int].
        let source = "fn main() -> Int {\n  let xs = List.new()\n  xs.len()\n}";
        let result = kyokara_hir::check_file(source);
        let analysis = Arc::new(FileAnalysis::from_check_result(result, source.to_string()));
        // Cursor on "new" — inside FieldExpr with base "List".
        let new_pos = source.find("new").expect("new offset");
        let offset = TextSize::from(new_pos as u32);
        let items = completion_items(&analysis, source, offset);
        assert!(
            items.iter().any(|i| i.label == "new"),
            "expected 'new' in List dot-completion: {items:?}"
        );
        // Should NOT include top-level items.
        assert!(
            !items.iter().any(|i| i.label == "Int"),
            "dot-completion should not include builtins: {items:?}"
        );
    }

    #[test]
    fn completion_no_free_function_intrinsics() {
        // Intrinsics should NOT appear as top-level completions.
        let source = "fn main() -> Int { 0 }";
        let result = kyokara_hir::check_file(source);
        let analysis = Arc::new(FileAnalysis::from_check_result(result, source.to_string()));
        let items = completion_items(&analysis, source, TextSize::from(0));
        assert!(
            !items.iter().any(|i| i.label == "println"),
            "intrinsic 'println' should NOT be a top-level completion: {items:?}"
        );
        assert!(
            !items.iter().any(|i| i.label == "list_len"),
            "intrinsic 'list_len' should NOT be a top-level completion: {items:?}"
        );
        assert!(
            !items.iter().any(|i| i.label == "map_new"),
            "intrinsic 'map_new' should NOT be a top-level completion: {items:?}"
        );
    }

    #[test]
    fn completion_hole_exact_type_is_ranked_first() {
        let source = "fn pick(x: Int, flag: Bool) -> Int { _ }";
        let result = kyokara_hir::check_file(source);
        let analysis = Arc::new(FileAnalysis::from_check_result(result, source.to_string()));
        let hole_offset = TextSize::from(source.find('_').expect("hole offset") as u32);
        let items = completion_items(&analysis, source, hole_offset);

        let x_item = items
            .iter()
            .find(|i| i.label == "x")
            .expect("expected x completion at hole");
        assert_eq!(
            x_item.sort_text.as_deref(),
            Some("0"),
            "exact type match should have highest rank, got: {x_item:?}"
        );

        let flag_item = items
            .iter()
            .find(|i| i.label == "flag")
            .expect("expected flag completion at hole");
        assert_eq!(
            flag_item.sort_text.as_deref(),
            Some("1"),
            "non-matching type should be lower-ranked, got: {flag_item:?}"
        );
    }
}
