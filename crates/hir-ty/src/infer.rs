//! Bidirectional type inference engine.
//!
//! Entry point: [`infer_body`] takes a function's signature and lowered body,
//! and produces an [`InferenceResult`] with per-expression/pattern types.

mod expr;
mod pat;

use kyokara_diagnostics::Diagnostic;
use kyokara_hir_def::body::{Body, LocalBindingOrigin};
use kyokara_hir_def::expr::ExprIdx;
use kyokara_hir_def::item_tree::{FnItem, FnItemIdx, ItemTree, TypeDefKind, TypeItemIdx};
use kyokara_hir_def::name::Name;
use kyokara_hir_def::pat::Pat;
use kyokara_hir_def::resolver::{CoreType, ModuleScope};
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

/// Source location attached to a type-check diagnostic.
#[derive(Debug, Clone, Copy)]
pub(crate) enum DiagLoc {
    Expr(ExprIdx),
    Pat(la_arena::Idx<Pat>),
}

/// Mutable inference context, threaded through expression/pattern inference.
pub(crate) struct InferenceCtx<'a> {
    pub table: UnificationTable,
    pub expr_types: ArenaMap<ExprIdx, Ty>,
    pub pat_types: ArenaMap<la_arena::Idx<Pat>, Ty>,
    pub holes: Vec<HoleInfo>,
    /// Diagnostics paired with the expression that caused them.
    pub diags: Vec<(TyDiagnosticData, DiagLoc)>,
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
    /// Declared trait bounds for each type parameter in scope.
    pub type_param_bounds: Vec<(Name, Vec<Name>)>,
    /// Parameter types by index (for ScopeDef::Param(i) lookups).
    pub param_types: Vec<Ty>,
    /// Parameter names by index (for locals collection in holes).
    pub param_names: Vec<Name>,
    /// Local variable types: PatIdx → Ty (for looking up bound names).
    pub local_types: ArenaMap<la_arena::Idx<Pat>, Ty>,
    /// Depth counter for scoped Seq<-{List,Deque} compatibility in traversal inference.
    pub traversal_seq_compat_depth: usize,
    /// Nesting depth of loop statements (`while` / `for`) during inference.
    pub loop_depth: usize,
}

impl<'a> InferenceCtx<'a> {
    pub(crate) fn with_traversal_seq_compat_scope<R>(
        &mut self,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.traversal_seq_compat_depth += 1;
        let out = f(self);
        self.traversal_seq_compat_depth -= 1;
        out
    }

    fn traversal_seq_compat_enabled(&self) -> bool {
        self.traversal_seq_compat_depth > 0
    }

    pub(crate) fn ty_satisfies_trait_name(&mut self, ty: &Ty, trait_name: Name) -> bool {
        let ty = self.table.resolve_deep(ty);
        match ty {
            Ty::Error | Ty::Never => true,
            Ty::Var(var) => self
                .type_params
                .iter()
                .find(|(_, param_ty)| matches!(self.table.resolve(param_ty), Ty::Var(v) if v == var))
                .and_then(|(name, _)| self.type_param_bounds.iter().find(|(n, _)| n == name))
                .is_some_and(|(_, bounds)| {
                    bounds
                        .iter()
                        .copied()
                        .any(|bound| self.trait_name_satisfies(bound, trait_name))
                }),
            Ty::Int => self.builtin_trait_name_satisfied("Int", trait_name),
            Ty::String => self.builtin_trait_name_satisfied("String", trait_name),
            Ty::Char => self.builtin_trait_name_satisfied("Char", trait_name),
            Ty::Bool => self.builtin_trait_name_satisfied("Bool", trait_name),
            Ty::Unit => self.builtin_trait_name_satisfied("Unit", trait_name),
            Ty::Float => self.builtin_trait_name_satisfied("Float", trait_name),
            Ty::Adt { def, args } => {
                let type_name = self.item_tree.types[def].name.resolve(self.interner);
                if self.module_scope.core_types.kind_for_idx(def).is_some() {
                    self.builtin_trait_name_satisfied(type_name, trait_name)
                } else {
                    self.user_adt_satisfies_trait(def, &args, trait_name)
                }
            }
            Ty::Record { fields } => match trait_name.resolve(self.interner) {
                "Eq" | "Ord" | "Hash" | "Show" => fields
                    .iter()
                    .all(|(_, field_ty)| self.ty_satisfies_trait_name(field_ty, trait_name)),
                _ => false,
            },
            Ty::Fn { .. } => false,
        }
    }

    pub(crate) fn ty_satisfies_trait(&mut self, ty: &Ty, trait_text: &str) -> bool {
        self.module_scope
            .traits
            .keys()
            .find(|name| name.resolve(self.interner) == trait_text)
            .copied()
            .is_some_and(|trait_name| self.ty_satisfies_trait_name(ty, trait_name))
    }

    fn builtin_trait_name_satisfied(&self, type_name: &str, trait_name: Name) -> bool {
        let trait_name = trait_name.resolve(self.interner);
        match trait_name {
            "Eq" => matches!(
                type_name,
                "Int"
                    | "String"
                    | "Char"
                    | "Bool"
                    | "Unit"
                    | "Option"
                    | "Result"
                    | "List"
                    | "MutableList"
                    | "Deque"
                    | "BitSet"
                    | "MutableBitSet"
                    | "Map"
                    | "MutableMap"
                    | "Set"
                    | "MutableSet"
                    | "ParseError"
            ),
            "Ord" => matches!(
                type_name,
                "Int"
                    | "String"
                    | "Char"
                    | "Bool"
                    | "Unit"
                    | "Option"
                    | "Result"
                    | "List"
                    | "MutableList"
                    | "Deque"
                    | "ParseError"
            ),
            "Hash" => matches!(
                type_name,
                "Int"
                    | "String"
                    | "Char"
                    | "Bool"
                    | "Unit"
                    | "Option"
                    | "Result"
                    | "List"
                    | "Deque"
                    | "BitSet"
                    | "Map"
                    | "Set"
                    | "ParseError"
            ),
            "Show" => !matches!(type_name, "<fn>"),
            _ => false,
        }
    }

    fn trait_name_satisfies(&self, bound_name: Name, target_name: Name) -> bool {
        if bound_name == target_name {
            return true;
        }
        let Some(&bound_idx) = self.module_scope.traits.get(&bound_name) else {
            return false;
        };
        self.item_tree.traits[bound_idx]
            .supertraits
            .iter()
            .filter_map(|trait_ref| trait_ref.path.last())
            .any(|super_name| self.trait_name_satisfies(super_name, target_name))
    }

    fn user_adt_satisfies_trait(
        &mut self,
        def: TypeItemIdx,
        args: &[Ty],
        trait_name: Name,
    ) -> bool {
        let type_item = &self.item_tree.types[def];

        if type_item.derives.iter().any(|derived| {
            derived
                .path
                .last()
                .is_some_and(|derived_name| self.trait_name_satisfies(derived_name, trait_name))
        }) {
            let mut env_type_params = self.type_params.clone();
            for (param, arg) in type_item.type_params.iter().zip(args.iter()) {
                env_type_params.push((param.name, arg.clone()));
            }
            let env = TyResolutionEnv {
                item_tree: self.item_tree,
                module_scope: self.module_scope,
                interner: self.interner,
                type_params: env_type_params,
                resolving_aliases: vec![],
            };

            let field_type_refs: Vec<TypeRef> = match &type_item.kind {
                TypeDefKind::Alias(TypeRef::Record { fields }) => {
                    fields.iter().map(|(_, ty)| ty.clone()).collect()
                }
                TypeDefKind::Record { fields } => fields.iter().map(|(_, ty)| ty.clone()).collect(),
                TypeDefKind::Adt { variants } => variants
                    .iter()
                    .flat_map(|variant| variant.fields.iter().cloned())
                    .collect(),
                TypeDefKind::Alias(_) => Vec::new(),
            };

            for field_ty_ref in field_type_refs {
                let field_ty = env.resolve_type_ref(&field_ty_ref, &mut self.table);
                if !self.ty_satisfies_trait_name(&field_ty, trait_name) {
                    return false;
                }
            }
            return true;
        }

        self.item_tree.impls.iter().any(|(_, impl_item)| {
            impl_item
                .trait_ref
                .path
                .last()
                .is_some_and(|impl_trait| impl_trait == trait_name)
                && matches!(
                    &impl_item.self_ty,
                    TypeRef::Path { path, .. } if path.last().is_some_and(|n| n == type_item.name)
                )
        })
    }

    pub(crate) fn with_loop_scope<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.loop_depth += 1;
        let out = f(self);
        self.loop_depth -= 1;
        out
    }

    pub(crate) fn in_loop(&self) -> bool {
        self.loop_depth > 0
    }

    /// Record a diagnostic at the current expression.
    pub(crate) fn push_diag(&mut self, data: TyDiagnosticData) {
        if let Some(expr_idx) = self.current_expr {
            self.diags.push((data, DiagLoc::Expr(expr_idx)));
        } else {
            // Fallback: use body root (shouldn't happen in practice).
            self.diags.push((data, DiagLoc::Expr(self.body.root)));
        }
    }

    /// Record a diagnostic at a specific pattern site.
    pub(crate) fn push_pat_diag(&mut self, pat_idx: la_arena::Idx<Pat>, data: TyDiagnosticData) {
        self.diags.push((data, DiagLoc::Pat(pat_idx)));
    }

    /// Unify two types, emitting a type mismatch diagnostic on failure.
    /// Returns the unified type (or Error on failure).
    pub(crate) fn unify_or_err(&mut self, expected: &Ty, actual: &Ty) -> Ty {
        let mut exact_table = self.table.clone();
        if exact_table.unify(expected, actual)
            || (self.traversal_seq_compat_enabled()
                && self.unify_seq_traversal_compat_with_table(
                    &mut exact_table,
                    expected,
                    actual,
                ))
        {
            self.table = exact_table;
            return self.table.resolve_deep(expected);
        }

        let expected_norm = self.normalize_record_aliases_for_unify(expected);
        let actual_norm = self.normalize_record_aliases_for_unify(actual);

        let mut normalized_table = self.table.clone();
        if normalized_table.unify(&expected_norm, &actual_norm)
            || (self.traversal_seq_compat_enabled()
                && self.unify_seq_traversal_compat_with_table(
                    &mut normalized_table,
                    &expected_norm,
                    &actual_norm,
                ))
        {
            self.table = normalized_table;
            self.table.resolve_deep(&expected_norm)
        } else {
            let expected = self.table.resolve_deep(&expected_norm);
            let actual = self.table.resolve_deep(&actual_norm);
            if !expected.is_poison() && !actual.is_poison() {
                self.push_diag(TyDiagnosticData::TypeMismatch {
                    expected: expected.clone(),
                    actual: actual.clone(),
                });
            }
            Ty::Error
        }
    }

    fn unify_seq_traversal_compat_with_table(
        &self,
        table: &mut UnificationTable,
        expected: &Ty,
        actual: &Ty,
    ) -> bool {
        let expected = table.resolve(expected);
        let actual = table.resolve(actual);
        let (
            Ty::Adt {
                def: expected_def,
                args: expected_args,
            },
            Ty::Adt {
                def: actual_def,
                args: actual_args,
            },
        ) = (&expected, &actual)
        else {
            return false;
        };

        let expected_core = self.module_scope.core_types.kind_for_idx(*expected_def);
        let actual_core = self.module_scope.core_types.kind_for_idx(*actual_def);

        if expected_core != Some(CoreType::Seq)
            || !matches!(
                actual_core,
                Some(CoreType::List | CoreType::MutableList | CoreType::Deque)
            )
        {
            return false;
        }
        if expected_args.len() != 1 || actual_args.len() != 1 {
            return false;
        }

        table.unify(&expected_args[0], &actual_args[0])
    }

    /// Expand record aliases into structural records for unification-only paths.
    ///
    /// We keep alias-to-record values as `Ty::Adt` elsewhere for method
    /// dispatch identity, but expectation unification should accept structurally
    /// equivalent shapes (e.g., `Option<PickStep>` vs `Option<{ value, state }>`).
    fn normalize_record_aliases_for_unify(&mut self, ty: &Ty) -> Ty {
        let resolved = self.table.resolve_deep(ty);
        self.normalize_record_aliases_inner(resolved)
    }

    fn normalize_record_aliases_inner(&mut self, ty: Ty) -> Ty {
        match ty {
            Ty::Adt { def, args } => {
                if let Some(fields) = self.expand_record_alias(def, &args) {
                    return Ty::Record { fields };
                }
                Ty::Adt {
                    def,
                    args: args
                        .into_iter()
                        .map(|arg| self.normalize_record_aliases_inner(arg))
                        .collect(),
                }
            }
            Ty::Record { fields } => Ty::Record {
                fields: fields
                    .into_iter()
                    .map(|(name, field_ty)| (name, self.normalize_record_aliases_inner(field_ty)))
                    .collect(),
            },
            Ty::Fn { params, ret } => Ty::Fn {
                params: params
                    .into_iter()
                    .map(|param| self.normalize_record_aliases_inner(param))
                    .collect(),
                ret: Box::new(self.normalize_record_aliases_inner(*ret)),
            },
            other => other,
        }
    }

    fn expand_record_alias(
        &mut self,
        type_idx: TypeItemIdx,
        args: &[Ty],
    ) -> Option<Vec<(Name, Ty)>> {
        let type_item = &self.item_tree.types[type_idx];
        let TypeDefKind::Alias(TypeRef::Record { fields }) = &type_item.kind else {
            return None;
        };

        let mut type_params = self.type_params.clone();
        for (i, param) in type_item.type_params.iter().enumerate() {
            let arg = args
                .get(i)
                .cloned()
                .unwrap_or_else(|| self.table.fresh_var());
            type_params.push((param.name, arg));
        }

        let env = TyResolutionEnv {
            item_tree: self.item_tree,
            module_scope: self.module_scope,
            interner: self.interner,
            type_params,
            resolving_aliases: vec![type_idx],
        };

        let resolved_fields = fields
            .iter()
            .map(|(field_name, field_ty_ref)| {
                let resolved = env.resolve_type_ref(field_ty_ref, &mut self.table);
                (
                    *field_name,
                    self.normalize_record_aliases_for_unify(&resolved),
                )
            })
            .collect();
        Some(resolved_fields)
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
    let mut type_param_bounds: Vec<(Name, Vec<Name>)> = Vec::new();
    for param in &fn_item.type_params {
        let var = table.fresh_var();
        type_params.push((param.name, var));
        type_param_bounds.push((
            param.name,
            param
                .bounds
                .iter()
                .filter_map(|bound| bound.path.last())
                .collect(),
        ));
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
    let caller_effects =
        EffectSet::from_with_effects(&fn_item.with_effects, &env, &mut table, interner);

    // Validate capability names.
    let mut diags: Vec<(TyDiagnosticData, DiagLoc)> = Vec::new();
    for cap_name in &caller_effects.effects {
        if !module_scope.effects.contains_key(cap_name) {
            diags.push((
                TyDiagnosticData::UnresolvedType {
                    name: cap_name.resolve(interner).to_owned(),
                },
                DiagLoc::Expr(body.root),
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
        type_param_bounds,
        param_types: param_tys.clone(),
        param_names: fn_item.params.iter().map(|p| p.name).collect(),
        local_types: ArenaMap::default(),
        traversal_seq_compat_depth: 0,
        loop_depth: 0,
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

    // Infer contract clause expressions so all sub-expressions get types.
    // Without this, literals and intermediates inside requires/ensures
    // would have no type entries, causing codegen to fail.
    for req_expr in body.requires.iter().copied() {
        ctx.infer_expr(req_expr, &Expectation::Has(Ty::Bool));
    }
    if !body.ensures.is_empty() {
        // Bind the `result` pattern to the return type so name resolution
        // finds it with the correct type (not Ty::Error).
        for (pat_idx, meta) in body.local_binding_meta.iter() {
            if meta.origin == LocalBindingOrigin::ContractResult {
                ctx.pat_types.insert(pat_idx, ctx.ret_ty.clone());
                ctx.local_types.insert(pat_idx, ctx.ret_ty.clone());
            }
        }
        for ens_expr in body.ensures.iter().copied() {
            ctx.infer_expr(ens_expr, &Expectation::Has(Ty::Bool));
        }
    }
    for inv_expr in body.invariant.iter().copied() {
        ctx.infer_expr(inv_expr, &Expectation::Has(Ty::Bool));
    }

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
        .map(|(d, loc)| {
            let range = match loc {
                DiagLoc::Expr(expr_idx) => body.expr_source_map.get(*expr_idx).copied(),
                DiagLoc::Pat(pat_idx) => body.pat_source_map.get(*pat_idx).copied(),
            }
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
        .map(|(d, loc)| {
            let range = match loc {
                DiagLoc::Expr(expr_idx) => body.expr_source_map.get(expr_idx).copied(),
                DiagLoc::Pat(pat_idx) => body.pat_source_map.get(pat_idx).copied(),
            }
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
    diags: &mut Vec<(TyDiagnosticData, DiagLoc)>,
) {
    match type_ref {
        TypeRef::Path { path, args } => {
            if path.is_single() {
                let name_str = path.segments[0].resolve(interner);
                diags.push((
                    TyDiagnosticData::UnresolvedType {
                        name: name_str.to_owned(),
                    },
                    DiagLoc::Expr(expr_idx),
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
