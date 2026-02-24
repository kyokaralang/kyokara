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
use kyokara_hir_def::body::lower::lower_body;
use kyokara_hir_def::item_tree::{FnItemIdx, ItemTree};
use kyokara_hir_def::resolver::ModuleScope;
use kyokara_intern::Interner;
use kyokara_span::{FileId, Span};
use kyokara_stdx::FxHashMap;
use kyokara_syntax::SyntaxNode;
use kyokara_syntax::ast::AstNode;
use kyokara_syntax::ast::nodes::FnDef;
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
    let mut fn_calls = Vec::new();

    let fn_defs: Vec<FnDef> = root.descendants().filter_map(FnDef::cast).collect();

    for (fn_idx, fn_item) in item_tree.functions.iter() {
        if !fn_item.has_body {
            continue;
        }

        let fn_name_str = fn_item.name.resolve(interner);
        let Some(fn_def) = fn_defs
            .iter()
            .find(|fd: &&FnDef| fd.name_token().is_some_and(|t| t.text() == fn_name_str))
        else {
            continue;
        };

        let body_result = lower_body(fn_def, module_scope, file_id, interner);

        let fn_span = Span {
            file: file_id,
            range: fn_def.syntax().text_range(),
        };

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

    TypeCheckResult {
        fn_results,
        fn_bodies,
        diagnostics: all_diagnostics,
        raw_diagnostics: all_raw_diagnostics,
        fn_calls,
    }
}
