//! Bidirectional type inference engine.
//!
//! Entry point: [`infer_body`] takes a function's signature and lowered body,
//! and produces an [`InferenceResult`] with per-expression/pattern types.

mod expr;
mod pat;

use kyokara_diagnostics::Diagnostic;
use kyokara_hir_def::body::Body;
use kyokara_hir_def::expr::ExprIdx;
use kyokara_hir_def::item_tree::{FnItem, FnItemIdx, ItemTree};
use kyokara_hir_def::name::Name;
use kyokara_hir_def::pat::Pat;
use kyokara_hir_def::resolver::ModuleScope;
use kyokara_hir_def::type_ref::TypeRef;
use kyokara_intern::Interner;
use kyokara_span::Span;
use la_arena::ArenaMap;

use crate::diagnostics::TyDiagnosticData;
use crate::effects::EffectSet;
use crate::holes::HoleInfo;
use crate::resolve::TyResolutionEnv;
use crate::ty::Ty;
use crate::unify::UnificationTable;

/// Top-down type expectation pushed during bidirectional inference.
#[derive(Debug, Clone)]
pub enum Expectation {
    /// We know the expected type from context.
    Has(Ty),
    /// No expectation — infer bottom-up.
    None,
}

impl Expectation {
    pub fn ty(&self) -> Option<&Ty> {
        match self {
            Expectation::Has(ty) => Some(ty),
            Expectation::None => None,
        }
    }
}

/// Per-function inference result.
#[derive(Debug)]
pub struct InferenceResult {
    pub expr_types: ArenaMap<ExprIdx, Ty>,
    pub pat_types: ArenaMap<la_arena::Idx<Pat>, Ty>,
    pub holes: Vec<HoleInfo>,
    pub diagnostics: Vec<Diagnostic>,
    /// Raw diagnostic data with spans, preserved for structured JSON output.
    pub raw_diagnostics: Vec<(TyDiagnosticData, Span)>,
    /// Names of functions called from this function body.
    pub calls: Vec<Name>,
    /// Resolved types for each function parameter, by index.
    pub param_types: Vec<Ty>,
    /// Resolved return type for this function.
    pub ret_ty: Ty,
}

/// Mutable inference context, threaded through expression/pattern inference.
pub(crate) struct InferenceCtx<'a> {
    pub table: UnificationTable,
    pub expr_types: ArenaMap<ExprIdx, Ty>,
    pub pat_types: ArenaMap<la_arena::Idx<Pat>, Ty>,
    pub holes: Vec<HoleInfo>,
    /// Diagnostics paired with the expression that caused them.
    pub diags: Vec<(TyDiagnosticData, ExprIdx)>,
    /// Names of functions called from this body (for symbol graph edges).
    pub calls: Vec<Name>,

    pub body: &'a Body,
    pub item_tree: &'a ItemTree,
    pub module_scope: &'a ModuleScope,
    pub interner: &'a Interner,
    pub _fn_span: Span,

    /// The expression currently being inferred (for diagnostic span tracking).
    pub current_expr: Option<ExprIdx>,
    /// Current function's return type (for `return` expressions).
    pub ret_ty: Ty,
    /// Current function's effect set (for effect checking at call sites).
    pub caller_effects: EffectSet,
    /// Type parameters in scope for the current function.
    pub type_params: Vec<(Name, Ty)>,
    /// Parameter types by index (for ScopeDef::Param(i) lookups).
    pub param_types: Vec<Ty>,
    /// Parameter names by index (for locals collection in holes).
    pub param_names: Vec<Name>,
    /// Local variable types: PatIdx → Ty (for looking up bound names).
    pub local_types: ArenaMap<la_arena::Idx<Pat>, Ty>,
}

impl<'a> InferenceCtx<'a> {
    /// Record a diagnostic at the current expression.
    pub(crate) fn push_diag(&mut self, data: TyDiagnosticData) {
        if let Some(expr_idx) = self.current_expr {
            self.diags.push((data, expr_idx));
        } else {
            // Fallback: use body root (shouldn't happen in practice).
            self.diags.push((data, self.body.root));
        }
    }

    /// Unify two types, emitting a type mismatch diagnostic on failure.
    /// Returns the unified type (or Error on failure).
    pub(crate) fn unify_or_err(&mut self, expected: &Ty, actual: &Ty) -> Ty {
        if self.table.unify(expected, actual) {
            self.table.resolve_deep(expected)
        } else {
            let expected = self.table.resolve_deep(expected);
            let actual = self.table.resolve_deep(actual);
            if !expected.is_poison() && !actual.is_poison() {
                self.push_diag(TyDiagnosticData::TypeMismatch {
                    expected: expected.clone(),
                    actual: actual.clone(),
                });
            }
            Ty::Error
        }
    }

    /// Build a `TyResolutionEnv` from non-table fields.
    /// This avoids borrowing `self` (which would conflict with `&mut self.table`).
    pub(crate) fn make_env(
        item_tree: &'a ItemTree,
        module_scope: &'a ModuleScope,
        interner: &'a Interner,
        type_params: &[(Name, Ty)],
    ) -> TyResolutionEnv<'a> {
        TyResolutionEnv {
            item_tree,
            module_scope,
            interner,
            type_params: type_params.to_vec(),
            resolving_aliases: vec![],
        }
    }
}

/// Infer types for a single function body.
pub fn infer_body(
    fn_idx: FnItemIdx,
    fn_item: &FnItem,
    body: &Body,
    item_tree: &ItemTree,
    module_scope: &ModuleScope,
    interner: &Interner,
    fn_span: Span,
) -> InferenceResult {
    let mut table = UnificationTable::new();

    // Build type parameter bindings (fresh vars for each).
    let mut type_params: Vec<(Name, Ty)> = Vec::new();
    for &name in &fn_item.type_params {
        let var = table.fresh_var();
        type_params.push((name, var));
    }

    let env = TyResolutionEnv {
        item_tree,
        module_scope,
        interner,
        type_params: type_params.clone(),
        resolving_aliases: vec![],
    };

    // Resolve return type.
    let ret_ty = fn_item
        .ret_type
        .as_ref()
        .map(|t| env.resolve_type_ref(t, &mut table))
        .unwrap_or(Ty::Unit);

    // Build caller effect set.
    let caller_effects = EffectSet::from_with_caps(&fn_item.with_caps, &env, &mut table, interner);

    // Validate capability names.
    let mut diags: Vec<(TyDiagnosticData, ExprIdx)> = Vec::new();
    for cap_name in &caller_effects.caps {
        if !module_scope.caps.contains_key(cap_name) {
            diags.push((
                TyDiagnosticData::UnresolvedType {
                    name: cap_name.resolve(interner).to_owned(),
                },
                body.root,
            ));
        }
    }

    // Emit diagnostic for unresolved return type.
    if ret_ty == Ty::Error
        && let Some(type_ref) = &fn_item.ret_type
    {
        collect_unresolved_type_names(type_ref, interner, body.root, &mut diags);
    }

    // Resolve parameter types eagerly (stored by index for ScopeDef::Param lookups).
    let mut param_tys = Vec::new();
    for param in &fn_item.params {
        let param_env = TyResolutionEnv {
            item_tree,
            module_scope,
            interner,
            type_params: type_params.clone(),
            resolving_aliases: vec![],
        };
        let ty = param_env.resolve_type_ref(&param.ty, &mut table);
        if ty == Ty::Error && param.ty != TypeRef::Error {
            collect_unresolved_type_names(&param.ty, interner, body.root, &mut diags);
        }
        param_tys.push(ty);
    }

    let mut ctx = InferenceCtx {
        table,
        expr_types: ArenaMap::default(),
        pat_types: ArenaMap::default(),
        holes: Vec::new(),
        diags,
        calls: Vec::new(),
        body,
        item_tree,
        module_scope,
        interner,
        _fn_span: fn_span,
        current_expr: None,
        ret_ty,
        caller_effects,
        type_params,
        param_types: param_tys.clone(),
        param_names: fn_item.params.iter().map(|p| p.name).collect(),
        local_types: ArenaMap::default(),
    };

    // Also try to bind param types to any matching pat_scopes entries.
    for (i, param) in fn_item.params.iter().enumerate() {
        let ty = &param_tys[i];
        for (pat_idx, _scope_idx) in &body.pat_scopes {
            if let Pat::Bind { name } = &body.pats[*pat_idx]
                && *name == param.name
            {
                ctx.pat_types.insert(*pat_idx, ty.clone());
                ctx.local_types.insert(*pat_idx, ty.clone());
                break;
            }
        }
    }
    let _ = fn_idx;

    // Infer the body expression with the return type as expectation.
    let body_ty = ctx.infer_expr(body.root, &Expectation::Has(ctx.ret_ty.clone()));

    // Unify body result with declared return type.
    // Attribute mismatch to the body root expression.
    ctx.current_expr = Some(body.root);
    let ret = ctx.ret_ty.clone();
    ctx.unify_or_err(&ret, &body_ty);

    // Resolve all types deeply.
    let mut expr_types = ArenaMap::default();
    for (idx, ty) in ctx.expr_types.into_iter() {
        expr_types.insert(idx, ctx.table.resolve_deep(&ty));
    }
    let mut pat_types = ArenaMap::default();
    for (idx, ty) in ctx.pat_types.into_iter() {
        pat_types.insert(idx, ctx.table.resolve_deep(&ty));
    }

    // Build raw diagnostics (data + span) for structured output.
    let raw_diagnostics: Vec<(TyDiagnosticData, Span)> = ctx
        .diags
        .iter()
        .map(|(d, expr_idx)| {
            let range = body
                .expr_source_map
                .get(*expr_idx)
                .copied()
                .unwrap_or(fn_span.range);
            (
                d.clone(),
                Span {
                    file: fn_span.file,
                    range,
                },
            )
        })
        .collect();

    // Convert diagnostics using expression-precise spans.
    let diagnostics: Vec<Diagnostic> = ctx
        .diags
        .into_iter()
        .map(|(d, expr_idx)| {
            let range = body
                .expr_source_map
                .get(expr_idx)
                .copied()
                .unwrap_or(fn_span.range);
            let span = Span {
                file: fn_span.file,
                range,
            };
            d.into_diagnostic(span, interner, item_tree)
        })
        .collect();

    let resolved_param_types = ctx
        .param_types
        .iter()
        .map(|ty| ctx.table.resolve_deep(ty))
        .collect();
    let resolved_ret_ty = ctx.table.resolve_deep(&ctx.ret_ty);

    InferenceResult {
        expr_types,
        pat_types,
        holes: ctx.holes,
        diagnostics,
        raw_diagnostics,
        calls: ctx.calls,
        param_types: resolved_param_types,
        ret_ty: resolved_ret_ty,
    }
}

/// Walk a [`TypeRef`] and collect `UnresolvedType` diagnostics for any
/// single-segment path names that are not built-in or resolvable.
fn collect_unresolved_type_names(
    type_ref: &TypeRef,
    interner: &Interner,
    expr_idx: ExprIdx,
    diags: &mut Vec<(TyDiagnosticData, ExprIdx)>,
) {
    match type_ref {
        TypeRef::Path { path, args } => {
            if path.is_single() {
                let name_str = path.segments[0].resolve(interner);
                diags.push((
                    TyDiagnosticData::UnresolvedType {
                        name: name_str.to_owned(),
                    },
                    expr_idx,
                ));
            }
            for arg in args {
                collect_unresolved_type_names(arg, interner, expr_idx, diags);
            }
        }
        TypeRef::Fn { params, ret } => {
            for p in params {
                collect_unresolved_type_names(p, interner, expr_idx, diags);
            }
            collect_unresolved_type_names(ret, interner, expr_idx, diags);
        }
        TypeRef::Record { fields } => {
            for (_, t) in fields {
                collect_unresolved_type_names(t, interner, expr_idx, diags);
            }
        }
        TypeRef::Refined { base, .. } => {
            collect_unresolved_type_names(base, interner, expr_idx, diags);
        }
        TypeRef::Error => {}
    }
}
