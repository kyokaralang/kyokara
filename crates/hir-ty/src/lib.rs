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
use kyokara_hir_def::body::lower::{lower_body, lower_property_body};
use kyokara_hir_def::item_tree::{FnItemIdx, ItemTree};
use kyokara_hir_def::resolver::ModuleScope;
use kyokara_hir_def::type_ref::TypeRef;
use kyokara_intern::Interner;
use kyokara_span::{FileId, Span};
use kyokara_stdx::FxHashMap;
use kyokara_syntax::SyntaxNode;
use kyokara_syntax::ast::AstNode;
use kyokara_syntax::ast::nodes::{FnDef, PropertyDef};
use kyokara_syntax::ast::traits::HasName;

use kyokara_hir_def::name::Name;

use crate::diagnostics::TyDiagnosticData;
use crate::infer::InferenceResult;

/// Result of type-checking an entire module.
#[derive(Debug)]
pub struct TypeCheckResult {
    /// Per-function inference results, keyed by function item index.
    pub fn_results: FxHashMap<FnItemIdx, InferenceResult>,
    /// Lowered bodies for each function, keyed by function item index.
    pub fn_bodies: FxHashMap<FnItemIdx, Body>,
    /// All diagnostics from type checking.
    pub diagnostics: Vec<Diagnostic>,
    /// Raw diagnostic data with spans for structured output.
    pub raw_diagnostics: Vec<(TyDiagnosticData, Span)>,
    /// Diagnostics from body lowering (e.g. unresolved names, duplicates).
    pub body_lowering_diagnostics: Vec<Diagnostic>,
    /// Per-function call edges: (caller name, list of callee names).
    pub fn_calls: Vec<(Name, Vec<Name>)>,
}

/// Type-check all functions in a module.
///
/// Takes an already-collected item tree and module scope, plus the CST root
/// for body lowering.
pub fn check_module(
    root: &SyntaxNode,
    item_tree: &ItemTree,
    module_scope: &ModuleScope,
    file_id: FileId,
    interner: &mut Interner,
) -> TypeCheckResult {
    let mut fn_results = FxHashMap::default();
    let mut fn_bodies = FxHashMap::default();
    let mut all_diagnostics = Vec::new();
    let mut all_raw_diagnostics = Vec::new();
    let mut body_lowering_diagnostics = Vec::new();
    let mut fn_calls = Vec::new();

    let fn_defs: Vec<FnDef> = root.descendants().filter_map(FnDef::cast).collect();
    let prop_defs: Vec<PropertyDef> = root.descendants().filter_map(PropertyDef::cast).collect();

    for (fn_idx, fn_item) in item_tree.functions.iter() {
        if !fn_item.has_body {
            continue;
        }

        // Match by source range when available (exact CST node identity),
        // falling back to name-based matching for imported functions.
        let fn_name_str = fn_item.name.resolve(interner);
        let fn_def = fn_defs.iter().find(|fd: &&FnDef| {
            if let Some(range) = fn_item.source_range {
                fd.syntax().text_range() == range
            } else {
                fd.name_token().is_some_and(|t| t.text() == fn_name_str)
            }
        });

        // Try FnDef first; if not found, try PropertyDef (synthetic FnItems
        // created for properties point at PropertyDef source ranges).
        let body_result = if let Some(fd) = fn_def {
            lower_body(fd, module_scope, file_id, interner)
        } else if let Some(pd) = prop_defs.iter().find(|pd| {
            fn_item
                .source_range
                .is_some_and(|range| pd.syntax().text_range() == range)
        }) {
            lower_property_body(pd, module_scope, file_id, interner)
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
        diagnostics: all_diagnostics,
        raw_diagnostics: all_raw_diagnostics,
        body_lowering_diagnostics,
        fn_calls,
    }
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
