//! Expression type inference for all [`Expr`] variants.

use kyokara_hir_def::expr::{BinaryOp, CallArg, Expr, ExprIdx, Literal, Stmt, UnaryOp};
use kyokara_hir_def::item_tree::TypeDefKind;
use kyokara_hir_def::pat::Pat;
use kyokara_hir_def::resolver::{ResolvedName, Resolver};
use kyokara_hir_def::scope::ScopeDef;

use crate::diagnostics::TyDiagnosticData;
use crate::effects;
use crate::holes::HoleInfo;
use crate::resolve::{TyResolutionEnv, instantiate_constructor, instantiate_fn_sig};
use crate::ty::Ty;

use super::{Expectation, InferenceCtx};

impl<'a> InferenceCtx<'a> {
    /// Infer the type of an expression, possibly guided by an expectation.
    pub(crate) fn infer_expr(&mut self, idx: ExprIdx, expected: &Expectation) -> Ty {
        let prev_expr = self.current_expr;
        self.current_expr = Some(idx);
        let ty = self.infer_expr_inner(idx, expected);
        self.expr_types.insert(idx, ty.clone());
        self.current_expr = prev_expr;

        // If we have an expectation, try to unify (but don't double-report).
        if let Expectation::Has(exp) = expected
            && !ty.is_poison()
            && !exp.is_poison()
        {
            // Already unified inside specific handlers for most cases.
            // Only unify here if the inner handler didn't (catch-all).
        }

        ty
    }

    fn infer_expr_inner(&mut self, idx: ExprIdx, expected: &Expectation) -> Ty {
        let expr = self.body.exprs[idx].clone();
        match expr {
            Expr::Missing => Ty::Error,

            Expr::Literal(lit) => self.infer_literal(&lit),

            Expr::Path(path) => {
                if !path.is_single() {
                    return Ty::Error;
                }
                let name = path.segments[0];
                self.infer_name(name, idx)
            }

            Expr::Binary { op, lhs, rhs } => self.infer_binary(op, lhs, rhs),

            Expr::Unary { op, operand } => self.infer_unary(op, operand),

            Expr::Call { callee, ref args } => {
                let args = args.clone();
                self.infer_call(callee, &args)
            }

            Expr::Field { base, field } => self.infer_field(base, field),

            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => self.infer_if(condition, then_branch, else_branch, expected),

            Expr::Match {
                scrutinee,
                ref arms,
            } => {
                let arms = arms.clone();
                self.infer_match(idx, scrutinee, &arms, expected)
            }

            Expr::Block { ref stmts, tail } => {
                let stmts = stmts.clone();
                self.infer_block(&stmts, tail, expected)
            }

            Expr::Return(val) => {
                let ret = self.ret_ty.clone();
                if let Some(val_idx) = val {
                    self.infer_expr(val_idx, &Expectation::Has(ret));
                } else {
                    self.unify_or_err(&ret, &Ty::Unit);
                }
                Ty::Never
            }

            Expr::RecordLit {
                ref path,
                ref fields,
            } => {
                let path = path.clone();
                let fields = fields.clone();
                self.infer_record_lit(path.as_ref(), &fields)
            }

            Expr::Lambda {
                ref params,
                body: body_expr,
            } => {
                let params = params.clone();
                self.infer_lambda(&params, body_expr, expected)
            }

            Expr::Old(inner) => self.infer_expr(inner, expected),

            Expr::Hole => {
                let expected_ty = expected.ty().cloned();
                let ty = expected_ty
                    .clone()
                    .unwrap_or_else(|| self.table.fresh_var());

                let locals = self.collect_locals_in_scope();

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
        let scope = self.find_scope_for_expr(expr_idx);
        let resolver = Resolver::new(self.module_scope, &self.body.scopes, scope);

        match resolver.resolve_name(name) {
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
                ScopeDef::Type(_) | ScopeDef::Cap(_) | ScopeDef::Import(_) => Ty::Error,
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

            Some(ResolvedName::Type(_) | ResolvedName::Cap(_) | ResolvedName::Import(_)) => {
                Ty::Error
            }

            None => Ty::Error,
        }
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
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
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
            BinaryOp::Eq | BinaryOp::NotEq => {
                self.unify_or_err(&lhs_ty, &rhs_ty);
                Ty::Bool
            }
            BinaryOp::Lt | BinaryOp::Gt | BinaryOp::LtEq | BinaryOp::GtEq => {
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
        }
    }

    fn infer_call(&mut self, callee: ExprIdx, args: &[CallArg]) -> Ty {
        let callee_ty = self.infer_expr(callee, &Expectation::None);
        let callee_ty = self.table.resolve_deep(&callee_ty);

        match callee_ty {
            Ty::Fn { params, ret } => {
                let positional_count = args
                    .iter()
                    .filter(|a| matches!(a, CallArg::Positional(_)))
                    .count();
                if positional_count != params.len() {
                    self.push_diag(TyDiagnosticData::ArgCountMismatch {
                        expected: params.len(),
                        actual: positional_count,
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

                let mut pos = 0;
                for arg in args {
                    match arg {
                        CallArg::Positional(arg_idx) => {
                            if pos < params.len() {
                                self.infer_expr(*arg_idx, &Expectation::Has(params[pos].clone()));
                            } else {
                                self.infer_expr(*arg_idx, &Expectation::None);
                            }
                            pos += 1;
                        }
                        CallArg::Named { value, .. } => {
                            self.infer_expr(*value, &Expectation::None);
                        }
                    }
                }

                self.check_callee_effects(callee);

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
                let mut param_tys = Vec::new();
                for arg in args {
                    match arg {
                        CallArg::Positional(e) | CallArg::Named { value: e, .. } => {
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

    fn check_callee_effects(&mut self, callee: ExprIdx) {
        let callee_expr = &self.body.exprs[callee];
        if let Expr::Path(path) = callee_expr
            && path.is_single()
        {
            let name = path.segments[0];
            // Record callee for symbol graph call edges.
            self.calls.push(name);
            if let Some(&fn_idx) = self.module_scope.functions.get(&name) {
                let fn_item = &self.item_tree.functions[fn_idx];
                let env = Self::make_env(
                    self.item_tree,
                    self.module_scope,
                    self.interner,
                    &self.type_params,
                );
                let callee_effects = effects::EffectSet::from_with_caps(
                    &fn_item.with_caps,
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
                if let TypeDefKind::Record { fields: def_fields } = &type_item.kind {
                    // Build substitution for type params.
                    let mut tp_map: Vec<(kyokara_hir_def::name::Name, Ty)> =
                        self.type_params.clone();
                    for (param_name, arg) in type_item.type_params.iter().zip(args.iter()) {
                        tp_map.push((*param_name, arg.clone()));
                    }
                    let env = TyResolutionEnv {
                        item_tree: self.item_tree,
                        module_scope: self.module_scope,
                        interner: self.interner,
                        type_params: tp_map,
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

        let then_ty = self.infer_expr(then_branch, expected);

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
                *def,
                arms,
                &self.body.pats,
                self.item_tree,
                self.interner,
                &mut self.diags,
                match_expr_idx,
            );
        }

        result_ty.unwrap_or(Ty::Unit)
    }

    fn infer_block(&mut self, stmts: &[Stmt], tail: Option<ExprIdx>, expected: &Expectation) -> Ty {
        for stmt in stmts {
            match stmt {
                Stmt::Let { pat, ty, init } => {
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
                }
                Stmt::Expr(e) => {
                    self.infer_expr(*e, &Expectation::None);
                }
            }
        }

        if let Some(tail_idx) = tail {
            self.infer_expr(tail_idx, expected)
        } else {
            Ty::Unit
        }
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
                if let TypeDefKind::Record { fields: def_fields } = &type_item.kind {
                    // Build substitution env.
                    let mut tp = self.type_params.clone();
                    let mut args = Vec::new();
                    for &tparam in &type_item.type_params {
                        let var = self.table.fresh_var();
                        tp.push((tparam, var.clone()));
                        args.push(var);
                    }
                    let env = TyResolutionEnv {
                        item_tree: self.item_tree,
                        module_scope: self.module_scope,
                        interner: self.interner,
                        type_params: tp,
                    };

                    // Resolve expected types for each field, then infer.
                    let expected_field_tys: Vec<_> = def_fields
                        .iter()
                        .map(|(n, tr)| (*n, env.resolve_type_ref(tr, &mut self.table)))
                        .collect();

                    for (fname, fexpr) in fields {
                        let exp = expected_field_tys
                            .iter()
                            .find(|(n, _)| n == fname)
                            .map(|(_, ty)| ty.clone());
                        if let Some(exp_ty) = exp {
                            self.infer_expr(*fexpr, &Expectation::Has(exp_ty));
                        } else {
                            self.infer_expr(*fexpr, &Expectation::None);
                        }
                    }

                    return Ty::Adt {
                        def: type_idx,
                        args,
                    };
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

        let expected_ret = expected_fn.map(|(_, r)| Expectation::Has(r));
        let body_ty = self.infer_expr(body_expr, &expected_ret.unwrap_or(Expectation::None));

        Ty::Fn {
            params: param_tys,
            ret: Box::new(body_ty),
        }
    }

    fn find_scope_for_expr(&self, expr_idx: ExprIdx) -> Option<kyokara_hir_def::scope::ScopeIdx> {
        self.body
            .expr_scopes
            .get(expr_idx)
            .copied()
            .or(self.body.scopes.root)
    }

    fn collect_locals_in_scope(&self) -> Vec<(kyokara_hir_def::name::Name, Ty)> {
        let mut locals = Vec::new();
        // Include function parameters.
        for (name, ty) in self.param_names.iter().zip(self.param_types.iter()) {
            locals.push((*name, self.table.resolve_deep(ty)));
        }
        // Include let-bound locals.
        for (pat_idx, _) in &self.body.pat_scopes {
            if let Pat::Bind { name } = &self.body.pats[*pat_idx]
                && let Some(ty) = self.local_types.get(*pat_idx)
            {
                locals.push((*name, self.table.resolve_deep(ty)));
            }
        }
        locals
    }
}
