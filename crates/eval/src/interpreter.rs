//! Core tree-walking interpreter.

use std::rc::Rc;

use kyokara_hir_def::body::Body;
use kyokara_hir_def::expr::{BinaryOp, CallArg, Expr, ExprIdx, Literal, MatchArm, Stmt, UnaryOp};
use kyokara_hir_def::item_tree::{FnItemIdx, ItemTree, TypeDefKind, TypeItemIdx};
use kyokara_hir_def::name::Name;
use kyokara_hir_def::pat::Pat;
use kyokara_hir_def::resolver::ModuleScope;
use kyokara_hir_def::type_ref::TypeRef;
use kyokara_intern::Interner;
use kyokara_stdx::FxHashMap;

use crate::env::Env;
use crate::error::RuntimeError;
use crate::intrinsics::{self, Args, IntrinsicFn};
use crate::manifest::CapabilityManifest;
use crate::value::{FnValue, MapKey, Value};

/// Tree-walking interpreter state.
pub struct Interpreter {
    item_tree: ItemTree,
    module_scope: ModuleScope,
    fn_bodies: FxHashMap<FnItemIdx, Body>,
    /// Per-function module-level function overrides used for project mode.
    /// Maps `current_fn_idx -> (name -> resolved fn_idx)`.
    fn_scope_overrides: FxHashMap<FnItemIdx, FxHashMap<Name, FnItemIdx>>,
    interner: Interner,
    intrinsics: FxHashMap<Name, IntrinsicFn>,
    /// Snapshot of the environment at function entry, used by `old()` in ensures clauses.
    old_env: Option<Env>,
    /// Cached Option::Some constructor (type_idx, variant_idx).
    option_some: Option<(TypeItemIdx, usize)>,
    /// Cached Option::None constructor (type_idx, variant_idx).
    option_none: Option<(TypeItemIdx, usize)>,
    /// Cached Result::Ok constructor (type_idx, variant_idx).
    result_ok: Option<(TypeItemIdx, usize)>,
    /// Cached Result::Err constructor (type_idx, variant_idx).
    result_err: Option<(TypeItemIdx, usize)>,
    /// Cached ParseError::InvalidInt constructor (type_idx, variant_idx).
    parse_error_invalid_int: Option<(TypeItemIdx, usize)>,
    /// Cached ParseError::InvalidFloat constructor (type_idx, variant_idx).
    parse_error_invalid_float: Option<(TypeItemIdx, usize)>,
    /// Optional capability manifest for deny-by-default enforcement.
    manifest: Option<CapabilityManifest>,
    /// Shared environment used across all function calls to avoid per-call allocation.
    env: Env,
    /// Current user function being evaluated (used to select scope overrides).
    current_fn: Option<FnItemIdx>,
}

/// Used to implement early return from functions.
enum ControlFlow {
    Value(Value),
    Return(Value),
}

enum LogicalEvalStep {
    ShortCircuit(Value),
    NeedRhs,
}

impl ControlFlow {
    fn into_value(self) -> Value {
        match self {
            ControlFlow::Value(v) | ControlFlow::Return(v) => v,
        }
    }
}

/// Evaluate a sub-expression, propagating `ControlFlow::Return` up the stack.
///
/// If the sub-expression triggers an early return (e.g. via `?` operator
/// or explicit `return`), this propagates it immediately instead of
/// extracting the inner value.
macro_rules! eval_propagate {
    ($self:expr, $env:expr, $body:expr, $idx:expr) => {{
        let cf = $self.eval_expr($env, $body, $idx)?;
        match cf {
            cf @ ControlFlow::Return(_) => return Ok(cf),
            ControlFlow::Value(v) => v,
        }
    }};
}

/// Like `eval_propagate` but for the shared-env path (`eval_expr_shared`).
macro_rules! eval_propagate_shared {
    ($self:expr, $body:expr, $idx:expr) => {{
        let cf = $self.eval_expr_shared($body, $idx)?;
        match cf {
            cf @ ControlFlow::Return(_) => return Ok(cf),
            ControlFlow::Value(v) => v,
        }
    }};
}

impl Interpreter {
    pub fn new(
        item_tree: ItemTree,
        module_scope: ModuleScope,
        fn_bodies: FxHashMap<FnItemIdx, Body>,
        fn_scope_overrides: FxHashMap<FnItemIdx, FxHashMap<Name, FnItemIdx>>,
        mut interner: Interner,
        manifest: Option<CapabilityManifest>,
    ) -> Self {
        let intrinsic_list = intrinsics::all_intrinsics(&mut interner);
        let intrinsics = intrinsic_list.into_iter().collect();

        let some_name = Name::new(&mut interner, "Some");
        let none_name = Name::new(&mut interner, "None");
        let ok_name = Name::new(&mut interner, "Ok");
        let err_name = Name::new(&mut interner, "Err");
        let option_some = module_scope.constructors.get(&some_name).copied();
        let option_none = module_scope.constructors.get(&none_name).copied();
        let result_ok = module_scope.constructors.get(&ok_name).copied();
        let result_err = module_scope.constructors.get(&err_name).copied();

        let invalid_int_name = Name::new(&mut interner, "InvalidInt");
        let invalid_float_name = Name::new(&mut interner, "InvalidFloat");
        let parse_error_invalid_int = module_scope.constructors.get(&invalid_int_name).copied();
        let parse_error_invalid_float = module_scope.constructors.get(&invalid_float_name).copied();

        Interpreter {
            item_tree,
            module_scope,
            fn_bodies,
            fn_scope_overrides,
            interner,
            intrinsics,
            old_env: None,
            option_some,
            option_none,
            result_ok,
            result_err,
            parse_error_invalid_int,
            parse_error_invalid_float,
            manifest,
            env: Env::new(),
            current_fn: None,
        }
    }

    /// Consume the interpreter and return the interner (for display).
    pub fn into_interner(self) -> Interner {
        self.interner
    }

    /// Call a user-defined function by arena index (public wrapper for PBT).
    pub fn call_fn_by_idx(&mut self, fn_idx: FnItemIdx, args: Args) -> Result<Value, RuntimeError> {
        self.call_fn(fn_idx, args)
    }

    /// Borrow the item tree.
    pub fn item_tree(&self) -> &ItemTree {
        &self.item_tree
    }

    /// Borrow the module scope.
    pub fn module_scope(&self) -> &ModuleScope {
        &self.module_scope
    }

    /// Borrow the interner.
    pub fn interner(&self) -> &Interner {
        &self.interner
    }

    /// Mutably borrow the interner.
    pub fn interner_mut(&mut self) -> &mut Interner {
        &mut self.interner
    }

    /// Borrow the function bodies map.
    pub fn fn_bodies(&self) -> &FxHashMap<FnItemIdx, Body> {
        &self.fn_bodies
    }

    /// Find and run the `main` function.
    pub fn run_main(&mut self) -> Result<Value, RuntimeError> {
        let main_name = Name::new(&mut self.interner, "main");
        let main_idx = self
            .module_scope
            .functions
            .get(&main_name)
            .copied()
            .ok_or(RuntimeError::NoMainFunction)?;

        self.call_fn(main_idx, Args::new())
    }

    fn args_in_source_order(&self, args: &[CallArg]) -> Vec<ExprIdx> {
        args.iter()
            .map(|a| match a {
                CallArg::Positional(idx) => *idx,
                CallArg::Named { value, .. } => *value,
            })
            .collect()
    }

    fn param_names_for_fn_idx(&self, fn_idx: FnItemIdx) -> Vec<Name> {
        self.item_tree.functions[fn_idx]
            .params
            .iter()
            .map(|p| p.name)
            .collect()
    }

    fn param_names_for_fn_value(&self, fv: &FnValue) -> Option<Vec<Name>> {
        match fv {
            FnValue::User(fn_idx) => Some(self.param_names_for_fn_idx(*fn_idx)),
            FnValue::Lambda { params, body, .. } => self.lambda_param_names(body, params),
            _ => None,
        }
    }

    /// Bind evaluated argument values into parameter slots.
    ///
    /// `arg_values` must be in source order (left-to-right evaluation order).
    fn bind_call_values_for_param_names(
        &self,
        callee_name: &str,
        args: &[CallArg],
        arg_values: Vec<Value>,
        param_names: &[Name],
    ) -> Result<Args, RuntimeError> {
        if args.len() != param_names.len() {
            return Err(RuntimeError::ArityMismatch {
                callee: callee_name.to_string(),
                expected: param_names.len(),
                actual: args.len(),
            });
        }

        let mut out = Args::with_capacity(param_names.len());
        let has_named = args.iter().any(|arg| matches!(arg, CallArg::Named { .. }));
        if !has_named {
            for value in arg_values {
                out.push(value);
            }
            return Ok(out);
        }

        let mut slots: Vec<Option<Value>> = vec![None; param_names.len()];
        let mut next_pos = 0usize;
        let mut saw_named = false;
        for (arg, value) in args.iter().zip(arg_values.into_iter()) {
            match arg {
                CallArg::Positional(_) => {
                    if saw_named {
                        return Err(RuntimeError::TypeError(
                            "positional argument cannot appear after named argument".into(),
                        ));
                    }
                    while next_pos < slots.len() && slots[next_pos].is_some() {
                        next_pos += 1;
                    }
                    if next_pos >= slots.len() {
                        return Err(RuntimeError::ArityMismatch {
                            callee: callee_name.to_string(),
                            expected: param_names.len(),
                            actual: args.len(),
                        });
                    }
                    slots[next_pos] = Some(value);
                    next_pos += 1;
                }
                CallArg::Named { name, .. } => {
                    saw_named = true;
                    let Some(slot_idx) = param_names.iter().position(|param| param == name) else {
                        return Err(RuntimeError::TypeError(format!(
                            "unknown named argument `{}`",
                            name.resolve(&self.interner)
                        )));
                    };
                    if slots[slot_idx].is_some() {
                        return Err(RuntimeError::TypeError(format!(
                            "duplicate named argument `{}`",
                            name.resolve(&self.interner)
                        )));
                    }
                    slots[slot_idx] = Some(value);
                }
            }
        }

        for (idx, slot) in slots.into_iter().enumerate() {
            if let Some(value) = slot {
                out.push(value);
            } else {
                return Err(RuntimeError::TypeError(format!(
                    "missing argument for parameter `{}`",
                    param_names[idx].resolve(&self.interner)
                )));
            }
        }

        Ok(out)
    }

    fn fn_value_for_fn_idx(&self, fn_idx: FnItemIdx) -> Value {
        let fn_item = &self.item_tree.functions[fn_idx];
        if let Some(intr) = self.intrinsics.get(&fn_item.name) {
            Value::Fn(Box::new(FnValue::Intrinsic(*intr)))
        } else {
            Value::Fn(Box::new(FnValue::User(fn_idx)))
        }
    }

    fn lambda_param_names(
        &self,
        body: &Body,
        params: &[kyokara_hir_def::expr::PatIdx],
    ) -> Option<Vec<Name>> {
        let mut names = Vec::with_capacity(params.len());
        for pat_idx in params {
            match &body.pats[*pat_idx] {
                Pat::Bind { name } => names.push(*name),
                _ => return None,
            }
        }
        Some(names)
    }

    /// Call a user-defined function by index.
    fn call_fn(&mut self, fn_idx: FnItemIdx, args: Args) -> Result<Value, RuntimeError> {
        let prev_fn = self.current_fn.replace(fn_idx);
        let result = self.call_fn_impl(fn_idx, args);
        self.current_fn = prev_fn;
        result
    }

    fn call_fn_impl(&mut self, fn_idx: FnItemIdx, args: Args) -> Result<Value, RuntimeError> {
        let body = self
            .fn_bodies
            .get(&fn_idx)
            .cloned()
            .ok_or_else(|| RuntimeError::UnresolvedName("function body not found".into()))?;

        let (params, fn_name) = {
            let fn_item = &self.item_tree.functions[fn_idx];
            self.ensure_user_fn_caps_allowed(fn_item)?;
            (fn_item.params.clone(), fn_item.name)
        };

        self.ensure_arity(
            &format!("function `{}`", fn_name.resolve(&self.interner)),
            params.len(),
            args.len(),
        )?;

        self.env.push_scope();

        for (param, val) in params.iter().zip(args.into_iter()) {
            self.env.bind(param.name, val);
        }

        let has_requires = body.requires.is_some();
        let has_ensures = body.ensures.is_some();
        let has_invariant = body.invariant.is_some();

        let mut prev_old_env = None;
        let mut swapped_old_env = false;
        let fn_name_str = fn_name.resolve(&self.interner).to_string();

        let result = (|| -> Result<Value, RuntimeError> {
            if !has_requires && !has_ensures && !has_invariant {
                let return_val = match self.eval_expr_shared(&body, body.root)? {
                    ControlFlow::Value(v) | ControlFlow::Return(v) => v,
                };
                Ok(return_val)
            } else {
                if let Some(req_idx) = body.requires {
                    let val = self.eval_expr_shared(&body, req_idx)?.into_value();
                    if !matches!(val, Value::Bool(true)) {
                        Err(RuntimeError::PreconditionFailed(fn_name_str.clone()))?;
                    }
                }

                if has_ensures {
                    prev_old_env = self.old_env.replace(self.env.clone());
                    swapped_old_env = true;
                }

                let return_val = match self.eval_expr_shared(&body, body.root)? {
                    ControlFlow::Value(v) | ControlFlow::Return(v) => v,
                };

                if let Some(inv_idx) = body.invariant {
                    let val = self.eval_expr_shared(&body, inv_idx)?.into_value();
                    if !matches!(val, Value::Bool(true)) {
                        Err(RuntimeError::InvariantViolated(fn_name_str.clone()))?;
                    }
                }

                if let Some(ens_idx) = body.ensures {
                    let result_name = Name::new(&mut self.interner, "result");
                    self.env.bind(result_name, return_val.clone());
                    let val = self.eval_expr_shared(&body, ens_idx)?.into_value();
                    if !matches!(val, Value::Bool(true)) {
                        Err(RuntimeError::PostconditionFailed(fn_name_str.clone()))?;
                    }
                }

                Ok(return_val)
            }
        })();

        self.env.pop_scope();
        if swapped_old_env {
            self.old_env = prev_old_env;
        }

        result
    }

    fn eval_expr(
        &mut self,
        env: &mut Env,
        body: &Body,
        idx: ExprIdx,
    ) -> Result<ControlFlow, RuntimeError> {
        let expr = &body.exprs[idx];
        match expr {
            Expr::Missing => Err(RuntimeError::MissingExpr),
            Expr::Hole => Err(RuntimeError::HoleEncountered),

            Expr::Literal(lit) => Ok(ControlFlow::Value(self.eval_literal(lit))),

            Expr::Path(path) => {
                // First segment is the variable/function name.
                // Additional segments are field accesses (e.g., `p.x.y`).
                let name = path.segments[0];
                let mut val = self.resolve_name(env, name)?;
                for &field in &path.segments[1..] {
                    val = self.eval_field(val, field)?;
                }
                Ok(ControlFlow::Value(val))
            }

            Expr::Binary { op, lhs, rhs } => {
                let op = *op;
                let lhs = *lhs;
                let rhs = *rhs;
                match op {
                    BinaryOp::And | BinaryOp::Or => {
                        let lv = eval_propagate!(self, env, body, lhs);
                        match self.logical_eval_step(op, &lv)? {
                            LogicalEvalStep::ShortCircuit(v) => Ok(ControlFlow::Value(v)),
                            LogicalEvalStep::NeedRhs => {
                                let rv = eval_propagate!(self, env, body, rhs);
                                Ok(ControlFlow::Value(rv))
                            }
                        }
                    }
                    _ => {
                        let lv = eval_propagate!(self, env, body, lhs);
                        let rv = eval_propagate!(self, env, body, rhs);
                        self.eval_binary(op, lv, rv).map(ControlFlow::Value)
                    }
                }
            }

            Expr::Unary { op, operand } => {
                let op = *op;
                let operand = *operand;
                let v = eval_propagate!(self, env, body, operand);
                self.eval_unary(op, v).map(ControlFlow::Value)
            }

            Expr::Call { callee, args } => {
                let callee_idx = *callee;
                let args = args.clone();

                // ── Module-qualified / static method / method call resolution ──
                if let Expr::Field { base, field } = &body.exprs[callee_idx] {
                    let base_idx = *base;
                    let field_name = *field;

                    // Before evaluating base as a value, check if it's a synthetic
                    // module or a type with static methods.
                    if let Expr::Path(ref path) = body.exprs[base_idx]
                        && path.is_single()
                    {
                        let seg = path.segments[0];

                        // Module-qualified call: io.println(s), math.min(a, b)
                        if self.module_scope.imported_modules.contains(&seg)
                            && let Some(mod_fns) = self.module_scope.synthetic_modules.get(&seg)
                            && let Some(&fn_idx) = mod_fns.get(&field_name)
                        {
                            let source_order = self.args_in_source_order(&args);
                            let mut arg_values = Vec::with_capacity(source_order.len());
                            for idx in &source_order {
                                let v = eval_propagate!(self, env, body, *idx);
                                arg_values.push(v);
                            }
                            let param_names = self.param_names_for_fn_idx(fn_idx);
                            let callee_name = format!(
                                "function `{}`",
                                self.item_tree.functions[fn_idx]
                                    .name
                                    .resolve(&self.interner)
                            );
                            let arg_vals = self.bind_call_values_for_param_names(
                                &callee_name,
                                &args,
                                arg_values,
                                &param_names,
                            )?;
                            let fn_val = self.fn_value_for_fn_idx(fn_idx);
                            return self.call_value(fn_val, arg_vals).map(ControlFlow::Value);
                        }

                        // Static method call: List.new(), Map.new()
                        if let Some(&fn_idx) =
                            self.module_scope.static_methods.get(&(seg, field_name))
                        {
                            let source_order = self.args_in_source_order(&args);
                            let mut arg_values = Vec::with_capacity(source_order.len());
                            for idx in &source_order {
                                let v = eval_propagate!(self, env, body, *idx);
                                arg_values.push(v);
                            }
                            let param_names = self.param_names_for_fn_idx(fn_idx);
                            let callee_name = format!(
                                "function `{}`",
                                self.item_tree.functions[fn_idx]
                                    .name
                                    .resolve(&self.interner)
                            );
                            let arg_vals = self.bind_call_values_for_param_names(
                                &callee_name,
                                &args,
                                arg_values,
                                &param_names,
                            )?;
                            let fn_val = self.fn_value_for_fn_idx(fn_idx);
                            return self.call_value(fn_val, arg_vals).map(ControlFlow::Value);
                        }
                    }

                    // Method call: value.method(args)
                    let base_val = eval_propagate!(self, env, body, base_idx);

                    if let Some(type_name) = self.value_type_name(&base_val)
                        && let Some(&fn_idx) =
                            self.module_scope.methods.get(&(type_name, field_name))
                    {
                        let source_order = self.args_in_source_order(&args);
                        let mut arg_values = Vec::with_capacity(source_order.len());
                        for idx in &source_order {
                            let v = eval_propagate!(self, env, body, *idx);
                            arg_values.push(v);
                        }
                        let full_param_names = self.param_names_for_fn_idx(fn_idx);
                        let method_param_names: Vec<Name> =
                            full_param_names.iter().skip(1).copied().collect();
                        let callee_name = format!(
                            "method `{}`",
                            self.item_tree.functions[fn_idx]
                                .name
                                .resolve(&self.interner)
                        );
                        let mut arg_vals = Args::with_capacity(method_param_names.len() + 1);
                        arg_vals.push(base_val);
                        let bound_args = self.bind_call_values_for_param_names(
                            &callee_name,
                            &args,
                            arg_values,
                            &method_param_names,
                        )?;
                        for value in bound_args {
                            arg_vals.push(value);
                        }
                        let method_fn = self.fn_value_for_fn_idx(fn_idx);
                        return self.call_value(method_fn, arg_vals).map(ControlFlow::Value);
                    }

                    // Not a method — fall through to field access + call.
                    let callee_val = self.eval_field(base_val, field_name)?;
                    let source_order = self.args_in_source_order(&args);
                    let mut arg_vals = Args::with_capacity(source_order.len());
                    for idx in &source_order {
                        let v = eval_propagate!(self, env, body, *idx);
                        arg_vals.push(v);
                    }
                    return self
                        .call_value(callee_val, arg_vals)
                        .map(ControlFlow::Value);
                }

                let callee_val = eval_propagate!(self, env, body, callee_idx);

                let source_order = self.args_in_source_order(&args);
                let mut evaluated_args = Vec::with_capacity(source_order.len());
                for idx in &source_order {
                    let v = eval_propagate!(self, env, body, *idx);
                    evaluated_args.push(v);
                }
                let arg_vals = if let Value::Fn(ref fv) = callee_val
                    && let Some(param_names) = self.param_names_for_fn_value(fv)
                {
                    self.bind_call_values_for_param_names(
                        "callable",
                        &args,
                        evaluated_args,
                        &param_names,
                    )?
                } else {
                    let mut direct = Args::with_capacity(evaluated_args.len());
                    for value in evaluated_args {
                        direct.push(value);
                    }
                    direct
                };
                self.call_value(callee_val, arg_vals)
                    .map(ControlFlow::Value)
            }

            Expr::Field { base, field } => {
                let base_idx = *base;
                let field = *field;
                let base_val = eval_propagate!(self, env, body, base_idx);
                self.eval_field(base_val, field).map(ControlFlow::Value)
            }

            Expr::Index { base, index } => {
                let base_idx = *base;
                let index_idx = *index;
                let base_val = eval_propagate!(self, env, body, base_idx);
                let index_val = eval_propagate!(self, env, body, index_idx);
                self.eval_index(base_val, index_val).map(ControlFlow::Value)
            }

            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond_idx = *condition;
                let then_idx = *then_branch;
                let else_idx = *else_branch;
                let cond = eval_propagate!(self, env, body, cond_idx);
                match cond {
                    Value::Bool(true) => self.eval_expr(env, body, then_idx),
                    Value::Bool(false) => {
                        if let Some(else_idx) = else_idx {
                            self.eval_expr(env, body, else_idx)
                        } else {
                            Ok(ControlFlow::Value(Value::Unit))
                        }
                    }
                    _ => Err(RuntimeError::TypeError("if condition must be Bool".into())),
                }
            }

            Expr::Match { scrutinee, arms } => {
                let scrutinee_idx = *scrutinee;
                let arms = arms.clone();
                let scrutinee_val = eval_propagate!(self, env, body, scrutinee_idx);
                self.eval_match(env, body, scrutinee_val, &arms)
            }

            Expr::Block { stmts, tail } => {
                let stmts = stmts.clone();
                let tail = *tail;
                self.eval_block(env, body, &stmts, tail)
            }

            Expr::Return(val) => {
                let val = *val;
                let v = if let Some(idx) = val {
                    eval_propagate!(self, env, body, idx)
                } else {
                    Value::Unit
                };
                Ok(ControlFlow::Return(v))
            }

            Expr::RecordLit { path, fields } => {
                let path = path.clone();
                let fields = fields.clone();
                self.eval_record_lit(env, body, path.as_ref(), &fields)
            }

            Expr::Lambda {
                params,
                body: lambda_body,
            } => {
                let param_pats: Vec<_> = params.iter().map(|(pat, _)| *pat).collect();
                let lambda_body_idx = *lambda_body;
                Ok(ControlFlow::Value(Value::Fn(Box::new(FnValue::Lambda {
                    params: param_pats,
                    body_expr: lambda_body_idx,
                    body: Rc::new(Body {
                        exprs: body.exprs.clone(),
                        pats: body.pats.clone(),
                        root: lambda_body_idx,
                        requires: None,
                        ensures: None,
                        invariant: None,
                        scopes: Default::default(),
                        pat_scopes: Vec::new(),
                        expr_scopes: Default::default(),
                        expr_source_map: Default::default(),
                        pat_source_map: Default::default(),
                        local_binding_meta: Default::default(),
                    }),
                    env: env.clone(),
                }))))
            }

            Expr::Old(inner) => {
                let inner = *inner;
                if let Some(mut snapshot) = self.old_env.take() {
                    let result = self.eval_expr(&mut snapshot, body, inner);
                    self.old_env = Some(snapshot);
                    result
                } else {
                    // No old_env available — fall back to current env.
                    self.eval_expr(env, body, inner)
                }
            }
        }
    }

    /// Evaluate an expression using the interpreter's shared environment (`self.env`).
    ///
    /// This avoids per-call `Env::new()` allocation and eliminates the need
    /// to clone Vec fields out of expressions (since `body` and `self.env` are
    /// separate objects with no borrow conflict).
    fn eval_expr_shared(&mut self, body: &Body, idx: ExprIdx) -> Result<ControlFlow, RuntimeError> {
        let expr = &body.exprs[idx];
        match expr {
            Expr::Missing => Err(RuntimeError::MissingExpr),
            Expr::Hole => Err(RuntimeError::HoleEncountered),

            Expr::Literal(lit) => Ok(ControlFlow::Value(self.eval_literal(lit))),

            Expr::Path(path) => {
                let name = path.segments[0];
                let mut val = self.resolve_name_shared(name)?;
                for &field in &path.segments[1..] {
                    val = self.eval_field(val, field)?;
                }
                Ok(ControlFlow::Value(val))
            }

            Expr::Binary { op, lhs, rhs } => {
                let op = *op;
                let lhs = *lhs;
                let rhs = *rhs;
                match op {
                    BinaryOp::And | BinaryOp::Or => {
                        let lv = eval_propagate_shared!(self, body, lhs);
                        match self.logical_eval_step(op, &lv)? {
                            LogicalEvalStep::ShortCircuit(v) => Ok(ControlFlow::Value(v)),
                            LogicalEvalStep::NeedRhs => {
                                let rv = eval_propagate_shared!(self, body, rhs);
                                Ok(ControlFlow::Value(rv))
                            }
                        }
                    }
                    _ => {
                        let lv = eval_propagate_shared!(self, body, lhs);
                        let rv = eval_propagate_shared!(self, body, rhs);
                        self.eval_binary(op, lv, rv).map(ControlFlow::Value)
                    }
                }
            }

            Expr::Unary { op, operand } => {
                let op = *op;
                let operand = *operand;
                let v = eval_propagate_shared!(self, body, operand);
                self.eval_unary(op, v).map(ControlFlow::Value)
            }

            Expr::Call { callee, args } => {
                let callee_idx = *callee;

                // ── Module-qualified / static method / method call resolution (shared path) ──
                if let Expr::Field { base, field } = &body.exprs[callee_idx] {
                    let base_idx = *base;
                    let field_name = *field;

                    // Before evaluating base as a value, check module/static dispatch.
                    if let Expr::Path(ref path) = body.exprs[base_idx]
                        && path.is_single()
                    {
                        let seg = path.segments[0];

                        // Module-qualified call: io.println(s)
                        if self.module_scope.imported_modules.contains(&seg)
                            && let Some(mod_fns) = self.module_scope.synthetic_modules.get(&seg)
                            && let Some(&fn_idx) = mod_fns.get(&field_name)
                        {
                            let source_order = self.args_in_source_order(args);
                            let mut arg_values = Vec::with_capacity(source_order.len());
                            for idx in &source_order {
                                let v = eval_propagate_shared!(self, body, *idx);
                                arg_values.push(v);
                            }
                            let param_names = self.param_names_for_fn_idx(fn_idx);
                            let callee_name = format!(
                                "function `{}`",
                                self.item_tree.functions[fn_idx]
                                    .name
                                    .resolve(&self.interner)
                            );
                            let arg_vals = self.bind_call_values_for_param_names(
                                &callee_name,
                                args,
                                arg_values,
                                &param_names,
                            )?;
                            let fn_val = self.fn_value_for_fn_idx(fn_idx);
                            return self.call_value(fn_val, arg_vals).map(ControlFlow::Value);
                        }

                        // Static method call: List.new()
                        if let Some(&fn_idx) =
                            self.module_scope.static_methods.get(&(seg, field_name))
                        {
                            let source_order = self.args_in_source_order(args);
                            let mut arg_values = Vec::with_capacity(source_order.len());
                            for idx in &source_order {
                                let v = eval_propagate_shared!(self, body, *idx);
                                arg_values.push(v);
                            }
                            let param_names = self.param_names_for_fn_idx(fn_idx);
                            let callee_name = format!(
                                "function `{}`",
                                self.item_tree.functions[fn_idx]
                                    .name
                                    .resolve(&self.interner)
                            );
                            let arg_vals = self.bind_call_values_for_param_names(
                                &callee_name,
                                args,
                                arg_values,
                                &param_names,
                            )?;
                            let fn_val = self.fn_value_for_fn_idx(fn_idx);
                            return self.call_value(fn_val, arg_vals).map(ControlFlow::Value);
                        }
                    }

                    let base_val = eval_propagate_shared!(self, body, base_idx);

                    if let Some(type_name) = self.value_type_name(&base_val)
                        && let Some(&fn_idx) =
                            self.module_scope.methods.get(&(type_name, field_name))
                    {
                        let source_order = self.args_in_source_order(args);
                        let mut arg_values = Vec::with_capacity(source_order.len());
                        for idx in &source_order {
                            let v = eval_propagate_shared!(self, body, *idx);
                            arg_values.push(v);
                        }
                        let full_param_names = self.param_names_for_fn_idx(fn_idx);
                        let method_param_names: Vec<Name> =
                            full_param_names.iter().skip(1).copied().collect();
                        let callee_name = format!(
                            "method `{}`",
                            self.item_tree.functions[fn_idx]
                                .name
                                .resolve(&self.interner)
                        );
                        let mut arg_vals = Args::with_capacity(method_param_names.len() + 1);
                        arg_vals.push(base_val);
                        let bound_args = self.bind_call_values_for_param_names(
                            &callee_name,
                            args,
                            arg_values,
                            &method_param_names,
                        )?;
                        for value in bound_args {
                            arg_vals.push(value);
                        }
                        let method_fn = self.fn_value_for_fn_idx(fn_idx);
                        return self.call_value(method_fn, arg_vals).map(ControlFlow::Value);
                    }

                    // Not a method — fall through to field access + call.
                    let callee_val = self.eval_field(base_val, field_name)?;
                    let source_order = self.args_in_source_order(args);
                    let mut arg_vals = Args::with_capacity(source_order.len());
                    for idx in &source_order {
                        let v = eval_propagate_shared!(self, body, *idx);
                        arg_vals.push(v);
                    }
                    return self
                        .call_value(callee_val, arg_vals)
                        .map(ControlFlow::Value);
                }

                let callee_val = eval_propagate_shared!(self, body, callee_idx);

                let source_order = self.args_in_source_order(args);
                let mut evaluated_args = Vec::with_capacity(source_order.len());
                for idx in &source_order {
                    let v = eval_propagate_shared!(self, body, *idx);
                    evaluated_args.push(v);
                }
                let arg_vals = if let Value::Fn(ref fv) = callee_val
                    && let Some(param_names) = self.param_names_for_fn_value(fv)
                {
                    self.bind_call_values_for_param_names(
                        "callable",
                        args,
                        evaluated_args,
                        &param_names,
                    )?
                } else {
                    let mut direct = Args::with_capacity(evaluated_args.len());
                    for value in evaluated_args {
                        direct.push(value);
                    }
                    direct
                };
                self.call_value(callee_val, arg_vals)
                    .map(ControlFlow::Value)
            }

            Expr::Field { base, field } => {
                let base_idx = *base;
                let field = *field;
                let base_val = eval_propagate_shared!(self, body, base_idx);
                self.eval_field(base_val, field).map(ControlFlow::Value)
            }

            Expr::Index { base, index } => {
                let base_idx = *base;
                let index_idx = *index;
                let base_val = eval_propagate_shared!(self, body, base_idx);
                let index_val = eval_propagate_shared!(self, body, index_idx);
                self.eval_index(base_val, index_val).map(ControlFlow::Value)
            }

            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond_idx = *condition;
                let then_idx = *then_branch;
                let else_idx = *else_branch;
                let cond = eval_propagate_shared!(self, body, cond_idx);
                match cond {
                    Value::Bool(true) => self.eval_expr_shared(body, then_idx),
                    Value::Bool(false) => {
                        if let Some(else_idx) = else_idx {
                            self.eval_expr_shared(body, else_idx)
                        } else {
                            Ok(ControlFlow::Value(Value::Unit))
                        }
                    }
                    _ => Err(RuntimeError::TypeError("if condition must be Bool".into())),
                }
            }

            Expr::Match { scrutinee, arms } => {
                let scrutinee_idx = *scrutinee;
                let scrutinee_val = eval_propagate_shared!(self, body, scrutinee_idx);
                self.eval_match_shared(body, scrutinee_val, arms)
            }

            Expr::Block { stmts, tail } => {
                let tail = *tail;
                self.eval_block_shared(body, stmts, tail)
            }

            Expr::Return(val) => {
                let val = *val;
                let v = if let Some(idx) = val {
                    eval_propagate_shared!(self, body, idx)
                } else {
                    Value::Unit
                };
                Ok(ControlFlow::Return(v))
            }

            Expr::RecordLit { path, fields } => {
                self.eval_record_lit_shared(body, path.as_ref(), fields)
            }

            Expr::Lambda {
                params,
                body: lambda_body,
            } => {
                let param_pats: Vec<_> = params.iter().map(|(pat, _)| *pat).collect();
                let lambda_body_idx = *lambda_body;
                Ok(ControlFlow::Value(Value::Fn(Box::new(FnValue::Lambda {
                    params: param_pats,
                    body_expr: lambda_body_idx,
                    body: Rc::new(Body {
                        exprs: body.exprs.clone(),
                        pats: body.pats.clone(),
                        root: lambda_body_idx,
                        requires: None,
                        ensures: None,
                        invariant: None,
                        scopes: Default::default(),
                        pat_scopes: Vec::new(),
                        expr_scopes: Default::default(),
                        expr_source_map: Default::default(),
                        pat_source_map: Default::default(),
                        local_binding_meta: Default::default(),
                    }),
                    env: self.env.clone(),
                }))))
            }

            Expr::Old(inner) => {
                let inner = *inner;
                if let Some(mut snapshot) = self.old_env.take() {
                    let result = self.eval_expr(&mut snapshot, body, inner);
                    self.old_env = Some(snapshot);
                    result
                } else {
                    self.eval_expr_shared(body, inner)
                }
            }
        }
    }

    #[inline(always)]
    fn eval_literal(&self, lit: &Literal) -> Value {
        match lit {
            Literal::Int(n) => Value::Int(*n),
            Literal::Float(f) => Value::Float(*f),
            Literal::String(s) => Value::String(s.clone()),
            Literal::Char(c) => Value::Char(*c),
            Literal::Bool(b) => Value::Bool(*b),
        }
    }

    #[inline(always)]
    fn resolve_name(&self, env: &Env, name: Name) -> Result<Value, RuntimeError> {
        // 1. Local variables (most common in hot loops).
        if let Some(val) = env.lookup(name) {
            return Ok(val.clone());
        }

        // 2. Function-local module overrides (project mode): resolve names in the
        // source module of the currently executing function before global scope.
        if let Some(cur_fn) = self.current_fn
            && let Some(overrides) = self.fn_scope_overrides.get(&cur_fn)
            && let Some(&fn_idx) = overrides.get(&name)
        {
            return Ok(Value::Fn(Box::new(FnValue::User(fn_idx))));
        }

        // 3. Module-level user functions (with bodies).
        //    Intrinsic stubs are no longer in scope.functions — they're only
        //    reachable via methods, modules, or static methods.
        if let Some(&fn_idx) = self.module_scope.functions.get(&name)
            && self.fn_bodies.contains_key(&fn_idx)
        {
            return Ok(Value::Fn(Box::new(FnValue::User(fn_idx))));
        }

        // 4. ADT constructors.
        if let Some(&(type_idx, variant_idx)) = self.module_scope.constructors.get(&name) {
            let type_item = &self.item_tree.types[type_idx];
            if let TypeDefKind::Adt { variants } = &type_item.kind {
                let variant = &variants[variant_idx];
                if variant.fields.is_empty() {
                    return Ok(Value::Adt {
                        type_idx,
                        variant: variant_idx,
                        fields: Vec::new(),
                    });
                }
                return Ok(Value::Fn(Box::new(FnValue::Constructor {
                    type_idx,
                    variant_idx,
                    arity: variant.fields.len(),
                })));
            }
        }

        let name_str = name.resolve(&self.interner);
        Err(RuntimeError::UnresolvedName(name_str.to_string()))
    }

    #[inline(always)]
    fn eval_binary(&self, op: BinaryOp, lhs: Value, rhs: Value) -> Result<Value, RuntimeError> {
        match (op, &lhs, &rhs) {
            // Int arithmetic (checked to prevent overflow panics).
            (BinaryOp::Add, Value::Int(a), Value::Int(b)) => a
                .checked_add(*b)
                .map(Value::Int)
                .ok_or(RuntimeError::IntegerOverflow),
            (BinaryOp::Sub, Value::Int(a), Value::Int(b)) => a
                .checked_sub(*b)
                .map(Value::Int)
                .ok_or(RuntimeError::IntegerOverflow),
            (BinaryOp::Mul, Value::Int(a), Value::Int(b)) => a
                .checked_mul(*b)
                .map(Value::Int)
                .ok_or(RuntimeError::IntegerOverflow),
            (BinaryOp::Div, Value::Int(_), Value::Int(0)) => Err(RuntimeError::DivisionByZero),
            (BinaryOp::Div, Value::Int(a), Value::Int(b)) => a
                .checked_div(*b)
                .map(Value::Int)
                .ok_or(RuntimeError::IntegerOverflow),
            (BinaryOp::Mod, Value::Int(_), Value::Int(0)) => Err(RuntimeError::DivisionByZero),
            (BinaryOp::Mod, Value::Int(a), Value::Int(b)) => a
                .checked_rem(*b)
                .map(Value::Int)
                .ok_or(RuntimeError::IntegerOverflow),

            // Float arithmetic.
            (BinaryOp::Add, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
            (BinaryOp::Sub, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
            (BinaryOp::Mul, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
            (BinaryOp::Div, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a / b)),
            (BinaryOp::Mod, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a % b)),

            // Bitwise operations (Int only).
            (BinaryOp::BitAnd, Value::Int(a), Value::Int(b)) => Ok(Value::Int(a & b)),
            (BinaryOp::BitOr, Value::Int(a), Value::Int(b)) => Ok(Value::Int(a | b)),
            (BinaryOp::BitXor, Value::Int(a), Value::Int(b)) => Ok(Value::Int(a ^ b)),
            (BinaryOp::Shl, Value::Int(a), Value::Int(b)) => {
                if *b < 0 || *b >= 64 {
                    Err(RuntimeError::TypeError(format!(
                        "shift amount {b} out of range (0..63)"
                    )))
                } else {
                    Ok(Value::Int(a.wrapping_shl(*b as u32)))
                }
            }
            (BinaryOp::Shr, Value::Int(a), Value::Int(b)) => {
                if *b < 0 || *b >= 64 {
                    Err(RuntimeError::TypeError(format!(
                        "shift amount {b} out of range (0..63)"
                    )))
                } else {
                    Ok(Value::Int(a.wrapping_shr(*b as u32)))
                }
            }

            // String concatenation via +.
            (BinaryOp::Add, Value::String(a), Value::String(b)) => {
                Ok(Value::String(format!("{a}{b}")))
            }

            // Int comparisons.
            (BinaryOp::Eq, Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a == b)),
            (BinaryOp::NotEq, Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a != b)),
            (BinaryOp::Lt, Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a < b)),
            (BinaryOp::Gt, Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a > b)),
            (BinaryOp::LtEq, Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a <= b)),
            (BinaryOp::GtEq, Value::Int(a), Value::Int(b)) => Ok(Value::Bool(a >= b)),

            // Float comparisons.
            (BinaryOp::Eq, Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a == b)),
            (BinaryOp::NotEq, Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a != b)),
            (BinaryOp::Lt, Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a < b)),
            (BinaryOp::Gt, Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a > b)),
            (BinaryOp::LtEq, Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a <= b)),
            (BinaryOp::GtEq, Value::Float(a), Value::Float(b)) => Ok(Value::Bool(a >= b)),

            // Bool equality.
            (BinaryOp::Eq, Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(a == b)),
            (BinaryOp::NotEq, Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(a != b)),

            // String equality.
            (BinaryOp::Eq, Value::String(a), Value::String(b)) => Ok(Value::Bool(a == b)),
            (BinaryOp::NotEq, Value::String(a), Value::String(b)) => Ok(Value::Bool(a != b)),

            // Char equality.
            (BinaryOp::Eq, Value::Char(a), Value::Char(b)) => Ok(Value::Bool(a == b)),
            (BinaryOp::NotEq, Value::Char(a), Value::Char(b)) => Ok(Value::Bool(a != b)),

            _ => Err(RuntimeError::TypeError(format!(
                "cannot apply {op:?} to {:?} and {:?}",
                std::mem::discriminant(&lhs),
                std::mem::discriminant(&rhs),
            ))),
        }
    }

    #[inline(always)]
    fn logical_eval_step(
        &self,
        op: BinaryOp,
        lhs: &Value,
    ) -> Result<LogicalEvalStep, RuntimeError> {
        match op {
            BinaryOp::And => match lhs {
                Value::Bool(false) => Ok(LogicalEvalStep::ShortCircuit(Value::Bool(false))),
                Value::Bool(true) => Ok(LogicalEvalStep::NeedRhs),
                _ => Err(RuntimeError::TypeError(format!(
                    "expected Bool for &&, got {:?}",
                    std::mem::discriminant(lhs),
                ))),
            },
            BinaryOp::Or => match lhs {
                Value::Bool(true) => Ok(LogicalEvalStep::ShortCircuit(Value::Bool(true))),
                Value::Bool(false) => Ok(LogicalEvalStep::NeedRhs),
                _ => Err(RuntimeError::TypeError(format!(
                    "expected Bool for ||, got {:?}",
                    std::mem::discriminant(lhs),
                ))),
            },
            _ => unreachable!("logical_eval_step called for non-logical op"),
        }
    }

    #[inline(always)]
    fn eval_unary(&self, op: UnaryOp, val: Value) -> Result<Value, RuntimeError> {
        match (op, &val) {
            (UnaryOp::Neg, Value::Int(n)) => n
                .checked_neg()
                .map(Value::Int)
                .ok_or(RuntimeError::IntegerOverflow),
            (UnaryOp::Neg, Value::Float(f)) => Ok(Value::Float(-f)),
            (UnaryOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
            (UnaryOp::BitNot, Value::Int(n)) => Ok(Value::Int(!n)),
            _ => Err(RuntimeError::TypeError(format!(
                "cannot apply {op:?} to {val:?}"
            ))),
        }
    }

    fn ensure_arity(
        &self,
        callee: &str,
        expected: usize,
        actual: usize,
    ) -> Result<(), RuntimeError> {
        if expected == actual {
            Ok(())
        } else {
            Err(RuntimeError::ArityMismatch {
                callee: callee.to_string(),
                expected,
                actual,
            })
        }
    }

    fn call_value(&mut self, callee: Value, args: Args) -> Result<Value, RuntimeError> {
        match callee {
            Value::Fn(fv) => match *fv {
                FnValue::User(fn_idx) => self.call_fn(fn_idx, args),
                FnValue::Intrinsic(intr) if intr.needs_interpreter() => {
                    self.check_intrinsic_cap(intr)?;
                    self.call_complex_intrinsic(intr, args)
                }
                FnValue::Intrinsic(intr) => {
                    self.check_intrinsic_cap(intr)?;
                    intr.call(args)
                }
                FnValue::Lambda {
                    params,
                    body_expr,
                    body,
                    env: captured_env,
                } => {
                    self.ensure_arity("lambda", params.len(), args.len())?;
                    let mut env = captured_env;
                    env.push_scope();
                    for (pat_idx, val) in params.iter().zip(args) {
                        self.bind_pat(&body, *pat_idx, &val, &mut env)?;
                    }
                    let result = self.eval_expr(&mut env, &body, body_expr)?;
                    Ok(result.into_value())
                }
                FnValue::Constructor {
                    type_idx,
                    variant_idx,
                    arity,
                    ..
                } => Ok(Value::Adt {
                    // Constructor calls must match declared field count.
                    // Type checking should guarantee this, but runtime calls
                    // can also happen through values and API entrypoints.
                    type_idx,
                    variant: variant_idx,
                    fields: {
                        self.ensure_arity("constructor", arity, args.len())?;
                        args.into_vec()
                    },
                }),
            },
            _ => Err(RuntimeError::TypeError(
                "called value is not a function".into(),
            )),
        }
    }

    fn ensure_user_fn_caps_allowed(
        &self,
        fn_item: &kyokara_hir_def::item_tree::FnItem,
    ) -> Result<(), RuntimeError> {
        let Some(manifest) = &self.manifest else {
            return Ok(());
        };

        if fn_item.with_caps.is_empty() {
            return Ok(());
        }

        for cap_ref in &fn_item.with_caps {
            if let TypeRef::Path { path, .. } = cap_ref
                && let Some(name) = path.last()
            {
                let cap_str = name.resolve(&self.interner);
                if !manifest.is_granted(cap_str) {
                    return Err(RuntimeError::CapabilityDenied {
                        capability: cap_str.to_string(),
                        function: fn_item.name.resolve(&self.interner).to_string(),
                    });
                }
            }
        }

        Ok(())
    }

    fn make_some(&self, val: Value) -> Value {
        let (type_idx, variant) = self.option_some.expect("Option::Some not registered");
        Value::Adt {
            type_idx,
            variant,
            fields: vec![val],
        }
    }

    fn make_none(&self) -> Value {
        let (type_idx, variant) = self.option_none.expect("Option::None not registered");
        Value::Adt {
            type_idx,
            variant,
            fields: vec![],
        }
    }

    fn make_ok(&self, val: Value) -> Value {
        let (type_idx, variant) = self.result_ok.expect("Result::Ok not registered");
        Value::Adt {
            type_idx,
            variant,
            fields: vec![val],
        }
    }

    fn make_err(&self, val: Value) -> Value {
        let (type_idx, variant) = self.result_err.expect("Result::Err not registered");
        Value::Adt {
            type_idx,
            variant,
            fields: vec![val],
        }
    }

    fn make_invalid_int(&self, msg: String) -> Value {
        let (type_idx, variant) = self
            .parse_error_invalid_int
            .expect("ParseError::InvalidInt not registered");
        Value::Adt {
            type_idx,
            variant,
            fields: vec![Value::String(msg)],
        }
    }

    fn make_invalid_float(&self, msg: String) -> Value {
        let (type_idx, variant) = self
            .parse_error_invalid_float
            .expect("ParseError::InvalidFloat not registered");
        Value::Adt {
            type_idx,
            variant,
            fields: vec![Value::String(msg)],
        }
    }

    fn check_intrinsic_cap(&self, intr: IntrinsicFn) -> Result<(), RuntimeError> {
        if let Some(ref manifest) = self.manifest
            && let Some(cap) = intr.required_capability()
            && !manifest.is_granted(cap)
        {
            return Err(RuntimeError::CapabilityDenied {
                capability: cap.to_string(),
                function: format!("{intr:?}"),
            });
        }
        Ok(())
    }

    fn call_complex_intrinsic(
        &mut self,
        intr: IntrinsicFn,
        args: Args,
    ) -> Result<Value, RuntimeError> {
        match intr {
            IntrinsicFn::ListGet => {
                let Value::List(xs) = &args[0] else {
                    return Err(RuntimeError::TypeError("list_get expects a List".into()));
                };
                let Value::Int(i) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "list_get expects an Int index".into(),
                    ));
                };
                let idx = *i as usize;
                if let Some(val) = xs.get(idx) {
                    Ok(self.make_some(val.clone()))
                } else {
                    Ok(self.make_none())
                }
            }
            IntrinsicFn::ListHead => {
                let Value::List(xs) = &args[0] else {
                    return Err(RuntimeError::TypeError("list_head expects a List".into()));
                };
                if let Some(val) = xs.first() {
                    Ok(self.make_some(val.clone()))
                } else {
                    Ok(self.make_none())
                }
            }
            IntrinsicFn::MapGet => {
                let Value::Map(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError("map_get expects a Map".into()));
                };
                let key = MapKey::from_value(&args[1])?;
                if let Some(v) = entries.get(&key) {
                    Ok(self.make_some(v.clone()))
                } else {
                    Ok(self.make_none())
                }
            }
            IntrinsicFn::ListMap => {
                let Value::List(xs) = &args[0] else {
                    return Err(RuntimeError::TypeError("list_map expects a List".into()));
                };
                let f = args[1].clone();
                let xs = xs.clone();
                let mut result = Vec::with_capacity(xs.len());
                for item in xs {
                    let val = self.call_value(f.clone(), smallvec::smallvec![item])?;
                    result.push(val);
                }
                Ok(Value::List(result))
            }
            IntrinsicFn::ListFilter => {
                let Value::List(xs) = &args[0] else {
                    return Err(RuntimeError::TypeError("list_filter expects a List".into()));
                };
                let f = args[1].clone();
                let xs = xs.clone();
                let mut result = Vec::new();
                for item in xs {
                    let keep = self.call_value(f.clone(), smallvec::smallvec![item.clone()])?;
                    if matches!(keep, Value::Bool(true)) {
                        result.push(item);
                    }
                }
                Ok(Value::List(result))
            }
            IntrinsicFn::ListFold => {
                let Value::List(xs) = &args[0] else {
                    return Err(RuntimeError::TypeError("list_fold expects a List".into()));
                };
                let xs = xs.clone();
                let mut acc = args[1].clone();
                let f = args[2].clone();
                for item in xs {
                    acc = self.call_value(f.clone(), smallvec::smallvec![acc, item])?;
                }
                Ok(acc)
            }
            IntrinsicFn::ListSortBy => {
                let Value::List(xs) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "list_sort_by expects a List".into(),
                    ));
                };
                let cmp_fn = args[1].clone();
                let mut items = xs.clone();
                // Insertion sort: stable, simple, avoids &mut self borrow
                // issues with Rust's sort_by.
                let len = items.len();
                for i in 1..len {
                    let mut j = i;
                    while j > 0 {
                        let cmp_result = self.call_value(
                            cmp_fn.clone(),
                            smallvec::smallvec![items[j - 1].clone(), items[j].clone()],
                        )?;
                        match cmp_result {
                            Value::Int(n) if n > 0 => {
                                items.swap(j - 1, j);
                                j -= 1;
                            }
                            Value::Int(_) => break,
                            _ => {
                                return Err(RuntimeError::TypeError(
                                    "list_sort_by: comparator must return Int".into(),
                                ));
                            }
                        }
                    }
                }
                Ok(Value::List(items))
            }
            IntrinsicFn::ParseInt => {
                let Value::String(s) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "parse_int expects a String argument".into(),
                    ));
                };
                match s.parse::<i64>() {
                    Ok(n) => Ok(self.make_ok(Value::Int(n))),
                    Err(e) => Ok(self.make_err(self.make_invalid_int(format!("{e}")))),
                }
            }
            IntrinsicFn::ParseFloat => {
                let Value::String(s) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "parse_float expects a String argument".into(),
                    ));
                };
                match s.parse::<f64>() {
                    Ok(n) => Ok(self.make_ok(Value::Float(n))),
                    Err(e) => Ok(self.make_err(self.make_invalid_float(format!("{e}")))),
                }
            }
            _ => Err(RuntimeError::TypeError("unknown complex intrinsic".into())),
        }
    }

    fn eval_field(&self, base: Value, field: Name) -> Result<Value, RuntimeError> {
        match base {
            Value::Record { fields, .. } => {
                for (name, val) in &fields {
                    if *name == field {
                        return Ok(val.clone());
                    }
                }
                let field_str = field.resolve(&self.interner);
                Err(RuntimeError::UnresolvedName(format!(
                    "field `{field_str}` not found"
                )))
            }
            Value::Adt { fields, .. } => {
                // ADT field access by name isn't standard, but record-style ADTs
                // might need it. For now, error.
                let _ = fields;
                Err(RuntimeError::TypeError(
                    "field access on ADT values not supported".into(),
                ))
            }
            _ => Err(RuntimeError::TypeError(
                "field access on non-record value".into(),
            )),
        }
    }

    /// Map a runtime value to its well-known type name for method lookup.
    fn value_type_name(&self, val: &Value) -> Option<Name> {
        let wk = &self.module_scope.well_known_names;
        match val {
            Value::String(_) => wk.string,
            Value::Int(_) => wk.int,
            Value::Float(_) => wk.float,
            Value::Bool(_) => wk.bool_,
            Value::Char(_) => wk.char_,
            Value::List(_) => wk.list,
            Value::Map(_) => wk.map,
            Value::Adt { type_idx, .. } => Some(self.item_tree.types[*type_idx].name),
            Value::Record {
                type_idx: Some(idx),
                ..
            } => Some(self.item_tree.types[*idx].name),
            _ => None,
        }
    }

    fn eval_index(&self, base: Value, index: Value) -> Result<Value, RuntimeError> {
        match (&base, &index) {
            (Value::List(items), Value::Int(i)) => {
                let i = *i;
                if i < 0 {
                    return Err(RuntimeError::IndexOutOfBounds {
                        index: i,
                        len: items.len() as i64,
                    });
                }
                let idx = i as usize;
                if idx < items.len() {
                    Ok(items[idx].clone())
                } else {
                    Err(RuntimeError::IndexOutOfBounds {
                        index: i,
                        len: items.len() as i64,
                    })
                }
            }
            (Value::String(s), Value::Int(i)) => {
                let i = *i;
                if i < 0 {
                    return Err(RuntimeError::IndexOutOfBounds {
                        index: i,
                        len: s.chars().count() as i64,
                    });
                }
                let idx = i as usize;
                match s.chars().nth(idx) {
                    Some(c) => Ok(Value::Char(c)),
                    None => Err(RuntimeError::IndexOutOfBounds {
                        index: i,
                        len: s.chars().count() as i64,
                    }),
                }
            }
            (Value::Map(entries), key) => {
                let k = MapKey::from_value(key)?;
                entries.get(&k).cloned().ok_or(RuntimeError::KeyNotFound)
            }
            _ => Err(RuntimeError::TypeError(
                "indexing requires List, String, or Map".into(),
            )),
        }
    }

    fn eval_match(
        &mut self,
        env: &mut Env,
        body: &Body,
        scrutinee: Value,
        arms: &[MatchArm],
    ) -> Result<ControlFlow, RuntimeError> {
        for arm in arms {
            let mut bindings = Vec::new();
            if self.match_pat(body, arm.pat, &scrutinee, &mut bindings) {
                env.push_scope();
                for (name, val) in bindings {
                    env.bind(name, val);
                }
                let result = self.eval_expr(env, body, arm.body);
                env.pop_scope();
                return result;
            }
        }
        Err(RuntimeError::PatternMatchFailure)
    }

    fn match_pat(
        &self,
        body: &Body,
        pat_idx: kyokara_hir_def::expr::PatIdx,
        value: &Value,
        bindings: &mut Vec<(Name, Value)>,
    ) -> bool {
        let pat = &body.pats[pat_idx];
        match pat {
            Pat::Bind { name } => {
                bindings.push((*name, value.clone()));
                true
            }
            Pat::Wildcard => true,
            Pat::Literal(lit) => match (lit, value) {
                (Literal::Int(a), Value::Int(b)) => a == b,
                (Literal::Float(a), Value::Float(b)) => a == b,
                (Literal::String(a), Value::String(b)) => a == b,
                (Literal::Char(a), Value::Char(b)) => a == b,
                (Literal::Bool(a), Value::Bool(b)) => a == b,
                _ => false,
            },
            Pat::Constructor { path, args } => {
                if !path.is_single() {
                    // Multi-segment constructor patterns are not resolved at runtime.
                    // Avoid leaf-name matching (e.g. `A.Some(_)` matching `Some(_)`).
                    return false;
                }

                let Value::Adt {
                    type_idx,
                    variant,
                    fields,
                } = value
                else {
                    return false;
                };

                // Resolve the constructor name.
                let ctor_name = match path.last() {
                    Some(n) => n,
                    None => return false,
                };
                let Some(&(expected_type, expected_variant)) =
                    self.module_scope.constructors.get(&ctor_name)
                else {
                    return false;
                };

                if *type_idx != expected_type || *variant != expected_variant {
                    return false;
                }

                if args.len() != fields.len() {
                    return false;
                }

                for (sub_pat, sub_val) in args.iter().zip(fields.iter()) {
                    if !self.match_pat(body, *sub_pat, sub_val, bindings) {
                        return false;
                    }
                }
                true
            }
            Pat::Record {
                fields: pat_fields, ..
            } => {
                let Value::Record {
                    fields: val_fields, ..
                } = value
                else {
                    return false;
                };
                // Each pattern field name must match a value field,
                // and we bind the name to the value.
                for pat_name in pat_fields {
                    let found = val_fields.iter().find(|(n, _)| n == pat_name);
                    if let Some((_, val)) = found {
                        bindings.push((*pat_name, val.clone()));
                    } else {
                        return false;
                    }
                }
                true
            }
            Pat::Missing => false,
        }
    }

    fn eval_block(
        &mut self,
        env: &mut Env,
        body: &Body,
        stmts: &[Stmt],
        tail: Option<ExprIdx>,
    ) -> Result<ControlFlow, RuntimeError> {
        env.push_scope();
        for stmt in stmts {
            match stmt {
                Stmt::Let { pat, init, .. } => {
                    let result = match self.eval_expr(env, body, *init) {
                        Ok(result) => result,
                        Err(err) => {
                            env.pop_scope();
                            return Err(err);
                        }
                    };
                    if let ControlFlow::Return(_) = &result {
                        env.pop_scope();
                        return Ok(result);
                    }
                    if let Err(err) = self.bind_pat(body, *pat, &result.into_value(), env) {
                        env.pop_scope();
                        return Err(err);
                    }
                }
                Stmt::Expr(idx) => {
                    let result = match self.eval_expr(env, body, *idx) {
                        Ok(result) => result,
                        Err(err) => {
                            env.pop_scope();
                            return Err(err);
                        }
                    };
                    if let ControlFlow::Return(_) = &result {
                        env.pop_scope();
                        return Ok(result);
                    }
                }
            }
        }
        let result = if let Some(tail_idx) = tail {
            self.eval_expr(env, body, tail_idx)
        } else {
            Ok(ControlFlow::Value(Value::Unit))
        };
        env.pop_scope();
        result
    }

    fn bind_pat(
        &self,
        body: &Body,
        pat_idx: kyokara_hir_def::expr::PatIdx,
        value: &Value,
        env: &mut Env,
    ) -> Result<(), RuntimeError> {
        let pat = &body.pats[pat_idx];
        match pat {
            Pat::Bind { name } => {
                env.bind(*name, value.clone());
                Ok(())
            }
            Pat::Wildcard => Ok(()),
            _ => {
                // For let bindings with complex patterns, try matching.
                let mut bindings = Vec::new();
                if self.match_pat(body, pat_idx, value, &mut bindings) {
                    for (name, val) in bindings {
                        env.bind(name, val);
                    }
                    Ok(())
                } else {
                    Err(RuntimeError::PatternMatchFailure)
                }
            }
        }
    }

    fn eval_record_lit(
        &mut self,
        env: &mut Env,
        body: &Body,
        path: Option<&kyokara_hir_def::path::Path>,
        fields: &[(Name, ExprIdx)],
    ) -> Result<ControlFlow, RuntimeError> {
        let mut field_vals = Vec::with_capacity(fields.len());
        for (name, expr_idx) in fields {
            let val = eval_propagate!(self, env, body, *expr_idx);
            field_vals.push((*name, val));
        }

        // Resolve type index from path for method resolution.
        let type_idx = path
            .and_then(|p| p.segments.first())
            .and_then(|name| self.module_scope.types.get(name).copied());

        // Record literals (`Name { field: value }`) always produce record
        // values. ADT constructors are handled separately through the call
        // path (`Name(value)`). This avoids misinterpreting record literals
        // as ADT constructors when names collide (issue #127).
        Ok(ControlFlow::Value(Value::Record {
            fields: field_vals,
            type_idx,
        }))
    }

    // --- Shared-env variants of helper methods ---

    /// Name resolution using `self.env`.
    #[inline(always)]
    fn resolve_name_shared(&self, name: Name) -> Result<Value, RuntimeError> {
        self.resolve_name(&self.env, name)
    }

    fn eval_match_shared(
        &mut self,
        body: &Body,
        scrutinee: Value,
        arms: &[MatchArm],
    ) -> Result<ControlFlow, RuntimeError> {
        for arm in arms {
            let mut bindings = Vec::new();
            if self.match_pat(body, arm.pat, &scrutinee, &mut bindings) {
                self.env.push_scope();
                for (name, val) in bindings {
                    self.env.bind(name, val);
                }
                let result = self.eval_expr_shared(body, arm.body);
                self.env.pop_scope();
                return result;
            }
        }
        Err(RuntimeError::PatternMatchFailure)
    }

    fn eval_block_shared(
        &mut self,
        body: &Body,
        stmts: &[Stmt],
        tail: Option<ExprIdx>,
    ) -> Result<ControlFlow, RuntimeError> {
        self.env.push_scope();
        for stmt in stmts {
            match stmt {
                Stmt::Let { pat, init, .. } => {
                    let result = match self.eval_expr_shared(body, *init) {
                        Ok(result) => result,
                        Err(err) => {
                            self.env.pop_scope();
                            return Err(err);
                        }
                    };
                    if let ControlFlow::Return(_) = &result {
                        self.env.pop_scope();
                        return Ok(result);
                    }
                    if let Err(err) = self.bind_pat_shared(body, *pat, &result.into_value()) {
                        self.env.pop_scope();
                        return Err(err);
                    }
                }
                Stmt::Expr(idx) => {
                    let result = match self.eval_expr_shared(body, *idx) {
                        Ok(result) => result,
                        Err(err) => {
                            self.env.pop_scope();
                            return Err(err);
                        }
                    };
                    if let ControlFlow::Return(_) = &result {
                        self.env.pop_scope();
                        return Ok(result);
                    }
                }
            }
        }
        let result = if let Some(tail_idx) = tail {
            self.eval_expr_shared(body, tail_idx)
        } else {
            Ok(ControlFlow::Value(Value::Unit))
        };
        self.env.pop_scope();
        result
    }

    fn bind_pat_shared(
        &mut self,
        body: &Body,
        pat_idx: kyokara_hir_def::expr::PatIdx,
        value: &Value,
    ) -> Result<(), RuntimeError> {
        let pat = &body.pats[pat_idx];
        match pat {
            Pat::Bind { name } => {
                self.env.bind(*name, value.clone());
                Ok(())
            }
            Pat::Wildcard => Ok(()),
            _ => {
                let mut bindings = Vec::new();
                if self.match_pat(body, pat_idx, value, &mut bindings) {
                    for (name, val) in bindings {
                        self.env.bind(name, val);
                    }
                    Ok(())
                } else {
                    Err(RuntimeError::PatternMatchFailure)
                }
            }
        }
    }

    fn eval_record_lit_shared(
        &mut self,
        body: &Body,
        path: Option<&kyokara_hir_def::path::Path>,
        fields: &[(Name, ExprIdx)],
    ) -> Result<ControlFlow, RuntimeError> {
        let mut field_vals = Vec::with_capacity(fields.len());
        for (name, expr_idx) in fields {
            let val = eval_propagate_shared!(self, body, *expr_idx);
            field_vals.push((*name, val));
        }

        let type_idx = path
            .and_then(|p| p.segments.first())
            .and_then(|name| self.module_scope.types.get(name).copied());

        // Record literals always produce record values (see eval_record_lit).
        Ok(ControlFlow::Value(Value::Record {
            fields: field_vals,
            type_idx,
        }))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use kyokara_hir::check_file;
    use kyokara_hir_def::item_tree::{TypeDefKind, TypeItem, VariantDef};
    use kyokara_hir_def::path::Path;
    use kyokara_hir_def::type_ref::TypeRef;
    use la_arena::{Arena, ArenaMap};
    use smallvec::smallvec;

    use super::*;

    fn make_test_interpreter_and_body() -> (Interpreter, Body, TypeItemIdx, Name, Name) {
        let mut interner = Interner::new();
        let some_name = Name::new(&mut interner, "Some");
        let none_name = Name::new(&mut interner, "None");
        let a_name = Name::new(&mut interner, "A");
        let opt_name = Name::new(&mut interner, "Option");

        let mut item_tree = ItemTree::default();
        let type_idx = item_tree.types.alloc(TypeItem {
            name: opt_name,
            is_pub: true,
            type_params: Vec::new(),
            kind: TypeDefKind::Adt {
                variants: vec![
                    VariantDef {
                        name: some_name,
                        fields: vec![TypeRef::Error],
                    },
                    VariantDef {
                        name: none_name,
                        fields: Vec::new(),
                    },
                ],
            },
        });

        let mut module_scope = ModuleScope::default();
        module_scope.constructors.insert(some_name, (type_idx, 0));
        module_scope.constructors.insert(none_name, (type_idx, 1));

        let interpreter = Interpreter::new(
            item_tree,
            module_scope,
            FxHashMap::default(),
            FxHashMap::default(),
            interner,
            None,
        );

        let mut exprs = Arena::new();
        let root = exprs.alloc(Expr::Missing);
        let body = Body {
            exprs,
            pats: Arena::new(),
            root,
            requires: None,
            ensures: None,
            invariant: None,
            scopes: kyokara_hir_def::scope::ScopeTree::default(),
            pat_scopes: Vec::new(),
            expr_scopes: ArenaMap::default(),
            expr_source_map: ArenaMap::default(),
            pat_source_map: ArenaMap::default(),
            local_binding_meta: ArenaMap::default(),
        };

        (interpreter, body, type_idx, a_name, some_name)
    }

    #[test]
    fn dotted_constructor_pattern_does_not_match_by_leaf_name() {
        let (interp, mut body, type_idx, a_name, some_name) = make_test_interpreter_and_body();

        let wildcard = body.pats.alloc(Pat::Wildcard);
        let dotted_ctor = body.pats.alloc(Pat::Constructor {
            path: Path {
                segments: vec![a_name, some_name],
            },
            args: vec![wildcard],
        });

        let value = Value::Adt {
            type_idx,
            variant: 0,
            fields: vec![Value::Int(1)],
        };
        let mut bindings = Vec::new();

        let matched = interp.match_pat(&body, dotted_ctor, &value, &mut bindings);
        assert!(
            !matched,
            "dotted constructor pattern must not match by leaf-name constructor lookup"
        );
    }

    #[test]
    fn single_segment_constructor_pattern_still_matches() {
        let (interp, mut body, type_idx, _a_name, some_name) = make_test_interpreter_and_body();

        let wildcard = body.pats.alloc(Pat::Wildcard);
        let some_ctor = body.pats.alloc(Pat::Constructor {
            path: Path::single(some_name),
            args: vec![wildcard],
        });

        let value = Value::Adt {
            type_idx,
            variant: 0,
            fields: vec![Value::Int(1)],
        };
        let mut bindings = Vec::new();

        let matched = interp.match_pat(&body, some_ctor, &value, &mut bindings);
        assert!(
            matched,
            "single-segment constructor pattern should match corresponding ADT value"
        );
    }

    fn make_checked_interpreter(source: &str) -> Interpreter {
        let checked = check_file(source);
        assert!(
            checked.parse_errors.is_empty(),
            "parse errors: {:?}",
            checked.parse_errors
        );
        assert!(
            checked.lowering_diagnostics.is_empty(),
            "lowering diagnostics: {:?}",
            checked.lowering_diagnostics
        );
        assert!(
            checked.type_check.body_lowering_diagnostics.is_empty(),
            "body lowering diagnostics: {:?}",
            checked.type_check.body_lowering_diagnostics
        );
        assert!(
            checked.type_check.raw_diagnostics.is_empty(),
            "type diagnostics: {:?}",
            checked.type_check.raw_diagnostics
        );

        Interpreter::new(
            checked.item_tree,
            checked.module_scope,
            checked.type_check.fn_bodies,
            FxHashMap::default(),
            checked.interner,
            None,
        )
    }

    fn fn_idx_by_name(interp: &mut Interpreter, name: &str) -> FnItemIdx {
        let name = Name::new(interp.interner_mut(), name);
        *interp
            .module_scope
            .functions
            .get(&name)
            .expect("function should exist")
    }

    #[test]
    fn user_function_call_reports_arity_mismatch() {
        let mut interp = make_checked_interpreter(
            "fn add(x: Int, y: Int) -> Int { x + y } fn main() -> Int { 0 }",
        );
        let add_idx = fn_idx_by_name(&mut interp, "add");

        let err = interp
            .call_fn_by_idx(add_idx, smallvec![Value::Int(1)])
            .expect_err("too-few args should fail");
        assert!(
            err.to_string().contains("arity mismatch"),
            "expected arity mismatch error, got: {err}"
        );

        let err = interp
            .call_fn_by_idx(
                add_idx,
                smallvec![Value::Int(1), Value::Int(2), Value::Int(3)],
            )
            .expect_err("too-many args should fail");
        assert!(
            err.to_string().contains("arity mismatch"),
            "expected arity mismatch error, got: {err}"
        );
    }

    #[test]
    fn lambda_call_reports_arity_mismatch() {
        let (mut interp, mut body, _type_idx, _a_name, _some_name) =
            make_test_interpreter_and_body();

        let x = Name::new(interp.interner_mut(), "x");
        let x_pat = body.pats.alloc(Pat::Bind { name: x });
        let root = body.exprs.alloc(Expr::Literal(Literal::Int(42)));
        body.root = root;

        let lambda = Value::Fn(Box::new(FnValue::Lambda {
            params: vec![x_pat],
            body_expr: root,
            body: Rc::new(body),
            env: Env::new(),
        }));

        let err = interp
            .call_value(lambda.clone(), Args::new())
            .expect_err("too-few lambda args should fail");
        assert!(
            err.to_string().contains("arity mismatch"),
            "expected arity mismatch error, got: {err}"
        );

        let err = interp
            .call_value(lambda, smallvec![Value::Int(1), Value::Int(2)])
            .expect_err("too-many lambda args should fail");
        assert!(
            err.to_string().contains("arity mismatch"),
            "expected arity mismatch error, got: {err}"
        );
    }

    #[test]
    fn constructor_call_reports_arity_mismatch() {
        let (mut interp, _body, type_idx, _a_name, _some_name) = make_test_interpreter_and_body();

        let ctor = Value::Fn(Box::new(FnValue::Constructor {
            type_idx,
            variant_idx: 0,
            arity: 1,
        }));

        let err = interp
            .call_value(ctor.clone(), Args::new())
            .expect_err("too-few constructor args should fail");
        assert!(
            err.to_string().contains("arity mismatch"),
            "expected arity mismatch error, got: {err}"
        );

        let err = interp
            .call_value(ctor, smallvec![Value::Int(1), Value::Int(2)])
            .expect_err("too-many constructor args should fail");
        assert!(
            err.to_string().contains("arity mismatch"),
            "expected arity mismatch error, got: {err}"
        );
    }

    fn assert_no_leaked_call_state(interp: &mut Interpreter) {
        assert!(
            interp.old_env.is_none(),
            "old_env should be restored on call failure"
        );
        let x_name = Name::new(interp.interner_mut(), "x");
        let result_name = Name::new(interp.interner_mut(), "result");
        assert!(
            interp.env.lookup(x_name).is_none(),
            "parameter `x` leaked into shared env after call failure: {:?}",
            interp.env
        );
        assert!(
            interp.env.lookup(result_name).is_none(),
            "`result` leaked into shared env after call failure: {:?}",
            interp.env
        );
    }

    #[test]
    fn contract_requires_error_does_not_leak_env_or_old_snapshot() {
        let mut interp = make_checked_interpreter(
            "fn bad(x: Int) -> Int requires x / 0 > 0 ensures old(x) == x { x }\n\
             fn main() -> Int { 0 }",
        );
        let bad_idx = fn_idx_by_name(&mut interp, "bad");
        let err = interp
            .call_fn_by_idx(bad_idx, smallvec![Value::Int(1)])
            .expect_err("requires expression should fail at runtime");
        assert!(matches!(err, RuntimeError::DivisionByZero));
        assert_no_leaked_call_state(&mut interp);
    }

    #[test]
    fn contract_body_error_does_not_leak_env_or_old_snapshot() {
        let mut interp = make_checked_interpreter(
            "fn bad(x: Int) -> Int ensures old(x) == x { x / 0 }\n\
             fn main() -> Int { 0 }",
        );
        let bad_idx = fn_idx_by_name(&mut interp, "bad");
        let err = interp
            .call_fn_by_idx(bad_idx, smallvec![Value::Int(1)])
            .expect_err("body should fail at runtime");
        assert!(matches!(err, RuntimeError::DivisionByZero));
        assert_no_leaked_call_state(&mut interp);
    }

    #[test]
    fn contract_invariant_error_does_not_leak_env_or_old_snapshot() {
        let mut interp = make_checked_interpreter(
            "fn bad(x: Int) -> Int ensures old(x) == x invariant x / 0 > 0 { x }\n\
             fn main() -> Int { 0 }",
        );
        let bad_idx = fn_idx_by_name(&mut interp, "bad");
        let err = interp
            .call_fn_by_idx(bad_idx, smallvec![Value::Int(1)])
            .expect_err("invariant expression should fail at runtime");
        assert!(matches!(err, RuntimeError::DivisionByZero));
        assert_no_leaked_call_state(&mut interp);
    }

    #[test]
    fn contract_ensures_error_does_not_leak_env_or_old_snapshot() {
        let mut interp = make_checked_interpreter(
            "fn bad(x: Int) -> Int ensures old(x) / 0 > 0 { x }\n\
             fn main() -> Int { 0 }",
        );
        let bad_idx = fn_idx_by_name(&mut interp, "bad");
        let err = interp
            .call_fn_by_idx(bad_idx, smallvec![Value::Int(1)])
            .expect_err("ensures expression should fail at runtime");
        assert!(matches!(err, RuntimeError::DivisionByZero));
        assert_no_leaked_call_state(&mut interp);
    }
}
