//! `textDocument/hover` — show type info for functions, types, and expressions.

use std::sync::Arc;

use kyokara_hir::{FnItem, ItemTree, TypeCheckResult, TypeDefKind, TypeItem, display_ty_with_tree};
use kyokara_intern::Interner;
use kyokara_syntax::SyntaxNode;
use kyokara_syntax::ast::AstNode;
use kyokara_syntax::ast::nodes::FnDef;
use kyokara_syntax::ast::traits::HasName;
use text_size::TextSize;
use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind};

use crate::db::FileAnalysis;
use crate::position::{self, SymbolAtPosition};

/// Compute hover information at the given byte offset.
pub fn hover(analysis: &Arc<FileAnalysis>, _source: &str, offset: TextSize) -> Option<Hover> {
    let root = analysis.syntax_root();
    let symbol = position::symbol_at_offset_with_scope(
        &root,
        offset,
        &analysis.module_scope,
        &analysis.interner,
    );

    let contents = match symbol {
        SymbolAtPosition::Function { ref name, .. } => {
            hover_function(name, &analysis.item_tree, &analysis.interner)
        }
        SymbolAtPosition::Type { ref name, .. } => {
            hover_type(name, &analysis.item_tree, &analysis.interner)
        }
        SymbolAtPosition::Capability { ref name, .. } => Some(format!("cap {name}")),
        SymbolAtPosition::Variant { ref name, .. } => {
            hover_variant(name, &analysis.item_tree, &analysis.interner)
        }
        SymbolAtPosition::Local { ref name } => hover_local(
            &root,
            offset,
            name,
            &analysis.type_check,
            &analysis.interner,
            &analysis.item_tree,
        ),
        SymbolAtPosition::FieldAccess { ref field_name } => Some(format!("field `{field_name}`")),
        SymbolAtPosition::Import { ref name } => Some(format!("import {name}")),
        SymbolAtPosition::None => None,
    }?;

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: format!("```kyokara\n{contents}\n```"),
        }),
        range: None,
    })
}

fn hover_function(name: &str, tree: &ItemTree, interner: &Interner) -> Option<String> {
    for (_, item) in tree.functions.iter() {
        if item.name.resolve(interner) == name {
            return Some(render_fn_signature(item, interner, tree));
        }
    }
    None
}

fn render_fn_signature(item: &FnItem, interner: &Interner, tree: &ItemTree) -> String {
    let name = item.name.resolve(interner);
    let params: Vec<String> = item
        .params
        .iter()
        .map(|p| {
            let pname = p.name.resolve(interner);
            let pty = display_ty_ref(&p.ty, interner, tree);
            format!("{pname}: {pty}")
        })
        .collect();
    let params_str = params.join(", ");
    let ret = item
        .ret_type
        .as_ref()
        .map(|t| format!(" -> {}", display_ty_ref(t, interner, tree)))
        .unwrap_or_default();
    let caps = if item.with_caps.is_empty() {
        String::new()
    } else {
        let caps: Vec<String> = item
            .with_caps
            .iter()
            .map(|c| display_ty_ref(c, interner, tree))
            .collect();
        format!(" with {}", caps.join(", "))
    };
    format!("fn {name}({params_str}){ret}{caps}")
}

fn display_ty_ref(ty_ref: &kyokara_hir::TypeRef, interner: &Interner, tree: &ItemTree) -> String {
    use kyokara_hir::TypeRef;
    match ty_ref {
        TypeRef::Path { path, args } => {
            let base = path.last().map(|n| n.resolve(interner)).unwrap_or("?");
            if args.is_empty() {
                base.to_string()
            } else {
                let args_str: Vec<String> = args
                    .iter()
                    .map(|a| display_ty_ref(a, interner, tree))
                    .collect();
                format!("{base}<{}>", args_str.join(", "))
            }
        }
        TypeRef::Fn { params, ret } => {
            let params_str: Vec<String> = params
                .iter()
                .map(|p| display_ty_ref(p, interner, tree))
                .collect();
            let ret_str = display_ty_ref(ret, interner, tree);
            format!("({}) -> {ret_str}", params_str.join(", "))
        }
        TypeRef::Record { fields } => {
            let fields_str: Vec<String> = fields
                .iter()
                .map(|(n, t): &(kyokara_hir::Name, TypeRef)| {
                    format!(
                        "{}: {}",
                        n.resolve(interner),
                        display_ty_ref(t, interner, tree)
                    )
                })
                .collect();
            format!("{{ {} }}", fields_str.join(", "))
        }
        TypeRef::Refined { name, base, .. } => {
            let n = name.resolve(interner);
            let b = display_ty_ref(base, interner, tree);
            format!("{{{n}: {b} | ...}}")
        }
        TypeRef::Error => "?".to_string(),
    }
}

fn hover_type(name: &str, tree: &ItemTree, interner: &Interner) -> Option<String> {
    for (_, item) in tree.types.iter() {
        if item.name.resolve(interner) == name {
            return Some(render_type_signature(item, interner, tree));
        }
    }
    None
}

fn render_type_signature(item: &TypeItem, interner: &Interner, _tree: &ItemTree) -> String {
    let name = item.name.resolve(interner);
    let type_params = if item.type_params.is_empty() {
        String::new()
    } else {
        let ps: Vec<&str> = item
            .type_params
            .iter()
            .map(|p| p.resolve(interner))
            .collect();
        format!("<{}>", ps.join(", "))
    };
    match &item.kind {
        TypeDefKind::Alias(ty_ref) => {
            format!(
                "type {name}{type_params} = {}",
                display_ty_ref(ty_ref, interner, _tree)
            )
        }
        TypeDefKind::Record { fields } => {
            let fields_str: Vec<String> = fields
                .iter()
                .map(|(n, t)| {
                    format!(
                        "  {}: {}",
                        n.resolve(interner),
                        display_ty_ref(t, interner, _tree)
                    )
                })
                .collect();
            format!(
                "type {name}{type_params} {{\n{}\n}}",
                fields_str.join(",\n")
            )
        }
        TypeDefKind::Adt { variants } => {
            let vs: Vec<String> = variants
                .iter()
                .map(|v| {
                    let vname = v.name.resolve(interner);
                    if v.fields.is_empty() {
                        vname.to_string()
                    } else {
                        let fs: Vec<String> = v
                            .fields
                            .iter()
                            .map(|f| display_ty_ref(f, interner, _tree))
                            .collect();
                        format!("{vname}({})", fs.join(", "))
                    }
                })
                .collect();
            format!("type {name}{type_params} = {}", vs.join(" | "))
        }
    }
}

fn hover_variant(name: &str, tree: &ItemTree, interner: &Interner) -> Option<String> {
    for (_, item) in tree.types.iter() {
        if let TypeDefKind::Adt { variants } = &item.kind {
            for v in variants {
                if v.name.resolve(interner) == name {
                    let type_name = item.name.resolve(interner);
                    if v.fields.is_empty() {
                        return Some(format!("{type_name}::{name}"));
                    }
                    let fs: Vec<String> = v
                        .fields
                        .iter()
                        .map(|f| display_ty_ref(f, interner, tree))
                        .collect();
                    return Some(format!("{type_name}::{name}({})", fs.join(", ")));
                }
            }
        }
    }
    None
}

/// Try to find the type of a local variable/expression at the cursor.
fn hover_local(
    root: &SyntaxNode,
    offset: TextSize,
    name: &str,
    type_check: &TypeCheckResult,
    interner: &Interner,
    item_tree: &ItemTree,
) -> Option<String> {
    // Find the enclosing FnDef, then look up expression types.
    let token = root.token_at_offset(offset).left_biased()?;
    let fn_def = token.parent_ancestors().find_map(FnDef::cast)?;
    let fn_name_str = fn_def.name_token()?.text().to_string();

    // Find the function's inference result.
    for (fn_idx, body) in &type_check.fn_bodies {
        let fn_item = &item_tree.functions[*fn_idx];
        if fn_item.name.resolve(interner) != fn_name_str {
            continue;
        }
        let Some(infer) = type_check.fn_results.get(fn_idx) else {
            continue;
        };

        // Look up the expression at the token's range.
        let token_range = token.text_range();
        for (expr_idx, range) in body.expr_source_map.iter() {
            if *range == token_range {
                if let Some(ty) = infer.expr_types.get(expr_idx) {
                    let ty_str = display_ty_with_tree(ty, interner, item_tree);
                    return Some(format!("{name}: {ty_str}"));
                }
            }
        }

        // Check parameter types.
        for param in fn_item.params.iter() {
            if param.name.resolve(interner) == name {
                let ty_str = display_ty_ref(&param.ty, interner, item_tree);
                return Some(format!("{name}: {ty_str}"));
            }
        }
    }

    Some(format!("{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::FileAnalysis;

    #[test]
    fn hover_on_function_name() {
        let source = "fn add(x: Int, y: Int) -> Int { x }";
        let result = kyokara_hir::check_file(source);
        let analysis = Arc::new(FileAnalysis::from_check_result(result, source.to_string()));
        let h = hover(&analysis, source, TextSize::from(3));
        assert!(h.is_some());
        let contents = match h.unwrap().contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };
        assert!(contents.contains("fn add"), "got: {contents}");
    }

    #[test]
    fn hover_on_type() {
        let source = "type Color = Red | Blue";
        let result = kyokara_hir::check_file(source);
        let analysis = Arc::new(FileAnalysis::from_check_result(result, source.to_string()));
        let h = hover(&analysis, source, TextSize::from(5));
        assert!(h.is_some());
        let contents = match h.unwrap().contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markup"),
        };
        assert!(contents.contains("type Color"), "got: {contents}");
    }
}
