//! Expression type inference for all [`Expr`] variants.

use std::collections::HashSet;

use kyokara_hir_def::expr::{BinaryOp, CallArg, Expr, ExprIdx, Literal, Stmt, UnaryOp};
use kyokara_hir_def::item_tree::{FnItemIdx, TypeDefKind};
use kyokara_hir_def::pat::Pat;
use kyokara_hir_def::resolver::{
    CoreType, PrimitiveType, ReceiverKey, ResolvedName, StaticOwnerKey,
};
use kyokara_hir_def::scope::ScopeDef;

use crate::diagnostics::TyDiagnosticData;
use crate::effects;
use crate::holes::HoleInfo;
use crate::resolve::{TyResolutionEnv, instantiate_constructor, instantiate_fn_sig};
use crate::ty::Ty;

use super::{Expectation, InferenceCtx};

enum AssignmentTargetInfo {
    MutableLocal,
    Immutable(String),
    Invalid,
}

impl<'a> InferenceCtx<'a> {
    /// Infer the type of an expression, possibly guided by an expectation.
    pub(crate) fn infer_expr(&mut self, idx: ExprIdx, expected: &Expectation) -> Ty {
        let prev_expr = self.current_expr;
        self.current_expr = Some(idx);
        let ty = self.infer_expr_inner(idx, expected);
        self.expr_types.insert(idx, ty.clone());

        // Fallback: enforce expectation for non-poison types.
        // On mismatch, return Ty::Error so parent expressions see poison
        // and don't re-report the same mismatch (prevents cascading).
        if let Expectation::Has(exp) = expected
            && !ty.is_poison()
            && !exp.is_poison()
        {
            let result = self.unify_or_err(exp, &ty);
            if result.is_poison() {
                self.current_expr = prev_expr;
                return Ty::Error;
            }
        }

        self.current_expr = prev_expr;
        ty
    }

    fn infer_expr_inner(&mut self, idx: ExprIdx, expected: &Expectation) -> Ty {
        match &self.body.exprs[idx] {
            Expr::Missing => Ty::Error,

            Expr::Literal(lit) => self.infer_literal(lit),

            Expr::Path(path) => {
                if !path.is_single() {
                    self.push_diag(TyDiagnosticData::MultiSegmentValuePath {
                        path: path
                            .segments
                            .iter()
                            .map(|s| s.resolve(self.interner).to_owned())
                            .collect::<Vec<_>>()
                            .join("."),
                    });
                    return Ty::Error;
                }
                let name = path.segments[0];
                self.infer_name(name, idx)
            }

            Expr::Binary { op, lhs, rhs } => self.infer_binary(*op, *lhs, *rhs),

            Expr::Unary { op, operand } => self.infer_unary(*op, *operand),

            Expr::Call { callee, args } => self.infer_call(*callee, args),

            Expr::Field { base, field } => self.infer_field(*base, *field),

            Expr::Index { base, index } => self.infer_index(*base, *index),

            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => self.infer_if(*condition, *then_branch, *else_branch, expected),

            Expr::Match { scrutinee, arms } => self.infer_match(idx, *scrutinee, arms, expected),

            Expr::Block { stmts, tail } => self.infer_block(stmts, *tail, expected),

            Expr::Return(val) => {
                let ret = self.ret_ty.clone();
                if let Some(val_idx) = *val {
                    self.infer_expr(val_idx, &Expectation::Has(ret));
                } else {
                    self.unify_or_err(&ret, &Ty::Unit);
                }
                Ty::Never
            }

            Expr::RecordLit { path, fields } => self.infer_record_lit(path.as_ref(), fields),

            Expr::Lambda { params, body } => self.infer_lambda(params, *body, expected),

            Expr::Old(inner) => self.infer_expr(*inner, expected),

            Expr::Hole => {
                let expected_ty = expected.ty().cloned();
                let ty = expected_ty
                    .clone()
                    .unwrap_or_else(|| self.table.fresh_var());

                let locals = self.collect_locals_in_scope(idx);

                let hole_span = self
                    .body
                    .expr_source_map
                    .get(idx)
                    .map(|range| kyokara_span::Span {
                        file: self._fn_span.file,
                        range: *range,
                    })
                    .unwrap_or(self._fn_span);

                self.holes.push(HoleInfo {
                    expected_type: expected_ty,
                    available_locals: locals,
                    span: hole_span,
                    effect_constraints: self.caller_effects.clone(),
                    name: None,
                });

                ty
            }
        }
    }

    fn infer_literal(&self, lit: &Literal) -> Ty {
        match lit {
            Literal::Int(_) => Ty::Int,
            Literal::Float(_) => Ty::Float,
            Literal::String(_) => Ty::String,
            Literal::Char(_) => Ty::Char,
            Literal::Bool(_) => Ty::Bool,
        }
    }

    fn infer_name(&mut self, name: kyokara_hir_def::name::Name, expr_idx: ExprIdx) -> Ty {
        let resolved = self
            .body
            .resolve_name_at(self.module_scope, expr_idx, name)
            .map(|r| r.resolved);
        match resolved {
            Some(ResolvedName::Local(scope_def)) => match &scope_def {
                ScopeDef::Local(pat_idx) => {
                    self.local_types.get(*pat_idx).cloned().unwrap_or(Ty::Error)
                }
                ScopeDef::Param(i) => {
                    // Look up by param index.
                    if *i < self.param_types.len() {
                        return self.param_types[*i].clone();
                    }
                    Ty::Error
                }
                ScopeDef::Fn(fn_idx) => {
                    let fn_idx = *fn_idx;
                    let env = Self::make_env(
                        self.item_tree,
                        self.module_scope,
                        self.interner,
                        &self.type_params,
                    );
                    let (params, ret) = instantiate_fn_sig(fn_idx, &env, &mut self.table);
                    Ty::Fn {
                        params,
                        ret: Box::new(ret),
                    }
                }
                ScopeDef::Constructor {
                    type_idx,
                    variant_idx,
                } => {
                    let (type_idx, variant_idx) = (*type_idx, *variant_idx);
                    self.constructor_as_fn(type_idx, variant_idx)
                }
                ScopeDef::LambdaParam(_) => {
                    // Lambda params are registered in local_types during lambda inference.
                    for (pat_idx, _) in &self.body.pat_scopes {
                        if let Pat::Bind { name: pat_name } = &self.body.pats[*pat_idx]
                            && *pat_name == name
                            && let Some(ty) = self.local_types.get(*pat_idx)
                        {
                            return ty.clone();
                        }
                    }
                    Ty::Error
                }
                ScopeDef::Type(_) => {
                    if let Some(&(type_idx, variant_idx)) =
                        self.module_scope.constructors.get(&name)
                    {
                        self.constructor_as_fn(type_idx, variant_idx)
                    } else {
                        self.non_value_name_in_expr("type", name)
                    }
                }
                ScopeDef::Effect(_) => self.non_value_name_in_expr("capability", name),
                ScopeDef::Import(_) => self.non_value_name_in_expr("import", name),
            },

            Some(ResolvedName::Fn(fn_idx)) => {
                let env = Self::make_env(
                    self.item_tree,
                    self.module_scope,
                    self.interner,
                    &self.type_params,
                );
                let (params, ret) = instantiate_fn_sig(fn_idx, &env, &mut self.table);
                Ty::Fn {
                    params,
                    ret: Box::new(ret),
                }
            }

            Some(ResolvedName::Constructor {
                type_idx,
                variant_idx,
            }) => self.constructor_as_fn(type_idx, variant_idx),

            Some(ResolvedName::Type(_)) => {
                if let Some(&(type_idx, variant_idx)) = self.module_scope.constructors.get(&name) {
                    self.constructor_as_fn(type_idx, variant_idx)
                } else {
                    self.non_value_name_in_expr("type", name)
                }
            }
            Some(ResolvedName::Effect(_)) => self.non_value_name_in_expr("capability", name),
            Some(ResolvedName::Trait(_)) => Ty::Error,
            Some(ResolvedName::Import(_)) => self.non_value_name_in_expr("import", name),

            // Module names (io, math, fs) are not values — they're only valid
            // as dot-call prefixes (io.println). Bare usage is an error.
            Some(ResolvedName::Module(_)) => Ty::Error,
            Some(ResolvedName::StaticMethodType(_)) => Ty::Error,

            None => Ty::Error,
        }
    }

    fn non_value_name_in_expr(&mut self, kind: &str, name: kyokara_hir_def::name::Name) -> Ty {
        self.push_diag(TyDiagnosticData::NonValueNameInExpr {
            kind: kind.to_string(),
            name: name.resolve(self.interner).to_owned(),
        });
        Ty::Error
    }

    fn constructor_as_fn(
        &mut self,
        type_idx: kyokara_hir_def::item_tree::TypeItemIdx,
        variant_idx: usize,
    ) -> Ty {
        let env = Self::make_env(
            self.item_tree,
            self.module_scope,
            self.interner,
            &self.type_params,
        );
        let (field_tys, adt_ty) =
            instantiate_constructor(type_idx, variant_idx, &env, &mut self.table);

        if field_tys.is_empty() {
            adt_ty
        } else {
            Ty::Fn {
                params: field_tys,
                ret: Box::new(adt_ty),
            }
        }
    }

    fn infer_binary(&mut self, op: BinaryOp, lhs: ExprIdx, rhs: ExprIdx) -> Ty {
        let lhs_ty = self.infer_expr(lhs, &Expectation::None);
        let rhs_ty = self.infer_expr(rhs, &Expectation::Has(lhs_ty.clone()));

        match op {
            BinaryOp::RangeUntil => {
                self.unify_or_err(&Ty::Int, &lhs_ty);
                self.unify_or_err(&Ty::Int, &rhs_ty);
                let Some(seq_info) = self.module_scope.core_types.get(CoreType::Seq) else {
                    return Ty::Error;
                };
                Ty::Adt {
                    def: seq_info.type_idx,
                    args: vec![Ty::Int],
                }
            }
            _ if op.is_numeric_arithmetic() => {
                let resolved = self.table.resolve_deep(&lhs_ty);
                if !resolved.is_poison() && !matches!(resolved, Ty::Int | Ty::Float | Ty::Var(_)) {
                    self.push_diag(TyDiagnosticData::InvalidArithmeticOperand {
                        ty: resolved.clone(),
                    });
                    return Ty::Error;
                }
                self.unify_or_err(&lhs_ty, &rhs_ty);
                lhs_ty
            }
            _ if op.is_equality() => {
                let resolved = self.table.resolve_deep(&lhs_ty);
                if !resolved.is_poison() && !resolved.is_equality_comparable() {
                    self.push_diag(TyDiagnosticData::InvalidEqualityOperand {
                        ty: resolved.clone(),
                    });
                    return Ty::Error;
                }
                self.unify_or_err(&lhs_ty, &rhs_ty);
                Ty::Bool
            }
            _ if op.is_ordering() => {
                let resolved = self.table.resolve_deep(&lhs_ty);
                if !resolved.is_poison() && !matches!(resolved, Ty::Int | Ty::Float | Ty::Var(_)) {
                    self.push_diag(TyDiagnosticData::InvalidComparisonOperand {
                        ty: resolved.clone(),
                    });
                    return Ty::Error;
                }
                self.unify_or_err(&lhs_ty, &rhs_ty);
                Ty::Bool
            }
            _ if op.is_logical() => {
                self.unify_or_err(&Ty::Bool, &lhs_ty);
                self.unify_or_err(&Ty::Bool, &rhs_ty);
                Ty::Bool
            }
            _ if op.is_bitwise_or_shift() => {
                let resolved = self.table.resolve_deep(&lhs_ty);
                if !resolved.is_poison() && !matches!(resolved, Ty::Int | Ty::Var(_)) {
                    self.push_diag(TyDiagnosticData::InvalidArithmeticOperand {
                        ty: resolved.clone(),
                    });
                    return Ty::Error;
                }
                self.unify_or_err(&lhs_ty, &rhs_ty);
                lhs_ty
            }
            _ => Ty::Error,
        }
    }

    fn infer_unary(&mut self, op: UnaryOp, operand: ExprIdx) -> Ty {
        let operand_ty = self.infer_expr(operand, &Expectation::None);
        match op {
            UnaryOp::Neg => {
                let resolved = self.table.resolve_deep(&operand_ty);
                if !resolved.is_poison() && !matches!(resolved, Ty::Int | Ty::Float | Ty::Var(_)) {
                    self.push_diag(TyDiagnosticData::InvalidNegationOperand { ty: resolved });
                    return Ty::Error;
                }
                operand_ty
            }
            UnaryOp::Not => {
                let resolved = self.table.resolve_deep(&operand_ty);
                if !resolved.is_poison() && !matches!(resolved, Ty::Bool | Ty::Var(_)) {
                    self.push_diag(TyDiagnosticData::InvalidNotOperand { ty: resolved });
                    return Ty::Error;
                }
                self.unify_or_err(&Ty::Bool, &operand_ty);
                Ty::Bool
            }
            UnaryOp::BitNot => {
                let resolved = self.table.resolve_deep(&operand_ty);
                if !resolved.is_poison() && !matches!(resolved, Ty::Int | Ty::Var(_)) {
                    self.push_diag(TyDiagnosticData::InvalidArithmeticOperand { ty: resolved });
                    return Ty::Error;
                }
                self.unify_or_err(&Ty::Int, &operand_ty);
                Ty::Int
            }
        }
    }

    /// Look up the parameter names for a callee expression by resolving it to
    /// a function definition. Returns `None` if the callee is not a simple
    /// function path or the definition cannot be found.
    fn callee_param_names(&self, callee: ExprIdx) -> Option<Vec<kyokara_hir_def::name::Name>> {
        let mut visited_locals = HashSet::new();
        self.param_names_for_expr(callee, &mut visited_locals)
    }

    fn param_names_for_expr(
        &self,
        expr_idx: ExprIdx,
        visited_locals: &mut HashSet<kyokara_hir_def::expr::PatIdx>,
    ) -> Option<Vec<kyokara_hir_def::name::Name>> {
        match &self.body.exprs[expr_idx] {
            Expr::Lambda { params, .. } => {
                let mut names = Vec::with_capacity(params.len());
                for (pat_idx, _) in params {
                    match &self.body.pats[*pat_idx] {
                        Pat::Bind { name } => names.push(*name),
                        _ => return None,
                    }
                }
                Some(names)
            }
            Expr::Path(path) => {
                if !path.is_single() {
                    return None;
                }

                let name = path.segments[0];
                let resolved = self
                    .body
                    .resolve_name_at(self.module_scope, expr_idx, name)
                    .map(|r| r.resolved)?;

                let fn_idx = match resolved {
                    ResolvedName::Fn(idx) | ResolvedName::Local(ScopeDef::Fn(idx)) => idx,
                    ResolvedName::Local(ScopeDef::Local(pat_idx)) => {
                        return self.local_binding_param_names(pat_idx, visited_locals);
                    }
                    _ => return None,
                };
                let fn_item = &self.item_tree.functions[fn_idx];
                Some(fn_item.params.iter().map(|p| p.name).collect())
            }
            _ => None,
        }
    }

    fn local_binding_param_names(
        &self,
        pat_idx: kyokara_hir_def::expr::PatIdx,
        visited_locals: &mut HashSet<kyokara_hir_def::expr::PatIdx>,
    ) -> Option<Vec<kyokara_hir_def::name::Name>> {
        if !visited_locals.insert(pat_idx) {
            return None;
        }
        let init = self.local_binding_init_expr(pat_idx)?;
        self.param_names_for_expr(init, visited_locals)
    }

    fn local_binding_init_expr(
        &self,
        local_pat: kyokara_hir_def::expr::PatIdx,
    ) -> Option<kyokara_hir_def::expr::ExprIdx> {
        for (_, expr) in self.body.exprs.iter() {
            if let Expr::Block { stmts, .. } = expr {
                for stmt in stmts {
                    if let Stmt::Let { pat, init, .. } = stmt
                        && *pat == local_pat
                    {
                        return Some(*init);
                    }
                }
            }
        }
        None
    }

    /// Infer call arguments in source order, while binding each argument to its
    /// target parameter slot for type expectations.
    ///
    /// Returns `true` if named-argument validation errors were emitted.
    fn infer_call_args_with_binding(
        &mut self,
        args: &[CallArg],
        param_tys: &[Ty],
        param_names: Option<&[kyokara_hir_def::name::Name]>,
    ) -> bool {
        let names = param_names.filter(|names| names.len() == param_tys.len());

        let mut has_errors = false;
        let mut seen = vec![false; param_tys.len()];
        let mut next_pos = 0;
        let mut saw_named = false;

        for arg in args {
            match arg {
                CallArg::Positional(arg_idx) => {
                    if saw_named {
                        has_errors = true;
                        self.push_diag(TyDiagnosticData::PositionalAfterNamedArg);
                    }

                    while next_pos < seen.len() && seen[next_pos] {
                        next_pos += 1;
                    }

                    let expectation = if next_pos < param_tys.len() {
                        seen[next_pos] = true;
                        Expectation::Has(param_tys[next_pos].clone())
                    } else {
                        has_errors = true;
                        Expectation::None
                    };

                    self.infer_expr(*arg_idx, &expectation);
                    next_pos += 1;
                }
                CallArg::Named { name, value } => {
                    saw_named = true;
                    let expectation = if let Some(names) = names {
                        if let Some(idx) = names.iter().position(|param_name| param_name == name) {
                            if seen[idx] {
                                has_errors = true;
                                self.push_diag(TyDiagnosticData::DuplicateNamedArg {
                                    name: name.resolve(self.interner).to_string(),
                                });
                            } else {
                                seen[idx] = true;
                            }
                            Expectation::Has(param_tys[idx].clone())
                        } else {
                            has_errors = true;
                            self.push_diag(TyDiagnosticData::UnknownNamedArg {
                                name: name.resolve(self.interner).to_string(),
                            });
                            Expectation::None
                        }
                    } else {
                        has_errors = true;
                        self.push_diag(TyDiagnosticData::UnknownNamedArg {
                            name: name.resolve(self.interner).to_string(),
                        });
                        Expectation::None
                    };
                    self.infer_expr(*value, &expectation);
                }
            }
        }

        if let Some(names) = names {
            for (idx, provided) in seen.iter().enumerate() {
                if !provided {
                    has_errors = true;
                    self.push_diag(TyDiagnosticData::MissingNamedArg {
                        name: names[idx].resolve(self.interner).to_string(),
                    });
                }
            }
        }

        has_errors
    }

    fn infer_call(&mut self, callee: ExprIdx, args: &[CallArg]) -> Ty {
        let field_callee = match &self.body.exprs[callee] {
            Expr::Field { base, field } => Some((*base, *field)),
            _ => None,
        };

        // ── Module-qualified and static method call resolution ───────
        // Before method call resolution, check if callee is `module.fn()`
        // or a type-owned static call (e.g., `io.println(s)`,
        // `collections.List.new()`).
        if let Some((base, field)) = field_callee
            && let Some(result) = self.try_infer_qualified_call(callee, base, field, args)
        {
            return result;
        }

        // ── Method call resolution ──────────────────────────────────
        // If the callee is `expr.field(args)`, try resolving as a method
        // call before falling through to normal field-access + call.
        if let Some((base, field)) = field_callee
            && let Some(result) = self.try_infer_method_call(callee, base, field, args)
        {
            return result;
        }

        let callee_ty = self.infer_expr(callee, &Expectation::None);
        let callee_ty = self.table.resolve_deep(&callee_ty);

        // Keep call-edge attribution and effect checking independent:
        // call edges feed symbol graph output, while effect checks emit
        // diagnostics. Reordering one should not alter the other.
        let call_target = self.call_target_for_attribution(callee);
        self.record_call_edge_if_top_level(call_target);
        self.check_call_effects_if_function(call_target);

        match callee_ty {
            Ty::Fn { params, ret } => {
                // Count all arguments (positional + named) for arity check.
                let total_arg_count = args.len();
                if total_arg_count != params.len() {
                    self.push_diag(TyDiagnosticData::ArgCountMismatch {
                        expected: params.len(),
                        actual: total_arg_count,
                    });
                    // Still infer args for completeness.
                    for arg in args {
                        match arg {
                            CallArg::Positional(e) | CallArg::Named { value: e, .. } => {
                                self.infer_expr(*e, &Expectation::None);
                            }
                        }
                    }
                    return Ty::Error;
                }

                // Resolve callee parameter names so named args can be matched
                // to the correct parameter type.
                let param_names = self.callee_param_names(callee);
                if self.infer_call_args_with_binding(args, &params, param_names.as_deref()) {
                    return Ty::Error;
                }

                *ret
            }
            Ty::Error | Ty::Never => {
                for arg in args {
                    match arg {
                        CallArg::Positional(e) | CallArg::Named { value: e, .. } => {
                            self.infer_expr(*e, &Expectation::None);
                        }
                    }
                }
                Ty::Error
            }
            Ty::Var(_) => {
                let mut saw_named = false;
                let mut param_tys = Vec::new();
                for arg in args {
                    match arg {
                        CallArg::Positional(e) | CallArg::Named { value: e, .. } => {
                            if matches!(arg, CallArg::Positional(_)) && saw_named {
                                self.push_diag(TyDiagnosticData::PositionalAfterNamedArg);
                            }
                            if matches!(arg, CallArg::Named { .. }) {
                                saw_named = true;
                            }
                            let ty = self.infer_expr(*e, &Expectation::None);
                            param_tys.push(ty);
                        }
                    }
                }
                let ret = self.table.fresh_var();
                let fn_ty = Ty::Fn {
                    params: param_tys,
                    ret: Box::new(ret.clone()),
                };
                self.table.unify(&callee_ty, &fn_ty);
                ret
            }
            _ => {
                self.push_diag(TyDiagnosticData::NotAFunction { ty: callee_ty });
                for arg in args {
                    match arg {
                        CallArg::Positional(e) | CallArg::Named { value: e, .. } => {
                            self.infer_expr(*e, &Expectation::None);
                        }
                    }
                }
                Ty::Error
            }
        }
    }

    fn call_target_for_attribution(&self, callee: ExprIdx) -> Option<kyokara_hir_def::name::Name> {
        let Expr::Path(path) = &self.body.exprs[callee] else {
            return None;
        };
        if !path.is_single() {
            return None;
        }

        let name = path.segments[0];
        match self.body.resolve_name_at(self.module_scope, callee, name) {
            Some(resolved) => match resolved.resolved {
                ResolvedName::Local(_) => None,
                _ => Some(name),
            },
            None => None,
        }
    }

    fn record_call_edge_if_top_level(&mut self, call_target: Option<kyokara_hir_def::name::Name>) {
        if let Some(name) = call_target {
            self.calls.push(name);
        }
    }

    fn check_call_effects_if_function(&mut self, call_target: Option<kyokara_hir_def::name::Name>) {
        let Some(name) = call_target else {
            return;
        };

        // Look up the function in scope.functions first, then fall back to intrinsic_fn_lookup
        // (intrinsics are no longer in scope.functions but still need effect checking).
        let fn_idx = self
            .module_scope
            .functions
            .get(&name)
            .or_else(|| self.module_scope.intrinsic_fn_lookup.get(&name))
            .copied();

        if let Some(fn_idx) = fn_idx {
            let fn_item = &self.item_tree.functions[fn_idx];
            let env = Self::make_env(
                self.item_tree,
                self.module_scope,
                self.interner,
                &self.type_params,
            );
            let callee_effects = effects::EffectSet::from_with_effects(
                &fn_item.with_effects,
                &env,
                &mut self.table,
                self.interner,
            );

            let missing = callee_effects.missing_from(&self.caller_effects);
            if !missing.is_empty() {
                let missing_names: Vec<String> = missing
                    .iter()
                    .map(|n| n.resolve(self.interner).to_owned())
                    .collect();
                self.push_diag(TyDiagnosticData::EffectViolation {
                    missing: missing_names,
                });
            }
        }
    }

    fn infer_field(&mut self, base: ExprIdx, field: kyokara_hir_def::name::Name) -> Ty {
        let base_ty = self.infer_expr(base, &Expectation::None);
        let base_ty = self.table.resolve_deep(&base_ty);

        match &base_ty {
            Ty::Record { fields } => {
                if let Some((_, ty)) = fields.iter().find(|(n, _)| *n == field) {
                    ty.clone()
                } else {
                    self.push_diag(TyDiagnosticData::NoSuchField {
                        field: field.resolve(self.interner).to_owned(),
                        ty: base_ty.clone(),
                    });
                    Ty::Error
                }
            }
            Ty::Adt { def, args } => {
                let type_item = &self.item_tree.types[*def];
                // Extract record fields from Record or Alias-to-Record kinds.
                let def_fields = match &type_item.kind {
                    TypeDefKind::Record { fields } => Some(fields),
                    TypeDefKind::Alias(kyokara_hir_def::type_ref::TypeRef::Record { fields }) => {
                        Some(fields)
                    }
                    _ => None,
                };
                if let Some(def_fields) = def_fields {
                    // Build substitution for type params.
                    let mut tp_map: Vec<(kyokara_hir_def::name::Name, Ty)> =
                        self.type_params.clone();
                    for (param_name, arg) in type_item.type_params.iter().zip(args.iter()) {
                        tp_map.push((param_name.name, arg.clone()));
                    }
                    let env = TyResolutionEnv {
                        item_tree: self.item_tree,
                        module_scope: self.module_scope,
                        interner: self.interner,
                        type_params: tp_map,
                        resolving_aliases: vec![],
                    };
                    for (n, type_ref) in def_fields {
                        if *n == field {
                            return env.resolve_type_ref(type_ref, &mut self.table);
                        }
                    }
                    self.push_diag(TyDiagnosticData::NoSuchField {
                        field: field.resolve(self.interner).to_owned(),
                        ty: base_ty.clone(),
                    });
                    Ty::Error
                } else {
                    self.push_diag(TyDiagnosticData::NoSuchField {
                        field: field.resolve(self.interner).to_owned(),
                        ty: base_ty.clone(),
                    });
                    Ty::Error
                }
            }
            Ty::Error | Ty::Never => Ty::Error,
            _ => {
                self.push_diag(TyDiagnosticData::NoSuchField {
                    field: field.resolve(self.interner).to_owned(),
                    ty: base_ty.clone(),
                });
                Ty::Error
            }
        }
    }

    fn infer_if(
        &mut self,
        condition: ExprIdx,
        then_branch: ExprIdx,
        else_branch: Option<ExprIdx>,
        expected: &Expectation,
    ) -> Ty {
        self.infer_expr(condition, &Expectation::Has(Ty::Bool));

        // Only propagate the expected type to then/else when both branches
        // exist, since if-without-else always returns Unit.
        let then_expectation = if else_branch.is_some() {
            expected.clone()
        } else {
            Expectation::None
        };
        let then_ty = self.infer_expr(then_branch, &then_expectation);

        if let Some(else_idx) = else_branch {
            let else_ty = self.infer_expr(else_idx, &Expectation::Has(then_ty.clone()));
            self.current_expr = Some(else_idx);
            self.unify_or_err(&then_ty, &else_ty);
            then_ty
        } else {
            Ty::Unit
        }
    }

    fn infer_match(
        &mut self,
        match_expr_idx: ExprIdx,
        scrutinee: ExprIdx,
        arms: &[kyokara_hir_def::expr::MatchArm],
        expected: &Expectation,
    ) -> Ty {
        let scrutinee_ty = self.infer_expr(scrutinee, &Expectation::None);

        let mut result_ty: Option<Ty> = expected.ty().cloned();

        for arm in arms {
            self.infer_pat(arm.pat, &scrutinee_ty);

            let arm_exp = result_ty
                .as_ref()
                .map(|t| Expectation::Has(t.clone()))
                .unwrap_or(Expectation::None);
            let arm_ty = self.infer_expr(arm.body, &arm_exp);

            if let Some(ref res) = result_ty {
                self.current_expr = Some(arm.body);
                self.unify_or_err(res, &arm_ty);
            } else {
                result_ty = Some(arm_ty);
            }
        }

        // Exhaustiveness check.
        let resolved_scrutinee = self.table.resolve_deep(&scrutinee_ty);
        if let Ty::Adt { def, .. } = &resolved_scrutinee {
            crate::exhaustiveness::check_exhaustiveness(
                crate::exhaustiveness::AdtExhaustivenessInput {
                    type_idx: *def,
                    arms,
                    pats: &self.body.pats,
                    pat_types: &self.pat_types,
                    table: &self.table,
                    item_tree: self.item_tree,
                    interner: self.interner,
                    match_expr_idx,
                },
                &mut self.diags,
            );
        } else {
            crate::exhaustiveness::check_non_adt_exhaustiveness(
                &resolved_scrutinee,
                arms,
                &self.body.pats,
                &mut self.diags,
                match_expr_idx,
            );
        }

        result_ty.unwrap_or(Ty::Unit)
    }

    fn infer_block(&mut self, stmts: &[Stmt], tail: Option<ExprIdx>, expected: &Expectation) -> Ty {
        for stmt in stmts {
            match stmt {
                Stmt::Let { pat, ty, init, .. } => {
                    let init_ty = if let Some(ty_ref) = ty {
                        let env = Self::make_env(
                            self.item_tree,
                            self.module_scope,
                            self.interner,
                            &self.type_params,
                        );
                        let annotation = env.resolve_type_ref(ty_ref, &mut self.table);
                        let init_ty = self.infer_expr(*init, &Expectation::Has(annotation.clone()));
                        // Attribute mismatch diagnostic to the init expression.
                        self.current_expr = Some(*init);
                        self.unify_or_err(&annotation, &init_ty);
                        annotation
                    } else {
                        self.infer_expr(*init, &Expectation::None)
                    };

                    self.infer_pat(*pat, &init_ty);
                    if !self.is_irrefutable_let_pattern(*pat, &init_ty) {
                        self.current_expr = Some(*init);
                        self.push_diag(TyDiagnosticData::RefutableLetPattern);
                    }
                }
                Stmt::Assign { target, value } => {
                    let target_ty = self.infer_expr(*target, &Expectation::None);
                    let target_info = self.assignment_target_info(*target);
                    match target_info {
                        AssignmentTargetInfo::MutableLocal => {
                            let value_ty =
                                self.infer_expr(*value, &Expectation::Has(target_ty.clone()));
                            self.current_expr = Some(*value);
                            self.unify_or_err(&target_ty, &value_ty);
                        }
                        AssignmentTargetInfo::Immutable(name) => {
                            self.current_expr = Some(*target);
                            self.push_diag(TyDiagnosticData::ImmutableAssignment { name });
                            self.infer_expr(*value, &Expectation::None);
                        }
                        AssignmentTargetInfo::Invalid => {
                            self.current_expr = Some(*target);
                            self.push_diag(TyDiagnosticData::InvalidAssignmentTarget);
                            self.infer_expr(*value, &Expectation::None);
                        }
                    }
                }
                Stmt::Expr(e) => {
                    self.infer_expr(*e, &Expectation::None);
                }
                Stmt::While { condition, body } => {
                    self.infer_expr(*condition, &Expectation::Has(Ty::Bool));
                    self.with_loop_scope(|ctx| {
                        ctx.infer_expr(*body, &Expectation::None);
                    });
                }
                Stmt::For { pat, source, body } => {
                    let source_ty = self.infer_expr(*source, &Expectation::None);
                    let source_ty_resolved = self.table.resolve_deep(&source_ty);
                    let elem_ty = self
                        .traversable_element_type(&source_ty_resolved)
                        .unwrap_or_else(|| {
                            self.push_diag(TyDiagnosticData::ForSourceNotTraversable {
                                ty: source_ty_resolved.clone(),
                            });
                            Ty::Error
                        });

                    self.infer_pat(*pat, &elem_ty);
                    if !self.is_irrefutable_let_pattern(*pat, &elem_ty) {
                        self.push_pat_diag(*pat, TyDiagnosticData::RefutableForPattern);
                    }

                    self.with_loop_scope(|ctx| {
                        ctx.infer_expr(*body, &Expectation::None);
                    });
                }
                Stmt::Break => {
                    if !self.in_loop() {
                        self.push_diag(TyDiagnosticData::BreakOutsideLoop);
                    }
                }
                Stmt::Continue => {
                    if !self.in_loop() {
                        self.push_diag(TyDiagnosticData::ContinueOutsideLoop);
                    }
                }
            }
        }

        if let Some(tail_idx) = tail {
            self.infer_expr(tail_idx, expected)
        } else {
            Ty::Unit
        }
    }

    fn is_irrefutable_let_pattern(&mut self, pat_idx: la_arena::Idx<Pat>, expected: &Ty) -> bool {
        let expected = self.table.resolve_deep(expected);
        if expected.is_poison() {
            return true;
        }

        match &self.body.pats[pat_idx] {
            Pat::Missing | Pat::Wildcard | Pat::Bind { .. } => true,
            Pat::Literal(_) => false,
            Pat::Record { fields, .. } => match expected {
                Ty::Record {
                    fields: ref rec_fields,
                } => fields
                    .iter()
                    .all(|f| rec_fields.iter().any(|(name, _)| name == f)),
                Ty::Adt { def, .. } => {
                    let type_item = &self.item_tree.types[def];
                    let def_fields = match &type_item.kind {
                        TypeDefKind::Record { fields: f } => Some(f),
                        TypeDefKind::Alias(kyokara_hir_def::type_ref::TypeRef::Record {
                            fields: f,
                        }) => Some(f),
                        _ => None,
                    };
                    if let Some(def_fields) = def_fields {
                        fields
                            .iter()
                            .all(|f| def_fields.iter().any(|(name, _)| name == f))
                    } else {
                        false
                    }
                }
                _ => false,
            },
            Pat::Constructor { path, args } => {
                if !path.is_single() {
                    return false;
                }
                let ctor = path.segments[0];
                let Some(&(type_idx, variant_idx)) = self.module_scope.constructors.get(&ctor)
                else {
                    return false;
                };

                let Ty::Adt { def, .. } = expected else {
                    return false;
                };
                if def != type_idx {
                    return false;
                }

                let TypeDefKind::Adt { variants } = &self.item_tree.types[type_idx].kind else {
                    return false;
                };
                // Constructor patterns are irrefutable only for single-variant ADTs.
                if variants.len() != 1 {
                    return false;
                }

                let env = Self::make_env(
                    self.item_tree,
                    self.module_scope,
                    self.interner,
                    &self.type_params,
                );
                let (field_tys, _adt_ty) =
                    instantiate_constructor(type_idx, variant_idx, &env, &mut self.table);

                let args = args.clone();
                if args.len() != field_tys.len() {
                    return false;
                }

                args.iter()
                    .zip(field_tys.iter())
                    .all(|(sub_pat, field_ty)| self.is_irrefutable_let_pattern(*sub_pat, field_ty))
            }
        }
    }

    fn traversable_element_type(&self, source_ty: &Ty) -> Option<Ty> {
        let Ty::Adt { def, args } = source_ty else {
            return None;
        };
        let core = self.module_scope.core_types.kind_for_idx(*def)?;
        if !matches!(
            core,
            CoreType::Seq | CoreType::List | CoreType::MutableList | CoreType::Deque
        ) {
            return None;
        }
        args.first().cloned()
    }

    fn infer_record_lit(
        &mut self,
        path: Option<&kyokara_hir_def::path::Path>,
        fields: &[(kyokara_hir_def::name::Name, ExprIdx)],
    ) -> Ty {
        if let Some(path) = path
            && path.is_single()
        {
            let name = path.segments[0];
            if let Some(&type_idx) = self.module_scope.types.get(&name) {
                let type_item = &self.item_tree.types[type_idx];
                // Extract record fields from either Record kind or Alias to Record.
                let def_fields = match &type_item.kind {
                    TypeDefKind::Record { fields: def_fields } => Some(def_fields),
                    TypeDefKind::Alias(kyokara_hir_def::type_ref::TypeRef::Record {
                        fields: def_fields,
                    }) => Some(def_fields),
                    _ => None,
                };
                if def_fields.is_none() {
                    // Path resolves to a type that isn't record-shaped.
                    self.push_diag(TyDiagnosticData::NotARecordType {
                        name: name.resolve(self.interner).to_owned(),
                    });
                    for (_fname, fexpr) in fields {
                        self.infer_expr(*fexpr, &Expectation::None);
                    }
                    return Ty::Error;
                }
                if let Some(def_fields) = def_fields {
                    // Build substitution env.
                    let mut tp = self.type_params.clone();
                    let mut args = Vec::new();
                    for tparam in &type_item.type_params {
                        let var = self.table.fresh_var();
                        tp.push((tparam.name, var.clone()));
                        args.push(var);
                    }
                    let env = TyResolutionEnv {
                        item_tree: self.item_tree,
                        module_scope: self.module_scope,
                        interner: self.interner,
                        type_params: tp,
                        resolving_aliases: vec![],
                    };

                    // Resolve expected types for each field, then infer.
                    let expected_field_tys: Vec<_> = def_fields
                        .iter()
                        .map(|(n, tr)| (*n, env.resolve_type_ref(tr, &mut self.table)))
                        .collect();

                    // Always produce Ty::Adt when the type was resolved from
                    // the module scope, even for alias-to-record types.
                    // This ensures method resolution can find the type name.
                    let result_ty = Ty::Adt {
                        def: type_idx,
                        args: args.clone(),
                    };

                    for (fname, fexpr) in fields {
                        let exp = expected_field_tys
                            .iter()
                            .find(|(n, _)| n == fname)
                            .map(|(_, ty)| ty.clone());
                        if let Some(exp_ty) = exp {
                            self.infer_expr(*fexpr, &Expectation::Has(exp_ty));
                        } else {
                            self.push_diag(TyDiagnosticData::NoSuchField {
                                field: fname.resolve(self.interner).to_owned(),
                                ty: result_ty.clone(),
                            });
                            self.infer_expr(*fexpr, &Expectation::None);
                        }
                    }

                    return result_ty;
                }
            }
        }

        // Structural record type.
        let fields: Vec<(kyokara_hir_def::name::Name, Ty)> = fields
            .iter()
            .map(|(n, e)| {
                let ty = self.infer_expr(*e, &Expectation::None);
                (*n, ty)
            })
            .collect();
        Ty::Record { fields }
    }

    fn infer_lambda(
        &mut self,
        params: &[(
            la_arena::Idx<Pat>,
            Option<kyokara_hir_def::type_ref::TypeRef>,
        )],
        body_expr: ExprIdx,
        expected: &Expectation,
    ) -> Ty {
        let expected_fn = expected.ty().and_then(|t| {
            let resolved = self.table.resolve_deep(t);
            match resolved {
                Ty::Fn { params, ret } => Some((params, *ret)),
                _ => None,
            }
        });

        let mut param_tys = Vec::new();
        for (i, (pat_idx, ty_ref)) in params.iter().enumerate() {
            let ty = if let Some(tr) = ty_ref {
                let env = Self::make_env(
                    self.item_tree,
                    self.module_scope,
                    self.interner,
                    &self.type_params,
                );
                env.resolve_type_ref(tr, &mut self.table)
            } else if let Some((ref exp_params, _)) = expected_fn {
                if i < exp_params.len() {
                    exp_params[i].clone()
                } else {
                    self.table.fresh_var()
                }
            } else {
                self.table.fresh_var()
            };
            param_tys.push(ty.clone());
            self.infer_pat(*pat_idx, &ty);
        }

        self.reject_captured_mutable_locals(body_expr);

        let expected_ret = expected_fn.map(|(_, r)| Expectation::Has(r));
        let body_ty = self.infer_expr(body_expr, &expected_ret.unwrap_or(Expectation::None));

        Ty::Fn {
            params: param_tys,
            ret: Box::new(body_ty),
        }
    }

    fn assignment_target_info(&self, target: ExprIdx) -> AssignmentTargetInfo {
        let Expr::Path(path) = &self.body.exprs[target] else {
            return AssignmentTargetInfo::Invalid;
        };
        if !path.is_single() {
            return AssignmentTargetInfo::Invalid;
        }
        let name = path.segments[0];
        let Some(resolved) = self.body.resolve_name_at(self.module_scope, target, name) else {
            return AssignmentTargetInfo::Invalid;
        };
        match resolved.resolved {
            ResolvedName::Local(ScopeDef::Local(pat_idx)) => {
                let Some(meta) = self.body.local_binding_meta.get(pat_idx) else {
                    return AssignmentTargetInfo::Invalid;
                };
                if meta.mutable {
                    AssignmentTargetInfo::MutableLocal
                } else {
                    AssignmentTargetInfo::Immutable(name.resolve(self.interner).to_string())
                }
            }
            ResolvedName::Local(ScopeDef::Param(_))
            | ResolvedName::Local(ScopeDef::LambdaParam(_)) => {
                AssignmentTargetInfo::Immutable(name.resolve(self.interner).to_string())
            }
            _ => AssignmentTargetInfo::Invalid,
        }
    }

    fn reject_captured_mutable_locals(&mut self, body_expr: ExprIdx) {
        let Some(lambda_scope) = self.body.expr_scopes.get(body_expr).copied() else {
            return;
        };
        self.reject_captured_mutable_locals_in_expr(body_expr, lambda_scope);
    }

    fn reject_captured_mutable_locals_in_expr(
        &mut self,
        expr_idx: ExprIdx,
        lambda_scope: kyokara_hir_def::scope::ScopeIdx,
    ) {
        match &self.body.exprs[expr_idx] {
            Expr::Path(path) if path.is_single() => {
                let name = path.segments[0];
                if let Some(resolved) = self.body.resolve_name_at(self.module_scope, expr_idx, name)
                    && let Some((_, meta)) = resolved.local_binding
                    && meta.mutable
                    && !self.scope_is_within(meta.scope, lambda_scope)
                {
                    let prev = self.current_expr;
                    self.current_expr = Some(expr_idx);
                    self.push_diag(TyDiagnosticData::CapturedMutableLocal {
                        name: name.resolve(self.interner).to_string(),
                    });
                    self.current_expr = prev;
                }
            }
            Expr::Binary { lhs, rhs, .. } => {
                self.reject_captured_mutable_locals_in_expr(*lhs, lambda_scope);
                self.reject_captured_mutable_locals_in_expr(*rhs, lambda_scope);
            }
            Expr::Unary { operand, .. } | Expr::Old(operand) => {
                self.reject_captured_mutable_locals_in_expr(*operand, lambda_scope);
            }
            Expr::Call { callee, args } => {
                self.reject_captured_mutable_locals_in_expr(*callee, lambda_scope);
                for arg in args {
                    match arg {
                        CallArg::Positional(idx) => {
                            self.reject_captured_mutable_locals_in_expr(*idx, lambda_scope);
                        }
                        CallArg::Named { value, .. } => {
                            self.reject_captured_mutable_locals_in_expr(*value, lambda_scope);
                        }
                    }
                }
            }
            Expr::Field { base, .. } => {
                self.reject_captured_mutable_locals_in_expr(*base, lambda_scope);
            }
            Expr::Index { base, index } => {
                self.reject_captured_mutable_locals_in_expr(*base, lambda_scope);
                self.reject_captured_mutable_locals_in_expr(*index, lambda_scope);
            }
            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.reject_captured_mutable_locals_in_expr(*condition, lambda_scope);
                self.reject_captured_mutable_locals_in_expr(*then_branch, lambda_scope);
                if let Some(else_branch) = else_branch {
                    self.reject_captured_mutable_locals_in_expr(*else_branch, lambda_scope);
                }
            }
            Expr::Match { scrutinee, arms } => {
                self.reject_captured_mutable_locals_in_expr(*scrutinee, lambda_scope);
                for arm in arms {
                    self.reject_captured_mutable_locals_in_expr(arm.body, lambda_scope);
                }
            }
            Expr::Block { stmts, tail } => {
                for stmt in stmts {
                    match stmt {
                        Stmt::Let { init, .. } => {
                            self.reject_captured_mutable_locals_in_expr(*init, lambda_scope);
                        }
                        Stmt::Assign { target, value } => {
                            self.reject_captured_mutable_locals_in_expr(*target, lambda_scope);
                            self.reject_captured_mutable_locals_in_expr(*value, lambda_scope);
                        }
                        Stmt::While { condition, body } => {
                            self.reject_captured_mutable_locals_in_expr(*condition, lambda_scope);
                            self.reject_captured_mutable_locals_in_expr(*body, lambda_scope);
                        }
                        Stmt::For { source, body, .. } => {
                            self.reject_captured_mutable_locals_in_expr(*source, lambda_scope);
                            self.reject_captured_mutable_locals_in_expr(*body, lambda_scope);
                        }
                        Stmt::Break | Stmt::Continue => {}
                        Stmt::Expr(expr) => {
                            self.reject_captured_mutable_locals_in_expr(*expr, lambda_scope);
                        }
                    }
                }
                if let Some(tail) = tail {
                    self.reject_captured_mutable_locals_in_expr(*tail, lambda_scope);
                }
            }
            Expr::Return(value) => {
                if let Some(value) = value {
                    self.reject_captured_mutable_locals_in_expr(*value, lambda_scope);
                }
            }
            Expr::RecordLit { fields, .. } => {
                for (_, value) in fields {
                    self.reject_captured_mutable_locals_in_expr(*value, lambda_scope);
                }
            }
            Expr::Lambda { .. } | Expr::Missing | Expr::Hole | Expr::Literal(_) | Expr::Path(_) => {
            }
        }
    }

    fn scope_is_within(
        &self,
        mut scope: kyokara_hir_def::scope::ScopeIdx,
        ancestor: kyokara_hir_def::scope::ScopeIdx,
    ) -> bool {
        loop {
            if scope == ancestor {
                return true;
            }
            let Some(parent) = self.body.scopes.scopes[scope].parent else {
                return false;
            };
            scope = parent;
        }
    }

    fn collect_locals_in_scope(&self, expr_idx: ExprIdx) -> Vec<(kyokara_hir_def::name::Name, Ty)> {
        // Use an ordered map to deduplicate by name (last binding per name wins).
        let mut seen = rustc_hash::FxHashMap::default();
        let mut order = Vec::new();

        // Include function parameters.
        for (name, ty) in self.param_names.iter().zip(self.param_types.iter()) {
            if seen.insert(*name, self.table.resolve_deep(ty)).is_none() {
                order.push(*name);
            } else {
                // Update existing entry with new type (shadowing).
                seen.insert(*name, self.table.resolve_deep(ty));
            }
        }
        // Include let-bound locals.
        for (pat_idx, _) in &self.body.pat_scopes {
            if let Pat::Bind { name } = &self.body.pats[*pat_idx]
                && let Some(ty) = self.local_types.get(*pat_idx)
            {
                let is_visible_here = matches!(
                    self.body
                        .resolve_name_at(self.module_scope, expr_idx, *name)
                        .map(|resolved| resolved.resolved),
                    Some(ResolvedName::Local(ScopeDef::Local(visible_pat_idx)))
                        if visible_pat_idx == *pat_idx
                );
                if !is_visible_here {
                    continue;
                }

                let resolved = self.table.resolve_deep(ty);
                if seen.insert(*name, resolved.clone()).is_none() {
                    order.push(*name);
                } else {
                    seen.insert(*name, resolved);
                }
            }
        }

        order
            .into_iter()
            .filter_map(|name| seen.remove(&name).map(|ty| (name, ty)))
            .collect()
    }

    /// Map a resolved type to its dispatch identity for method lookup.
    fn receiver_key_for_ty(&self, ty: &Ty) -> Option<ReceiverKey> {
        match ty {
            Ty::String => Some(ReceiverKey::Primitive(PrimitiveType::String)),
            Ty::Int => Some(ReceiverKey::Primitive(PrimitiveType::Int)),
            Ty::Float => Some(ReceiverKey::Primitive(PrimitiveType::Float)),
            Ty::Bool => Some(ReceiverKey::Primitive(PrimitiveType::Bool)),
            Ty::Char => Some(ReceiverKey::Primitive(PrimitiveType::Char)),
            Ty::Adt { def, .. } => Some(
                self.module_scope
                    .core_types
                    .kind_for_idx(*def)
                    .map(ReceiverKey::Core)
                    .unwrap_or(ReceiverKey::User(*def)),
            ),
            _ => None,
        }
    }

    fn static_owner_key_for_type_idx(
        &self,
        type_idx: kyokara_hir_def::item_tree::TypeItemIdx,
    ) -> StaticOwnerKey {
        self.module_scope
            .core_types
            .kind_for_idx(type_idx)
            .map(StaticOwnerKey::Core)
            .unwrap_or(StaticOwnerKey::User(type_idx))
    }

    /// Try to resolve `base.field(args)` as a module-qualified or static method call.
    ///
    /// Handles patterns like `io.println(s)`, `math.min(a, b)`,
    /// `collections.List.new()`.
    /// Returns `Some(return_type)` if resolved, `None` to fall through.
    fn try_infer_qualified_call(
        &mut self,
        callee: ExprIdx,
        base: ExprIdx,
        field: kyokara_hir_def::name::Name,
        args: &[CallArg],
    ) -> Option<Ty> {
        // Nested module static call: collections.Deque.new()
        if let Expr::Field {
            base: module_base,
            field: type_name,
        } = &self.body.exprs[base]
            && let Expr::Path(ref module_path) = self.body.exprs[*module_base]
            && module_path.is_single()
        {
            let module_name = module_path.segments[0];
            if self.module_scope.imported_modules.contains(&module_name) {
                if let Some(&fn_idx) = self.module_scope.synthetic_module_static_methods.get(&(
                    module_name,
                    *type_name,
                    field,
                )) {
                    return Some(self.infer_qualified_fn_call(callee, fn_idx, args));
                }
                let module_str = module_name.resolve(self.interner);
                let type_str = type_name.resolve(self.interner);
                let method_str = field.resolve(self.interner);
                self.push_diag(TyDiagnosticData::NoSuchMethod {
                    method: format!("{module_str}.{type_str}.{method_str}"),
                    ty: Ty::Error,
                });
                for arg in args {
                    match arg {
                        CallArg::Positional(e) | CallArg::Named { value: e, .. } => {
                            self.infer_expr(*e, &Expectation::None);
                        }
                    }
                }
                return Some(Ty::Error);
            }

            return None;
        }

        let Expr::Path(ref path) = self.body.exprs[base] else {
            return None;
        };
        if !path.is_single() {
            return None;
        }
        let name = path.segments[0];

        // Module-qualified call: io.println(s), math.min(a, b), fs.read_file(path)
        // Only resolves if the module has been explicitly imported.
        if self.module_scope.imported_modules.contains(&name)
            && let Some(mod_fns) = self.module_scope.synthetic_modules.get(&name)
        {
            if let Some(&fn_idx) = mod_fns.get(&field) {
                return Some(self.infer_qualified_fn_call(callee, fn_idx, args));
            }
            // Module exists but function not found — emit diagnostic.
            let mod_str = name.resolve(self.interner);
            let fn_str = field.resolve(self.interner);
            self.push_diag(TyDiagnosticData::NoSuchMethod {
                method: format!("{mod_str}.{fn_str}"),
                ty: Ty::Error,
            });
            for arg in args {
                match arg {
                    CallArg::Positional(e) | CallArg::Named { value: e, .. } => {
                        self.infer_expr(*e, &Expectation::None);
                    }
                }
            }
            return Some(Ty::Error);
        }

        // Trait-qualified call: Ord.compare(a, b), Show.show(x)
        if let Some(&trait_idx) = self.module_scope.traits.get(&name) {
            let trait_item = &self.item_tree.traits[trait_idx];
            if let Some(method) = trait_item.methods.iter().find(|method| method.name == field) {
                let Some(first_arg) = args.first() else {
                    self.push_diag(TyDiagnosticData::ArgCountMismatch {
                        expected: method.params.len(),
                        actual: 0,
                    });
                    return Some(Ty::Error);
                };

                let first_expr = match first_arg {
                    CallArg::Positional(expr) | CallArg::Named { value: expr, .. } => *expr,
                };
                let recv_ty = self.infer_expr(first_expr, &Expectation::None);
                let recv_ty = self.table.resolve_deep(&recv_ty);
                if !self.ty_satisfies_trait_name(&recv_ty, name) {
                    self.push_diag(TyDiagnosticData::MissingTraitImpl {
                        trait_name: name.resolve(self.interner).to_owned(),
                        ty: recv_ty,
                    });
                    for arg in &args[1..] {
                        match arg {
                            CallArg::Positional(expr) | CallArg::Named { value: expr, .. } => {
                                self.infer_expr(*expr, &Expectation::None);
                            }
                        }
                    }
                    return Some(Ty::Error);
                }

                let env = Self::make_env(
                    self.item_tree,
                    self.module_scope,
                    self.interner,
                    &self.type_params,
                );
                let param_tys = method
                    .params
                    .iter()
                    .map(|param| self.resolve_trait_method_type(&param.ty, &recv_ty, &env))
                    .collect::<Vec<_>>();
                let param_names = method.params.iter().map(|param| param.name).collect::<Vec<_>>();
                let has_arg_errors =
                    self.infer_call_args_with_binding(args, &param_tys, Some(&param_names));
                if has_arg_errors {
                    return Some(Ty::Error);
                }
                let ret = method
                    .ret_type
                    .as_ref()
                    .map(|ret| self.resolve_trait_method_type(ret, &recv_ty, &env))
                    .unwrap_or(Ty::Unit);
                return Some(ret);
            }

            self.push_diag(TyDiagnosticData::NoSuchMethod {
                method: format!(
                    "{}.{}",
                    name.resolve(self.interner),
                    field.resolve(self.interner)
                ),
                ty: Ty::Error,
            });
            for arg in args {
                match arg {
                    CallArg::Positional(expr) | CallArg::Named { value: expr, .. } => {
                        self.infer_expr(*expr, &Expectation::None);
                    }
                }
            }
            return Some(Ty::Error);
        }

        // Type-owned static call: bare `Type.method()` if registered.
        if let Some(&type_idx) = self.module_scope.types.get(&name) {
            let owner_key = self.static_owner_key_for_type_idx(type_idx);
            if let Some(&fn_idx) = self.module_scope.static_methods.get(&(owner_key, field)) {
                return Some(self.infer_qualified_fn_call(callee, fn_idx, args));
            }
            // Type exists but static method not found — emit deterministic diagnostic
            // instead of falling through to "type name used as value".
            self.push_diag(TyDiagnosticData::NoSuchMethod {
                method: field.resolve(self.interner).to_string(),
                ty: Ty::Adt {
                    def: type_idx,
                    args: Vec::new(),
                },
            });
            for arg in args {
                match arg {
                    CallArg::Positional(e) | CallArg::Named { value: e, .. } => {
                        self.infer_expr(*e, &Expectation::None);
                    }
                }
            }
            return Some(Ty::Error);
        }

        None
    }

    fn resolve_trait_method_type(
        &mut self,
        ty_ref: &kyokara_hir_def::type_ref::TypeRef,
        recv_ty: &Ty,
        env: &TyResolutionEnv<'_>,
    ) -> Ty {
        match ty_ref {
            kyokara_hir_def::type_ref::TypeRef::Path { path, args } if path.is_single() && args.is_empty() => {
                let seg = path.segments[0];
                if seg.resolve(self.interner) == "Self" {
                    recv_ty.clone()
                } else {
                    env.resolve_type_ref(ty_ref, &mut self.table)
                }
            }
            _ => env.resolve_type_ref(ty_ref, &mut self.table),
        }
    }

    /// Infer a call to a resolved FnItemIdx (no receiver). Used for module-qualified
    /// and static method calls.
    fn infer_qualified_fn_call(
        &mut self,
        callee: ExprIdx,
        fn_idx: kyokara_hir_def::item_tree::FnItemIdx,
        args: &[CallArg],
    ) -> Ty {
        let env = Self::make_env(
            self.item_tree,
            self.module_scope,
            self.interner,
            &self.type_params,
        );
        let (params, ret) = instantiate_fn_sig(fn_idx, &env, &mut self.table);

        // Record the callee type.
        let fn_ty = Ty::Fn {
            params: params.clone(),
            ret: Box::new(ret.clone()),
        };
        self.expr_types.insert(callee, fn_ty);

        // Check arity.
        if args.len() != params.len() {
            self.push_diag(TyDiagnosticData::ArgCountMismatch {
                expected: params.len(),
                actual: args.len(),
            });
            for arg in args {
                match arg {
                    CallArg::Positional(e) | CallArg::Named { value: e, .. } => {
                        self.infer_expr(*e, &Expectation::None);
                    }
                }
            }
            return Ty::Error;
        }

        let param_names: Vec<_> = self.item_tree.functions[fn_idx]
            .params
            .iter()
            .map(|p| p.name)
            .collect();

        if self.infer_call_args_with_binding(args, &params, Some(&param_names)) {
            return Ty::Error;
        }

        // Effect checking: look up the function's required capabilities.
        let method_name = self.item_tree.functions[fn_idx].name;
        self.record_call_edge_if_top_level(Some(method_name));
        self.check_call_effects_if_function(Some(method_name));

        ret
    }

    fn is_traversal_intrinsic_name(name: &str) -> bool {
        matches!(
            name,
            "seq_map"
                | "seq_filter"
                | "seq_fold"
                | "seq_scan"
                | "seq_enumerate"
                | "seq_zip"
                | "seq_chunks"
                | "seq_windows"
                | "seq_count"
                | "seq_count_by"
                | "seq_frequencies"
                | "seq_any"
                | "seq_all"
                | "seq_find"
                | "seq_to_list"
        )
    }

    fn method_candidates_for_name(
        &self,
        base_ty: &Ty,
        field: kyokara_hir_def::name::Name,
    ) -> Option<&[FnItemIdx]> {
        self.receiver_key_for_ty(base_ty)
            .and_then(|receiver_key| {
                self.module_scope
                    .methods
                    .get(&(receiver_key, field))
                    .map(Vec::as_slice)
            })
            .or_else(|| {
                self.module_scope
                    .methods
                    .get(&(ReceiverKey::Any, field))
                    .map(Vec::as_slice)
            })
    }

    fn select_method_candidate_by_arity(
        &self,
        candidates: &[FnItemIdx],
        actual_arg_count: usize,
    ) -> Result<FnItemIdx, Vec<usize>> {
        if let Some(&fn_idx) = candidates.iter().find(|&&fn_idx| {
            self.item_tree.functions[fn_idx]
                .params
                .len()
                .saturating_sub(1)
                == actual_arg_count
        }) {
            return Ok(fn_idx);
        }

        let mut expected: Vec<usize> = candidates
            .iter()
            .map(|&fn_idx| {
                self.item_tree.functions[fn_idx]
                    .params
                    .len()
                    .saturating_sub(1)
            })
            .collect();
        expected.sort_unstable();
        expected.dedup();
        Err(expected)
    }

    /// Try to resolve `base.field(args)` as a method call.
    ///
    /// Returns `Some(return_type)` if a method was found, `None` if the caller
    /// should fall through to normal field-access + call semantics.
    fn try_infer_method_call(
        &mut self,
        callee: ExprIdx,
        base: ExprIdx,
        field: kyokara_hir_def::name::Name,
        args: &[CallArg],
    ) -> Option<Ty> {
        // Infer the receiver type.
        let base_ty = self.infer_expr(base, &Expectation::None);
        let base_ty_resolved = self.table.resolve_deep(&base_ty);

        // Skip method resolution for record/adt types that have actual fields,
        // to let field access + call work for callable record fields.
        match &base_ty_resolved {
            Ty::Record { fields } => {
                if fields.iter().any(|(n, _)| *n == field) {
                    return None; // actual field exists, fall through
                }
            }
            Ty::Adt { def, .. } => {
                let type_item = &self.item_tree.types[*def];
                let def_fields = match &type_item.kind {
                    TypeDefKind::Record { fields } => Some(fields.as_slice()),
                    TypeDefKind::Alias(kyokara_hir_def::type_ref::TypeRef::Record { fields }) => {
                        Some(fields.as_slice())
                    }
                    _ => None,
                };
                if let Some(def_fields) = def_fields
                    && def_fields.iter().any(|(n, _)| *n == field)
                {
                    return None; // actual field exists, fall through
                }
            }
            _ => {}
        }

        // Look up method in the registry: exact receiver method first, Any fallback second.
        let candidates = match self.method_candidates_for_name(&base_ty_resolved, field) {
            Some(candidates) => candidates,
            None => {
                // Type has a name but no such method exists — emit diagnostic.
                self.push_diag(TyDiagnosticData::NoSuchMethod {
                    method: field.resolve(self.interner).to_owned(),
                    ty: base_ty_resolved.clone(),
                });
                // Record the callee expr type as Error so the caller doesn't
                // re-emit a "no field" diagnostic.
                self.expr_types.insert(callee, Ty::Error);
                // Infer args for completeness.
                for arg in args {
                    match arg {
                        CallArg::Positional(e) | CallArg::Named { value: e, .. } => {
                            self.infer_expr(*e, &Expectation::None);
                        }
                    }
                }
                return Some(Ty::Error);
            }
        };
        let fn_idx = match self.select_method_candidate_by_arity(candidates, args.len()) {
            Ok(fn_idx) => fn_idx,
            Err(expected) => {
                self.push_diag(TyDiagnosticData::ArgCountMismatchOneOf {
                    expected,
                    actual: args.len(),
                });
                for arg in args {
                    match arg {
                        CallArg::Positional(e) | CallArg::Named { value: e, .. } => {
                            self.infer_expr(*e, &Expectation::None);
                        }
                    }
                }
                self.expr_types.insert(callee, Ty::Error);
                return Some(Ty::Error);
            }
        };

        // Instantiate the method's function signature with fresh type variables.
        let env = Self::make_env(
            self.item_tree,
            self.module_scope,
            self.interner,
            &self.type_params,
        );
        let (params, ret) = instantiate_fn_sig(fn_idx, &env, &mut self.table);
        let method_name = self.item_tree.functions[fn_idx].name;
        let method_intrinsic_str = method_name.resolve(self.interner);
        let traversal_intrinsic = Self::is_traversal_intrinsic_name(method_intrinsic_str);

        // The method's first parameter is the receiver (`self`).
        // Unify receiver type with first param type.
        if params.is_empty() {
            return None; // degenerate case: method with no params
        }
        let receiver_unified = if traversal_intrinsic {
            self.with_traversal_seq_compat_scope(|ctx| {
                !matches!(ctx.unify_or_err(&params[0], &base_ty), Ty::Error)
            })
        } else {
            !matches!(self.unify_or_err(&params[0], &base_ty), Ty::Error)
        };
        if !receiver_unified {
            for arg in args {
                match arg {
                    CallArg::Positional(e) | CallArg::Named { value: e, .. } => {
                        self.infer_expr(*e, &Expectation::None);
                    }
                }
            }
            return Some(Ty::Error);
        }

        // Record the callee expression type as the method's function type.
        let fn_ty = Ty::Fn {
            params: params.clone(),
            ret: Box::new(ret.clone()),
        };
        self.expr_types.insert(callee, fn_ty);

        // Check arity: caller provides args for params[1..] (receiver is implicit).
        let expected_arg_count = params.len() - 1;
        if args.len() != expected_arg_count {
            self.push_diag(TyDiagnosticData::ArgCountMismatch {
                expected: expected_arg_count,
                actual: args.len(),
            });
            for arg in args {
                match arg {
                    CallArg::Positional(e) | CallArg::Named { value: e, .. } => {
                        self.infer_expr(*e, &Expectation::None);
                    }
                }
            }
            return Some(Ty::Error);
        }

        let method_param_tys = &params[1..];
        let method_param_names: Vec<_> = self.item_tree.functions[fn_idx]
            .params
            .iter()
            .skip(1)
            .map(|p| p.name)
            .collect();
        let has_arg_errors = if traversal_intrinsic {
            self.with_traversal_seq_compat_scope(|ctx| {
                ctx.infer_call_args_with_binding(args, method_param_tys, Some(&method_param_names))
            })
        } else {
            self.infer_call_args_with_binding(args, method_param_tys, Some(&method_param_names))
        };
        if has_arg_errors {
            return Some(Ty::Error);
        }

        // Record effect checking for the resolved method.
        let call_target = Some(method_name);
        self.record_call_edge_if_top_level(call_target);
        self.check_call_effects_if_function(call_target);

        // Check Map key type: methods that take a key (insert, get, contains, remove)
        // must have a hashable key type. Re-resolve after unification so type vars
        // from the receiver are now concrete.
        let base_after = self.table.resolve_deep(&base_ty);
        if let Ty::Adt { def, args } = &base_after {
            let core = self.module_scope.core_types.kind_for_idx(*def);
            let method_str = method_name.resolve(self.interner);

            if matches!(core, Some(CoreType::Map | CoreType::MutableMap))
                && args.len() >= 2
                && matches!(
                    method_str,
                    "map_insert"
                        | "map_get"
                        | "map_contains"
                        | "map_remove"
                        | "mutable_map_insert"
                        | "mutable_map_get"
                        | "mutable_map_contains"
                        | "mutable_map_remove"
                )
            {
                let key_ty = self.table.resolve_deep(&args[0]);
                if !(self.ty_satisfies_trait(&key_ty, "Hash")
                    && self.ty_satisfies_trait(&key_ty, "Eq"))
                {
                    self.push_diag(TyDiagnosticData::InvalidMapKey { ty: key_ty });
                }
            }

            // Check Set element type: methods that take an element (insert, contains, remove)
            // must have a hashable element type.
            if matches!(core, Some(CoreType::Set | CoreType::MutableSet))
                && !args.is_empty()
                && matches!(
                    method_str,
                    "set_insert"
                        | "set_contains"
                        | "set_remove"
                        | "mutable_set_insert"
                        | "mutable_set_contains"
                        | "mutable_set_remove"
                )
            {
                let elem_ty = self.table.resolve_deep(&args[0]);
                if !(self.ty_satisfies_trait(&elem_ty, "Hash")
                    && self.ty_satisfies_trait(&elem_ty, "Eq"))
                {
                    self.push_diag(TyDiagnosticData::InvalidSetElement { ty: elem_ty });
                }
            }

            // Check frequencies() element type: traversal elements become Map keys.
            if matches!(
                core,
                Some(CoreType::List | CoreType::MutableList | CoreType::Deque | CoreType::Seq)
            ) && !args.is_empty()
                && method_str == "seq_frequencies"
            {
                let elem_ty = self.table.resolve_deep(&args[0]);
                if !(self.ty_satisfies_trait(&elem_ty, "Hash")
                    && self.ty_satisfies_trait(&elem_ty, "Eq"))
                {
                    self.push_diag(TyDiagnosticData::InvalidMapKey { ty: elem_ty });
                }
            }

            // Check List.sort()/List.binary_search() element type: only naturally
            // orderable types allowed.
            if core == Some(CoreType::List)
                && !args.is_empty()
                && matches!(method_str, "list_sort" | "list_binary_search")
            {
                let elem_ty = self.table.resolve_deep(&args[0]);
                if !self.ty_satisfies_trait(&elem_ty, "Ord") {
                    self.push_diag(TyDiagnosticData::UnsortableElement { ty: elem_ty });
                }
            }
        }

        Some(ret)
    }

    fn infer_index(&mut self, base: ExprIdx, index: ExprIdx) -> Ty {
        let base_ty = self.infer_expr(base, &Expectation::None);
        let base_ty = self.table.resolve_deep(&base_ty);

        match &base_ty {
            Ty::Adt { def, args } => match self.module_scope.core_types.kind_for_idx(*def) {
                Some(CoreType::List) => {
                    self.infer_expr(index, &Expectation::Has(Ty::Int));
                    args.first().cloned().unwrap_or(Ty::Error)
                }
                Some(CoreType::MutableList) => {
                    self.infer_expr(index, &Expectation::Has(Ty::Int));
                    args.first().cloned().unwrap_or(Ty::Error)
                }
                Some(CoreType::Map) => {
                    let key_ty = args
                        .first()
                        .cloned()
                        .unwrap_or_else(|| self.table.fresh_var());
                    self.infer_expr(index, &Expectation::Has(key_ty.clone()));
                    let resolved_key = self.table.resolve_deep(&key_ty);
                    if !(self.ty_satisfies_trait(&resolved_key, "Hash")
                        && self.ty_satisfies_trait(&resolved_key, "Eq"))
                    {
                        self.push_diag(TyDiagnosticData::InvalidMapKey { ty: resolved_key });
                    }
                    args.get(1).cloned().unwrap_or(Ty::Error)
                }
                Some(CoreType::MutableMap) => {
                    let key_ty = args
                        .first()
                        .cloned()
                        .unwrap_or_else(|| self.table.fresh_var());
                    self.infer_expr(index, &Expectation::Has(key_ty.clone()));
                    let resolved_key = self.table.resolve_deep(&key_ty);
                    if !(self.ty_satisfies_trait(&resolved_key, "Hash")
                        && self.ty_satisfies_trait(&resolved_key, "Eq"))
                    {
                        self.push_diag(TyDiagnosticData::InvalidMapKey { ty: resolved_key });
                    }
                    args.get(1).cloned().unwrap_or(Ty::Error)
                }
                _ => {
                    self.infer_expr(index, &Expectation::None);
                    self.push_diag(TyDiagnosticData::InvalidIndexTarget {
                        ty: base_ty.clone(),
                    });
                    Ty::Error
                }
            },
            Ty::String => {
                self.infer_expr(index, &Expectation::Has(Ty::Int));
                Ty::Char
            }
            Ty::Error | Ty::Never => {
                self.infer_expr(index, &Expectation::None);
                Ty::Error
            }
            _ => {
                self.infer_expr(index, &Expectation::None);
                self.push_diag(TyDiagnosticData::InvalidIndexTarget {
                    ty: base_ty.clone(),
                });
                Ty::Error
            }
        }
    }
}
