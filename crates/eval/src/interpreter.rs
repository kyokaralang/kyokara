//! Core tree-walking interpreter.

use std::rc::Rc;

use kyokara_hir_def::body::Body;
use kyokara_hir_def::expr::{BinaryOp, CallArg, Expr, ExprIdx, Literal, MatchArm, Stmt, UnaryOp};
use kyokara_hir_def::item_tree::{FnItemIdx, ItemTree, TypeDefKind};
use kyokara_hir_def::name::Name;
use kyokara_hir_def::pat::Pat;
use kyokara_hir_def::resolver::ModuleScope;
use kyokara_intern::Interner;
use kyokara_stdx::FxHashMap;

use crate::env::Env;
use crate::error::RuntimeError;
use crate::intrinsics::{self, IntrinsicFn};
use crate::value::{FnValue, Value};

/// Tree-walking interpreter state.
pub struct Interpreter {
    item_tree: ItemTree,
    module_scope: ModuleScope,
    fn_bodies: FxHashMap<FnItemIdx, Body>,
    interner: Interner,
    intrinsics: FxHashMap<Name, IntrinsicFn>,
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

impl Interpreter {
    pub fn new(
        item_tree: ItemTree,
        module_scope: ModuleScope,
        fn_bodies: FxHashMap<FnItemIdx, Body>,
        mut interner: Interner,
    ) -> Self {
        let intrinsic_list = intrinsics::all_intrinsics(&mut interner);
        let intrinsics = intrinsic_list.into_iter().collect();
        Interpreter {
            item_tree,
            module_scope,
            fn_bodies,
            interner,
            intrinsics,
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

        self.call_fn(main_idx, vec![])
    }

    /// Call a user-defined function by index.
    fn call_fn(&mut self, fn_idx: FnItemIdx, args: Vec<Value>) -> Result<Value, RuntimeError> {
        let body = self
            .fn_bodies
            .get(&fn_idx)
            .ok_or_else(|| RuntimeError::UnresolvedName("function body not found".into()))?
            as *const Body;
        // SAFETY: we hold &mut self but only mutate interner/env, not fn_bodies.
        let body = unsafe { &*body };

        let fn_item = &self.item_tree.functions[fn_idx];
        let mut env = Env::new();

        // Bind parameters.
        for (i, param) in fn_item.params.iter().enumerate() {
            if let Some(val) = args.get(i) {
                env.bind(param.name, val.clone());
            }
        }

        match self.eval_expr(&mut env, body, body.root)? {
            ControlFlow::Value(v) | ControlFlow::Return(v) => Ok(v),
        }
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
                let mut arg_vals = Vec::with_capacity(args.len());
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
                Ok(ControlFlow::Value(Value::Fn(FnValue::Lambda {
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
                    }),
                    env: env.clone(),
                })))
            }

            Expr::Old(inner) => {
                let inner = *inner;
                self.eval_expr(env, body, inner)
            }
        }
    }

    fn eval_literal(&self, lit: &Literal) -> Value {
        match lit {
            Literal::Int(n) => Value::Int(*n),
            Literal::Float(f) => Value::Float(*f),
            Literal::String(s) => Value::String(s.clone()),
            Literal::Char(c) => Value::Char(*c),
            Literal::Bool(b) => Value::Bool(*b),
        }
    }

    fn resolve_name(&self, env: &Env, name: Name) -> Result<Value, RuntimeError> {
        // 1. Local variables.
        if let Some(val) = env.lookup(name) {
            return Ok(val.clone());
        }

        // 2. Intrinsics.
        if let Some(&intr) = self.intrinsics.get(&name) {
            return Ok(Value::Fn(FnValue::Intrinsic(intr)));
        }

        // 3. Module-level functions.
        if let Some(&fn_idx) = self.module_scope.functions.get(&name) {
            return Ok(Value::Fn(FnValue::User(fn_idx)));
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
                return Ok(Value::Fn(FnValue::Constructor {
                    type_idx,
                    variant_idx,
                    arity: variant.fields.len(),
                }));
            }
        }

        let name_str = name.resolve(&self.interner);
        Err(RuntimeError::UnresolvedName(name_str.to_string()))
    }

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

            _ => Err(RuntimeError::TypeError(format!(
                "cannot apply {op:?} to {:?} and {:?}",
                std::mem::discriminant(&lhs),
                std::mem::discriminant(&rhs),
            ))),
        }
    }

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

    fn call_value(&mut self, callee: Value, args: Vec<Value>) -> Result<Value, RuntimeError> {
        match callee {
            Value::Fn(FnValue::User(fn_idx)) => self.call_fn(fn_idx, args),
            Value::Fn(FnValue::Intrinsic(intr)) => intr.call(args),
            Value::Fn(FnValue::Lambda {
                params,
                body_expr,
                body,
                env: captured_env,
            }) => {
                let mut env = captured_env;
                env.push_scope();
                for (pat_idx, val) in params.iter().zip(args) {
                    self.bind_pat(&body, *pat_idx, &val, &mut env)?;
                }
                let result = self.eval_expr(&mut env, &body, body_expr)?;
                Ok(result.into_value())
            }
            Value::Fn(FnValue::Constructor {
                type_idx,
                variant_idx,
                ..
            }) => Ok(Value::Adt {
                type_idx,
                variant: variant_idx,
                fields: args,
            }),
            _ => Err(RuntimeError::TypeError(
                "called value is not a function".into(),
            )),
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
}
