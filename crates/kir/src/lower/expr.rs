//! Expression lowering: HIR `Expr` → KIR instructions.

use rustc_hash::{FxHashMap, FxHashSet};

use kyokara_hir_def::call_family::{
    CallFamilySelection, bind_call_args_to_params, select_call_family_candidate,
};
use kyokara_hir_def::expr::{BinaryOp, CallArg, Expr, ExprIdx, Literal, MatchArm, Stmt};
use kyokara_hir_def::item_tree::TypeDefKind;
use kyokara_hir_def::name::Name;
use kyokara_hir_def::pat::Pat;
use kyokara_hir_def::path::Path;
use kyokara_hir_def::resolver::ResolvedName;
use kyokara_hir_def::resolver::{CoreType, PrimitiveType, ReceiverKey, StaticOwnerKey};
use kyokara_hir_def::scope::ScopeDef;
use kyokara_hir_def::type_ref::TypeRef;
use kyokara_hir_ty::effects::EffectSet;
use kyokara_hir_ty::ty::Ty;

use crate::block::{BranchTarget, SwitchCase, Terminator};
use crate::build::KirBuilder;
use crate::function::KirContracts;
use crate::inst::{CallTarget, Constant, Inst};
use crate::value::ValueId;

use super::{CallableParamSpec, HiddenNames, Labels, LoweringCtx};

impl<'a> LoweringCtx<'a> {
    /// Lower an HIR expression to a KIR value.
    pub(crate) fn lower_expr(&mut self, expr_idx: ExprIdx) -> ValueId {
        let expr = self.body.exprs[expr_idx].clone();
        let ty = self.expr_ty(expr_idx);

        match expr {
            Expr::Literal(lit) => self.lower_literal(lit, ty),
            Expr::Path(path) => self.lower_path(path, ty),
            Expr::Binary { op, lhs, rhs } => match op {
                BinaryOp::And | BinaryOp::Or => self.lower_logical_binary(op, lhs, rhs, ty),
                BinaryOp::RangeUntil => {
                    let lv = self.lower_expr(lhs);
                    let rv = self.lower_expr(rhs);
                    self.builder.push_call(
                        CallTarget::Intrinsic("seq_range".to_string()),
                        vec![lv, rv],
                        ty,
                    )
                }
                _ => {
                    let lv = self.lower_expr(lhs);
                    let rv = self.lower_expr(rhs);
                    self.builder.push_binary(op, lv, rv, ty)
                }
            },
            Expr::Unary { op, operand } => {
                let v = self.lower_expr(operand);
                self.builder.push_unary(op, v, ty)
            }
            Expr::Call { callee, args } => self.lower_call(callee, args, ty),
            Expr::Field { base, field } => {
                if let Some(value) = self.try_lower_constructor_field_value(base, field, ty.clone())
                {
                    return value;
                }
                let bv = self.lower_expr(base);
                self.builder.push_field_get(bv, field, ty)
            }
            Expr::Index { base, index } => {
                let bv = self.lower_expr(base);
                let iv = self.lower_expr(index);
                let base_ty = self.expr_ty(base);
                match self.receiver_key_for_ty(&base_ty) {
                    Some(ReceiverKey::Core(CoreType::List)) => self.builder.push_call(
                        CallTarget::Intrinsic("list_index".to_string()),
                        vec![bv, iv],
                        ty,
                    ),
                    Some(ReceiverKey::Core(CoreType::MutableList)) => self.builder.push_call(
                        CallTarget::Intrinsic("mutable_list_index".to_string()),
                        vec![bv, iv],
                        ty,
                    ),
                    _ => {
                        let id = self.next_hole_id();
                        self.builder.push_hole(id, vec![], ty)
                    }
                }
            }
            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => self.lower_if(condition, then_branch, else_branch, ty),
            Expr::Match { scrutinee, arms } => self.lower_match(scrutinee, arms, ty),
            Expr::Block { stmts, tail } => self.lower_block(stmts, tail, ty),
            Expr::Return(val) => self.lower_return(val),
            Expr::RecordLit { path, fields } => self.lower_record_lit(path, fields, ty),
            Expr::Lambda { params, body } => self.lower_lambda(expr_idx, &params, body, ty),
            Expr::Old(inner) => self.lower_expr_in_old_scope(inner),
            Expr::Hole => {
                let id = self.next_hole_id();
                self.builder.push_hole(id, vec![], ty)
            }
            Expr::Missing => self.builder.push_const(Constant::Unit, Ty::Error),
        }
    }

    // ── Literal ──────────────────────────────────────────────────

    fn lower_literal(&mut self, lit: Literal, ty: Ty) -> ValueId {
        let c = match lit {
            Literal::Int(v) => Constant::Int(v),
            Literal::Float(v) => Constant::Float(v),
            Literal::String(s) => Constant::String(s),
            Literal::Char(c) => Constant::Char(c),
            Literal::Bool(b) => Constant::Bool(b),
        };
        self.builder.push_const(c, ty)
    }

    fn record_callable_param_spec(
        &mut self,
        value: ValueId,
        param_spec: CallableParamSpec,
    ) -> ValueId {
        self.callable_param_specs.insert(value, param_spec);
        value
    }

    fn callable_arg_values(&mut self, callee: ValueId, args: &[CallArg]) -> Vec<ValueId> {
        if let Some(spec) = self.callable_param_specs.get(&callee).cloned() {
            return self.lower_call_args_for_param_specs(
                args,
                &spec.param_names,
                Some(&spec.param_named_only),
            );
        }
        self.lower_call_args_source_order(args)
    }

    fn function_value_param_spec(
        &self,
        fn_idx: kyokara_hir_def::item_tree::FnItemIdx,
    ) -> CallableParamSpec {
        CallableParamSpec {
            param_names: self.param_names_for_fn_idx(fn_idx),
            param_named_only: self.param_named_only_for_fn_idx(fn_idx),
        }
    }

    fn lambda_value_param_spec(
        &self,
        params: &[(kyokara_hir_def::expr::PatIdx, Option<TypeRef>)],
    ) -> CallableParamSpec {
        CallableParamSpec {
            param_names: params
                .iter()
                .map(|(pat_idx, _)| self.lambda_param_name(*pat_idx))
                .collect(),
            param_named_only: vec![false; params.len()],
        }
    }

    fn lower_lambda(
        &mut self,
        lambda_expr: ExprIdx,
        params: &[(kyokara_hir_def::expr::PatIdx, Option<TypeRef>)],
        body_expr: ExprIdx,
        ty: Ty,
    ) -> ValueId {
        if self.lambda_uses_outer_local_capture(body_expr) {
            let captures = self.lambda_captured_outer_locals(body_expr);
            if captures.is_empty() {
                let id = self.next_hole_id();
                return self.builder.push_hole(id, vec![], ty);
            }
            let capture_values: Vec<_> = captures
                .iter()
                .filter_map(|capture| self.lookup_local(*capture))
                .collect();
            if capture_values.len() != captures.len() {
                let id = self.next_hole_id();
                return self.builder.push_hole(id, vec![], ty);
            }
            let lambda_name = self.lambda_name_for_expr(lambda_expr);
            if !self
                .lambda_functions
                .iter()
                .any(|func| func.name == lambda_name)
            {
                let (lambda_fn, nested_lambdas) =
                    self.build_lambda_function(lambda_name, params, body_expr, &ty, &captures);
                self.lambda_functions.extend(nested_lambdas);
                self.lambda_functions.push(lambda_fn);
            }

            let closure = self
                .builder
                .push_closure_create(lambda_name, capture_values, ty);
            return self.record_callable_param_spec(closure, self.lambda_value_param_spec(params));
        }

        let lambda_name = self.lambda_name_for_expr(lambda_expr);
        if !self
            .lambda_functions
            .iter()
            .any(|func| func.name == lambda_name)
        {
            let (lambda_fn, nested_lambdas) =
                self.build_lambda_function(lambda_name, params, body_expr, &ty, &[]);
            self.lambda_functions.extend(nested_lambdas);
            self.lambda_functions.push(lambda_fn);
        }

        let fn_ref = self.builder.push_fn_ref(lambda_name, ty);
        self.record_callable_param_spec(fn_ref, self.lambda_value_param_spec(params))
    }

    fn build_lambda_function(
        &mut self,
        lambda_name: Name,
        params: &[(kyokara_hir_def::expr::PatIdx, Option<TypeRef>)],
        body_expr: ExprIdx,
        lambda_ty: &Ty,
        captures: &[Name],
    ) -> (
        crate::function::KirFunction,
        Vec<crate::function::KirFunction>,
    ) {
        let (param_tys, ret_ty) = match lambda_ty {
            Ty::Fn { params, ret } => (params.clone(), (**ret).clone()),
            _ => (
                params
                    .iter()
                    .map(|(pat_idx, _)| self.pat_ty(*pat_idx))
                    .collect(),
                self.expr_ty(body_expr),
            ),
        };
        let capture_tys: Vec<Ty> = captures
            .iter()
            .map(|name| {
                self.lookup_local(*name)
                    .map(|vid| self.builder_value_ty(vid))
                    .unwrap_or(Ty::Error)
            })
            .collect();

        let (lambda_fn, nested_lambdas) = {
            let interner = &mut *self.interner;
            let labels = Labels::new(interner);
            let hidden = HiddenNames::new(interner);
            let mut child = LoweringCtx {
                builder: KirBuilder::new(),
                body: self.body,
                infer: self.infer,
                item_tree: self.item_tree,
                module_scope: self.module_scope,
                interner,
                locals: Vec::new(),
                intrinsics: self.intrinsics.clone(),
                hole_counter: 0,
                labels,
                hidden,
                ensures_exprs: Vec::new(),
                result_name: None,
                ensures_vids: Vec::new(),
                old_scope: FxHashMap::default(),
                loop_stack: Vec::new(),
                current_fn_name: self.current_fn_name,
                lambda_names: FxHashMap::default(),
                lambda_functions: Vec::new(),
                callable_param_specs: FxHashMap::default(),
                captured_lambda_locals: Vec::new(),
            };

            let entry_block = child.builder.new_block(Some(child.labels.entry));
            child.builder.switch_to(entry_block);
            child.push_scope();

            let lowered_params: Vec<(Name, Ty)> = params
                .iter()
                .enumerate()
                .map(|(index, (pat_idx, _))| {
                    let param_name = child.lambda_param_name(*pat_idx);
                    let param_ty = param_tys.get(index).cloned().unwrap_or(Ty::Error);
                    let vid = child.builder.alloc_value(
                        param_ty.clone(),
                        Inst::FnParam {
                            index: index as u32,
                        },
                    );
                    child.define_local(param_name, vid);
                    child.old_scope.insert(param_name, vid);
                    (param_name, param_ty)
                })
                .collect();
            let capture_offset = lowered_params.len();
            let lowered_captures: Vec<(Name, Ty)> = captures
                .iter()
                .enumerate()
                .map(|(index, name)| {
                    let capture_ty = capture_tys.get(index).cloned().unwrap_or(Ty::Error);
                    let vid = child.builder.alloc_value(
                        capture_ty.clone(),
                        Inst::FnParam {
                            index: (capture_offset + index) as u32,
                        },
                    );
                    child.define_local(*name, vid);
                    child.old_scope.insert(*name, vid);
                    (*name, capture_ty)
                })
                .collect();
            let mut lowered_params = lowered_params;
            lowered_params.extend(lowered_captures);

            let root_val = child.lower_expr(body_expr);
            if !child.block_has_terminator() {
                child.builder.set_return(root_val);
            }
            child.pop_scope();

            let nested_lambdas = std::mem::take(&mut child.lambda_functions);
            let lambda_fn = child.builder.build_with_captures(
                lambda_name,
                lowered_params,
                capture_tys,
                ret_ty,
                EffectSet::default(),
                entry_block,
                KirContracts::default(),
            );
            (lambda_fn, nested_lambdas)
        };

        (lambda_fn, nested_lambdas)
    }

    fn lambda_param_name(&self, pat_idx: kyokara_hir_def::expr::PatIdx) -> Name {
        match &self.body.pats[pat_idx] {
            Pat::Bind { name } => *name,
            _ => unreachable!("lambda parameters lower as binding patterns"),
        }
    }

    fn lambda_name_for_expr(&mut self, expr_idx: ExprIdx) -> Name {
        if let Some(name) = self.lambda_names.get(&expr_idx).copied() {
            return name;
        }

        let owner = self.current_fn_name.resolve(self.interner).to_owned();
        let name = Name::new(
            self.interner,
            &format!("$lambda${owner}${}", expr_idx.into_raw().into_u32()),
        );
        self.lambda_names.insert(expr_idx, name);
        name
    }

    fn lambda_uses_outer_local_capture(&self, body_expr: ExprIdx) -> bool {
        let Some(lambda_scope) = self.body.expr_scopes.get(body_expr).copied() else {
            return false;
        };
        self.expr_uses_outer_local_capture(body_expr, lambda_scope)
    }

    fn lambda_captured_outer_locals(&self, body_expr: ExprIdx) -> Vec<Name> {
        let Some(lambda_scope) = self.body.expr_scopes.get(body_expr).copied() else {
            return Vec::new();
        };
        let mut seen = FxHashSet::default();
        let mut out = Vec::new();
        self.collect_expr_outer_local_captures(body_expr, lambda_scope, &mut seen, &mut out);
        out
    }

    fn collect_expr_outer_local_captures(
        &self,
        expr_idx: ExprIdx,
        lambda_scope: kyokara_hir_def::scope::ScopeIdx,
        seen: &mut FxHashSet<Name>,
        out: &mut Vec<Name>,
    ) {
        match &self.body.exprs[expr_idx] {
            Expr::Path(path) if path.is_single() => {
                let name = path.segments[0];
                let Some(resolved) = self.body.resolve_name_at(self.module_scope, expr_idx, name)
                else {
                    return;
                };
                let captured = match resolved.resolved {
                    ResolvedName::Local(ScopeDef::Local(_)) => resolved
                        .local_binding
                        .map(|(_, meta)| !self.scope_is_within(meta.scope, lambda_scope))
                        .unwrap_or(false),
                    ResolvedName::Local(ScopeDef::Param(_)) => self
                        .body
                        .scopes
                        .root
                        .map(|root| !self.scope_is_within(root, lambda_scope))
                        .unwrap_or(false),
                    _ => false,
                };
                if captured && seen.insert(name) {
                    out.push(name);
                }
            }
            Expr::Path(_) => {}
            Expr::Binary { lhs, rhs, .. } => {
                self.collect_expr_outer_local_captures(*lhs, lambda_scope, seen, out);
                self.collect_expr_outer_local_captures(*rhs, lambda_scope, seen, out);
            }
            Expr::Unary { operand, .. } | Expr::Old(operand) | Expr::Return(Some(operand)) => {
                self.collect_expr_outer_local_captures(*operand, lambda_scope, seen, out);
            }
            Expr::Return(None) | Expr::Missing | Expr::Hole | Expr::Literal(_) => {}
            Expr::Call { callee, args } => {
                self.collect_expr_outer_local_captures(*callee, lambda_scope, seen, out);
                for arg in args {
                    let arg_expr = match arg {
                        CallArg::Positional(idx) => *idx,
                        CallArg::Named { value, .. } => *value,
                    };
                    self.collect_expr_outer_local_captures(arg_expr, lambda_scope, seen, out);
                }
            }
            Expr::Field { base, .. } => {
                self.collect_expr_outer_local_captures(*base, lambda_scope, seen, out);
            }
            Expr::Index { base, index } => {
                self.collect_expr_outer_local_captures(*base, lambda_scope, seen, out);
                self.collect_expr_outer_local_captures(*index, lambda_scope, seen, out);
            }
            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.collect_expr_outer_local_captures(*condition, lambda_scope, seen, out);
                self.collect_expr_outer_local_captures(*then_branch, lambda_scope, seen, out);
                if let Some(else_branch) = else_branch {
                    self.collect_expr_outer_local_captures(*else_branch, lambda_scope, seen, out);
                }
            }
            Expr::Match { scrutinee, arms } => {
                self.collect_expr_outer_local_captures(*scrutinee, lambda_scope, seen, out);
                for arm in arms {
                    self.collect_expr_outer_local_captures(arm.body, lambda_scope, seen, out);
                }
            }
            Expr::Block { stmts, tail } => {
                for stmt in stmts {
                    match stmt {
                        Stmt::Let { init, .. } => {
                            self.collect_expr_outer_local_captures(*init, lambda_scope, seen, out);
                        }
                        Stmt::Assign { target, value } => {
                            self.collect_expr_outer_local_captures(
                                *target,
                                lambda_scope,
                                seen,
                                out,
                            );
                            self.collect_expr_outer_local_captures(*value, lambda_scope, seen, out);
                        }
                        Stmt::While { condition, body } => {
                            self.collect_expr_outer_local_captures(
                                *condition,
                                lambda_scope,
                                seen,
                                out,
                            );
                            self.collect_expr_outer_local_captures(*body, lambda_scope, seen, out);
                        }
                        Stmt::For { source, body, .. } => {
                            self.collect_expr_outer_local_captures(
                                *source,
                                lambda_scope,
                                seen,
                                out,
                            );
                            self.collect_expr_outer_local_captures(*body, lambda_scope, seen, out);
                        }
                        Stmt::Break | Stmt::Continue => {}
                        Stmt::Expr(expr) => {
                            self.collect_expr_outer_local_captures(*expr, lambda_scope, seen, out);
                        }
                    }
                }
                if let Some(tail) = tail {
                    self.collect_expr_outer_local_captures(*tail, lambda_scope, seen, out);
                }
            }
            Expr::RecordLit { fields, .. } => {
                for (_, value) in fields {
                    self.collect_expr_outer_local_captures(*value, lambda_scope, seen, out);
                }
            }
            Expr::Lambda { .. } => {}
        }
    }

    fn expr_uses_outer_local_capture(
        &self,
        expr_idx: ExprIdx,
        lambda_scope: kyokara_hir_def::scope::ScopeIdx,
    ) -> bool {
        match &self.body.exprs[expr_idx] {
            Expr::Path(path) if path.is_single() => {
                let name = path.segments[0];
                let Some(resolved) = self.body.resolve_name_at(self.module_scope, expr_idx, name)
                else {
                    return false;
                };
                match resolved.resolved {
                    ResolvedName::Local(ScopeDef::Local(_)) => resolved
                        .local_binding
                        .map(|(_, meta)| !self.scope_is_within(meta.scope, lambda_scope))
                        .unwrap_or(false),
                    ResolvedName::Local(ScopeDef::Param(_)) => self
                        .body
                        .scopes
                        .root
                        .map(|root| !self.scope_is_within(root, lambda_scope))
                        .unwrap_or(false),
                    _ => false,
                }
            }
            Expr::Binary { lhs, rhs, .. } => {
                self.expr_uses_outer_local_capture(*lhs, lambda_scope)
                    || self.expr_uses_outer_local_capture(*rhs, lambda_scope)
            }
            Expr::Unary { operand, .. } => {
                self.expr_uses_outer_local_capture(*operand, lambda_scope)
            }
            Expr::Call { callee, args } => {
                self.expr_uses_outer_local_capture(*callee, lambda_scope)
                    || args.iter().any(|arg| {
                        let arg_expr = match arg {
                            CallArg::Positional(idx) => *idx,
                            CallArg::Named { value, .. } => *value,
                        };
                        self.expr_uses_outer_local_capture(arg_expr, lambda_scope)
                    })
            }
            Expr::Field { base, .. } => self.expr_uses_outer_local_capture(*base, lambda_scope),
            Expr::Index { base, index } => {
                self.expr_uses_outer_local_capture(*base, lambda_scope)
                    || self.expr_uses_outer_local_capture(*index, lambda_scope)
            }
            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr_uses_outer_local_capture(*condition, lambda_scope)
                    || self.expr_uses_outer_local_capture(*then_branch, lambda_scope)
                    || else_branch
                        .map(|idx| self.expr_uses_outer_local_capture(idx, lambda_scope))
                        .unwrap_or(false)
            }
            Expr::Match { scrutinee, arms } => {
                self.expr_uses_outer_local_capture(*scrutinee, lambda_scope)
                    || arms
                        .iter()
                        .any(|arm| self.expr_uses_outer_local_capture(arm.body, lambda_scope))
            }
            Expr::Block { stmts, tail } => {
                stmts.iter().any(|stmt| match stmt {
                    Stmt::Let { init, .. } => {
                        self.expr_uses_outer_local_capture(*init, lambda_scope)
                    }
                    Stmt::Assign { target, value } => {
                        self.expr_uses_outer_local_capture(*target, lambda_scope)
                            || self.expr_uses_outer_local_capture(*value, lambda_scope)
                    }
                    Stmt::While { condition, body } => {
                        self.expr_uses_outer_local_capture(*condition, lambda_scope)
                            || self.expr_uses_outer_local_capture(*body, lambda_scope)
                    }
                    Stmt::For { source, body, .. } => {
                        self.expr_uses_outer_local_capture(*source, lambda_scope)
                            || self.expr_uses_outer_local_capture(*body, lambda_scope)
                    }
                    Stmt::Expr(idx) => self.expr_uses_outer_local_capture(*idx, lambda_scope),
                    Stmt::Break | Stmt::Continue => false,
                }) || tail
                    .map(|idx| self.expr_uses_outer_local_capture(idx, lambda_scope))
                    .unwrap_or(false)
            }
            Expr::Return(value) => value
                .map(|idx| self.expr_uses_outer_local_capture(idx, lambda_scope))
                .unwrap_or(false),
            Expr::RecordLit { fields, .. } => fields
                .iter()
                .any(|(_, value)| self.expr_uses_outer_local_capture(*value, lambda_scope)),
            Expr::Old(inner) => self.expr_uses_outer_local_capture(*inner, lambda_scope),
            Expr::Lambda { .. } | Expr::Missing | Expr::Hole | Expr::Literal(_) => false,
            Expr::Path(_) => false,
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

    // ── Path ─────────────────────────────────────────────────────

    fn lower_path(&mut self, path: Path, ty: Ty) -> ValueId {
        if path.segments.is_empty() {
            let id = self.next_hole_id();
            return self.builder.push_hole(id, vec![], ty);
        }

        let first = path.segments[0];

        // Local variable — chain field gets for multi-segment paths.
        if let Some(vid) = self.lookup_local(first) {
            return self.chain_field_gets(vid, &path.segments[1..], ty);
        }

        // ADT constructor (zero-arg produces a value directly).
        if let Some((type_idx, variant_idx)) = self.module_scope.resolve_constructor_path(&path) {
            let is_nullary = matches!(
                &self.item_tree.types[type_idx].kind,
                TypeDefKind::Adt { variants } if variants[variant_idx].fields.is_empty()
            );
            if is_nullary {
                let variant_name = *path.segments.last().unwrap_or(&first);
                return self
                    .builder
                    .push_adt_construct(type_idx, variant_name, vec![], ty);
            }
            // Multi-field constructor as value — placeholder.
            let id = self.next_hole_id();
            return self.builder.push_hole(id, vec![], ty);
        }

        // Function reference — first-class fn value.
        if self
            .module_scope
            .functions
            .get(&first)
            .map(|candidates| candidates.len() == 1)
            .unwrap_or(false)
        {
            let fn_idx = self.module_scope.functions[&first][0];
            let fn_ref = self.builder.push_fn_ref(first, ty);
            return self.record_callable_param_spec(fn_ref, self.function_value_param_spec(fn_idx));
        }

        // Unknown — emit hole.
        let id = self.next_hole_id();
        self.builder.push_hole(id, vec![], ty)
    }

    fn try_lower_constructor_field_value(
        &mut self,
        base_idx: ExprIdx,
        field: Name,
        ty: Ty,
    ) -> Option<ValueId> {
        let resolve_nullary_variant =
            |this: &mut Self, type_idx: kyokara_hir_def::item_tree::TypeItemIdx| {
                let &variant_idx = this.module_scope.type_variants.get(&(type_idx, field))?;
                let is_nullary = matches!(
                    &this.item_tree.types[type_idx].kind,
                    TypeDefKind::Adt { variants } if variants[variant_idx].fields.is_empty()
                );
                if !is_nullary {
                    return None;
                }
                Some(
                    this.builder
                        .push_adt_construct(type_idx, field, vec![], ty.clone()),
                )
            };

        if let Expr::Path(path) = &self.body.exprs[base_idx]
            && path.is_single()
            && let Some(&type_idx) = self.module_scope.types.get(&path.segments[0])
        {
            return resolve_nullary_variant(self, type_idx);
        }

        if let Expr::Field {
            base: module_base_idx,
            field: type_name,
        } = &self.body.exprs[base_idx]
            && let Expr::Path(module_path) = &self.body.exprs[*module_base_idx]
            && module_path.is_single()
        {
            let module_name = module_path.segments[0];
            if let Some(namespace) = self.module_scope.visible_namespace(module_name)
                && let Some(&type_idx) = namespace.types.get(type_name)
            {
                return resolve_nullary_variant(self, type_idx);
            }
        }

        None
    }

    fn chain_field_gets(
        &mut self,
        mut val: ValueId,
        segments: &[kyokara_hir_def::name::Name],
        final_ty: Ty,
    ) -> ValueId {
        if segments.is_empty() {
            return val;
        }
        for (i, &seg) in segments.iter().enumerate() {
            let ty = if i == segments.len() - 1 {
                final_ty.clone()
            } else {
                Ty::Error // intermediate types not easily available
            };
            val = self.builder.push_field_get(val, seg, ty);
        }
        val
    }

    fn param_names_for_fn_idx(&self, fn_idx: kyokara_hir_def::item_tree::FnItemIdx) -> Vec<Name> {
        self.item_tree.functions[fn_idx]
            .params
            .iter()
            .map(|p| p.name)
            .collect()
    }

    fn param_named_only_for_fn_idx(
        &self,
        fn_idx: kyokara_hir_def::item_tree::FnItemIdx,
    ) -> Vec<bool> {
        self.item_tree.functions[fn_idx]
            .params
            .iter()
            .map(|p| p.named_only)
            .collect()
    }

    fn select_method_candidate_by_call_shape(
        &self,
        candidates: &[kyokara_hir_def::item_tree::FnItemIdx],
        args: &[CallArg],
    ) -> CallFamilySelection<kyokara_hir_def::item_tree::FnItemIdx> {
        select_call_family_candidate(args, candidates, |fn_idx| {
            &self.item_tree.functions[fn_idx].params[1..]
        })
    }

    fn lower_call_args_source_order(&mut self, args: &[CallArg]) -> Vec<ValueId> {
        args.iter()
            .map(|arg| {
                let idx = match arg {
                    CallArg::Positional(idx) => *idx,
                    CallArg::Named { value, .. } => *value,
                };
                self.lower_expr(idx)
            })
            .collect()
    }

    /// Reorder lowered argument values from source order into parameter order.
    ///
    /// If call arguments are invalid (unknown/duplicate/missing names or
    /// positional-after-named), this returns the original source order as a
    /// defensive fallback. Type-checking should already reject such calls.
    fn reorder_lowered_args_for_names(
        &self,
        args: &[CallArg],
        lowered_source_order: Vec<ValueId>,
        param_names: &[Name],
    ) -> Vec<ValueId> {
        let has_named = args.iter().any(|arg| matches!(arg, CallArg::Named { .. }));
        if !has_named {
            return lowered_source_order;
        }

        let mut slots: Vec<Option<ValueId>> = vec![None; param_names.len()];
        let mut next_pos = 0usize;
        let mut saw_named = false;

        for (arg, value) in args.iter().zip(lowered_source_order.iter().copied()) {
            match arg {
                CallArg::Positional(_) => {
                    if saw_named {
                        return lowered_source_order;
                    }
                    while next_pos < slots.len() && slots[next_pos].is_some() {
                        next_pos += 1;
                    }
                    if next_pos >= slots.len() {
                        return lowered_source_order;
                    }
                    slots[next_pos] = Some(value);
                    next_pos += 1;
                }
                CallArg::Named { name, .. } => {
                    saw_named = true;
                    let Some(slot_idx) = param_names.iter().position(|param| param == name) else {
                        return lowered_source_order;
                    };
                    if slots[slot_idx].is_some() {
                        return lowered_source_order;
                    }
                    slots[slot_idx] = Some(value);
                }
            }
        }

        if slots.iter().any(|slot| slot.is_none()) {
            return lowered_source_order;
        }

        slots.into_iter().flatten().collect()
    }

    fn lower_call_args_for_param_names(
        &mut self,
        args: &[CallArg],
        param_names: &[Name],
    ) -> Vec<ValueId> {
        let lowered = self.lower_call_args_source_order(args);
        self.reorder_lowered_args_for_names(args, lowered, param_names)
    }

    fn lower_call_args_for_param_specs(
        &mut self,
        args: &[CallArg],
        param_names: &[Name],
        param_named_only: Option<&[bool]>,
    ) -> Vec<ValueId> {
        let params: Vec<kyokara_hir_def::item_tree::FnParam> = param_names
            .iter()
            .enumerate()
            .map(|(idx, name)| kyokara_hir_def::item_tree::FnParam {
                name: *name,
                ty: TypeRef::Error,
                named_only: param_named_only
                    .and_then(|flags| flags.get(idx))
                    .copied()
                    .unwrap_or(false),
            })
            .collect();
        let lowered = self.lower_call_args_source_order(args);
        let binding = bind_call_args_to_params(args, &params);
        if !binding.errors.is_empty() {
            return lowered;
        }
        let mut source = lowered.into_iter().map(Some).collect::<Vec<_>>();
        let mut out = Vec::with_capacity(params.len());
        for arg_idx in binding.param_to_arg {
            let arg_idx = arg_idx.expect("valid binding fills all params");
            out.push(source[arg_idx].take().expect("arg consumed once"));
        }
        out
    }

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

    fn type_has_field_named(&self, ty: &Ty, field: Name) -> bool {
        match ty {
            Ty::Record { fields } => fields.iter().any(|(name, _)| *name == field),
            Ty::Adt { def, .. } => {
                let type_item = &self.item_tree.types[*def];
                let def_fields = match &type_item.kind {
                    TypeDefKind::Record { fields } => Some(fields.as_slice()),
                    TypeDefKind::Alias(TypeRef::Record { fields }) => Some(fields.as_slice()),
                    _ => None,
                };
                def_fields
                    .map(|fields| fields.iter().any(|(name, _)| *name == field))
                    .unwrap_or(false)
            }
            _ => false,
        }
    }

    // ── Call ─────────────────────────────────────────────────────

    fn lower_call(&mut self, callee: ExprIdx, args: Vec<CallArg>, ty: Ty) -> ValueId {
        let callee_expr = self.body.exprs[callee].clone();

        // Simple path callee (common case).
        if let Expr::Path(ref path) = callee_expr
            && path.is_single()
        {
            let name = path.segments[0];

            // 1. Local variable (indirect call) — locals shadow everything.
            if let Some(captured) = self.lookup_captured_lambda_local(name).cloned() {
                let mut arg_vals = self.lower_call_args_for_param_specs(
                    &args,
                    &captured.param_spec.param_names,
                    Some(&captured.param_spec.param_named_only),
                );
                arg_vals.extend(captured.capture_values);
                return self.builder.push_call(
                    CallTarget::Direct(captured.lambda_name),
                    arg_vals,
                    ty,
                );
            }
            if let Some(vid) = self.lookup_local(name) {
                let arg_vals = self.callable_arg_values(vid, &args);
                return self
                    .builder
                    .push_call(CallTarget::Indirect(vid), arg_vals, ty);
            }

            // 2. Constructor call → AdtConstruct.
            if let Some((type_idx, _)) = self.module_scope.resolve_constructor_path(path) {
                let arg_vals = self.lower_call_args_source_order(&args);
                let ctor_name = *path.segments.last().unwrap_or(&name);
                return self
                    .builder
                    .push_adt_construct(type_idx, ctor_name, arg_vals, ty);
            }

            // 3. Module-level function (direct call — user-defined takes precedence).
            if let Some(candidates) = self.module_scope.functions.get(&name) {
                let fn_idx = match candidates.as_slice() {
                    [fn_idx] => Some(*fn_idx),
                    _ => match select_call_family_candidate(&args, candidates, |fn_idx| {
                        &self.item_tree.functions[fn_idx].params
                    }) {
                        CallFamilySelection::Selected { candidate, .. } => Some(candidate),
                        _ => None,
                    },
                };
                if let Some(fn_idx) = fn_idx {
                    let param_names = self.param_names_for_fn_idx(fn_idx);
                    let param_named_only = self.param_named_only_for_fn_idx(fn_idx);
                    let arg_vals = self.lower_call_args_for_param_specs(
                        &args,
                        &param_names,
                        Some(&param_named_only),
                    );
                    return self
                        .builder
                        .push_call(CallTarget::Direct(name), arg_vals, ty);
                }
            }

            // 4. Intrinsic (has entry in intrinsic lookup but no body).
            if self.intrinsics.contains(&name) {
                let arg_vals = self.lower_call_args_source_order(&args);
                let name_str = name.resolve(self.interner).to_string();
                return self
                    .builder
                    .push_call(CallTarget::Intrinsic(name_str), arg_vals, ty);
            }

            // Fallback: treat as direct call (might be imported).
            let arg_vals = self.lower_call_args_source_order(&args);
            return self
                .builder
                .push_call(CallTarget::Direct(name), arg_vals, ty);
        }

        // Module-qualified, static method, or method call.
        if let Expr::Field { base, field } = callee_expr {
            // Nested module-qualified static call:
            // collections.Deque.new()
            if let Expr::Field {
                base: module_base,
                field: type_name,
            } = self.body.exprs[base]
                && let Expr::Path(ref module_path) = self.body.exprs[module_base]
                && module_path.is_single()
            {
                let module_name = module_path.segments[0];
                if let Some(namespace) = self.module_scope.visible_namespace(module_name)
                    && let Some(&type_idx) = namespace.types.get(&type_name)
                    && self
                        .module_scope
                        .type_variants
                        .contains_key(&(type_idx, field))
                {
                    let arg_vals = self.lower_call_args_source_order(&args);
                    return self
                        .builder
                        .push_adt_construct(type_idx, field, arg_vals, ty);
                }
                if self.module_scope.imported_modules.contains(&module_name)
                    && let Some(&fn_idx) = self.module_scope.synthetic_module_static_methods.get(&(
                        module_name,
                        type_name,
                        field,
                    ))
                {
                    let param_names = self.param_names_for_fn_idx(fn_idx);
                    let arg_vals = self.lower_call_args_for_param_names(&args, &param_names);
                    let fn_item = &self.item_tree.functions[fn_idx];
                    let target = if self.intrinsics.contains(&fn_item.name) {
                        CallTarget::Intrinsic(fn_item.name.resolve(self.interner).to_string())
                    } else {
                        CallTarget::Direct(fn_item.name)
                    };
                    return self.builder.push_call(target, arg_vals, ty);
                }
            }

            if let Expr::Path(ref path) = self.body.exprs[base]
                && path.is_single()
            {
                let seg = path.segments[0];

                if let Some(&type_idx) = self.module_scope.types.get(&seg)
                    && self
                        .module_scope
                        .type_variants
                        .contains_key(&(type_idx, field))
                {
                    let arg_vals = self.lower_call_args_source_order(&args);
                    return self
                        .builder
                        .push_adt_construct(type_idx, field, arg_vals, ty);
                }

                // Module-qualified call: io.println(s), math.min(a, b)
                if self.module_scope.imported_modules.contains(&seg)
                    && let Some(mod_fns) = self.module_scope.synthetic_modules.get(&seg)
                    && let Some(&fn_idx) = mod_fns.get(&field)
                {
                    let param_names = self.param_names_for_fn_idx(fn_idx);
                    let arg_vals = self.lower_call_args_for_param_names(&args, &param_names);
                    let fn_item = &self.item_tree.functions[fn_idx];
                    let target = if self.intrinsics.contains(&fn_item.name) {
                        CallTarget::Intrinsic(fn_item.name.resolve(self.interner).to_string())
                    } else {
                        CallTarget::Direct(fn_item.name)
                    };
                    return self.builder.push_call(target, arg_vals, ty);
                }

                if let Some(namespace) = self.module_scope.visible_namespace(seg)
                    && let Some(candidates) = namespace.functions.get(&field)
                {
                    let fn_idx = match candidates.as_slice() {
                        [fn_idx] => Some(*fn_idx),
                        _ => match select_call_family_candidate(&args, candidates, |fn_idx| {
                            &self.item_tree.functions[fn_idx].params
                        }) {
                            CallFamilySelection::Selected { candidate, .. } => Some(candidate),
                            _ => None,
                        },
                    };
                    if let Some(fn_idx) = fn_idx {
                        let param_names = self.param_names_for_fn_idx(fn_idx);
                        let param_named_only = self.param_named_only_for_fn_idx(fn_idx);
                        let arg_vals = self.lower_call_args_for_param_specs(
                            &args,
                            &param_names,
                            Some(&param_named_only),
                        );
                        let fn_item = &self.item_tree.functions[fn_idx];
                        let target = if self.intrinsics.contains(&fn_item.name) {
                            CallTarget::Intrinsic(fn_item.name.resolve(self.interner).to_string())
                        } else {
                            CallTarget::Direct(fn_item.name)
                        };
                        return self.builder.push_call(target, arg_vals, ty);
                    }
                }

                // Type-owned static call: bare `Type.method()` if registered.
                if let Some(&type_idx) = self.module_scope.types.get(&seg) {
                    let owner_key = self.static_owner_key_for_type_idx(type_idx);
                    if let Some(&fn_idx) = self.module_scope.static_methods.get(&(owner_key, field))
                    {
                        let param_names = self.param_names_for_fn_idx(fn_idx);
                        let arg_vals = self.lower_call_args_for_param_names(&args, &param_names);
                        let fn_item = &self.item_tree.functions[fn_idx];
                        let target = if self.intrinsics.contains(&fn_item.name) {
                            CallTarget::Intrinsic(fn_item.name.resolve(self.interner).to_string())
                        } else {
                            CallTarget::Direct(fn_item.name)
                        };
                        return self.builder.push_call(target, arg_vals, ty);
                    }
                }
            }
        }
        // Method call or field access — fall through to complex callee lowering.

        if let Expr::Field { base, field } = callee_expr {
            let base_ty = self.expr_ty(base);
            let method_fn_idx = if self.type_has_field_named(&base_ty, field) {
                None
            } else {
                let selection = self
                    .receiver_key_for_ty(&base_ty)
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
                    .map(|candidates| {
                        self.select_method_candidate_by_call_shape(candidates, &args)
                    });
                match selection {
                    Some(CallFamilySelection::Selected { candidate, .. }) => Some(candidate),
                    _ => None,
                }
            };
            if let Some(fn_idx) = method_fn_idx {
                let base_val = self.lower_expr(base);
                let full_param_names = self.param_names_for_fn_idx(fn_idx);
                let full_param_named_only = self.param_named_only_for_fn_idx(fn_idx);
                let method_param_names: Vec<Name> =
                    full_param_names.iter().skip(1).copied().collect();
                let method_param_named_only: Vec<bool> =
                    full_param_named_only.iter().skip(1).copied().collect();

                let mut arg_vals =
                    Vec::with_capacity(1usize.saturating_add(method_param_names.len()));
                arg_vals.push(base_val);
                let mut lowered_method_args = self.lower_call_args_for_param_specs(
                    &args,
                    &method_param_names,
                    Some(&method_param_named_only),
                );
                arg_vals.append(&mut lowered_method_args);

                let fn_item = &self.item_tree.functions[fn_idx];
                let target = if self.intrinsics.contains(&fn_item.name) {
                    CallTarget::Intrinsic(fn_item.name.resolve(self.interner).to_string())
                } else {
                    CallTarget::Direct(fn_item.name)
                };
                return self.builder.push_call(target, arg_vals, ty);
            }
        }

        // Complex callee expression.
        let callee_val = self.lower_expr(callee);
        let arg_vals = self.callable_arg_values(callee_val, &args);
        self.builder
            .push_call(CallTarget::Indirect(callee_val), arg_vals, ty)
    }

    /// Lower `&&` and `||` with short-circuit semantics using explicit CFG.
    ///
    /// `a && b`:
    /// - if `a` true  -> evaluate `b`
    /// - if `a` false -> result `false`
    ///
    /// `a || b`:
    /// - if `a` true  -> result `true`
    /// - if `a` false -> evaluate `b`
    fn lower_logical_binary(
        &mut self,
        op: BinaryOp,
        lhs: ExprIdx,
        rhs: ExprIdx,
        ty: Ty,
    ) -> ValueId {
        let lhs_val = self.lower_expr(lhs);

        let rhs_blk = self.builder.new_block(None);
        let short_blk = self.builder.new_block(None);
        let merge_blk = self.builder.new_block(Some(self.labels.merge));

        match op {
            BinaryOp::And => self.builder.set_branch(
                lhs_val,
                BranchTarget {
                    block: rhs_blk,
                    args: vec![],
                },
                BranchTarget {
                    block: short_blk,
                    args: vec![],
                },
            ),
            BinaryOp::Or => self.builder.set_branch(
                lhs_val,
                BranchTarget {
                    block: short_blk,
                    args: vec![],
                },
                BranchTarget {
                    block: rhs_blk,
                    args: vec![],
                },
            ),
            _ => unreachable!("non-logical operator passed to lower_logical_binary"),
        }

        self.builder.switch_to(rhs_blk);
        let rhs_val = self.lower_expr(rhs);
        if !self.block_has_terminator() {
            self.builder.set_jump(BranchTarget {
                block: merge_blk,
                args: vec![rhs_val],
            });
        }

        self.builder.switch_to(short_blk);
        let short_val = self
            .builder
            .push_const(Constant::Bool(matches!(op, BinaryOp::Or)), ty.clone());
        self.builder.set_jump(BranchTarget {
            block: merge_blk,
            args: vec![short_val],
        });

        let result = self.builder.add_block_param(merge_blk, None, ty);
        self.builder.switch_to(merge_blk);
        result
    }

    // ── If ───────────────────────────────────────────────────────

    fn lower_if(
        &mut self,
        condition: ExprIdx,
        then_branch: ExprIdx,
        else_branch: Option<ExprIdx>,
        ty: Ty,
    ) -> ValueId {
        let mut seen = FxHashSet::default();
        let mut carried_names = Vec::new();
        self.collect_assigned_outer_locals_in_expr(
            then_branch,
            then_branch,
            &mut seen,
            &mut carried_names,
        );
        if let Some(else_branch) = else_branch {
            self.collect_assigned_outer_locals_in_expr(
                else_branch,
                else_branch,
                &mut seen,
                &mut carried_names,
            );
        }
        let carried: Vec<(Name, Ty)> = carried_names
            .iter()
            .filter_map(|name| {
                self.lookup_local(*name)
                    .map(|vid| (*name, self.builder_value_ty(vid)))
            })
            .collect();
        let saved_locals = self.locals.clone();
        let cond_val = self.lower_expr(condition);

        let then_blk = self.builder.new_block(Some(self.labels.then_));
        let else_blk = self.builder.new_block(Some(self.labels.else_));
        let merge_blk = self.builder.new_block(Some(self.labels.merge));

        self.builder.set_branch(
            cond_val,
            BranchTarget {
                block: then_blk,
                args: vec![],
            },
            BranchTarget {
                block: else_blk,
                args: vec![],
            },
        );

        // Then branch.
        self.builder.switch_to(then_blk);
        let then_val = self.lower_expr(then_branch);
        let then_term = self.block_has_terminator();
        if !then_term {
            let mut args = vec![then_val];
            args.extend(self.current_loop_args(&carried_names));
            self.builder.set_jump(BranchTarget {
                block: merge_blk,
                args,
            });
        }

        // Else branch.
        self.locals = saved_locals.clone();
        self.builder.switch_to(else_blk);
        let else_val = match else_branch {
            Some(e) => self.lower_expr(e),
            None => self.builder.push_const(Constant::Unit, Ty::Unit),
        };
        let else_term = self.block_has_terminator();
        if !else_term {
            let mut args = vec![else_val];
            args.extend(self.current_loop_args(&carried_names));
            self.builder.set_jump(BranchTarget {
                block: merge_blk,
                args,
            });
        }

        // Merge block.
        let result = self.builder.add_block_param(merge_blk, None, ty);
        let carried_params: Vec<_> = carried
            .iter()
            .map(|(name, ty)| {
                self.builder
                    .add_block_param(merge_blk, Some(*name), ty.clone())
            })
            .collect();
        self.locals = saved_locals;
        self.builder.switch_to(merge_blk);
        for ((name, _), param) in carried.iter().zip(carried_params.iter()) {
            let _ = self.rebind_local(*name, *param);
        }
        if then_term && else_term {
            self.builder.set_unreachable();
        }
        result
    }

    // ── Match ────────────────────────────────────────────────────

    fn lower_match(&mut self, scrutinee: ExprIdx, arms: Vec<MatchArm>, ty: Ty) -> ValueId {
        let scr_ty = self.expr_ty(scrutinee);
        let scr_val = self.lower_expr(scrutinee);

        if self.is_adt_match(&scr_ty, &arms) {
            self.lower_match_adt(scr_val, &arms, ty)
        } else {
            self.lower_match_sequential(scr_val, &arms, ty)
        }
    }

    fn is_adt_match(&self, scr_ty: &Ty, arms: &[MatchArm]) -> bool {
        matches!(scr_ty, Ty::Adt { .. })
            && arms
                .iter()
                .any(|arm| matches!(&self.body.pats[arm.pat], Pat::Constructor { .. }))
            && arms.iter().all(|arm| {
                matches!(
                    &self.body.pats[arm.pat],
                    Pat::Constructor { .. } | Pat::Wildcard | Pat::Bind { .. }
                )
            })
    }

    fn lower_match_adt(&mut self, scr: ValueId, arms: &[MatchArm], ty: Ty) -> ValueId {
        let mut seen = FxHashSet::default();
        let mut carried_names = Vec::new();
        for arm in arms {
            self.collect_assigned_outer_locals_in_expr(
                arm.body,
                arm.body,
                &mut seen,
                &mut carried_names,
            );
        }
        let carried: Vec<(Name, Ty)> = carried_names
            .iter()
            .filter_map(|name| {
                self.lookup_local(*name)
                    .map(|vid| (*name, self.builder_value_ty(vid)))
            })
            .collect();
        let saved_locals = self.locals.clone();
        let merge_blk = self.builder.new_block(Some(self.labels.merge));
        #[allow(clippy::unwrap_used)] // lowering always starts with an entry block
        let switch_blk = self.builder.current_block().unwrap();

        // First pass: create case blocks, collect switch info.
        let mut cases = Vec::new();
        let mut default_target = None;
        let mut seen_variants = FxHashSet::default();

        struct ArmInfo {
            block: crate::block::BlockId,
            body: ExprIdx,
            pat_data: Pat,
        }
        let mut arm_infos = Vec::new();

        for arm in arms {
            // Once a catch-all arm is seen, all subsequent arms are
            // unreachable — stop building switch dispatch entries.
            if default_target.is_some() {
                break;
            }

            let pat = self.body.pats[arm.pat].clone();
            match &pat {
                Pat::Constructor { path, .. } => {
                    let ctor_name = path.last().expect("constructor path must not be empty");
                    // Skip duplicate constructor arms (first match wins).
                    if !seen_variants.insert(ctor_name) {
                        continue;
                    }
                    let case_blk = self.builder.new_block(Some(ctor_name));
                    cases.push(SwitchCase {
                        variant: ctor_name,
                        target: BranchTarget {
                            block: case_blk,
                            args: vec![],
                        },
                    });
                    arm_infos.push(ArmInfo {
                        block: case_blk,
                        body: arm.body,
                        pat_data: pat,
                    });
                }
                Pat::Wildcard | Pat::Bind { .. } => {
                    let default_blk = self.builder.new_block(Some(self.labels.default));
                    default_target = Some(BranchTarget {
                        block: default_blk,
                        args: vec![],
                    });
                    arm_infos.push(ArmInfo {
                        block: default_blk,
                        body: arm.body,
                        pat_data: pat,
                    });
                }
                _ => {}
            }
        }

        // Build a fallback target for nested pattern mismatch (default or unreachable).
        let has_default = default_target.is_some();
        let fallback_target = default_target.clone().unwrap_or_else(|| {
            let dead = self.builder.new_block(None);
            BranchTarget {
                block: dead,
                args: vec![],
            }
        });

        // Emit switch in the original block.
        self.builder.switch_to(switch_blk);
        self.builder.set_terminator(Terminator::Switch {
            scrutinee: scr,
            cases,
            default: default_target,
        });

        // Second pass: lower each arm's body.
        let mut all_terminated = true;
        for info in arm_infos {
            self.locals = saved_locals.clone();
            self.builder.switch_to(info.block);
            self.push_scope();

            match &info.pat_data {
                Pat::Constructor { args, .. } => {
                    // Extract fields and check nested literal subpatterns.
                    let mut field_vals = Vec::new();
                    for (i, sub_pat) in args.iter().enumerate() {
                        let field_ty = self.pat_ty(*sub_pat);
                        let field_val = self.builder.push_adt_field_get(scr, i as u32, field_ty);
                        field_vals.push((*sub_pat, field_val));
                    }

                    // Emit equality checks for nested literal subpatterns.
                    for &(sub_pat_idx, field_val) in &field_vals {
                        let sub_pat = self.body.pats[sub_pat_idx].clone();
                        if let Pat::Literal(lit) = sub_pat {
                            let lit_const = literal_to_constant(&lit);
                            let field_ty = self.builder.value_ty(field_val).clone();
                            let lit_val = self.builder.push_const(lit_const, field_ty);
                            let eq_val = self.builder.push_binary(
                                kyokara_hir_def::expr::BinaryOp::Eq,
                                field_val,
                                lit_val,
                                Ty::Bool,
                            );
                            // Branch: match → continue, mismatch → fallback.
                            let continue_blk = self.builder.new_block(None);
                            self.builder.set_branch(
                                eq_val,
                                BranchTarget {
                                    block: continue_blk,
                                    args: vec![],
                                },
                                fallback_target.clone(),
                            );
                            self.builder.switch_to(continue_blk);
                        } else {
                            self.bind_pattern(sub_pat_idx, field_val);
                        }
                    }
                }
                Pat::Bind { name } => {
                    self.define_local(*name, scr);
                }
                Pat::Wildcard => {}
                _ => {}
            }

            let body_val = self.lower_expr(info.body);
            if !self.block_has_terminator() {
                let mut args = vec![body_val];
                args.extend(self.current_loop_args(&carried_names));
                self.builder.set_jump(BranchTarget {
                    block: merge_blk,
                    args,
                });
                all_terminated = false;
            }
            self.pop_scope();
        }

        // If we created a dead fallback block (no default arm), mark it unreachable.
        if !has_default {
            self.builder.switch_to(fallback_target.block);
            if !self.block_has_terminator() {
                self.builder.set_unreachable();
            }
        }

        let result = self.builder.add_block_param(merge_blk, None, ty);
        let carried_params: Vec<_> = carried
            .iter()
            .map(|(name, ty)| {
                self.builder
                    .add_block_param(merge_blk, Some(*name), ty.clone())
            })
            .collect();
        self.locals = saved_locals;
        self.builder.switch_to(merge_blk);
        for ((name, _), param) in carried.iter().zip(carried_params.iter()) {
            let _ = self.rebind_local(*name, *param);
        }
        if all_terminated {
            self.builder.set_unreachable();
        }
        result
    }

    fn lower_match_sequential(&mut self, scr: ValueId, arms: &[MatchArm], ty: Ty) -> ValueId {
        let mut seen = FxHashSet::default();
        let mut carried_names = Vec::new();
        for arm in arms {
            self.collect_assigned_outer_locals_in_expr(
                arm.body,
                arm.body,
                &mut seen,
                &mut carried_names,
            );
        }
        let carried: Vec<(Name, Ty)> = carried_names
            .iter()
            .filter_map(|name| {
                self.lookup_local(*name)
                    .map(|vid| (*name, self.builder_value_ty(vid)))
            })
            .collect();
        let saved_locals = self.locals.clone();
        let merge_blk = self.builder.new_block(Some(self.labels.merge));
        let mut all_terminated = true;

        for (i, arm) in arms.iter().enumerate() {
            self.locals = saved_locals.clone();
            let pat = self.body.pats[arm.pat].clone();
            let is_last = i == arms.len() - 1;

            match &pat {
                Pat::Literal(lit) => {
                    let lit_const = literal_to_constant(lit);
                    let scr_ty = self.builder_value_ty(scr);
                    let lit_val = self.builder.push_const(lit_const, scr_ty);
                    let eq_val = self.builder.push_binary(
                        kyokara_hir_def::expr::BinaryOp::Eq,
                        scr,
                        lit_val,
                        Ty::Bool,
                    );

                    let body_blk = self.builder.new_block(None);
                    let next_blk = if is_last {
                        // Last literal arm — create an unreachable fallthrough block.
                        let dead_blk = self.builder.new_block(None);
                        self.builder.set_branch(
                            eq_val,
                            BranchTarget {
                                block: body_blk,
                                args: vec![],
                            },
                            BranchTarget {
                                block: dead_blk,
                                args: vec![],
                            },
                        );
                        // Mark the fallthrough as unreachable.
                        self.builder.switch_to(dead_blk);
                        self.builder.set_unreachable();
                        None
                    } else {
                        let next_blk = self.builder.new_block(None);
                        self.builder.set_branch(
                            eq_val,
                            BranchTarget {
                                block: body_blk,
                                args: vec![],
                            },
                            BranchTarget {
                                block: next_blk,
                                args: vec![],
                            },
                        );
                        Some(next_blk)
                    };

                    // Lower the arm body.
                    self.builder.switch_to(body_blk);
                    self.push_scope();
                    let body_val = self.lower_expr(arm.body);
                    if !self.block_has_terminator() {
                        let mut args = vec![body_val];
                        args.extend(self.current_loop_args(&carried_names));
                        self.builder.set_jump(BranchTarget {
                            block: merge_blk,
                            args,
                        });
                        all_terminated = false;
                    }
                    self.pop_scope();

                    // Continue in next_blk for subsequent arms.
                    if let Some(next) = next_blk {
                        self.builder.switch_to(next);
                    }
                }
                Pat::Wildcard => {
                    self.push_scope();
                    let body_val = self.lower_expr(arm.body);
                    if !self.block_has_terminator() {
                        let mut args = vec![body_val];
                        args.extend(self.current_loop_args(&carried_names));
                        self.builder.set_jump(BranchTarget {
                            block: merge_blk,
                            args,
                        });
                        all_terminated = false;
                    }
                    self.pop_scope();
                    break; // catch-all: subsequent arms are unreachable
                }
                Pat::Bind { name } => {
                    self.push_scope();
                    self.define_local(*name, scr);
                    let body_val = self.lower_expr(arm.body);
                    if !self.block_has_terminator() {
                        let mut args = vec![body_val];
                        args.extend(self.current_loop_args(&carried_names));
                        self.builder.set_jump(BranchTarget {
                            block: merge_blk,
                            args,
                        });
                        all_terminated = false;
                    }
                    self.pop_scope();
                    break; // catch-all: subsequent arms are unreachable
                }
                Pat::Record { .. } | Pat::Constructor { .. } => {
                    self.push_scope();
                    self.bind_pattern(arm.pat, scr);
                    let body_val = self.lower_expr(arm.body);
                    if !self.block_has_terminator() {
                        let mut args = vec![body_val];
                        args.extend(self.current_loop_args(&carried_names));
                        self.builder.set_jump(BranchTarget {
                            block: merge_blk,
                            args,
                        });
                        all_terminated = false;
                    }
                    self.pop_scope();
                }
                _ => {}
            }
        }

        let result = self.builder.add_block_param(merge_blk, None, ty);
        let carried_params: Vec<_> = carried
            .iter()
            .map(|(name, ty)| {
                self.builder
                    .add_block_param(merge_blk, Some(*name), ty.clone())
            })
            .collect();
        self.locals = saved_locals;
        self.builder.switch_to(merge_blk);
        for ((name, _), param) in carried.iter().zip(carried_params.iter()) {
            let _ = self.rebind_local(*name, *param);
        }
        if all_terminated {
            self.builder.set_unreachable();
        }
        result
    }

    /// Get the type of an already-allocated value.
    fn builder_value_ty(&self, vid: ValueId) -> Ty {
        self.builder.value_ty(vid).clone()
    }

    fn assignment_target_name(&self, expr_idx: ExprIdx) -> Option<Name> {
        let Expr::Path(path) = &self.body.exprs[expr_idx] else {
            return None;
        };
        path.is_single().then_some(path.segments[0])
    }

    fn assignment_target_name_for_loop(
        &self,
        expr_idx: ExprIdx,
        _loop_body: ExprIdx,
    ) -> Option<Name> {
        let name = self.assignment_target_name(expr_idx)?;
        let resolved = self
            .body
            .resolve_name_at(self.module_scope, expr_idx, name)?;
        match resolved.resolved {
            ResolvedName::Local(ScopeDef::Local(_)) => {
                let (_, meta) = resolved.local_binding?;
                if !meta.mutable { None } else { Some(name) }
            }
            _ => None,
        }
    }

    fn collect_loop_carried_locals(&self, loop_body: ExprIdx) -> Vec<Name> {
        let mut seen = FxHashSet::default();
        let mut names = Vec::new();
        self.collect_assigned_outer_locals_in_expr(loop_body, loop_body, &mut seen, &mut names);
        names
    }

    fn collect_assigned_outer_locals_in_expr(
        &self,
        expr_idx: ExprIdx,
        loop_body: ExprIdx,
        seen: &mut FxHashSet<Name>,
        out: &mut Vec<Name>,
    ) {
        match &self.body.exprs[expr_idx] {
            Expr::Literal(_) | Expr::Path(_) | Expr::Hole | Expr::Missing => {}
            Expr::Binary { lhs, rhs, .. } => {
                self.collect_assigned_outer_locals_in_expr(*lhs, loop_body, seen, out);
                self.collect_assigned_outer_locals_in_expr(*rhs, loop_body, seen, out);
            }
            Expr::Unary { operand, .. } | Expr::Old(operand) | Expr::Return(Some(operand)) => {
                self.collect_assigned_outer_locals_in_expr(*operand, loop_body, seen, out);
            }
            Expr::Return(None) => {}
            Expr::Call { callee, args } => {
                self.collect_assigned_outer_locals_in_expr(*callee, loop_body, seen, out);
                for arg in args {
                    let idx = match arg {
                        CallArg::Positional(idx) => *idx,
                        CallArg::Named { value, .. } => *value,
                    };
                    self.collect_assigned_outer_locals_in_expr(idx, loop_body, seen, out);
                }
            }
            Expr::Field { base, .. } => {
                self.collect_assigned_outer_locals_in_expr(*base, loop_body, seen, out);
            }
            Expr::Index { base, index } => {
                self.collect_assigned_outer_locals_in_expr(*base, loop_body, seen, out);
                self.collect_assigned_outer_locals_in_expr(*index, loop_body, seen, out);
            }
            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.collect_assigned_outer_locals_in_expr(*condition, loop_body, seen, out);
                self.collect_assigned_outer_locals_in_expr(*then_branch, loop_body, seen, out);
                if let Some(else_branch) = else_branch {
                    self.collect_assigned_outer_locals_in_expr(*else_branch, loop_body, seen, out);
                }
            }
            Expr::Match { scrutinee, arms } => {
                self.collect_assigned_outer_locals_in_expr(*scrutinee, loop_body, seen, out);
                for arm in arms {
                    self.collect_assigned_outer_locals_in_expr(arm.body, loop_body, seen, out);
                }
            }
            Expr::Block { stmts, tail } => {
                self.collect_assigned_outer_locals_in_stmts(stmts, loop_body, seen, out);
                if let Some(tail) = tail {
                    self.collect_assigned_outer_locals_in_expr(*tail, loop_body, seen, out);
                }
            }
            Expr::RecordLit { fields, .. } => {
                for (_, value) in fields {
                    self.collect_assigned_outer_locals_in_expr(*value, loop_body, seen, out);
                }
            }
            Expr::Lambda { .. } => {}
        }
    }

    fn collect_assigned_outer_locals_in_stmts(
        &self,
        stmts: &[Stmt],
        loop_body: ExprIdx,
        seen: &mut FxHashSet<Name>,
        out: &mut Vec<Name>,
    ) {
        for stmt in stmts {
            match stmt {
                Stmt::Let { init, .. } => {
                    self.collect_assigned_outer_locals_in_expr(*init, loop_body, seen, out);
                }
                Stmt::Assign { target, value } => {
                    if let Some(name) = self.assignment_target_name_for_loop(*target, loop_body)
                        && seen.insert(name)
                    {
                        out.push(name);
                    }
                    self.collect_assigned_outer_locals_in_expr(*value, loop_body, seen, out);
                }
                Stmt::While { condition, body } => {
                    self.collect_assigned_outer_locals_in_expr(*condition, loop_body, seen, out);
                    self.collect_assigned_outer_locals_in_expr(*body, loop_body, seen, out);
                }
                Stmt::For { source, body, .. } => {
                    self.collect_assigned_outer_locals_in_expr(*source, loop_body, seen, out);
                    self.collect_assigned_outer_locals_in_expr(*body, loop_body, seen, out);
                }
                Stmt::Break | Stmt::Continue => {}
                Stmt::Expr(expr) => {
                    self.collect_assigned_outer_locals_in_expr(*expr, loop_body, seen, out);
                }
            }
        }
    }

    fn current_loop_args(&self, carried_names: &[Name]) -> Vec<ValueId> {
        carried_names
            .iter()
            .filter_map(|name| self.lookup_local(*name))
            .collect()
    }

    fn lower_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { pat, init, .. } => {
                if let Pat::Bind { name } = self.body.pats[*pat]
                    && let Expr::Lambda { params, body } = &self.body.exprs[*init]
                    && self.lambda_uses_outer_local_capture(*body)
                {
                    let captures = self.lambda_captured_outer_locals(*body);
                    let capture_values: Vec<_> = captures
                        .iter()
                        .filter_map(|capture| self.lookup_local(*capture))
                        .collect();
                    if !captures.is_empty() && captures.len() == capture_values.len() {
                        let lambda_name = self.lambda_name_for_expr(*init);
                        if !self
                            .lambda_functions
                            .iter()
                            .any(|func| func.name == lambda_name)
                        {
                            let (lambda_fn, nested_lambdas) = self.build_lambda_function(
                                lambda_name,
                                params,
                                *body,
                                &self.expr_ty(*init),
                                &captures,
                            );
                            self.lambda_functions.extend(nested_lambdas);
                            self.lambda_functions.push(lambda_fn);
                        }
                        let closure = self.builder.push_closure_create(
                            lambda_name,
                            capture_values.clone(),
                            self.expr_ty(*init),
                        );
                        let param_spec = self.lambda_value_param_spec(params);
                        let callable = self.record_callable_param_spec(closure, param_spec.clone());
                        self.bind_pattern(*pat, callable);
                        self.define_captured_lambda_local(
                            name,
                            crate::lower::CapturedLambdaLocal {
                                lambda_name,
                                capture_values,
                                param_spec,
                            },
                        );
                        return;
                    }
                }
                let init_val = self.lower_expr(*init);
                if let Pat::Bind { name } = self.body.pats[*pat] {
                    self.remove_captured_lambda_local(name);
                }
                self.bind_pattern(*pat, init_val);
            }
            Stmt::Assign { target, value } => {
                let value = self.lower_expr(*value);
                if let Some(name) = self.assignment_target_name(*target) {
                    self.remove_captured_lambda_local(name);
                    let _ = self.rebind_local(name, value);
                }
            }
            Stmt::While {
                condition,
                body: loop_body,
            } => {
                self.lower_while_stmt(*condition, *loop_body);
            }
            Stmt::For {
                pat,
                source,
                body: loop_body,
            } => {
                if !self.try_lower_for_range_stmt(*pat, *source, *loop_body) {
                    // RFC 0006 phase-1 compatibility fallback for non-range
                    // traversal sources that still lower via seq intrinsics.
                    self.lower_expr(*source);
                    self.push_scope();
                    let hole = self.next_hole_id();
                    let elem = self.builder.push_hole(hole, vec![], self.pat_ty(*pat));
                    self.bind_pattern(*pat, elem);
                    self.lower_expr(*loop_body);
                    self.pop_scope();
                }
            }
            Stmt::Break | Stmt::Continue => {
                if let Some(loop_ctx) = self.loop_stack.last() {
                    let target_block = match stmt {
                        Stmt::Break => loop_ctx.break_block,
                        Stmt::Continue => loop_ctx.continue_block,
                        _ => unreachable!(),
                    };
                    let carried_names = loop_ctx.carried_names.clone();
                    self.lower_loop_jump(target_block, &carried_names);
                } else {
                    let hole = self.next_hole_id();
                    self.builder.push_hole(hole, vec![], Ty::Unit);
                }
            }
            Stmt::Expr(expr) => {
                self.lower_expr(*expr);
            }
        }
    }

    fn lower_loop_jump(&mut self, block: crate::block::BlockId, carried_names: &[Name]) {
        self.builder.set_jump(BranchTarget {
            block,
            args: self.current_loop_args(carried_names),
        });
        let dead_blk = self.builder.new_block(None);
        self.builder.switch_to(dead_blk);
        self.builder.set_unreachable();
    }

    fn lower_while_stmt(&mut self, condition: ExprIdx, loop_body: ExprIdx) {
        let carried_names = self.collect_loop_carried_locals(loop_body);
        let carried: Vec<(Name, Ty)> = carried_names
            .iter()
            .filter_map(|name| {
                self.lookup_local(*name)
                    .map(|vid| (*name, self.builder_value_ty(vid)))
            })
            .collect();
        let cond_blk = self.builder.new_block(None);
        let body_blk = self.builder.new_block(None);
        let exit_blk = self.builder.new_block(None);

        self.builder.set_jump(BranchTarget {
            block: cond_blk,
            args: self.current_loop_args(&carried_names),
        });

        let cond_params: Vec<_> = carried
            .iter()
            .map(|(name, ty)| {
                self.builder
                    .add_block_param(cond_blk, Some(*name), ty.clone())
            })
            .collect();
        self.builder.switch_to(cond_blk);
        for ((name, _), param) in carried.iter().zip(cond_params.iter()) {
            let _ = self.rebind_local(*name, *param);
        }

        let cond_val = self.lower_expr(condition);
        if !self.block_has_terminator() {
            self.builder.set_branch(
                cond_val,
                BranchTarget {
                    block: body_blk,
                    args: vec![],
                },
                BranchTarget {
                    block: exit_blk,
                    args: self.current_loop_args(&carried_names),
                },
            );
        }

        self.builder.switch_to(body_blk);
        self.loop_stack.push(super::LoopContext {
            continue_block: cond_blk,
            break_block: exit_blk,
            carried_names: carried_names.clone(),
        });
        self.lower_expr(loop_body);
        self.loop_stack.pop();
        if !self.block_has_terminator() {
            self.builder.set_jump(BranchTarget {
                block: cond_blk,
                args: self.current_loop_args(&carried_names),
            });
        }

        let exit_params: Vec<_> = carried
            .iter()
            .map(|(name, ty)| {
                self.builder
                    .add_block_param(exit_blk, Some(*name), ty.clone())
            })
            .collect();
        self.builder.switch_to(exit_blk);
        for ((name, _), param) in carried.iter().zip(exit_params.iter()) {
            let _ = self.rebind_local(*name, *param);
        }
    }

    fn try_lower_range_bounds(&mut self, expr_idx: ExprIdx) -> Option<(ValueId, ValueId)> {
        match self.body.exprs[expr_idx].clone() {
            Expr::Binary {
                op: BinaryOp::RangeUntil,
                lhs,
                rhs,
            } => {
                let start = self.lower_expr(lhs);
                let end = self.lower_expr(rhs);
                Some((start, end))
            }
            Expr::Block { stmts, tail } => {
                let tail = tail?;
                self.push_scope();
                for stmt in &stmts {
                    if self.block_has_terminator() {
                        self.pop_scope();
                        return None;
                    }
                    self.lower_stmt(stmt);
                }
                let bounds = if self.block_has_terminator() {
                    None
                } else {
                    self.try_lower_range_bounds(tail)
                };
                self.pop_scope();
                bounds
            }
            _ => None,
        }
    }

    fn try_lower_for_range_stmt(
        &mut self,
        pat: kyokara_hir_def::expr::PatIdx,
        source: ExprIdx,
        loop_body: ExprIdx,
    ) -> bool {
        let Some((start, end)) = self.try_lower_range_bounds(source) else {
            return false;
        };

        self.push_scope();
        self.define_local(self.hidden.for_current, start);

        let mut loop_state_names = vec![self.hidden.for_current];
        loop_state_names.extend(self.collect_loop_carried_locals(loop_body));
        let loop_state: Vec<(Name, Ty)> = loop_state_names
            .iter()
            .filter_map(|name| {
                self.lookup_local(*name)
                    .map(|vid| (*name, self.builder_value_ty(vid)))
            })
            .collect();

        let cond_blk = self.builder.new_block(None);
        let body_blk = self.builder.new_block(None);
        let exit_blk = self.builder.new_block(None);

        self.builder.set_jump(BranchTarget {
            block: cond_blk,
            args: self.current_loop_args(&loop_state_names),
        });

        let cond_params: Vec<_> = loop_state
            .iter()
            .map(|(name, ty)| {
                self.builder
                    .add_block_param(cond_blk, Some(*name), ty.clone())
            })
            .collect();
        self.builder.switch_to(cond_blk);
        for ((name, _), param) in loop_state.iter().zip(cond_params.iter()) {
            let _ = self.rebind_local(*name, *param);
        }

        let current = self
            .lookup_local(self.hidden.for_current)
            .expect("for loop current must be in scope");
        let cond_val = self
            .builder
            .push_binary(BinaryOp::Lt, current, end, Ty::Bool);
        if !self.block_has_terminator() {
            self.builder.set_branch(
                cond_val,
                BranchTarget {
                    block: body_blk,
                    args: vec![],
                },
                BranchTarget {
                    block: exit_blk,
                    args: self.current_loop_args(&loop_state_names),
                },
            );
        }

        self.builder.switch_to(body_blk);
        self.loop_stack.push(super::LoopContext {
            continue_block: cond_blk,
            break_block: exit_blk,
            carried_names: loop_state_names.clone(),
        });
        self.push_scope();
        self.bind_pattern(pat, current);
        let one = self.builder.push_const(Constant::Int(1), Ty::Int);
        let next = self
            .builder
            .push_binary(BinaryOp::Add, current, one, Ty::Int);
        let _ = self.rebind_local(self.hidden.for_current, next);
        self.lower_expr(loop_body);
        self.pop_scope();
        self.loop_stack.pop();
        if !self.block_has_terminator() {
            self.builder.set_jump(BranchTarget {
                block: cond_blk,
                args: self.current_loop_args(&loop_state_names),
            });
        }

        let exit_params: Vec<_> = loop_state
            .iter()
            .map(|(name, ty)| {
                self.builder
                    .add_block_param(exit_blk, Some(*name), ty.clone())
            })
            .collect();
        self.builder.switch_to(exit_blk);
        for ((name, _), param) in loop_state.iter().zip(exit_params.iter()) {
            let _ = self.rebind_local(*name, *param);
        }
        self.pop_scope();
        true
    }

    // ── Block ────────────────────────────────────────────────────

    fn lower_block(&mut self, stmts: Vec<Stmt>, tail: Option<ExprIdx>, _ty: Ty) -> ValueId {
        self.push_scope();

        for stmt in &stmts {
            if self.block_has_terminator() {
                break; // dead code after return
            }
            self.lower_stmt(stmt);
        }

        let result = if self.block_has_terminator() {
            // Block already terminated (e.g. by a return statement).
            self.builder
                .alloc_value(Ty::Never, Inst::Const(Constant::Unit))
        } else if let Some(tail_expr) = tail {
            self.lower_expr(tail_expr)
        } else {
            self.builder.push_const(Constant::Unit, Ty::Unit)
        };

        self.pop_scope();
        result
    }

    // ── Return ───────────────────────────────────────────────────

    fn lower_return(&mut self, val: Option<ExprIdx>) -> ValueId {
        let ret_val = match val {
            Some(expr) => self.lower_expr(expr),
            None => self.builder.push_const(Constant::Unit, Ty::Unit),
        };

        // Emit ensures assertions before the return terminator.
        if !self.ensures_exprs.is_empty()
            && let Some(rn) = self.result_name
        {
            // Temporarily clear ensures expressions to avoid re-entrant emission.
            let ensures_exprs = std::mem::take(&mut self.ensures_exprs);
            self.push_scope();
            self.define_local(rn, ret_val);
            for ens_expr in ensures_exprs.iter().copied() {
                let cond = self.lower_expr(ens_expr);
                let vid = self
                    .builder
                    .push_assert(cond, "ensures".to_string(), Ty::Unit);
                self.ensures_vids.push(vid);
            }
            self.pop_scope();
            // Restore ensures expressions for subsequent return statements.
            self.ensures_exprs = ensures_exprs;
        }

        self.builder.set_return(ret_val);

        // Create dead block for any subsequent code.
        let dead_blk = self.builder.new_block(None);
        self.builder.switch_to(dead_blk);
        self.builder.set_unreachable();

        // Return a dummy value (not pushed to any block).
        self.builder
            .alloc_value(Ty::Never, Inst::Const(Constant::Unit))
    }

    // ── RecordLit ────────────────────────────────────────────────

    fn lower_record_lit(
        &mut self,
        path: Option<Path>,
        fields: Vec<(kyokara_hir_def::name::Name, ExprIdx)>,
        ty: Ty,
    ) -> ValueId {
        let field_vals: Vec<_> = fields
            .iter()
            .map(|(name, expr)| (*name, self.lower_expr(*expr)))
            .collect();

        // Named constructor → AdtConstruct.
        if let Some(path) = &path
            && let Some((type_idx, _)) = self.module_scope.resolve_constructor_path(path)
        {
            let ctor_name = path.last().expect("constructor path must have a variant");
            let vals: Vec<_> = field_vals.into_iter().map(|(_, v)| v).collect();
            return self
                .builder
                .push_adt_construct(type_idx, ctor_name, vals, ty);
        }

        // Plain record literal.
        self.builder.push_record_create(field_vals, ty)
    }
}

fn literal_to_constant(lit: &kyokara_hir_def::expr::Literal) -> Constant {
    match lit {
        Literal::Int(v) => Constant::Int(*v),
        Literal::Float(v) => Constant::Float(*v),
        Literal::String(s) => Constant::String(s.clone()),
        Literal::Char(c) => Constant::Char(*c),
        Literal::Bool(b) => Constant::Bool(*b),
    }
}
