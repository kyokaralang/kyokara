//! Core tree-walking interpreter.

use std::rc::Rc;

use kyokara_hir_def::body::Body;
use kyokara_hir_def::expr::{BinaryOp, CallArg, Expr, ExprIdx, Literal, MatchArm, Stmt, UnaryOp};
use kyokara_hir_def::item_tree::{FnItemIdx, FnParam, ItemTree, TypeDefKind, TypeItemIdx};
use kyokara_hir_def::name::Name;
use kyokara_hir_def::pat::Pat;
use kyokara_hir_def::resolver::ModuleScope;
use kyokara_intern::Interner;
use kyokara_stdx::FxHashMap;

use crate::env::Env;
use crate::error::RuntimeError;
use crate::intrinsics::{self, Args, IntrinsicFn};
use crate::manifest::CapabilityManifest;
use crate::value::{FnValue, Value};

/// Tree-walking interpreter state.
pub struct Interpreter {
    item_tree: ItemTree,
    module_scope: ModuleScope,
    fn_bodies: FxHashMap<FnItemIdx, Body>,
    interner: Interner,
    intrinsics: FxHashMap<Name, IntrinsicFn>,
    /// Snapshot of the environment at function entry, used by `old()` in ensures clauses.
    old_env: Option<Env>,
    /// Cached Option::Some constructor (type_idx, variant_idx).
    option_some: Option<(TypeItemIdx, usize)>,
    /// Cached Option::None constructor (type_idx, variant_idx).
    option_none: Option<(TypeItemIdx, usize)>,
    /// Optional capability manifest for deny-by-default enforcement.
    manifest: Option<CapabilityManifest>,
    /// Shared environment used across all function calls to avoid per-call allocation.
    env: Env,
}

/// Used to implement early return from functions.
enum ControlFlow {
    Value(Value),
    Return(Value),
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
        mut interner: Interner,
        manifest: Option<CapabilityManifest>,
    ) -> Self {
        let intrinsic_list = intrinsics::all_intrinsics(&mut interner);
        let intrinsics = intrinsic_list.into_iter().collect();

        let some_name = Name::new(&mut interner, "Some");
        let none_name = Name::new(&mut interner, "None");
        let option_some = module_scope.constructors.get(&some_name).copied();
        let option_none = module_scope.constructors.get(&none_name).copied();

        Interpreter {
            item_tree,
            module_scope,
            fn_bodies,
            interner,
            intrinsics,
            old_env: None,
            option_some,
            option_none,
            manifest,
            env: Env::new(),
        }
    }

    /// Consume the interpreter and return the interner (for display).
    pub fn into_interner(self) -> Interner {
        self.interner
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

    /// Call a user-defined function by index.
    fn call_fn(&mut self, fn_idx: FnItemIdx, args: Args) -> Result<Value, RuntimeError> {
        let body = self
            .fn_bodies
            .get(&fn_idx)
            .ok_or_else(|| RuntimeError::UnresolvedName("function body not found".into()))?
            as *const Body;
        // SAFETY: we hold &mut self but only mutate interner/env/old_env, not fn_bodies.
        let body = unsafe { &*body };

        let fn_item = &self.item_tree.functions[fn_idx];

        // Check user-declared capabilities against the manifest (only when manifest exists
        // and the function actually declares capabilities).
        if let Some(manifest) = &self.manifest
            && !fn_item.with_caps.is_empty()
        {
            let fn_name_str = fn_item.name.resolve(&self.interner).to_string();
            for cap_ref in &fn_item.with_caps {
                if let kyokara_hir_def::type_ref::TypeRef::Path { path, .. } = cap_ref
                    && let Some(name) = path.last()
                {
                    let cap_str = name.resolve(&self.interner);
                    if !manifest.is_granted(cap_str) {
                        return Err(RuntimeError::CapabilityDenied {
                            capability: cap_str.to_string(),
                            function: fn_name_str,
                        });
                    }
                }
            }
        }

        // Use the shared environment with a new scope instead of allocating a fresh Env.
        self.env.push_scope();

        // Bind parameters directly from the item tree (no intermediate Vec allocation).
        let params = &fn_item.params;
        for (i, param) in params.iter().enumerate() {
            if let Some(val) = args.get(i) {
                self.env.bind(param.name, val.clone());
            }
        }

        // Fast path: skip contract checks when no contracts are present.
        let has_requires = body.requires.is_some();
        let has_ensures = body.ensures.is_some();
        let has_invariant = body.invariant.is_some();

        if !has_requires && !has_ensures && !has_invariant {
            // Hot path: no contracts — just evaluate the body.
            let result = self.eval_expr_shared(body, body.root);
            self.env.pop_scope();
            let return_val = match result? {
                ControlFlow::Value(v) | ControlFlow::Return(v) => v,
            };
            return Ok(return_val);
        }

        // Slow path: full contract checking.
        let fn_name_str = fn_item.name.resolve(&self.interner).to_string();

        // Check precondition.
        if let Some(req_idx) = body.requires {
            let val = self.eval_expr_shared(body, req_idx)?.into_value();
            if !matches!(val, Value::Bool(true)) {
                self.env.pop_scope();
                return Err(RuntimeError::PreconditionFailed(fn_name_str));
            }
        }

        // Snapshot env for old() before evaluating the body.
        let prev_old_env = self.old_env.take();
        if has_ensures {
            self.old_env = Some(self.env.clone());
        }

        // Evaluate the function body.
        let return_val = match self.eval_expr_shared(body, body.root)? {
            ControlFlow::Value(v) | ControlFlow::Return(v) => v,
        };

        // Check invariant.
        if let Some(inv_idx) = body.invariant {
            let val = self.eval_expr_shared(body, inv_idx)?.into_value();
            if !matches!(val, Value::Bool(true)) {
                self.env.pop_scope();
                self.old_env = prev_old_env;
                return Err(RuntimeError::InvariantViolated(fn_name_str));
            }
        }

        // Check postcondition.
        if let Some(ens_idx) = body.ensures {
            let result_name = Name::new(&mut self.interner, "result");
            self.env.bind(result_name, return_val.clone());
            let val = self.eval_expr_shared(body, ens_idx)?.into_value();
            self.env.pop_scope();
            self.old_env = prev_old_env;
            if !matches!(val, Value::Bool(true)) {
                return Err(RuntimeError::PostconditionFailed(fn_name_str));
            }
        } else {
            self.env.pop_scope();
            self.old_env = prev_old_env;
        }

        Ok(return_val)
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
                let lv = eval_propagate!(self, env, body, lhs);
                let rv = eval_propagate!(self, env, body, rhs);
                self.eval_binary(op, lv, rv).map(ControlFlow::Value)
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
                let callee_val = eval_propagate!(self, env, body, callee_idx);
                let mut arg_vals = Args::with_capacity(args.len());
                for arg in &args {
                    match arg {
                        CallArg::Positional(idx) => {
                            let v = eval_propagate!(self, env, body, *idx);
                            arg_vals.push(v);
                        }
                        CallArg::Named { value, .. } => {
                            let v = eval_propagate!(self, env, body, *value);
                            arg_vals.push(v);
                        }
                    }
                }
                self.call_value(callee_val, arg_vals)
                    .map(ControlFlow::Value)
            }

            Expr::Field { base, field } => {
                let base_idx = *base;
                let field = *field;
                let base_val = eval_propagate!(self, env, body, base_idx);
                self.eval_field(base_val, field).map(ControlFlow::Value)
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
                let lv = eval_propagate_shared!(self, body, lhs);
                let rv = eval_propagate_shared!(self, body, rhs);
                self.eval_binary(op, lv, rv).map(ControlFlow::Value)
            }

            Expr::Unary { op, operand } => {
                let op = *op;
                let operand = *operand;
                let v = eval_propagate_shared!(self, body, operand);
                self.eval_unary(op, v).map(ControlFlow::Value)
            }

            Expr::Call { callee, args } => {
                let callee_idx = *callee;
                // Note: args borrows body.exprs[idx] immutably, which is compatible
                // with passing body to eval_expr_shared (also immutable).
                // The &mut self in eval_expr_shared doesn't conflict because body
                // is not borrowed from self (it comes from a raw pointer).
                let callee_val = eval_propagate_shared!(self, body, callee_idx);

                // Fast path: direct user function call — evaluate args directly
                // into the shared env, avoiding Vec<Value> allocation entirely.
                if let Value::Fn(ref fv) = callee_val
                    && let FnValue::User(fn_idx) = **fv
                {
                    let fn_body = self.fn_bodies.get(&fn_idx).ok_or_else(|| {
                        RuntimeError::UnresolvedName("function body not found".into())
                    })? as *const Body;
                    let fn_body = unsafe { &*fn_body };
                    let fn_item = &self.item_tree.functions[fn_idx];

                    // Capability check.
                    if let Some(manifest) = &self.manifest
                        && !fn_item.with_caps.is_empty()
                    {
                        let fn_name_str = fn_item.name.resolve(&self.interner).to_string();
                        for cap_ref in &fn_item.with_caps {
                            if let kyokara_hir_def::type_ref::TypeRef::Path { path, .. } = cap_ref
                                && let Some(name) = path.last()
                            {
                                let cap_str = name.resolve(&self.interner);
                                if !manifest.is_granted(cap_str) {
                                    return Err(RuntimeError::CapabilityDenied {
                                        capability: cap_str.to_string(),
                                        function: fn_name_str,
                                    });
                                }
                            }
                        }
                    }

                    let has_contracts = fn_body.requires.is_some()
                        || fn_body.ensures.is_some()
                        || fn_body.invariant.is_some();

                    if !has_contracts {
                        // Hot path: push scope, bind args directly, evaluate body.
                        // We use a raw pointer to fn_item.params to avoid holding
                        // an immutable borrow on self.item_tree across eval calls.
                        // SAFETY: eval_expr_shared never mutates item_tree.
                        let params_ptr = &fn_item.params as *const Vec<FnParam>;
                        let params = unsafe { &*params_ptr };

                        self.env.push_scope();
                        for (i, arg) in args.iter().enumerate() {
                            let arg_idx = match arg {
                                CallArg::Positional(idx) => *idx,
                                CallArg::Named { value, .. } => *value,
                            };
                            let val = eval_propagate_shared!(self, body, arg_idx);
                            if let Some(param) = params.get(i) {
                                self.env.bind(param.name, val);
                            }
                        }

                        let result = self.eval_expr_shared(fn_body, fn_body.root);
                        self.env.pop_scope();
                        let return_val = match result? {
                            ControlFlow::Value(v) | ControlFlow::Return(v) => v,
                        };
                        return Ok(ControlFlow::Value(return_val));
                    }

                    // Slow path: contracts present — collect args into SmallVec.
                    let mut args_vec = Args::with_capacity(args.len());
                    for arg in args {
                        let arg_idx = match arg {
                            CallArg::Positional(idx) => *idx,
                            CallArg::Named { value, .. } => *value,
                        };
                        let v = eval_propagate_shared!(self, body, arg_idx);
                        args_vec.push(v);
                    }
                    return self.call_fn(fn_idx, args_vec).map(ControlFlow::Value);
                }

                // Non-user-function call (intrinsic, lambda, constructor).
                let mut arg_vals = Args::with_capacity(args.len());
                for arg in args {
                    match arg {
                        CallArg::Positional(idx) => {
                            let v = eval_propagate_shared!(self, body, *idx);
                            arg_vals.push(v);
                        }
                        CallArg::Named { value, .. } => {
                            let v = eval_propagate_shared!(self, body, *value);
                            arg_vals.push(v);
                        }
                    }
                }
                self.call_value(callee_val, arg_vals)
                    .map(ControlFlow::Value)
            }

            Expr::Field { base, field } => {
                let base_idx = *base;
                let field = *field;
                let base_val = eval_propagate_shared!(self, body, base_idx);
                self.eval_field(base_val, field).map(ControlFlow::Value)
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

        // 2. Intrinsics (checked before module functions because intrinsic names
        //    are also registered in module_scope.functions for type checking, but
        //    lack bodies — resolving them as User functions would fail at call time).
        if let Some(&intr) = self.intrinsics.get(&name) {
            return Ok(Value::Fn(Box::new(FnValue::Intrinsic(intr))));
        }

        // 3. Module-level functions.
        if let Some(&fn_idx) = self.module_scope.functions.get(&name) {
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
            // Int arithmetic.
            (BinaryOp::Add, Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
            (BinaryOp::Sub, Value::Int(a), Value::Int(b)) => Ok(Value::Int(a - b)),
            (BinaryOp::Mul, Value::Int(a), Value::Int(b)) => Ok(Value::Int(a * b)),
            (BinaryOp::Div, Value::Int(_), Value::Int(0)) => Err(RuntimeError::DivisionByZero),
            (BinaryOp::Div, Value::Int(a), Value::Int(b)) => Ok(Value::Int(a / b)),

            // Float arithmetic.
            (BinaryOp::Add, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
            (BinaryOp::Sub, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
            (BinaryOp::Mul, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
            (BinaryOp::Div, Value::Float(a), Value::Float(b)) => Ok(Value::Float(a / b)),

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
    fn eval_unary(&self, op: UnaryOp, val: Value) -> Result<Value, RuntimeError> {
        match (op, &val) {
            (UnaryOp::Neg, Value::Int(n)) => Ok(Value::Int(-n)),
            (UnaryOp::Neg, Value::Float(f)) => Ok(Value::Float(-f)),
            (UnaryOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
            _ => Err(RuntimeError::TypeError(format!(
                "cannot apply {op:?} to {val:?}"
            ))),
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
                    ..
                } => Ok(Value::Adt {
                    type_idx,
                    variant: variant_idx,
                    fields: args.into_vec(),
                }),
            },
            _ => Err(RuntimeError::TypeError(
                "called value is not a function".into(),
            )),
        }
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
                let key = &args[1];
                for (k, v) in entries {
                    if k == key {
                        return Ok(self.make_some(v.clone()));
                    }
                }
                Ok(self.make_none())
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
            _ => Err(RuntimeError::TypeError("unknown complex intrinsic".into())),
        }
    }

    fn eval_field(&self, base: Value, field: Name) -> Result<Value, RuntimeError> {
        match base {
            Value::Record { fields } => {
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
                let Value::Record { fields: val_fields } = value else {
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
                    let result = self.eval_expr(env, body, *init)?;
                    if let ControlFlow::Return(_) = &result {
                        env.pop_scope();
                        return Ok(result);
                    }
                    self.bind_pat(body, *pat, &result.into_value(), env)?;
                }
                Stmt::Expr(idx) => {
                    let result = self.eval_expr(env, body, *idx)?;
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
        // If the tail is a Return, we need to propagate it.
        let r = result?;
        env.pop_scope();
        Ok(r)
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

        // If there's a path, it's an ADT record constructor.
        if let Some(path) = path
            && let Some(ctor_name) = path.last()
            && let Some(&(type_idx, variant_idx)) = self.module_scope.constructors.get(&ctor_name)
        {
            let vals: Vec<Value> = field_vals.into_iter().map(|(_, v)| v).collect();
            return Ok(ControlFlow::Value(Value::Adt {
                type_idx,
                variant: variant_idx,
                fields: vals,
            }));
        }

        Ok(ControlFlow::Value(Value::Record { fields: field_vals }))
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
                    let result = self.eval_expr_shared(body, *init)?;
                    if let ControlFlow::Return(_) = &result {
                        self.env.pop_scope();
                        return Ok(result);
                    }
                    self.bind_pat_shared(body, *pat, &result.into_value())?;
                }
                Stmt::Expr(idx) => {
                    let result = self.eval_expr_shared(body, *idx)?;
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
        let r = result?;
        self.env.pop_scope();
        Ok(r)
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

        if let Some(path) = path
            && let Some(ctor_name) = path.last()
            && let Some(&(type_idx, variant_idx)) = self.module_scope.constructors.get(&ctor_name)
        {
            let vals: Vec<Value> = field_vals.into_iter().map(|(_, v)| v).collect();
            return Ok(ControlFlow::Value(Value::Adt {
                type_idx,
                variant: variant_idx,
                fields: vals,
            }));
        }

        Ok(ControlFlow::Value(Value::Record { fields: field_vals }))
    }
}
