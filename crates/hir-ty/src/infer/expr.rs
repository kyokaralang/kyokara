//! Expression type inference for all [`Expr`] variants.

use std::collections::HashSet;

use kyokara_hir_def::expr::{BinaryOp, CallArg, Expr, ExprIdx, Literal, Stmt, UnaryOp};
use kyokara_hir_def::item_tree::TypeDefKind;
use kyokara_hir_def::pat::Pat;
use kyokara_hir_def::resolver::ResolvedName;
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
        let expr = self.body.exprs[idx].clone();
        match expr {
            Expr::Missing => Ty::Error,

            Expr::Literal(lit) => self.infer_literal(&lit),

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
                ScopeDef::Cap(_) => self.non_value_name_in_expr("capability", name),
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
            Some(ResolvedName::Cap(_)) => self.non_value_name_in_expr("capability", name),
            Some(ResolvedName::Import(_)) => self.non_value_name_in_expr("import", name),

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
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
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
            BinaryOp::And | BinaryOp::Or => {
                self.unify_or_err(&Ty::Bool, &lhs_ty);
                self.unify_or_err(&Ty::Bool, &rhs_ty);
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

    fn infer_call(&mut self, callee: ExprIdx, args: &[CallArg]) -> Ty {
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

                let mut seen = vec![false; params.len()];
                let mut next_pos = 0;
                let mut has_named_arg_errors = false;
                for arg in args {
                    match arg {
                        CallArg::Positional(arg_idx) => {
                            while next_pos < seen.len() && seen[next_pos] {
                                next_pos += 1;
                            }
                            if next_pos < params.len() {
                                seen[next_pos] = true;
                                self.infer_expr(
                                    *arg_idx,
                                    &Expectation::Has(params[next_pos].clone()),
                                );
                            } else {
                                has_named_arg_errors = true;
                                self.infer_expr(*arg_idx, &Expectation::None);
                            }
                            next_pos += 1;
                        }
                        CallArg::Named { name, value } => {
                            // Try to find the parameter index by name.
                            let expectation = if let Some(ref pnames) = param_names {
                                if let Some(idx) = pnames.iter().position(|pn| pn == name) {
                                    if seen[idx] {
                                        has_named_arg_errors = true;
                                        self.push_diag(TyDiagnosticData::DuplicateNamedArg {
                                            name: name.resolve(self.interner).to_string(),
                                        });
                                    } else {
                                        seen[idx] = true;
                                    }
                                    Expectation::Has(params[idx].clone())
                                } else {
                                    has_named_arg_errors = true;
                                    self.push_diag(TyDiagnosticData::UnknownNamedArg {
                                        name: name.resolve(self.interner).to_string(),
                                    });
                                    Expectation::None
                                }
                            } else {
                                has_named_arg_errors = true;
                                self.push_diag(TyDiagnosticData::UnknownNamedArg {
                                    name: name.resolve(self.interner).to_string(),
                                });
                                Expectation::None
                            };
                            self.infer_expr(*value, &expectation);
                        }
                    }
                }

                if let Some(ref pnames) = param_names {
                    for (idx, provided) in seen.iter().enumerate() {
                        if !provided {
                            has_named_arg_errors = true;
                            self.push_diag(TyDiagnosticData::MissingNamedArg {
                                name: pnames[idx].resolve(self.interner).to_string(),
                            });
                        }
                    }
                }

                if has_named_arg_errors {
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
                *def,
                arms,
                &self.body.pats,
                self.item_tree,
                self.interner,
                &mut self.diags,
                match_expr_idx,
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
                    if !self.is_irrefutable_let_pattern(*pat, &init_ty) {
                        self.current_expr = Some(*init);
                        self.push_diag(TyDiagnosticData::RefutableLetPattern);
                    }
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

    fn is_irrefutable_let_pattern(&mut self, pat_idx: la_arena::Idx<Pat>, expected: &Ty) -> bool {
        let expected = self.table.resolve_deep(expected);
        if expected.is_poison() {
            return true;
        }

        match self.body.pats[pat_idx].clone() {
            Pat::Missing | Pat::Wildcard | Pat::Bind { .. } => true,
            Pat::Literal(_) => false,
            Pat::Record { fields, .. } => match expected {
                Ty::Record {
                    fields: ref rec_fields,
                } => fields
                    .iter()
                    .all(|f| rec_fields.iter().any(|(name, _)| name == f)),
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

                if args.len() != field_tys.len() {
                    return false;
                }

                args.iter()
                    .zip(field_tys.iter())
                    .all(|(sub_pat, field_ty)| self.is_irrefutable_let_pattern(*sub_pat, field_ty))
            }
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
                // Extract record fields from either Record kind or Alias to Record.
                let (def_fields, is_true_record) = match &type_item.kind {
                    TypeDefKind::Record { fields: def_fields } => (Some(def_fields), true),
                    TypeDefKind::Alias(kyokara_hir_def::type_ref::TypeRef::Record {
                        fields: def_fields,
                    }) => (Some(def_fields), false),
                    _ => (None, false),
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
                        resolving_aliases: vec![],
                    };

                    // Resolve expected types for each field, then infer.
                    let expected_field_tys: Vec<_> = def_fields
                        .iter()
                        .map(|(n, tr)| (*n, env.resolve_type_ref(tr, &mut self.table)))
                        .collect();

                    let result_ty = if is_true_record {
                        Ty::Adt {
                            def: type_idx,
                            args: args.clone(),
                        }
                    } else {
                        Ty::Record {
                            fields: expected_field_tys
                                .iter()
                                .map(|(n, ty)| (*n, ty.clone()))
                                .collect(),
                        }
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

        let expected_ret = expected_fn.map(|(_, r)| Expectation::Has(r));
        let body_ty = self.infer_expr(body_expr, &expected_ret.unwrap_or(Expectation::None));

        Ty::Fn {
            params: param_tys,
            ret: Box::new(body_ty),
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
}
