//! `kyokara-hir-ty` — Type inference and checking.
//!
//! Contains:
//! - **Type inference** (bidirectional, Hindley-Milner with unification)
//! - **Unification** (Robinson's algorithm with occurs check)
//! - **Exhaustiveness checking** for pattern matches
//! - **Effect checking** (algebraic effects / capabilities)
//! - **Typed holes** (record expected type + available locals)
//!
//! Operates on the HIR data types from `kyokara-hir-def`.

pub mod diagnostics;
pub mod effects;
pub mod exhaustiveness;
pub mod holes;
pub mod infer;
pub mod resolve;
pub mod ty;
pub mod unify;

use kyokara_diagnostics::Diagnostic;
use kyokara_hir_def::body::Body;
use kyokara_hir_def::body::lower::{
    lower_body, lower_impl_method_body, lower_property_body, lower_top_level_let_body,
};
use kyokara_hir_def::item_tree::{FnItemIdx, ItemTree, LetItemIdx};
use kyokara_hir_def::resolver::ModuleScope;
use kyokara_hir_def::type_ref::TypeRef;
use kyokara_intern::Interner;
use kyokara_span::{FileId, Span, TextRange};
use kyokara_stdx::FxHashMap;
use kyokara_syntax::SyntaxNode;
use kyokara_syntax::ast::AstNode;
use kyokara_syntax::ast::nodes::{FnDef, ImplMethodDef, LetBinding, PropertyDef};
use kyokara_hir_def::name::Name;

use crate::diagnostics::TyDiagnosticData;
use crate::infer::InferenceResult;
use crate::ty::Ty;

struct BodyLookupIndex {
    fn_by_range: FxHashMap<TextRange, FnDef>,
    impl_by_range: FxHashMap<TextRange, ImplMethodDef>,
    prop_by_range: FxHashMap<TextRange, PropertyDef>,
    let_by_range: FxHashMap<TextRange, LetBinding>,
}

fn build_body_lookup_index(
    fn_defs: &[FnDef],
    impl_defs: &[ImplMethodDef],
    prop_defs: &[PropertyDef],
    let_defs: &[LetBinding],
) -> BodyLookupIndex {
    let mut fn_by_range = FxHashMap::default();
    let mut impl_by_range = FxHashMap::default();
    let mut prop_by_range = FxHashMap::default();
    let mut let_by_range = FxHashMap::default();

    for fd in fn_defs {
        let range = fd.syntax().text_range();
        fn_by_range.insert(range, fd.clone());
    }

    for fd in impl_defs {
        let range = fd.syntax().text_range();
        impl_by_range.insert(range, fd.clone());
    }

    for pd in prop_defs {
        let range = pd.syntax().text_range();
        prop_by_range.insert(range, pd.clone());
    }

    for let_def in let_defs {
        let range = let_def.syntax().text_range();
        let_by_range.insert(range, let_def.clone());
    }

    BodyLookupIndex {
        fn_by_range,
        impl_by_range,
        prop_by_range,
        let_by_range,
    }
}

fn lookup_fn_def(index: &BodyLookupIndex, source_range: Option<TextRange>) -> Option<&FnDef> {
    source_range.and_then(|range| index.fn_by_range.get(&range))
}

fn lookup_property_def(
    index: &BodyLookupIndex,
    source_range: Option<TextRange>,
) -> Option<&PropertyDef> {
    source_range.and_then(|range| index.prop_by_range.get(&range))
}

fn lookup_impl_method_def(
    index: &BodyLookupIndex,
    source_range: Option<TextRange>,
) -> Option<&ImplMethodDef> {
    source_range.and_then(|range| index.impl_by_range.get(&range))
}

fn lookup_let_binding(
    index: &BodyLookupIndex,
    source_range: Option<TextRange>,
) -> Option<&LetBinding> {
    source_range.and_then(|range| index.let_by_range.get(&range))
}

/// Result of type-checking an entire module.
#[derive(Debug)]
pub struct TypeCheckResult {
    /// Per-function inference results, keyed by function item index.
    pub fn_results: FxHashMap<FnItemIdx, InferenceResult>,
    /// Lowered bodies for each function, keyed by function item index.
    pub fn_bodies: FxHashMap<FnItemIdx, Body>,
    /// Per-top-level-let inference results, keyed by let item index.
    pub let_results: FxHashMap<LetItemIdx, InferenceResult>,
    /// Lowered bodies for each top-level let initializer.
    pub let_bodies: FxHashMap<LetItemIdx, Body>,
    /// All diagnostics from type checking.
    pub diagnostics: Vec<Diagnostic>,
    /// Raw diagnostic data with spans for structured output.
    pub raw_diagnostics: Vec<(TyDiagnosticData, Span)>,
    /// Diagnostics from body lowering (e.g. unresolved names, duplicates).
    pub body_lowering_diagnostics: Vec<Diagnostic>,
    /// Per-function call edges: (caller name, list of callee names).
    pub fn_calls: Vec<(Name, Vec<Name>)>,
}

fn module_scope_with_visible_lets(
    module_scope: &ModuleScope,
    item_tree: &ItemTree,
    visible_until: LetItemIdx,
) -> ModuleScope {
    let mut scoped = module_scope.clone();
    scoped.lets.clear();
    for (let_idx, let_item) in item_tree.lets.iter() {
        if let_idx == visible_until {
            break;
        }
        scoped.lets.insert(let_item.name, let_idx);
    }
    scoped
}

fn visible_let_types(
    inferred: &FxHashMap<LetItemIdx, Ty>,
    item_tree: &ItemTree,
    visible_until: LetItemIdx,
) -> FxHashMap<LetItemIdx, Ty> {
    let mut visible = FxHashMap::default();
    for (let_idx, _) in item_tree.lets.iter() {
        if let_idx == visible_until {
            break;
        }
        if let Some(ty) = inferred.get(&let_idx) {
            visible.insert(let_idx, ty.clone());
        }
    }
    visible
}

fn stabilize_top_level_let_ty(ty: &Ty) -> Ty {
    match ty {
        Ty::Var(_) => Ty::Error,
        Ty::Adt { def, args } => Ty::Adt {
            def: *def,
            args: args.iter().map(stabilize_top_level_let_ty).collect(),
        },
        Ty::Record { fields } => Ty::Record {
            fields: fields
                .iter()
                .map(|(name, field_ty)| (*name, stabilize_top_level_let_ty(field_ty)))
                .collect(),
        },
        Ty::Fn { params, ret } => Ty::Fn {
            params: params.iter().map(stabilize_top_level_let_ty).collect(),
            ret: Box::new(stabilize_top_level_let_ty(ret)),
        },
        _ => ty.clone(),
    }
}

/// Type-check all functions in a module.
///
/// Takes an already-collected item tree and module scope, plus the CST root
/// for body lowering.
pub fn check_module(
    root: &SyntaxNode,
    item_tree: &ItemTree,
    module_scope: &ModuleScope,
    parse_error_ranges: &[TextRange],
    file_id: FileId,
    interner: &mut Interner,
) -> TypeCheckResult {
    let mut fn_results = FxHashMap::default();
    let mut fn_bodies = FxHashMap::default();
    let mut let_results = FxHashMap::default();
    let mut let_bodies = FxHashMap::default();
    let mut let_types = FxHashMap::default();
    let mut all_diagnostics = Vec::new();
    let mut all_raw_diagnostics = Vec::new();
    let mut body_lowering_diagnostics = Vec::new();
    let mut fn_calls = Vec::new();

    let fn_defs: Vec<FnDef> = root.descendants().filter_map(FnDef::cast).collect();
    let impl_method_defs: Vec<ImplMethodDef> =
        root.descendants().filter_map(ImplMethodDef::cast).collect();
    let prop_defs: Vec<PropertyDef> = root.descendants().filter_map(PropertyDef::cast).collect();
    let let_defs: Vec<LetBinding> = root.descendants().filter_map(LetBinding::cast).collect();
    let body_lookup = build_body_lookup_index(&fn_defs, &impl_method_defs, &prop_defs, &let_defs);

    for (let_idx, let_item) in item_tree.lets.iter() {
        if let_item
            .source_range
            .is_some_and(|range| overlaps_parse_error(range, parse_error_ranges))
        {
            continue;
        }

        let Some(let_binding) = lookup_let_binding(&body_lookup, let_item.source_range) else {
            continue;
        };

        let scoped_module = module_scope_with_visible_lets(module_scope, item_tree, let_idx);
        let body_result = lower_top_level_let_body(let_binding, &scoped_module, file_id, interner);
        let let_span = Span {
            file: file_id,
            range: let_item
                .source_range
                .unwrap_or(kyokara_span::TextRange::default()),
        };

        body_lowering_diagnostics.extend(body_result.diagnostics.iter().cloned());
        all_diagnostics.extend(body_result.diagnostics);

        let visible_types = visible_let_types(&let_types, item_tree, let_idx);
        let result = infer::infer_top_level_let(
            let_item,
            &body_result.body,
            item_tree,
            &scoped_module,
            &visible_types,
            interner,
            let_span,
        );

        all_diagnostics.extend(result.diagnostics.iter().cloned());
        all_raw_diagnostics.extend(result.raw_diagnostics.iter().cloned());
        let_types.insert(let_idx, stabilize_top_level_let_ty(&result.ret_ty));
        let_results.insert(let_idx, result);
        let_bodies.insert(let_idx, body_result.body);
    }

    for (fn_idx, fn_item) in item_tree.functions.iter() {
        if !fn_item.has_body {
            continue;
        }
        if fn_item
            .source_range
            .is_some_and(|range| overlaps_parse_error(range, parse_error_ranges))
        {
            continue;
        }

        // Match only by source range. Imported clones intentionally have no
        // body in this module; runtime wires their actual bodies separately.
        let fn_def = lookup_fn_def(&body_lookup, fn_item.source_range);

        // Try FnDef first; if not found, try PropertyDef (synthetic FnItems
        // created for properties point at PropertyDef source ranges).
        let body_result = if let Some(fd) = fn_def {
            lower_body(fd, module_scope, file_id, interner)
        } else if let Some(pd) = lookup_property_def(&body_lookup, fn_item.source_range) {
            lower_property_body(pd, module_scope, file_id, interner)
        } else if let Some(imd) = lookup_impl_method_def(&body_lookup, fn_item.source_range) {
            lower_impl_method_body(imd, module_scope, file_id, interner)
        } else {
            continue;
        };

        let fn_span = Span {
            file: file_id,
            range: fn_item
                .source_range
                .unwrap_or(kyokara_span::TextRange::default()),
        };

        body_lowering_diagnostics.extend(body_result.diagnostics.iter().cloned());
        all_diagnostics.extend(body_result.diagnostics);

        let result = infer::infer_body(
            fn_idx,
            fn_item,
            &body_result.body,
            item_tree,
            module_scope,
            &let_types,
            interner,
            fn_span,
        );

        all_diagnostics.extend(result.diagnostics.iter().cloned());
        all_raw_diagnostics.extend(result.raw_diagnostics.iter().cloned());
        fn_calls.push((fn_item.name, result.calls.clone()));
        fn_results.insert(fn_idx, result);
        fn_bodies.insert(fn_idx, body_result.body);
    }

    // Validate type argument arities in function signatures.
    for (_, fn_item) in item_tree.functions.iter() {
        if fn_item
            .source_range
            .is_some_and(|range| overlaps_parse_error(range, parse_error_ranges))
        {
            continue;
        }
        let fn_span = fn_item
            .source_range
            .map(|r| Span {
                file: file_id,
                range: r,
            })
            .unwrap_or(Span {
                file: file_id,
                range: kyokara_span::TextRange::default(),
            });
        for param in &fn_item.params {
            validate_type_arity(
                &param.ty,
                item_tree,
                module_scope,
                interner,
                fn_span,
                &mut body_lowering_diagnostics,
            );
        }
        if let Some(ret) = &fn_item.ret_type {
            validate_type_arity(
                ret,
                item_tree,
                module_scope,
                interner,
                fn_span,
                &mut body_lowering_diagnostics,
            );
        }
    }

    TypeCheckResult {
        fn_results,
        fn_bodies,
        let_results,
        let_bodies,
        diagnostics: all_diagnostics,
        raw_diagnostics: all_raw_diagnostics,
        body_lowering_diagnostics,
        fn_calls,
    }
}

fn overlaps_parse_error(range: TextRange, parse_error_ranges: &[TextRange]) -> bool {
    parse_error_ranges
        .iter()
        .any(|parse_range| range.intersect(*parse_range).is_some())
}

/// Walk a TypeRef tree and emit diagnostics for type argument arity mismatches.
fn validate_type_arity(
    ty: &TypeRef,
    item_tree: &ItemTree,
    module_scope: &ModuleScope,
    interner: &Interner,
    span: Span,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match ty {
        TypeRef::Path { path, args } => {
            if path.is_single() && !args.is_empty() {
                let name = path.segments[0];
                if let Some(&type_idx) = module_scope.types.get(&name) {
                    let type_item = &item_tree.types[type_idx];
                    let expected = type_item.type_params.len();
                    let got = args.len();
                    if got > expected {
                        let name_str = name.resolve(interner);
                        diagnostics.push(Diagnostic::error(
                            format!(
                                "`{name_str}` expects {expected} type argument{} but got {got}",
                                if expected == 1 { "" } else { "s" }
                            ),
                            span,
                        ));
                    }
                }
            }
            // Recurse into type arguments.
            for arg in args {
                validate_type_arity(arg, item_tree, module_scope, interner, span, diagnostics);
            }
        }
        TypeRef::Fn { params, ret } => {
            for p in params {
                validate_type_arity(p, item_tree, module_scope, interner, span, diagnostics);
            }
            validate_type_arity(ret, item_tree, module_scope, interner, span, diagnostics);
        }
        TypeRef::Record { fields } => {
            for (_, t) in fields {
                validate_type_arity(t, item_tree, module_scope, interner, span, diagnostics);
            }
        }
        TypeRef::Refined { base, .. } => {
            validate_type_arity(base, item_tree, module_scope, interner, span, diagnostics);
        }
        TypeRef::Error => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kyokara_syntax::ast::nodes::SourceFile;

    #[test]
    fn body_lookup_prefers_source_range_match() {
        let parse = kyokara_syntax::parse(
            r#"
            fn alpha() -> Int { 1 }
            fn beta() -> Int { 2 }
            "#,
        );
        let root = SyntaxNode::new_root(parse.green);
        let _source_file = SourceFile::cast(root.clone()).expect("source file");

        let fn_defs: Vec<FnDef> = root.descendants().filter_map(FnDef::cast).collect();
        let prop_defs: Vec<PropertyDef> =
            root.descendants().filter_map(PropertyDef::cast).collect();
        let index = build_body_lookup_index(&fn_defs, &[], &prop_defs, &[]);

        let beta = &fn_defs[1];
        let beta_range = beta.syntax().text_range();
        let by_range = lookup_fn_def(&index, Some(beta_range))
            .expect("range lookup should prefer exact node");
        assert_eq!(by_range.syntax().text_range(), beta_range);
    }

    #[test]
    fn body_lookup_returns_none_when_range_is_missing() {
        let parse = kyokara_syntax::parse(
            r#"
            fn main() -> Int { helper() }
            fn helper() -> Int { 7 }
            "#,
        );
        let root = SyntaxNode::new_root(parse.green);
        let _source_file = SourceFile::cast(root.clone()).expect("source file");

        let fn_defs: Vec<FnDef> = root.descendants().filter_map(FnDef::cast).collect();
        let prop_defs: Vec<PropertyDef> =
            root.descendants().filter_map(PropertyDef::cast).collect();
        let index = build_body_lookup_index(&fn_defs, &[], &prop_defs, &[]);

        assert!(
            lookup_fn_def(&index, None).is_none(),
            "missing source range should not guess a body by name"
        );
    }
}
