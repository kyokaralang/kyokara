//! Core tree-walking interpreter.

use std::cmp::Ordering;
use std::collections::VecDeque;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use kyokara_hir_def::body::{Body, LocalSlotRef};
use kyokara_hir_def::expr::{BinaryOp, CallArg, Expr, ExprIdx, Literal, MatchArm, Stmt, UnaryOp};
use kyokara_hir_def::item_tree::{FnItemIdx, ItemTree, LetItemIdx, TypeDefKind, TypeItemIdx};
use kyokara_hir_def::name::Name;
use kyokara_hir_def::pat::Pat;
use kyokara_hir_def::resolver::{
    CoreType, ModuleScope, PrimitiveType, ReceiverKey, StaticOwnerKey,
};
use kyokara_hir_def::scope::ScopeIdx;
use kyokara_hir_def::type_ref::TypeRef;
use kyokara_intern::Interner;
use kyokara_stdx::FxHashMap;
use la_arena::ArenaMap;

use crate::env::Env;
use crate::error::RuntimeError;
use crate::intrinsics::{self, Args, IntrinsicFn};
use crate::manifest::CapabilityManifest;
use crate::value::{
    FnValue, MapKey, MapValue, MutableMapValue, MutablePriorityQueueValue, MutableSetValue,
    PriorityQueueDirection, PriorityQueueEntry, SeqPlan, SeqSource, SetValue, Value,
};

/// Tree-walking interpreter state.
pub struct Interpreter {
    item_tree: ItemTree,
    module_scope: ModuleScope,
    fn_bodies: FxHashMap<FnItemIdx, Rc<Body>>,
    let_bodies: FxHashMap<LetItemIdx, Rc<Body>>,
    body_local_accesses: FxHashMap<usize, ArenaMap<ExprIdx, LocalSlotRef>>,
    /// Per-function module-level immutable let overrides used for project mode.
    /// Maps `current_fn_idx -> (name -> value)` for the function's source module.
    let_scope_overrides: FxHashMap<FnItemIdx, FxHashMap<Name, Value>>,
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
    /// Current shared body handle being evaluated (used by nested lambda literals).
    current_body: Option<Rc<Body>>,
    /// Optional local slot to move instead of clone during a dead-after-use eval path.
    consuming_local: Option<LocalSlotRef>,
    /// Top-level immutable let bindings are materialized before user code runs.
    top_level_lets_initialized: bool,
}

/// Used to implement early return from functions.
enum ControlFlow {
    Value(Value),
    Return(Value),
    Break,
    Continue,
}

enum LogicalEvalStep {
    ShortCircuit(Value),
    NeedRhs,
}

enum SeqEmitControl {
    Continue,
    Break,
}

enum SeqIterState {
    Source(SeqSourceIter),
    Map {
        input: Box<SeqIterState>,
        f: Value,
    },
    Filter {
        input: Box<SeqIterState>,
        f: Value,
    },
    Scan {
        input: Box<SeqIterState>,
        acc: Value,
        f: Value,
        emitted_init: bool,
    },
    Unfold {
        state: Option<Value>,
        step: Value,
    },
    Enumerate {
        input: Box<SeqIterState>,
        idx: i64,
        index_name: Name,
        value_name: Name,
    },
    Zip {
        left: Box<SeqIterState>,
        right: Box<SeqIterState>,
        left_name: Name,
        right_name: Name,
    },
    Chunks {
        input: Box<SeqIterState>,
        n: usize,
    },
    Windows {
        input: Box<SeqIterState>,
        n: usize,
        window: VecDeque<Value>,
        primed: bool,
    },
}

enum SeqSourceIter {
    Range {
        current: i64,
        end: i64,
    },
    FromList {
        items: Rc<Vec<Value>>,
        idx: usize,
    },
    BitSetValues {
        bitset: crate::value::BitSetValue,
        word_idx: usize,
        pending_word: u64,
    },
    FromDeque {
        items: Rc<VecDeque<Value>>,
        idx: usize,
    },
    StringSplit {
        s: Rc<String>,
        delim: Rc<String>,
        next_start: usize,
        emitted_empty_leading: bool,
        emitted_empty_trailing: bool,
    },
    StringLines {
        s: Rc<String>,
        next_start: usize,
        finished: bool,
    },
    StringChars {
        s: Rc<String>,
        next_start: usize,
    },
    MapKeys {
        entries: Rc<MapValue>,
        idx: usize,
    },
    MapValues {
        entries: Rc<MapValue>,
        idx: usize,
    },
    SetValues {
        entries: Rc<SetValue>,
        idx: usize,
    },
}

fn stable_merge_sort_by<T: Clone, E, F>(items: &[T], cmp: &mut F) -> Result<Vec<T>, E>
where
    F: FnMut(&T, &T) -> Result<Ordering, E>,
{
    if items.len() <= 1 {
        return Ok(items.to_vec());
    }

    let mid = items.len() / 2;
    let left = stable_merge_sort_by(&items[..mid], cmp)?;
    let right = stable_merge_sort_by(&items[mid..], cmp)?;
    merge_sorted_runs(left, right, cmp)
}

fn merge_sorted_runs<T: Clone, E, F>(left: Vec<T>, right: Vec<T>, cmp: &mut F) -> Result<Vec<T>, E>
where
    F: FnMut(&T, &T) -> Result<Ordering, E>,
{
    let mut merged = Vec::with_capacity(left.len() + right.len());
    let mut i = 0;
    let mut j = 0;

    while i < left.len() && j < right.len() {
        let ord = cmp(&left[i], &right[j])?;
        if ord == Ordering::Greater {
            merged.push(right[j].clone());
            j += 1;
        } else {
            merged.push(left[i].clone());
            i += 1;
        }
    }

    while i < left.len() {
        merged.push(left[i].clone());
        i += 1;
    }
    while j < right.len() {
        merged.push(right[j].clone());
        j += 1;
    }

    Ok(merged)
}

impl ControlFlow {
    fn into_value(self) -> Value {
        match self {
            ControlFlow::Value(v) | ControlFlow::Return(v) => v,
            ControlFlow::Break | ControlFlow::Continue => Value::Unit,
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
            cf @ ControlFlow::Break | cf @ ControlFlow::Continue => return Ok(cf),
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
            cf @ ControlFlow::Break | cf @ ControlFlow::Continue => return Ok(cf),
            ControlFlow::Value(v) => v,
        }
    }};
}

impl Interpreter {
    fn resolve_core_variant(
        item_tree: &ItemTree,
        module_scope: &ModuleScope,
        interner: &mut Interner,
        core: CoreType,
        variant_name: &str,
    ) -> Option<(TypeItemIdx, usize)> {
        let type_idx = module_scope.core_types.get(core)?.type_idx;
        let variant_name = Name::new(interner, variant_name);
        let TypeDefKind::Adt { variants } = &item_tree.types[type_idx].kind else {
            return None;
        };
        let variant_idx = variants.iter().position(|v| v.name == variant_name)?;
        Some((type_idx, variant_idx))
    }

    fn resolve_parse_error_variant(
        item_tree: &ItemTree,
        module_scope: &ModuleScope,
        interner: &mut Interner,
        variant_name: &str,
    ) -> Option<(TypeItemIdx, usize)> {
        let (type_idx, variant_idx) = Self::resolve_core_variant(
            item_tree,
            module_scope,
            interner,
            CoreType::ParseError,
            variant_name,
        )?;
        let string_name = Name::new(interner, "String");
        let TypeDefKind::Adt { variants } = &item_tree.types[type_idx].kind else {
            return None;
        };
        let variant = &variants[variant_idx];

        if variant.fields.len() != 1 {
            return None;
        }

        let TypeRef::Path { path, args } = &variant.fields[0] else {
            return None;
        };
        if !path.is_single() || path.segments[0] != string_name || !args.is_empty() {
            return None;
        }

        Some((type_idx, variant_idx))
    }

    fn body_key(body: &Body) -> usize {
        body as *const Body as usize
    }

    fn collect_body_local_accesses(
        body: &Body,
        module_scope: &ModuleScope,
    ) -> ArenaMap<ExprIdx, LocalSlotRef> {
        let mut accesses = ArenaMap::default();
        for (expr_idx, expr) in body.exprs.iter() {
            let Expr::Path(path) = expr else {
                continue;
            };
            let Some(&name) = path.segments.first() else {
                continue;
            };
            if let Some(access) = body.resolve_local_access_at(module_scope, expr_idx, name) {
                accesses.insert(expr_idx, access);
            }
        }
        accesses
    }

    pub fn new(
        item_tree: ItemTree,
        module_scope: ModuleScope,
        fn_bodies: FxHashMap<FnItemIdx, Body>,
        let_bodies: FxHashMap<LetItemIdx, Body>,
        let_scope_overrides: FxHashMap<FnItemIdx, FxHashMap<Name, Value>>,
        fn_scope_overrides: FxHashMap<FnItemIdx, FxHashMap<Name, FnItemIdx>>,
        mut interner: Interner,
        manifest: Option<CapabilityManifest>,
    ) -> Self {
        let intrinsic_list = intrinsics::all_intrinsics(&mut interner);
        let intrinsics = intrinsic_list.into_iter().collect();

        let option_some = Self::resolve_core_variant(
            &item_tree,
            &module_scope,
            &mut interner,
            CoreType::Option,
            "Some",
        );
        let option_none = Self::resolve_core_variant(
            &item_tree,
            &module_scope,
            &mut interner,
            CoreType::Option,
            "None",
        );
        let result_ok = Self::resolve_core_variant(
            &item_tree,
            &module_scope,
            &mut interner,
            CoreType::Result,
            "Ok",
        );
        let result_err = Self::resolve_core_variant(
            &item_tree,
            &module_scope,
            &mut interner,
            CoreType::Result,
            "Err",
        );

        // ParseError variants must be resolved by owning type identity, not by
        // global constructor-name lookup (constructor names can collide).
        let parse_error_invalid_int = Self::resolve_parse_error_variant(
            &item_tree,
            &module_scope,
            &mut interner,
            "InvalidInt",
        );
        let parse_error_invalid_float = Self::resolve_parse_error_variant(
            &item_tree,
            &module_scope,
            &mut interner,
            "InvalidFloat",
        );
        let fn_bodies = fn_bodies
            .into_iter()
            .map(|(fn_idx, body)| (fn_idx, Rc::new(body)))
            .collect::<FxHashMap<_, _>>();
        let let_bodies = let_bodies
            .into_iter()
            .map(|(let_idx, body)| (let_idx, Rc::new(body)))
            .collect::<FxHashMap<_, _>>();
        let body_local_accesses = fn_bodies
            .values()
            .chain(let_bodies.values())
            .map(|body| {
                (
                    Self::body_key(body),
                    Self::collect_body_local_accesses(body, &module_scope),
                )
            })
            .collect();

        Interpreter {
            item_tree,
            module_scope,
            fn_bodies,
            let_bodies,
            body_local_accesses,
            let_scope_overrides,
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
            current_body: None,
            consuming_local: None,
            top_level_lets_initialized: false,
        }
    }

    /// Consume the interpreter and return the interner (for display).
    pub fn into_interner(self) -> Interner {
        self.interner
    }

    pub fn materialize_top_level_let_values(
        &mut self,
    ) -> Result<FxHashMap<Name, Value>, RuntimeError> {
        self.initialize_top_level_lets()?;
        let mut values = FxHashMap::default();
        for (_, let_item) in self.item_tree.lets.iter() {
            if let Some(value) = self.env.lookup(let_item.name) {
                values.insert(let_item.name, value.clone());
            }
        }
        Ok(values)
    }

    fn initialize_top_level_lets(&mut self) -> Result<(), RuntimeError> {
        if self.top_level_lets_initialized {
            return Ok(());
        }

        let lets: Vec<_> = self
            .item_tree
            .lets
            .iter()
            .map(|(let_idx, let_item)| (let_idx, let_item.name))
            .collect();

        for (let_idx, let_name) in lets {
            let Some(body) = self.let_bodies.get(&let_idx).cloned() else {
                continue;
            };
            let prev_body = self.current_body.replace(body.clone());
            let result = self.eval_expr_shared(&body, body.root);
            self.current_body = prev_body;

            let value = match result? {
                ControlFlow::Value(value) => value,
                ControlFlow::Return(_) => {
                    return Err(RuntimeError::TypeError(
                        "top-level let initializer cannot return early".into(),
                    ));
                }
                ControlFlow::Break => {
                    return Err(RuntimeError::TypeError(
                        "top-level let initializer cannot use `break`".into(),
                    ));
                }
                ControlFlow::Continue => {
                    return Err(RuntimeError::TypeError(
                        "top-level let initializer cannot use `continue`".into(),
                    ));
                }
            };

            self.env.bind(let_name, value);
        }

        self.top_level_lets_initialized = true;
        Ok(())
    }

    /// Call a user-defined function by arena index (public wrapper for PBT).
    pub fn call_fn_by_idx(&mut self, fn_idx: FnItemIdx, args: Args) -> Result<Value, RuntimeError> {
        self.initialize_top_level_lets()?;
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
    pub fn fn_bodies(&self) -> &FxHashMap<FnItemIdx, Rc<Body>> {
        &self.fn_bodies
    }

    fn local_accesses_for_body(&mut self, body: &Body) -> &ArenaMap<ExprIdx, LocalSlotRef> {
        let key = Self::body_key(body);
        self.body_local_accesses
            .entry(key)
            .or_insert_with(|| Self::collect_body_local_accesses(body, &self.module_scope))
    }

    fn local_access_for_expr(&mut self, body: &Body, expr_idx: ExprIdx) -> Option<LocalSlotRef> {
        self.local_accesses_for_body(body).get(expr_idx).copied()
    }

    fn with_consuming_local<T>(
        &mut self,
        access: LocalSlotRef,
        f: impl FnOnce(&mut Self) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        let prev = self.consuming_local.replace(access);
        let result = f(self);
        self.consuming_local = prev;
        result
    }

    fn count_local_access_uses(
        &mut self,
        body: &Body,
        expr_idx: ExprIdx,
        target: LocalSlotRef,
    ) -> usize {
        match &body.exprs[expr_idx] {
            Expr::Missing | Expr::Hole | Expr::Literal(_) => 0,
            Expr::Path(_) => {
                usize::from(self.local_access_for_expr(body, expr_idx) == Some(target))
            }
            Expr::Binary { lhs, rhs, .. } => {
                self.count_local_access_uses(body, *lhs, target)
                    + self.count_local_access_uses(body, *rhs, target)
            }
            Expr::Unary { operand, .. } | Expr::Old(operand) => {
                self.count_local_access_uses(body, *operand, target)
            }
            Expr::Call { callee, args } => {
                let mut count = self.count_local_access_uses(body, *callee, target);
                for arg in args {
                    let arg_idx = match arg {
                        CallArg::Positional(idx) => *idx,
                        CallArg::Named { value, .. } => *value,
                    };
                    count += self.count_local_access_uses(body, arg_idx, target);
                }
                count
            }
            Expr::Field { base, .. } => self.count_local_access_uses(body, *base, target),
            Expr::Index { base, index } => {
                self.count_local_access_uses(body, *base, target)
                    + self.count_local_access_uses(body, *index, target)
            }
            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let mut count = self.count_local_access_uses(body, *condition, target)
                    + self.count_local_access_uses(body, *then_branch, target);
                if let Some(else_idx) = else_branch {
                    count += self.count_local_access_uses(body, *else_idx, target);
                }
                count
            }
            Expr::Match { scrutinee, arms } => {
                let mut count = self.count_local_access_uses(body, *scrutinee, target);
                for arm in arms {
                    count += self.count_local_access_uses(body, arm.body, target);
                }
                count
            }
            Expr::Block { stmts, tail } => {
                let mut count = 0;
                for stmt in stmts {
                    count += match stmt {
                        Stmt::Let { init, .. } => self.count_local_access_uses(body, *init, target),
                        Stmt::Assign { target: lhs, value } => {
                            self.count_local_access_uses(body, *lhs, target)
                                + self.count_local_access_uses(body, *value, target)
                        }
                        Stmt::While {
                            condition,
                            body: loop_body,
                        } => {
                            self.count_local_access_uses(body, *condition, target)
                                + self.count_local_access_uses(body, *loop_body, target)
                        }
                        Stmt::For {
                            source,
                            body: loop_body,
                            ..
                        } => {
                            self.count_local_access_uses(body, *source, target)
                                + self.count_local_access_uses(body, *loop_body, target)
                        }
                        Stmt::Expr(idx) => self.count_local_access_uses(body, *idx, target),
                        Stmt::Break | Stmt::Continue => 0,
                    };
                }
                if let Some(tail_idx) = tail {
                    count += self.count_local_access_uses(body, *tail_idx, target);
                }
                count
            }
            Expr::Return(value) => value
                .map(|idx| self.count_local_access_uses(body, idx, target))
                .unwrap_or(0),
            Expr::RecordLit { fields, .. } => fields
                .iter()
                .map(|(_, idx)| self.count_local_access_uses(body, *idx, target))
                .sum(),
            Expr::Lambda {
                body: lambda_body, ..
            } => self.count_local_access_uses(body, *lambda_body, target),
        }
    }

    fn expr_contains_lambda(&self, body: &Body, expr_idx: ExprIdx) -> bool {
        match &body.exprs[expr_idx] {
            Expr::Missing | Expr::Hole | Expr::Literal(_) | Expr::Path(_) => false,
            Expr::Binary { lhs, rhs, .. } => {
                self.expr_contains_lambda(body, *lhs) || self.expr_contains_lambda(body, *rhs)
            }
            Expr::Unary { operand, .. } | Expr::Old(operand) => {
                self.expr_contains_lambda(body, *operand)
            }
            Expr::Call { callee, args } => {
                self.expr_contains_lambda(body, *callee)
                    || args.iter().any(|arg| {
                        let arg_idx = match arg {
                            CallArg::Positional(idx) => *idx,
                            CallArg::Named { value, .. } => *value,
                        };
                        self.expr_contains_lambda(body, arg_idx)
                    })
            }
            Expr::Field { base, .. } => self.expr_contains_lambda(body, *base),
            Expr::Index { base, index } => {
                self.expr_contains_lambda(body, *base) || self.expr_contains_lambda(body, *index)
            }
            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr_contains_lambda(body, *condition)
                    || self.expr_contains_lambda(body, *then_branch)
                    || else_branch.is_some_and(|else_idx| self.expr_contains_lambda(body, else_idx))
            }
            Expr::Match { scrutinee, arms } => {
                self.expr_contains_lambda(body, *scrutinee)
                    || arms
                        .iter()
                        .any(|arm| self.expr_contains_lambda(body, arm.body))
            }
            Expr::Block { stmts, tail } => {
                stmts.iter().any(|stmt| match stmt {
                    Stmt::Let { init, .. } => self.expr_contains_lambda(body, *init),
                    Stmt::Assign { target, value } => {
                        self.expr_contains_lambda(body, *target)
                            || self.expr_contains_lambda(body, *value)
                    }
                    Stmt::While {
                        condition,
                        body: loop_body,
                    } => {
                        self.expr_contains_lambda(body, *condition)
                            || self.expr_contains_lambda(body, *loop_body)
                    }
                    Stmt::For {
                        source,
                        body: loop_body,
                        ..
                    } => {
                        self.expr_contains_lambda(body, *source)
                            || self.expr_contains_lambda(body, *loop_body)
                    }
                    Stmt::Expr(idx) => self.expr_contains_lambda(body, *idx),
                    Stmt::Break | Stmt::Continue => false,
                }) || tail.is_some_and(|tail_idx| self.expr_contains_lambda(body, tail_idx))
            }
            Expr::Return(value) => {
                value.is_some_and(|return_value| self.expr_contains_lambda(body, return_value))
            }
            Expr::RecordLit { fields, .. } => fields
                .iter()
                .any(|(_, value)| self.expr_contains_lambda(body, *value)),
            Expr::Lambda { .. } => true,
        }
    }

    fn leftmost_method_chain_receiver(
        &mut self,
        body: &Body,
        expr_idx: ExprIdx,
    ) -> Option<(ExprIdx, LocalSlotRef, Name)> {
        let Expr::Call { callee, .. } = &body.exprs[expr_idx] else {
            return None;
        };
        let Expr::Field { base, .. } = &body.exprs[*callee] else {
            return None;
        };
        self.leftmost_method_chain_receiver_from_base(body, *base)
    }

    fn leftmost_method_chain_receiver_from_base(
        &mut self,
        body: &Body,
        expr_idx: ExprIdx,
    ) -> Option<(ExprIdx, LocalSlotRef, Name)> {
        match &body.exprs[expr_idx] {
            Expr::Path(path) if path.is_single() => self
                .local_access_for_expr(body, expr_idx)
                .map(|access| (expr_idx, access, path.segments[0])),
            Expr::Call { .. } => self.leftmost_method_chain_receiver(body, expr_idx),
            _ => None,
        }
    }

    fn stmt_scope(body: &Body, stmt: &Stmt) -> Option<ScopeIdx> {
        let expr_idx = match stmt {
            Stmt::Let { init, .. } => *init,
            Stmt::Assign { value, .. } => *value,
            Stmt::While { condition, .. } => *condition,
            Stmt::For { source, .. } => *source,
            Stmt::Expr(idx) => *idx,
            Stmt::Break | Stmt::Continue => return None,
        };
        body.expr_scopes.get(expr_idx).copied()
    }

    fn root_block_scope(body: &Body) -> Option<ScopeIdx> {
        let Expr::Block { stmts, tail } = &body.exprs[body.root] else {
            return None;
        };
        tail.and_then(|idx| body.expr_scopes.get(idx).copied())
            .or_else(|| stmts.iter().find_map(|stmt| Self::stmt_scope(body, stmt)))
    }

    fn consuming_local_for_terminal_expr(
        &mut self,
        body: &Body,
        expr_idx: ExprIdx,
    ) -> Option<LocalSlotRef> {
        if self.expr_contains_lambda(body, expr_idx) {
            return None;
        }
        let (_, access, _) = self.leftmost_method_chain_receiver(body, expr_idx)?;
        (self.count_local_access_uses(body, expr_idx, access) == 1).then_some(access)
    }

    fn consuming_local_for_shadow_rebind_init(
        &mut self,
        body: &Body,
        pat_idx: kyokara_hir_def::expr::PatIdx,
        expr_idx: ExprIdx,
    ) -> Option<LocalSlotRef> {
        let Pat::Bind { name } = body.pats[pat_idx] else {
            return None;
        };
        if self.expr_contains_lambda(body, expr_idx) {
            return None;
        }
        let (_, access, receiver_name) = self.leftmost_method_chain_receiver(body, expr_idx)?;
        let pat_scope = body.local_binding_meta.get(pat_idx)?.scope;
        if body.local_binding_meta.get(pat_idx)?.mutable {
            return None;
        }
        let allow_root_block_shadow = Self::root_block_scope(body) == Some(pat_scope);
        if access.depth != 0 && !allow_root_block_shadow {
            return None;
        }
        if receiver_name != name {
            return None;
        }
        (self.count_local_access_uses(body, expr_idx, access) == 1).then_some(access)
    }

    fn eval_consuming_expr_shared(
        &mut self,
        body: &Body,
        expr_idx: ExprIdx,
    ) -> Result<ControlFlow, RuntimeError> {
        if let Some(access) = self.consuming_local_for_terminal_expr(body, expr_idx) {
            self.with_consuming_local(access, |this| this.eval_expr_shared(body, expr_idx))
        } else {
            self.eval_expr_shared(body, expr_idx)
        }
    }

    fn eval_consuming_expr(
        &mut self,
        env: &mut Env,
        body: &Body,
        expr_idx: ExprIdx,
    ) -> Result<ControlFlow, RuntimeError> {
        if let Some(access) = self.consuming_local_for_terminal_expr(body, expr_idx) {
            self.with_consuming_local(access, |this| this.eval_expr(env, body, expr_idx))
        } else {
            self.eval_expr(env, body, expr_idx)
        }
    }

    fn eval_terminal_expr_shared(
        &mut self,
        body: &Body,
        idx: ExprIdx,
    ) -> Result<ControlFlow, RuntimeError> {
        match &body.exprs[idx] {
            Expr::Block { stmts, tail } => self.eval_block_shared(body, stmts, *tail, true),
            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond = eval_propagate_shared!(self, body, *condition);
                match cond {
                    Value::Bool(true) => self.eval_terminal_expr_shared(body, *then_branch),
                    Value::Bool(false) => {
                        if let Some(else_idx) = else_branch {
                            self.eval_terminal_expr_shared(body, *else_idx)
                        } else {
                            Ok(ControlFlow::Value(Value::Unit))
                        }
                    }
                    _ => Err(RuntimeError::TypeError("if condition must be Bool".into())),
                }
            }
            Expr::Match { scrutinee, arms } => {
                let scrutinee_val = eval_propagate_shared!(self, body, *scrutinee);
                self.eval_match_terminal_shared(body, scrutinee_val, arms)
            }
            Expr::Return(val) => {
                let v = if let Some(idx) = *val {
                    match self.eval_terminal_expr_shared(body, idx)? {
                        ControlFlow::Value(v) | ControlFlow::Return(v) => v,
                        ControlFlow::Break => {
                            return Err(RuntimeError::TypeError(
                                "`break` used outside loop".into(),
                            ));
                        }
                        ControlFlow::Continue => {
                            return Err(RuntimeError::TypeError(
                                "`continue` used outside loop".into(),
                            ));
                        }
                    }
                } else {
                    Value::Unit
                };
                Ok(ControlFlow::Return(v))
            }
            _ => self.eval_consuming_expr_shared(body, idx),
        }
    }

    fn eval_terminal_expr(
        &mut self,
        env: &mut Env,
        body: &Body,
        idx: ExprIdx,
    ) -> Result<ControlFlow, RuntimeError> {
        match &body.exprs[idx] {
            Expr::Block { stmts, tail } => self.eval_block(env, body, stmts, *tail, true),
            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond = eval_propagate!(self, env, body, *condition);
                match cond {
                    Value::Bool(true) => self.eval_terminal_expr(env, body, *then_branch),
                    Value::Bool(false) => {
                        if let Some(else_idx) = else_branch {
                            self.eval_terminal_expr(env, body, *else_idx)
                        } else {
                            Ok(ControlFlow::Value(Value::Unit))
                        }
                    }
                    _ => Err(RuntimeError::TypeError("if condition must be Bool".into())),
                }
            }
            Expr::Match { scrutinee, arms } => {
                let scrutinee_val = eval_propagate!(self, env, body, *scrutinee);
                self.eval_match_terminal(env, body, scrutinee_val, arms)
            }
            Expr::Return(val) => {
                let v = if let Some(idx) = *val {
                    match self.eval_terminal_expr(env, body, idx)? {
                        ControlFlow::Value(v) | ControlFlow::Return(v) => v,
                        ControlFlow::Break => {
                            return Err(RuntimeError::TypeError(
                                "`break` used outside loop".into(),
                            ));
                        }
                        ControlFlow::Continue => {
                            return Err(RuntimeError::TypeError(
                                "`continue` used outside loop".into(),
                            ));
                        }
                    }
                } else {
                    Value::Unit
                };
                Ok(ControlFlow::Return(v))
            }
            _ => self.eval_consuming_expr(env, body, idx),
        }
    }

    /// Find and run the `main` function.
    pub fn run_main(&mut self) -> Result<Value, RuntimeError> {
        self.initialize_top_level_lets()?;
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

    fn select_method_candidate_by_arity(
        &self,
        candidates: &[FnItemIdx],
        actual_arg_count: usize,
    ) -> Option<FnItemIdx> {
        candidates.iter().copied().find(|&fn_idx| {
            self.item_tree.functions[fn_idx]
                .params
                .len()
                .saturating_sub(1)
                == actual_arg_count
        })
    }

    fn method_candidate_for_value(
        &self,
        base_val: &Value,
        field_name: Name,
        actual_arg_count: usize,
    ) -> Option<FnItemIdx> {
        self.receiver_key_for_value(base_val)
            .and_then(|receiver_key| {
                self.module_scope
                    .methods
                    .get(&(receiver_key, field_name))
                    .and_then(|candidates| {
                        self.select_method_candidate_by_arity(candidates, actual_arg_count)
                    })
            })
            .or_else(|| {
                self.module_scope
                    .methods
                    .get(&(ReceiverKey::Any, field_name))
                    .and_then(|candidates| {
                        self.select_method_candidate_by_arity(candidates, actual_arg_count)
                    })
            })
    }

    fn trait_method_dispatch_fn_idx(
        &self,
        trait_name: Name,
        method_name: Name,
        recv: &Value,
    ) -> Option<FnItemIdx> {
        self.item_tree.impls.iter().find_map(|(_, impl_item)| {
            let impl_trait_name = impl_item.trait_ref.path.last()?;
            if impl_trait_name != trait_name
                || !self.type_ref_matches_value(&impl_item.self_ty, recv)
            {
                return None;
            }
            impl_item
                .methods
                .iter()
                .copied()
                .find(|&fn_idx| self.item_tree.functions[fn_idx].name == method_name)
        })
    }

    fn type_ref_matches_value(&self, ty: &TypeRef, value: &Value) -> bool {
        let TypeRef::Path { path, .. } = ty else {
            return false;
        };
        let Some(name) = path.last() else {
            return false;
        };
        match value {
            Value::Int(_) => name.resolve(&self.interner) == "Int",
            Value::Float(_) => name.resolve(&self.interner) == "Float",
            Value::String(_) => name.resolve(&self.interner) == "String",
            Value::Char(_) => name.resolve(&self.interner) == "Char",
            Value::Bool(_) => name.resolve(&self.interner) == "Bool",
            Value::Unit => name.resolve(&self.interner) == "Unit",
            Value::Adt { type_idx, .. } => name == self.item_tree.types[*type_idx].name,
            Value::Record {
                type_idx: Some(type_idx),
                ..
            } => name == self.item_tree.types[*type_idx].name,
            Value::Record { type_idx: None, .. } => false,
            Value::List(_) => name.resolve(&self.interner) == "List",
            Value::MutableList(_) => name.resolve(&self.interner) == "MutableList",
            Value::MutablePriorityQueue(_) => {
                name.resolve(&self.interner) == "MutablePriorityQueue"
            }
            Value::Deque(_) => name.resolve(&self.interner) == "Deque",
            Value::BitSet(_) => name.resolve(&self.interner) == "BitSet",
            Value::MutableBitSet(_) => name.resolve(&self.interner) == "MutableBitSet",
            Value::Map(_) => name.resolve(&self.interner) == "Map",
            Value::MutableMap(_) => name.resolve(&self.interner) == "MutableMap",
            Value::Set(_) => name.resolve(&self.interner) == "Set",
            Value::MutableSet(_) => name.resolve(&self.interner) == "MutableSet",
            Value::Seq(_) => name.resolve(&self.interner) == "Seq",
            Value::Fn(_) => false,
        }
    }

    fn call_trait_qualified(
        &mut self,
        trait_name: Name,
        method_name: Name,
        args: &[CallArg],
        arg_values: Vec<Value>,
    ) -> Result<Value, RuntimeError> {
        let Some(&trait_idx) = self.module_scope.traits.get(&trait_name) else {
            return Err(RuntimeError::UnresolvedName(
                trait_name.resolve(&self.interner).to_string(),
            ));
        };
        let trait_item = &self.item_tree.traits[trait_idx];
        let Some(method) = trait_item
            .methods
            .iter()
            .find(|method| method.name == method_name)
        else {
            return Err(RuntimeError::TypeError(format!(
                "trait `{}` has no method `{}`",
                trait_name.resolve(&self.interner),
                method_name.resolve(&self.interner)
            )));
        };
        let param_names: Vec<Name> = method.params.iter().map(|param| param.name).collect();
        let callee_name = format!(
            "trait method `{}.{}`",
            trait_name.resolve(&self.interner),
            method_name.resolve(&self.interner)
        );
        let bound_args =
            self.bind_call_values_for_param_names(&callee_name, args, arg_values, &param_names)?;
        let Some(receiver) = bound_args.first() else {
            return Err(RuntimeError::ArityMismatch {
                callee: callee_name,
                expected: method.params.len(),
                actual: 0,
            });
        };
        if let Some(fn_idx) = self.trait_method_dispatch_fn_idx(trait_name, method_name, receiver) {
            return self.call_fn(fn_idx, bound_args);
        }
        self.call_builtin_trait_method(trait_name, method_name, bound_args)
    }

    fn call_builtin_trait_method(
        &mut self,
        trait_name: Name,
        method_name: Name,
        args: Args,
    ) -> Result<Value, RuntimeError> {
        match (
            trait_name.resolve(&self.interner),
            method_name.resolve(&self.interner),
            args.as_slice(),
        ) {
            ("Eq", "eq", [lhs, rhs]) => Ok(Value::Bool(self.trait_eq_values(lhs, rhs)?)),
            ("Ord", "compare", [lhs, rhs]) => Ok(Value::Int(self.trait_compare_values(lhs, rhs)?)),
            ("Hash", "hash", [value]) => Ok(Value::Int(self.trait_hash_value(value)?)),
            ("Show", "show", [value]) => Ok(Value::String(self.trait_show_value(value)?)),
            _ => Err(RuntimeError::TypeError(format!(
                "unsupported trait call `{}.{}`",
                trait_name.resolve(&self.interner),
                method_name.resolve(&self.interner)
            ))),
        }
    }

    fn trait_eq_values(&mut self, lhs: &Value, rhs: &Value) -> Result<bool, RuntimeError> {
        if let Some(fn_idx) = self.resolve_named_trait_method("Eq", "eq", lhs) {
            let value = self.call_fn(fn_idx, smallvec::smallvec![lhs.clone(), rhs.clone()])?;
            let Value::Bool(result) = value else {
                return Err(RuntimeError::TypeError("Eq.eq must return Bool".into()));
            };
            return Ok(result);
        }

        match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => Ok(a == b),
            (Value::String(a), Value::String(b)) => Ok(a == b),
            (Value::Char(a), Value::Char(b)) => Ok(a == b),
            (Value::Bool(a), Value::Bool(b)) => Ok(a == b),
            (Value::Unit, Value::Unit) => Ok(true),
            (
                Value::Adt {
                    type_idx: t1,
                    variant: v1,
                    fields: f1,
                },
                Value::Adt {
                    type_idx: t2,
                    variant: v2,
                    fields: f2,
                },
            ) => {
                if t1 != t2 || v1 != v2 || f1.len() != f2.len() {
                    return Ok(false);
                }
                for (lhs_field, rhs_field) in f1.iter().zip(f2.iter()) {
                    if !self.trait_eq_values(lhs_field, rhs_field)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            (Value::Record { fields: f1, .. }, Value::Record { fields: f2, .. }) => {
                if f1.len() != f2.len() {
                    return Ok(false);
                }
                let mut lhs_fields: Vec<_> = f1.iter().collect();
                let mut rhs_fields: Vec<_> = f2.iter().collect();
                lhs_fields.sort_by(|(lhs_name, _), (rhs_name, _)| {
                    lhs_name
                        .resolve(&self.interner)
                        .cmp(rhs_name.resolve(&self.interner))
                });
                rhs_fields.sort_by(|(lhs_name, _), (rhs_name, _)| {
                    lhs_name
                        .resolve(&self.interner)
                        .cmp(rhs_name.resolve(&self.interner))
                });
                for ((lhs_name, lhs_value), (rhs_name, rhs_value)) in
                    lhs_fields.into_iter().zip(rhs_fields.into_iter())
                {
                    if lhs_name != rhs_name || !self.trait_eq_values(lhs_value, rhs_value)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            (Value::List(lhs), Value::List(rhs)) => self.trait_eq_slice(lhs, rhs),
            (Value::MutableList(lhs), Value::MutableList(rhs)) => {
                let lhs_items = lhs.snapshot();
                let rhs_items = rhs.snapshot();
                self.trait_eq_slice(lhs_items.as_ref(), rhs_items.as_ref())
            }
            (Value::Deque(lhs), Value::Deque(rhs)) => {
                if lhs.len() != rhs.len() {
                    return Ok(false);
                }
                for (lhs_value, rhs_value) in lhs.iter().zip(rhs.iter()) {
                    if !self.trait_eq_values(lhs_value, rhs_value)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            (Value::BitSet(lhs), Value::BitSet(rhs)) => Ok(lhs == rhs),
            (Value::Map(lhs), Value::Map(rhs)) => Ok(lhs == rhs),
            (Value::Set(lhs), Value::Set(rhs)) => Ok(lhs == rhs),
            (Value::MutableMap(lhs), Value::MutableMap(rhs)) => Ok(*lhs.borrow() == *rhs.borrow()),
            (Value::MutableSet(lhs), Value::MutableSet(rhs)) => Ok(*lhs.borrow() == *rhs.borrow()),
            (Value::MutableBitSet(lhs), Value::MutableBitSet(rhs)) => {
                Ok(lhs.snapshot() == rhs.snapshot())
            }
            (Value::Fn(_), Value::Fn(_)) => Err(RuntimeError::TypeError(
                "functions do not implement Eq".into(),
            )),
            (Value::Seq(_), Value::Seq(_)) => {
                Err(RuntimeError::TypeError("Seq does not implement Eq".into()))
            }
            _ => Ok(false),
        }
    }

    fn trait_eq_slice(&mut self, lhs: &[Value], rhs: &[Value]) -> Result<bool, RuntimeError> {
        if lhs.len() != rhs.len() {
            return Ok(false);
        }
        for (lhs_value, rhs_value) in lhs.iter().zip(rhs.iter()) {
            if !self.trait_eq_values(lhs_value, rhs_value)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn trait_compare_values(&mut self, lhs: &Value, rhs: &Value) -> Result<i64, RuntimeError> {
        if let Some(fn_idx) = self.resolve_named_trait_method("Ord", "compare", lhs) {
            let value = self.call_fn(fn_idx, smallvec::smallvec![lhs.clone(), rhs.clone()])?;
            let Value::Int(result) = value else {
                return Err(RuntimeError::TypeError(
                    "Ord.compare must return Int".into(),
                ));
            };
            return Ok(result);
        }

        let ord = match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => a.cmp(b),
            (Value::String(a), Value::String(b)) => a.cmp(b),
            (Value::Char(a), Value::Char(b)) => a.cmp(b),
            (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
            (Value::Unit, Value::Unit) => Ordering::Equal,
            (
                Value::Adt {
                    type_idx: t1,
                    variant: v1,
                    fields: f1,
                },
                Value::Adt {
                    type_idx: t2,
                    variant: v2,
                    fields: f2,
                },
            ) if t1 == t2 => {
                let variant_ord = v1.cmp(v2);
                if variant_ord != Ordering::Equal {
                    variant_ord
                } else {
                    self.trait_compare_slices(f1, f2)?
                }
            }
            (Value::Record { fields: f1, .. }, Value::Record { fields: f2, .. }) => {
                let mut lhs_fields: Vec<_> = f1.iter().collect();
                let mut rhs_fields: Vec<_> = f2.iter().collect();
                lhs_fields.sort_by(|(lhs_name, _), (rhs_name, _)| {
                    lhs_name
                        .resolve(&self.interner)
                        .cmp(rhs_name.resolve(&self.interner))
                });
                rhs_fields.sort_by(|(lhs_name, _), (rhs_name, _)| {
                    lhs_name
                        .resolve(&self.interner)
                        .cmp(rhs_name.resolve(&self.interner))
                });
                let name_ord = lhs_fields
                    .iter()
                    .map(|(name, _)| name.resolve(&self.interner))
                    .cmp(
                        rhs_fields
                            .iter()
                            .map(|(name, _)| name.resolve(&self.interner)),
                    );
                if name_ord != Ordering::Equal {
                    name_ord
                } else {
                    let lhs_values: Vec<_> = lhs_fields
                        .into_iter()
                        .map(|(_, value)| value.clone())
                        .collect();
                    let rhs_values: Vec<_> = rhs_fields
                        .into_iter()
                        .map(|(_, value)| value.clone())
                        .collect();
                    self.trait_compare_slices_refs(&lhs_values, &rhs_values)?
                }
            }
            (Value::List(lhs), Value::List(rhs)) => self.trait_compare_slices(lhs, rhs)?,
            (Value::MutableList(lhs), Value::MutableList(rhs)) => {
                let lhs_items = lhs.snapshot();
                let rhs_items = rhs.snapshot();
                self.trait_compare_slices(lhs_items.as_ref(), rhs_items.as_ref())?
            }
            (Value::MutablePriorityQueue(_), Value::MutablePriorityQueue(_)) => {
                return Err(RuntimeError::TypeError(
                    "MutablePriorityQueue does not implement Ord".into(),
                ));
            }
            (Value::Deque(lhs), Value::Deque(rhs)) => {
                let lhs_values: Vec<_> = lhs.iter().cloned().collect();
                let rhs_values: Vec<_> = rhs.iter().cloned().collect();
                self.trait_compare_slices_refs(&lhs_values, &rhs_values)?
            }
            (Value::BitSet(lhs), Value::BitSet(rhs)) => lhs.words().cmp(&rhs.words()),
            _ => {
                return Err(RuntimeError::TypeError(
                    "value does not implement Ord".into(),
                ));
            }
        };

        Ok(match ord {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        })
    }

    fn trait_compare_slices(
        &mut self,
        lhs: &[Value],
        rhs: &[Value],
    ) -> Result<Ordering, RuntimeError> {
        self.trait_compare_slices_refs(lhs, rhs)
    }

    fn trait_compare_slices_refs(
        &mut self,
        lhs: &[Value],
        rhs: &[Value],
    ) -> Result<Ordering, RuntimeError> {
        for (lhs_value, rhs_value) in lhs.iter().zip(rhs.iter()) {
            let ord = self.trait_compare_values(lhs_value, rhs_value)?;
            match ord.cmp(&0) {
                Ordering::Equal => {}
                non_eq => return Ok(non_eq),
            }
        }
        Ok(lhs.len().cmp(&rhs.len()))
    }

    fn priority_queue_is_better(
        &mut self,
        direction: PriorityQueueDirection,
        lhs: &PriorityQueueEntry,
        rhs: &PriorityQueueEntry,
    ) -> Result<bool, RuntimeError> {
        let ord = self.trait_compare_values(&lhs.priority, &rhs.priority)?;
        if ord != 0 {
            return Ok(match direction {
                PriorityQueueDirection::Min => ord < 0,
                PriorityQueueDirection::Max => ord > 0,
            });
        }
        Ok(lhs.seq < rhs.seq)
    }

    fn priority_queue_bubble_up(
        &mut self,
        queue: &mut MutablePriorityQueueValue,
        mut idx: usize,
    ) -> Result<(), RuntimeError> {
        while idx > 0 {
            let parent = (idx - 1) / 2;
            if !self.priority_queue_is_better(
                queue.direction,
                &queue.entries[idx],
                &queue.entries[parent],
            )? {
                break;
            }
            queue.entries.swap(idx, parent);
            idx = parent;
        }
        Ok(())
    }

    fn priority_queue_bubble_down(
        &mut self,
        queue: &mut MutablePriorityQueueValue,
        mut idx: usize,
    ) -> Result<(), RuntimeError> {
        let len = queue.entries.len();
        loop {
            let left = idx * 2 + 1;
            if left >= len {
                break;
            }
            let right = left + 1;
            let mut best = left;
            if right < len
                && self.priority_queue_is_better(
                    queue.direction,
                    &queue.entries[right],
                    &queue.entries[left],
                )?
            {
                best = right;
            }
            if !self.priority_queue_is_better(
                queue.direction,
                &queue.entries[best],
                &queue.entries[idx],
            )? {
                break;
            }
            queue.entries.swap(idx, best);
            idx = best;
        }
        Ok(())
    }

    fn priority_queue_make_payload(&mut self, entry: PriorityQueueEntry) -> Value {
        let priority_name = Name::new(&mut self.interner, "priority");
        let value_name = Name::new(&mut self.interner, "value");
        Value::Record {
            fields: vec![(priority_name, entry.priority), (value_name, entry.value)],
            type_idx: None,
        }
    }

    fn trait_hash_value(&mut self, value: &Value) -> Result<i64, RuntimeError> {
        if let Some(fn_idx) = self.resolve_named_trait_method("Hash", "hash", value) {
            let value = self.call_fn(fn_idx, smallvec::smallvec![value.clone()])?;
            let Value::Int(result) = value else {
                return Err(RuntimeError::TypeError("Hash.hash must return Int".into()));
            };
            return Ok(result);
        }

        let mut hasher = DefaultHasher::new();
        self.write_builtin_trait_hash(value, &mut hasher)?;
        Ok(i64::from_ne_bytes(hasher.finish().to_ne_bytes()))
    }

    fn write_builtin_trait_hash(
        &mut self,
        value: &Value,
        hasher: &mut DefaultHasher,
    ) -> Result<(), RuntimeError> {
        match value {
            Value::Int(n) => {
                0u8.hash(hasher);
                n.hash(hasher);
            }
            Value::String(s) => {
                1u8.hash(hasher);
                s.hash(hasher);
            }
            Value::Char(c) => {
                2u8.hash(hasher);
                c.hash(hasher);
            }
            Value::Bool(b) => {
                3u8.hash(hasher);
                b.hash(hasher);
            }
            Value::Unit => {
                4u8.hash(hasher);
            }
            Value::Adt {
                type_idx,
                variant,
                fields,
            } => {
                5u8.hash(hasher);
                self.item_tree.types[*type_idx]
                    .name
                    .resolve(&self.interner)
                    .hash(hasher);
                variant.hash(hasher);
                for field in fields {
                    self.trait_hash_value(field)?.hash(hasher);
                }
            }
            Value::Record { type_idx, fields } => {
                6u8.hash(hasher);
                if let Some(type_idx) = type_idx {
                    self.item_tree.types[*type_idx]
                        .name
                        .resolve(&self.interner)
                        .hash(hasher);
                }
                let mut sorted_fields: Vec<_> = fields.iter().collect();
                sorted_fields.sort_by(|(lhs_name, _), (rhs_name, _)| {
                    lhs_name
                        .resolve(&self.interner)
                        .cmp(rhs_name.resolve(&self.interner))
                });
                for (name, value) in sorted_fields {
                    name.resolve(&self.interner).hash(hasher);
                    self.trait_hash_value(value)?.hash(hasher);
                }
            }
            Value::List(items) => {
                7u8.hash(hasher);
                for item in items.iter() {
                    self.trait_hash_value(item)?.hash(hasher);
                }
            }
            Value::Deque(items) => {
                8u8.hash(hasher);
                for item in items.iter() {
                    self.trait_hash_value(item)?.hash(hasher);
                }
            }
            Value::BitSet(bitset) => {
                9u8.hash(hasher);
                bitset.size_bits().hash(hasher);
                bitset.words().hash(hasher);
            }
            Value::Map(entries) => {
                10u8.hash(hasher);
                let mut entry_hashes: Vec<u64> = entries
                    .entries()
                    .iter()
                    .map(|entry| {
                        let mut entry_hasher = DefaultHasher::new();
                        entry.hash.hash(&mut entry_hasher);
                        self.trait_hash_value(&entry.key)?.hash(&mut entry_hasher);
                        self.trait_hash_value(&entry.value)?.hash(&mut entry_hasher);
                        Ok(entry_hasher.finish())
                    })
                    .collect::<Result<_, RuntimeError>>()?;
                entry_hashes.sort_unstable();
                entry_hashes.hash(hasher);
            }
            Value::Set(entries) => {
                11u8.hash(hasher);
                let mut entry_hashes: Vec<u64> = entries
                    .entries()
                    .iter()
                    .map(|entry| {
                        let mut entry_hasher = DefaultHasher::new();
                        entry.hash.hash(&mut entry_hasher);
                        self.trait_hash_value(&entry.value)?.hash(&mut entry_hasher);
                        Ok(entry_hasher.finish())
                    })
                    .collect::<Result<_, RuntimeError>>()?;
                entry_hashes.sort_unstable();
                entry_hashes.hash(hasher);
            }
            Value::MutableList(_)
            | Value::MutablePriorityQueue(_)
            | Value::MutableMap(_)
            | Value::MutableSet(_)
            | Value::MutableBitSet(_)
            | Value::Seq(_)
            | Value::Fn(_)
            | Value::Float(_) => {
                return Err(RuntimeError::TypeError(
                    "value does not implement Hash".into(),
                ));
            }
        }
        Ok(())
    }

    fn trait_show_value(&mut self, value: &Value) -> Result<String, RuntimeError> {
        if let Some(fn_idx) = self.resolve_named_trait_method("Show", "show", value) {
            let value = self.call_fn(fn_idx, smallvec::smallvec![value.clone()])?;
            let Value::String(result) = value else {
                return Err(RuntimeError::TypeError(
                    "Show.show must return String".into(),
                ));
            };
            return Ok(result);
        }

        match value {
            Value::Int(n) => Ok(n.to_string()),
            Value::Float(f) => Ok(f.to_string()),
            Value::String(s) => Ok(s.clone()),
            Value::Char(c) => Ok(c.to_string()),
            Value::Bool(b) => Ok(b.to_string()),
            Value::Unit => Ok("()".to_string()),
            Value::Adt {
                type_idx,
                variant,
                fields,
            } => {
                let type_item = &self.item_tree.types[*type_idx];
                let TypeDefKind::Adt { variants } = &type_item.kind else {
                    return Ok(value.display(&self.interner));
                };
                let variant_name = variants
                    .get(*variant)
                    .map(|variant| variant.name.resolve(&self.interner).to_string())
                    .unwrap_or("<variant>".to_string());
                if fields.is_empty() {
                    return Ok(variant_name);
                }
                let mut shown_fields = Vec::with_capacity(fields.len());
                for field in fields {
                    shown_fields.push(self.trait_show_value(field)?);
                }
                Ok(format!("{variant_name}({})", shown_fields.join(", ")))
            }
            Value::Record { fields, .. } => {
                let mut shown_fields = Vec::with_capacity(fields.len());
                for (name, value) in fields {
                    let field_name = name.resolve(&self.interner).to_string();
                    let shown_value = self.trait_show_value(value)?;
                    shown_fields.push(format!("{field_name}: {shown_value}"));
                }
                shown_fields.sort();
                Ok(format!("{{ {} }}", shown_fields.join(", ")))
            }
            Value::List(items) => Ok(format!(
                "[{}]",
                items
                    .iter()
                    .map(|item| self.trait_show_value(item))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ")
            )),
            Value::Deque(items) => Ok(format!(
                "Deque([{}])",
                items
                    .iter()
                    .map(|item| self.trait_show_value(item))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ")
            )),
            Value::Seq(_) => Ok("<seq>".to_string()),
            _ => Ok(value.display(&self.interner)),
        }
    }

    fn resolve_named_trait_method(
        &self,
        trait_name: &str,
        method_name: &str,
        recv: &Value,
    ) -> Option<FnItemIdx> {
        self.item_tree.impls.iter().find_map(|(_, impl_item)| {
            let impl_trait_name = impl_item.trait_ref.path.last()?;
            if impl_trait_name.resolve(&self.interner) != trait_name
                || !self.type_ref_matches_value(&impl_item.self_ty, recv)
            {
                return None;
            }
            impl_item.methods.iter().copied().find(|&fn_idx| {
                self.item_tree.functions[fn_idx]
                    .name
                    .resolve(&self.interner)
                    == method_name
            })
        })
    }

    fn hash_for_collection_key(&mut self, value: &Value) -> Result<i64, RuntimeError> {
        if let Ok(key) = MapKey::from_value(value) {
            return Ok(key.primitive_hash());
        }
        self.trait_hash_value(value)
    }

    fn map_get_value(
        &mut self,
        entries: &MapValue,
        key: &Value,
    ) -> Result<Option<Value>, RuntimeError> {
        if let Ok(map_key) = MapKey::from_value(key) {
            return Ok(entries.get(&map_key).cloned());
        }
        let hash = self.hash_for_collection_key(key)?;
        entries.get_cloned_with(hash, key, &mut |lhs, rhs| self.trait_eq_values(lhs, rhs))
    }

    fn map_contains_value(
        &mut self,
        entries: &MapValue,
        key: &Value,
    ) -> Result<bool, RuntimeError> {
        if let Ok(map_key) = MapKey::from_value(key) {
            return Ok(entries.get(&map_key).is_some());
        }
        let hash = self.hash_for_collection_key(key)?;
        entries.contains_with(hash, key, &mut |lhs, rhs| self.trait_eq_values(lhs, rhs))
    }

    fn mutable_map_get_value(
        &mut self,
        entries: &MutableMapValue,
        key: &Value,
    ) -> Result<Option<Value>, RuntimeError> {
        if let Ok(map_key) = MapKey::from_value(key) {
            return Ok(entries.get_cloned_primitive(&map_key));
        }
        let hash = self.hash_for_collection_key(key)?;
        entries.get_cloned_with(hash, key, &mut |lhs, rhs| self.trait_eq_values(lhs, rhs))
    }

    fn mutable_map_contains_value(
        &mut self,
        entries: &MutableMapValue,
        key: &Value,
    ) -> Result<bool, RuntimeError> {
        if let Ok(map_key) = MapKey::from_value(key) {
            return Ok(entries.contains_primitive(&map_key));
        }
        let hash = self.hash_for_collection_key(key)?;
        entries.contains_with(hash, key, &mut |lhs, rhs| self.trait_eq_values(lhs, rhs))
    }

    fn map_insert_value(
        &mut self,
        entries: &MapValue,
        key: Value,
        value: Value,
    ) -> Result<MapValue, RuntimeError> {
        let hash = self.hash_for_collection_key(&key)?;
        entries.insert_persistent_with(hash, key, value, &mut |lhs, rhs| {
            self.trait_eq_values(lhs, rhs)
        })
    }

    fn set_contains_value(
        &mut self,
        entries: &SetValue,
        value: &Value,
    ) -> Result<bool, RuntimeError> {
        if let Ok(map_key) = MapKey::from_value(value) {
            return Ok(entries.contains(&map_key));
        }
        let hash = self.hash_for_collection_key(value)?;
        entries.contains_with(hash, value, &mut |lhs, rhs| self.trait_eq_values(lhs, rhs))
    }

    fn mutable_set_contains_value(
        &mut self,
        entries: &MutableSetValue,
        value: &Value,
    ) -> Result<bool, RuntimeError> {
        if let Ok(map_key) = MapKey::from_value(value) {
            return Ok(entries.contains_primitive(&map_key));
        }
        let hash = self.hash_for_collection_key(value)?;
        entries.contains_with(hash, value, &mut |lhs, rhs| self.trait_eq_values(lhs, rhs))
    }

    fn param_names_for_fn_value(&self, fv: &FnValue) -> Option<Vec<Name>> {
        match fv {
            FnValue::User(fn_idx) => Some(self.param_names_for_fn_idx(*fn_idx)),
            FnValue::Lambda { params, body, .. } => self.lambda_param_names(body, params),
            _ => None,
        }
    }

    fn bind_call_items_for_param_names<T>(
        &self,
        callee_name: &str,
        args: &[CallArg],
        arg_items: Vec<T>,
        param_names: &[Name],
    ) -> Result<Vec<T>, RuntimeError> {
        if args.len() != param_names.len() {
            return Err(RuntimeError::ArityMismatch {
                callee: callee_name.to_string(),
                expected: param_names.len(),
                actual: args.len(),
            });
        }

        let has_named = args.iter().any(|arg| matches!(arg, CallArg::Named { .. }));
        if !has_named {
            return Ok(arg_items);
        }

        let mut slots: Vec<Option<T>> = (0..param_names.len()).map(|_| None).collect();
        let mut next_pos = 0usize;
        let mut saw_named = false;
        for (arg, item) in args.iter().zip(arg_items.into_iter()) {
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
                    slots[next_pos] = Some(item);
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
                    slots[slot_idx] = Some(item);
                }
            }
        }

        let mut out = Vec::with_capacity(param_names.len());
        for (idx, slot) in slots.into_iter().enumerate() {
            if let Some(item) = slot {
                out.push(item);
            } else {
                return Err(RuntimeError::TypeError(format!(
                    "missing argument for parameter `{}`",
                    param_names[idx].resolve(&self.interner)
                )));
            }
        }

        Ok(out)
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
        let items =
            self.bind_call_items_for_param_names(callee_name, args, arg_values, param_names)?;
        let mut out = Args::with_capacity(items.len());
        for value in items {
            out.push(value);
        }
        Ok(out)
    }

    fn intrinsic_for_fn_idx(&self, fn_idx: FnItemIdx) -> Option<IntrinsicFn> {
        let fn_item = &self.item_tree.functions[fn_idx];
        self.intrinsics.get(&fn_item.name).copied()
    }

    fn eval_immediate_zero_arg_lambda(
        &mut self,
        env: &mut Env,
        body: &Body,
        expr_idx: ExprIdx,
    ) -> Result<Option<Value>, RuntimeError> {
        let Expr::Lambda {
            params,
            body: lambda_body,
        } = &body.exprs[expr_idx]
        else {
            return Ok(None);
        };
        if !params.is_empty() {
            return Ok(None);
        }
        env.push_scope();
        let result = self.eval_terminal_expr(env, body, *lambda_body);
        env.pop_scope();
        result.map(|value| Some(value.into_value()))
    }

    fn try_eval_lazy_mutable_map_get_or_insert_with_local(
        &mut self,
        env: &mut Env,
        body: &Body,
        base_val: &Value,
        args: &[CallArg],
        fn_idx: FnItemIdx,
    ) -> Result<Option<Value>, RuntimeError> {
        if self.intrinsic_for_fn_idx(fn_idx) != Some(IntrinsicFn::MutableMapGetOrInsertWith) {
            return Ok(None);
        }
        let Value::MutableMap(entries) = base_val else {
            return Ok(None);
        };
        let (key_expr, thunk_expr) = match args {
            [
                CallArg::Positional(key_expr),
                CallArg::Positional(thunk_expr),
            ] => (*key_expr, *thunk_expr),
            _ => {
                let full_param_names = self.param_names_for_fn_idx(fn_idx);
                let method_param_names: Vec<Name> =
                    full_param_names.iter().skip(1).copied().collect();
                let arg_exprs = self.bind_call_items_for_param_names(
                    "method `get_or_insert_with`",
                    args,
                    self.args_in_source_order(args),
                    &method_param_names,
                )?;
                (arg_exprs[0], arg_exprs[1])
            }
        };
        let key = self.eval_terminal_expr(env, body, key_expr)?.into_value();
        if let Some(existing) = {
            let borrowed = entries.borrow();
            self.mutable_map_get_value(&borrowed, &key)?
        } {
            return Ok(Some(existing.clone()));
        }
        let computed =
            if let Some(value) = self.eval_immediate_zero_arg_lambda(env, body, thunk_expr)? {
                value
            } else {
                let thunk = self.eval_terminal_expr(env, body, thunk_expr)?.into_value();
                self.call_value(thunk, Args::new())?
            };
        if let Some(existing) = {
            let borrowed = entries.borrow();
            self.mutable_map_get_value(&borrowed, &key)?
        } {
            return Ok(Some(existing.clone()));
        }
        if let Ok(primitive_key) = MapKey::from_value(&key) {
            entries
                .borrow_mut()
                .insert_primitive(primitive_key, computed.clone());
        } else {
            let hash = self.hash_for_collection_key(&key)?;
            entries
                .borrow_mut()
                .insert_with(hash, key, computed.clone(), &mut |lhs, rhs| {
                    self.trait_eq_values(lhs, rhs)
                })?;
        }
        Ok(Some(computed))
    }

    fn try_eval_lazy_mutable_map_get_or_insert_with<F>(
        &mut self,
        base_val: &Value,
        args: &[CallArg],
        fn_idx: FnItemIdx,
        mut eval_expr_value: F,
    ) -> Result<Option<Value>, RuntimeError>
    where
        F: FnMut(&mut Self, ExprIdx) -> Result<Value, RuntimeError>,
    {
        if self.intrinsic_for_fn_idx(fn_idx) != Some(IntrinsicFn::MutableMapGetOrInsertWith) {
            return Ok(None);
        }
        let Value::MutableMap(entries) = base_val else {
            return Ok(None);
        };
        let (key_expr, thunk_expr) = match args {
            [
                CallArg::Positional(key_expr),
                CallArg::Positional(thunk_expr),
            ] => (*key_expr, *thunk_expr),
            _ => {
                let full_param_names = self.param_names_for_fn_idx(fn_idx);
                let method_param_names: Vec<Name> =
                    full_param_names.iter().skip(1).copied().collect();
                let arg_exprs = self.bind_call_items_for_param_names(
                    "method `get_or_insert_with`",
                    args,
                    self.args_in_source_order(args),
                    &method_param_names,
                )?;
                (arg_exprs[0], arg_exprs[1])
            }
        };
        let key = eval_expr_value(self, key_expr)?;
        if let Some(existing) = {
            let borrowed = entries.borrow();
            self.mutable_map_get_value(&borrowed, &key)?
        } {
            return Ok(Some(existing.clone()));
        }
        let thunk = eval_expr_value(self, thunk_expr)?;
        let computed = self.call_value(thunk, Args::new())?;
        if let Some(existing) = {
            let borrowed = entries.borrow();
            self.mutable_map_get_value(&borrowed, &key)?
        } {
            return Ok(Some(existing.clone()));
        }
        if let Ok(primitive_key) = MapKey::from_value(&key) {
            entries
                .borrow_mut()
                .insert_primitive(primitive_key, computed.clone());
        } else {
            let hash = self.hash_for_collection_key(&key)?;
            entries
                .borrow_mut()
                .insert_with(hash, key, computed.clone(), &mut |lhs, rhs| {
                    self.trait_eq_values(lhs, rhs)
                })?;
        }
        Ok(Some(computed))
    }

    fn fn_value_for_fn_idx(&self, fn_idx: FnItemIdx) -> Value {
        let fn_item = &self.item_tree.functions[fn_idx];
        if let Some(intr) = self.intrinsics.get(&fn_item.name) {
            Value::Fn(Box::new(FnValue::Intrinsic(*intr)))
        } else {
            Value::Fn(Box::new(FnValue::User(fn_idx)))
        }
    }

    fn resolve_direct_user_fn_idx(&self, name: Name) -> Option<FnItemIdx> {
        if let Some(cur_fn) = self.current_fn
            && let Some(overrides) = self.fn_scope_overrides.get(&cur_fn)
            && let Some(&fn_idx) = overrides.get(&name)
        {
            return Some(fn_idx);
        }

        self.module_scope
            .functions
            .get(&name)
            .copied()
            .filter(|fn_idx| self.fn_bodies.contains_key(fn_idx))
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

    fn eval_fn_body_value(&mut self, body: &Body) -> Result<Value, RuntimeError> {
        match self.eval_terminal_expr_shared(body, body.root)? {
            ControlFlow::Value(v) | ControlFlow::Return(v) => Ok(v),
            ControlFlow::Break => Err(RuntimeError::TypeError("`break` used outside loop".into())),
            ControlFlow::Continue => Err(RuntimeError::TypeError(
                "`continue` used outside loop".into(),
            )),
        }
    }

    fn call_fn_with_contracts(
        &mut self,
        body: &Body,
        fn_name: Name,
    ) -> Result<Value, RuntimeError> {
        let has_ensures = !body.ensures.is_empty();
        let fn_name_str = fn_name.resolve(&self.interner).to_string();
        let mut prev_old_env = None;
        let mut swapped_old_env = false;

        let result = (|| -> Result<Value, RuntimeError> {
            for req_idx in body.requires.iter().copied() {
                let val = self.eval_expr_shared(body, req_idx)?.into_value();
                if !matches!(val, Value::Bool(true)) {
                    Err(RuntimeError::PreconditionFailed(fn_name_str.clone()))?;
                }
            }

            if has_ensures {
                prev_old_env = self.old_env.replace(self.env.clone());
                swapped_old_env = true;
            }

            let return_val = self.eval_fn_body_value(body)?;

            for inv_idx in body.invariant.iter().copied() {
                let val = self.eval_expr_shared(body, inv_idx)?.into_value();
                if !matches!(val, Value::Bool(true)) {
                    Err(RuntimeError::InvariantViolated(fn_name_str.clone()))?;
                }
            }

            if has_ensures {
                let result_name = Name::new(&mut self.interner, "result");
                for ens_idx in body.ensures.iter().copied() {
                    self.env.push_scope();
                    self.env.bind(result_name, return_val.clone());
                    if let Some(old_env) = self.old_env.as_mut() {
                        old_env.push_scope();
                    }
                    let val = match self.eval_expr_shared(body, ens_idx) {
                        Ok(value) => value.into_value(),
                        Err(err) => {
                            self.env.pop_scope();
                            if let Some(old_env) = self.old_env.as_mut() {
                                old_env.pop_scope();
                            }
                            return Err(err);
                        }
                    };
                    self.env.pop_scope();
                    if let Some(old_env) = self.old_env.as_mut() {
                        old_env.pop_scope();
                    }
                    if !matches!(val, Value::Bool(true)) {
                        Err(RuntimeError::PostconditionFailed(fn_name_str.clone()))?;
                    }
                }
            }

            Ok(return_val)
        })();

        if swapped_old_env {
            self.old_env = prev_old_env;
        }

        result
    }

    fn call_fn_impl(&mut self, fn_idx: FnItemIdx, args: Args) -> Result<Value, RuntimeError> {
        let body = self
            .fn_bodies
            .get(&fn_idx)
            .cloned()
            .ok_or_else(|| RuntimeError::UnresolvedName("function body not found".into()))?;
        let actual = args.len();
        let fn_name = {
            let fn_item = &self.item_tree.functions[fn_idx];
            self.ensure_user_fn_caps_allowed(fn_item)?;
            let expected = fn_item.params.len();
            if expected != actual {
                return Err(RuntimeError::ArityMismatch {
                    callee: format!("function `{}`", fn_item.name.resolve(&self.interner)),
                    expected,
                    actual,
                });
            }
            self.env.push_scope();
            for (param, val) in fn_item.params.iter().zip(args.into_iter()) {
                self.env.bind(param.name, val);
            }
            fn_item.name
        };

        let result =
            if body.requires.is_empty() && body.ensures.is_empty() && body.invariant.is_empty() {
                let prev_body = self.current_body.replace(body.clone());
                let result = self.eval_fn_body_value(&body);
                self.current_body = prev_body;
                result
            } else {
                let prev_body = self.current_body.replace(body.clone());
                let result = self.call_fn_with_contracts(&body, fn_name);
                self.current_body = prev_body;
                result
            };

        self.env.pop_scope();
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
                let mut val = self.resolve_path_value(env, body, idx, path)?;
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

                if args.iter().all(|arg| matches!(arg, CallArg::Positional(_)))
                    && let Expr::Path(path) = &body.exprs[callee_idx]
                    && path.is_single()
                    && self.local_access_for_expr(body, callee_idx).is_none()
                    && let Some(fn_idx) = self.resolve_direct_user_fn_idx(path.segments[0])
                {
                    let mut arg_vals = Args::with_capacity(args.len());
                    for arg in &args {
                        let CallArg::Positional(arg_idx) = arg else {
                            unreachable!("guard ensures all args are positional");
                        };
                        let value = eval_propagate!(self, env, body, *arg_idx);
                        arg_vals.push(value);
                    }
                    return self.call_fn(fn_idx, arg_vals).map(ControlFlow::Value);
                }

                // ── Module-qualified / static method / method call resolution ──
                if let Expr::Field { base, field } = &body.exprs[callee_idx] {
                    let base_idx = *base;
                    let field_name = *field;

                    // Nested module-qualified static method:
                    // collections.Deque.new()
                    if let Expr::Field {
                        base: module_base_idx,
                        field: type_name,
                    } = &body.exprs[base_idx]
                        && let Expr::Path(ref module_path) = body.exprs[*module_base_idx]
                        && module_path.is_single()
                    {
                        let module_name = module_path.segments[0];
                        if self.module_scope.imported_modules.contains(&module_name)
                            && let Some(&fn_idx) = self
                                .module_scope
                                .synthetic_module_static_methods
                                .get(&(module_name, *type_name, field_name))
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

                    // Before evaluating base as a value, check if it's a synthetic
                    // module or a type with static methods.
                    if let Expr::Path(ref path) = body.exprs[base_idx]
                        && path.is_single()
                    {
                        let seg = path.segments[0];

                        if self.module_scope.traits.contains_key(&seg) {
                            let source_order = self.args_in_source_order(&args);
                            let mut arg_values = Vec::with_capacity(source_order.len());
                            for idx in &source_order {
                                let v = eval_propagate!(self, env, body, *idx);
                                arg_values.push(v);
                            }
                            return self
                                .call_trait_qualified(seg, field_name, &args, arg_values)
                                .map(ControlFlow::Value);
                        }

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

                        // Type-owned static call: bare `Type.method()` if registered.
                        if let Some(owner_key) = self.static_owner_key_for_name(seg)
                            && let Some(&fn_idx) = self
                                .module_scope
                                .static_methods
                                .get(&(owner_key, field_name))
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

                    let base_has_field = matches!(
                        &base_val,
                        Value::Record { fields, .. }
                            if fields.iter().any(|(name, _)| *name == field_name)
                    );
                    let method_fn_idx = if base_has_field {
                        None
                    } else {
                        self.method_candidate_for_value(&base_val, field_name, args.len())
                    };

                    if let Some(fn_idx) = method_fn_idx {
                        if let Some(value) = self
                            .try_eval_lazy_mutable_map_get_or_insert_with_local(
                                env, body, &base_val, &args, fn_idx,
                            )?
                        {
                            return Ok(ControlFlow::Value(value));
                        }
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
                self.eval_block(env, body, &stmts, tail, false)
            }

            Expr::Return(val) => {
                let val = *val;
                let v = if let Some(idx) = val {
                    match self.eval_terminal_expr(env, body, idx)? {
                        ControlFlow::Value(v) | ControlFlow::Return(v) => v,
                        ControlFlow::Break => {
                            return Err(RuntimeError::TypeError(
                                "`break` used outside loop".into(),
                            ));
                        }
                        ControlFlow::Continue => {
                            return Err(RuntimeError::TypeError(
                                "`continue` used outside loop".into(),
                            ));
                        }
                    }
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
                let body_handle = self
                    .current_body
                    .as_ref()
                    .cloned()
                    .expect("lambda evaluation should have a current shared body handle");
                Ok(ControlFlow::Value(Value::Fn(Box::new(FnValue::Lambda {
                    params: param_pats,
                    body_expr: lambda_body_idx,
                    body: body_handle,
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
                let mut val = if let Some(access) = self.local_access_for_expr(body, idx) {
                    if self.consuming_local == Some(access) {
                        self.consuming_local = None;
                        self.env
                            .take_slot(access.depth, access.slot)
                            .ok_or_else(|| {
                                RuntimeError::TypeError(
                                    "internal runtime error: consumed local binding unavailable"
                                        .into(),
                                )
                            })?
                    } else {
                        self.env
                            .lookup_slot(access.depth, access.slot)
                            .cloned()
                            .ok_or_else(|| {
                                RuntimeError::TypeError(
                                    "internal runtime error: local binding unavailable".into(),
                                )
                            })?
                    }
                } else {
                    self.resolve_name(&self.env, name)?
                };
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

                if args.iter().all(|arg| matches!(arg, CallArg::Positional(_)))
                    && let Expr::Path(path) = &body.exprs[callee_idx]
                    && path.is_single()
                    && self.local_access_for_expr(body, callee_idx).is_none()
                    && let Some(fn_idx) = self.resolve_direct_user_fn_idx(path.segments[0])
                {
                    let mut arg_vals = Args::with_capacity(args.len());
                    for arg in args {
                        let CallArg::Positional(arg_idx) = arg else {
                            unreachable!("guard ensures all args are positional");
                        };
                        let value = eval_propagate_shared!(self, body, *arg_idx);
                        arg_vals.push(value);
                    }
                    return self.call_fn(fn_idx, arg_vals).map(ControlFlow::Value);
                }

                // ── Module-qualified / static method / method call resolution (shared path) ──
                if let Expr::Field { base, field } = &body.exprs[callee_idx] {
                    let base_idx = *base;
                    let field_name = *field;

                    // Nested module-qualified static method:
                    // collections.Deque.new()
                    if let Expr::Field {
                        base: module_base_idx,
                        field: type_name,
                    } = &body.exprs[base_idx]
                        && let Expr::Path(ref module_path) = body.exprs[*module_base_idx]
                        && module_path.is_single()
                    {
                        let module_name = module_path.segments[0];
                        if self.module_scope.imported_modules.contains(&module_name)
                            && let Some(&fn_idx) = self
                                .module_scope
                                .synthetic_module_static_methods
                                .get(&(module_name, *type_name, field_name))
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

                    // Before evaluating base as a value, check module/static dispatch.
                    if let Expr::Path(ref path) = body.exprs[base_idx]
                        && path.is_single()
                    {
                        let seg = path.segments[0];

                        if self.module_scope.traits.contains_key(&seg) {
                            let source_order = self.args_in_source_order(args);
                            let mut arg_values = Vec::with_capacity(source_order.len());
                            for idx in &source_order {
                                let v = eval_propagate_shared!(self, body, *idx);
                                arg_values.push(v);
                            }
                            return self
                                .call_trait_qualified(seg, field_name, args, arg_values)
                                .map(ControlFlow::Value);
                        }

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

                        // Type-owned static call: bare `Type.method()` if registered.
                        if let Some(owner_key) = self.static_owner_key_for_name(seg)
                            && let Some(&fn_idx) = self
                                .module_scope
                                .static_methods
                                .get(&(owner_key, field_name))
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

                    let base_has_field = matches!(
                        &base_val,
                        Value::Record { fields, .. }
                            if fields.iter().any(|(name, _)| *name == field_name)
                    );
                    let method_fn_idx = if base_has_field {
                        None
                    } else {
                        self.method_candidate_for_value(&base_val, field_name, args.len())
                    };

                    if let Some(fn_idx) = method_fn_idx {
                        if let Some(value) = self.try_eval_lazy_mutable_map_get_or_insert_with(
                            &base_val,
                            args,
                            fn_idx,
                            |this, idx| {
                                this.eval_terminal_expr_shared(body, idx)
                                    .map(ControlFlow::into_value)
                            },
                        )? {
                            return Ok(ControlFlow::Value(value));
                        }
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
                self.eval_block_shared(body, stmts, tail, false)
            }

            Expr::Return(val) => {
                let val = *val;
                let v = if let Some(idx) = val {
                    match self.eval_terminal_expr_shared(body, idx)? {
                        ControlFlow::Value(v) | ControlFlow::Return(v) => v,
                        ControlFlow::Break => {
                            return Err(RuntimeError::TypeError(
                                "`break` used outside loop".into(),
                            ));
                        }
                        ControlFlow::Continue => {
                            return Err(RuntimeError::TypeError(
                                "`continue` used outside loop".into(),
                            ));
                        }
                    }
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
                let body_handle = self
                    .current_body
                    .as_ref()
                    .cloned()
                    .expect("lambda evaluation should have a current shared body handle");
                Ok(ControlFlow::Value(Value::Fn(Box::new(FnValue::Lambda {
                    params: param_pats,
                    body_expr: lambda_body_idx,
                    body: body_handle,
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
    fn resolve_path_value(
        &mut self,
        env: &mut Env,
        body: &Body,
        expr_idx: ExprIdx,
        path: &kyokara_hir_def::path::Path,
    ) -> Result<Value, RuntimeError> {
        let name = path.segments[0];
        if let Some(access) = self.local_access_for_expr(body, expr_idx) {
            if self.consuming_local == Some(access) {
                self.consuming_local = None;
                return env.take_slot(access.depth, access.slot).ok_or_else(|| {
                    RuntimeError::TypeError(
                        "internal runtime error: consumed local binding unavailable".into(),
                    )
                });
            }
            return env
                .lookup_slot(access.depth, access.slot)
                .cloned()
                .ok_or_else(|| {
                    RuntimeError::TypeError(
                        "internal runtime error: local binding unavailable".into(),
                    )
                });
        }
        self.resolve_name(env, name)
    }

    #[inline(always)]
    fn resolve_name(&self, env: &Env, name: Name) -> Result<Value, RuntimeError> {
        // 1. Function-local module immutable lets (project mode).
        if let Some(cur_fn) = self.current_fn
            && let Some(overrides) = self.let_scope_overrides.get(&cur_fn)
            && let Some(value) = overrides.get(&name)
        {
            return Ok(value.clone());
        }

        // 2. Local variables and entry-module top-level lets.
        if let Some(val) = env.lookup(name) {
            return Ok(val.clone());
        }

        // 3. Function-local module overrides (project mode): resolve names in the
        // source module of the currently executing function before global scope.
        if let Some(cur_fn) = self.current_fn
            && let Some(overrides) = self.fn_scope_overrides.get(&cur_fn)
            && let Some(&fn_idx) = overrides.get(&name)
        {
            return Ok(Value::Fn(Box::new(FnValue::User(fn_idx))));
        }

        // 4. Module-level user functions (with bodies).
        //    Intrinsic stubs are no longer in scope.functions — they're only
        //    reachable via methods, modules, or static methods.
        if let Some(&fn_idx) = self.module_scope.functions.get(&name)
            && self.fn_bodies.contains_key(&fn_idx)
        {
            return Ok(Value::Fn(Box::new(FnValue::User(fn_idx))));
        }

        // 5. ADT constructors.
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
            // Half-open ascending integer range source.
            (BinaryOp::RangeUntil, Value::Int(start), Value::Int(end)) => {
                Ok(Value::seq_source(SeqSource::Range {
                    start: *start,
                    end: *end,
                }))
            }
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
                    let prev_body = self.current_body.replace(body.clone());
                    let result = (|| -> Result<Value, RuntimeError> {
                        for (pat_idx, val) in params.iter().zip(args) {
                            self.bind_pat(&body, *pat_idx, &val, &mut env)?;
                        }
                        let result = self.eval_terminal_expr(&mut env, &body, body_expr)?;
                        Ok(result.into_value())
                    })();
                    self.current_body = prev_body;
                    result
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

        if fn_item.with_effects.is_empty() {
            return Ok(());
        }

        for cap_ref in &fn_item.with_effects {
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

    fn make_some(&self, val: Value) -> Result<Value, RuntimeError> {
        let Some((type_idx, variant)) = self.option_some else {
            return Err(RuntimeError::TypeError(
                "core constructor Option::Some is unavailable".into(),
            ));
        };
        Ok(Value::Adt {
            type_idx,
            variant,
            fields: vec![val],
        })
    }

    fn make_none(&self) -> Result<Value, RuntimeError> {
        let Some((type_idx, variant)) = self.option_none else {
            return Err(RuntimeError::TypeError(
                "core constructor Option::None is unavailable".into(),
            ));
        };
        Ok(Value::Adt {
            type_idx,
            variant,
            fields: vec![],
        })
    }

    fn make_ok(&self, val: Value) -> Result<Value, RuntimeError> {
        let Some((type_idx, variant)) = self.result_ok else {
            return Err(RuntimeError::TypeError(
                "core constructor Result::Ok is unavailable".into(),
            ));
        };
        Ok(Value::Adt {
            type_idx,
            variant,
            fields: vec![val],
        })
    }

    fn make_err(&self, val: Value) -> Result<Value, RuntimeError> {
        let Some((type_idx, variant)) = self.result_err else {
            return Err(RuntimeError::TypeError(
                "core constructor Result::Err is unavailable".into(),
            ));
        };
        Ok(Value::Adt {
            type_idx,
            variant,
            fields: vec![val],
        })
    }

    /// Decode an `Option` value into `Some(payload)` for `Some`, `None` for `None`.
    ///
    /// Resolve by core type identity so user shadowing cannot reinterpret ADTs.
    fn decode_option_some_payload(
        &self,
        value: &Value,
        intrinsic_name: &str,
    ) -> Result<Option<Value>, RuntimeError> {
        let Value::Adt {
            type_idx,
            variant,
            fields,
        } = value
        else {
            return Err(RuntimeError::TypeError(format!(
                "{intrinsic_name} expects an Option value"
            )));
        };

        let Some(option_type_idx) = self
            .module_scope
            .core_types
            .get(CoreType::Option)
            .map(|i| i.type_idx)
        else {
            return Err(RuntimeError::TypeError(
                "core type Option is unavailable".into(),
            ));
        };
        if *type_idx != option_type_idx {
            return Err(RuntimeError::TypeError(format!(
                "{intrinsic_name} expects an Option value"
            )));
        }

        let TypeDefKind::Adt { variants } = &self.item_tree.types[*type_idx].kind else {
            return Err(RuntimeError::TypeError(format!(
                "{intrinsic_name} expects an Option ADT"
            )));
        };
        if variants.get(*variant).is_none() {
            return Err(RuntimeError::TypeError(format!(
                "{intrinsic_name} received invalid Option variant index"
            )));
        }

        let Some((_, some_variant)) = self.option_some else {
            return Err(RuntimeError::TypeError(
                "core constructor Option::Some is unavailable".into(),
            ));
        };
        let Some((_, none_variant)) = self.option_none else {
            return Err(RuntimeError::TypeError(
                "core constructor Option::None is unavailable".into(),
            ));
        };

        if *variant == some_variant {
            if fields.len() != 1 {
                return Err(RuntimeError::TypeError(format!(
                    "{intrinsic_name} expects Option::Some to carry one value"
                )));
            }
            Ok(Some(fields[0].clone()))
        } else if *variant == none_variant {
            if !fields.is_empty() {
                return Err(RuntimeError::TypeError(format!(
                    "{intrinsic_name} expects Option::None to carry no values"
                )));
            }
            Ok(None)
        } else {
            Err(RuntimeError::TypeError(format!(
                "{intrinsic_name} expects Option::Some/None variants"
            )))
        }
    }

    fn decode_unfold_step_payload(
        &mut self,
        payload: Value,
    ) -> Result<(Value, Value), RuntimeError> {
        let Value::Record { fields, .. } = payload else {
            return Err(RuntimeError::TypeError(
                "seq_unfold: step must return Option<{ value: T, state: S }>".into(),
            ));
        };

        let value_name = Name::new(&mut self.interner, "value");
        let state_name = Name::new(&mut self.interner, "state");

        let mut value: Option<Value> = None;
        let mut state: Option<Value> = None;
        for (field_name, field_value) in fields {
            if field_name == value_name {
                if value.is_some() {
                    return Err(RuntimeError::TypeError(
                        "seq_unfold: step must return Option<{ value: T, state: S }>".into(),
                    ));
                }
                value = Some(field_value);
            } else if field_name == state_name {
                if state.is_some() {
                    return Err(RuntimeError::TypeError(
                        "seq_unfold: step must return Option<{ value: T, state: S }>".into(),
                    ));
                }
                state = Some(field_value);
            } else {
                return Err(RuntimeError::TypeError(
                    "seq_unfold: step must return Option<{ value: T, state: S }>".into(),
                ));
            }
        }

        match (value, state) {
            (Some(value), Some(state)) => Ok((value, state)),
            _ => Err(RuntimeError::TypeError(
                "seq_unfold: step must return Option<{ value: T, state: S }>".into(),
            )),
        }
    }

    /// Decode a `Result` value into `(is_ok, payload)`.
    ///
    /// Resolve by core type identity so user shadowing cannot reinterpret ADTs.
    fn decode_result_payload(
        &self,
        value: &Value,
        intrinsic_name: &str,
    ) -> Result<(bool, Value), RuntimeError> {
        let Value::Adt {
            type_idx,
            variant,
            fields,
        } = value
        else {
            return Err(RuntimeError::TypeError(format!(
                "{intrinsic_name} expects a Result value"
            )));
        };

        let Some(result_type_idx) = self
            .module_scope
            .core_types
            .get(CoreType::Result)
            .map(|i| i.type_idx)
        else {
            return Err(RuntimeError::TypeError(
                "core type Result is unavailable".into(),
            ));
        };
        if *type_idx != result_type_idx {
            return Err(RuntimeError::TypeError(format!(
                "{intrinsic_name} expects a Result value"
            )));
        }

        let TypeDefKind::Adt { variants } = &self.item_tree.types[*type_idx].kind else {
            return Err(RuntimeError::TypeError(format!(
                "{intrinsic_name} expects a Result ADT"
            )));
        };
        if variants.get(*variant).is_none() {
            return Err(RuntimeError::TypeError(format!(
                "{intrinsic_name} received invalid Result variant index"
            )));
        }

        let Some((_, ok_variant)) = self.result_ok else {
            return Err(RuntimeError::TypeError(
                "core constructor Result::Ok is unavailable".into(),
            ));
        };
        let Some((_, err_variant)) = self.result_err else {
            return Err(RuntimeError::TypeError(
                "core constructor Result::Err is unavailable".into(),
            ));
        };

        if fields.len() != 1 {
            return Err(RuntimeError::TypeError(format!(
                "{intrinsic_name} expects Result::Ok/Err to carry one value"
            )));
        }

        if *variant == ok_variant {
            Ok((true, fields[0].clone()))
        } else if *variant == err_variant {
            Ok((false, fields[0].clone()))
        } else {
            Err(RuntimeError::TypeError(format!(
                "{intrinsic_name} expects Result::Ok/Err variants"
            )))
        }
    }

    /// Decode a `Result` value into `Some(ok_payload)` for `Ok`, `None` for `Err`.
    ///
    /// Resolve by core type identity so user shadowing cannot reinterpret ADTs.
    fn decode_result_ok_payload(
        &self,
        value: &Value,
        intrinsic_name: &str,
    ) -> Result<Option<Value>, RuntimeError> {
        let (is_ok, payload) = self.decode_result_payload(value, intrinsic_name)?;
        if is_ok { Ok(Some(payload)) } else { Ok(None) }
    }

    fn make_invalid_int(&self, msg: String) -> Result<Value, RuntimeError> {
        let Some((type_idx, variant)) = self.parse_error_invalid_int else {
            return Err(RuntimeError::TypeError(
                "parse_int cannot construct ParseError::InvalidInt(String)".into(),
            ));
        };
        Ok(Value::Adt {
            type_idx,
            variant,
            fields: vec![Value::String(msg)],
        })
    }

    fn make_invalid_float(&self, msg: String) -> Result<Value, RuntimeError> {
        let Some((type_idx, variant)) = self.parse_error_invalid_float else {
            return Err(RuntimeError::TypeError(
                "parse_float cannot construct ParseError::InvalidFloat(String)".into(),
            ));
        };
        Ok(Value::Adt {
            type_idx,
            variant,
            fields: vec![Value::String(msg)],
        })
    }

    fn require_traversal_plan(
        &self,
        value: &Value,
        intrinsic_name: &str,
    ) -> Result<Rc<SeqPlan>, RuntimeError> {
        match value {
            Value::Seq(plan) => Ok(plan.clone()),
            Value::List(xs) => Ok(Rc::new(SeqPlan::Source(SeqSource::FromList(xs.clone())))),
            Value::MutableList(xs) => {
                Ok(Rc::new(SeqPlan::Source(SeqSource::FromList(xs.snapshot()))))
            }
            Value::Deque(xs) => Ok(Rc::new(SeqPlan::Source(SeqSource::FromDeque(xs.clone())))),
            _ => Err(RuntimeError::TypeError(format!(
                "{intrinsic_name} expects a traversal source"
            ))),
        }
    }

    fn seq_iter_from_plan(&mut self, plan: &SeqPlan) -> Result<SeqIterState, RuntimeError> {
        match plan {
            SeqPlan::Source(source) => {
                let state = match source {
                    SeqSource::Range { start, end } => SeqSourceIter::Range {
                        current: *start,
                        end: *end,
                    },
                    SeqSource::FromList(xs) => SeqSourceIter::FromList {
                        items: xs.clone(),
                        idx: 0,
                    },
                    SeqSource::BitSetValues(bitset) => SeqSourceIter::BitSetValues {
                        bitset: bitset.clone(),
                        word_idx: 0,
                        pending_word: bitset.words().first().copied().unwrap_or(0),
                    },
                    SeqSource::FromDeque(xs) => SeqSourceIter::FromDeque {
                        items: xs.clone(),
                        idx: 0,
                    },
                    SeqSource::StringSplit { s, delim } => SeqSourceIter::StringSplit {
                        s: s.clone(),
                        delim: delim.clone(),
                        next_start: 0,
                        emitted_empty_leading: false,
                        emitted_empty_trailing: false,
                    },
                    SeqSource::StringLines { s } => SeqSourceIter::StringLines {
                        s: s.clone(),
                        next_start: 0,
                        finished: false,
                    },
                    SeqSource::StringChars { s } => SeqSourceIter::StringChars {
                        s: s.clone(),
                        next_start: 0,
                    },
                    SeqSource::MapKeys(entries) => SeqSourceIter::MapKeys {
                        entries: entries.clone(),
                        idx: 0,
                    },
                    SeqSource::MapValues(entries) => SeqSourceIter::MapValues {
                        entries: entries.clone(),
                        idx: 0,
                    },
                    SeqSource::SetValues(entries) => SeqSourceIter::SetValues {
                        entries: entries.clone(),
                        idx: 0,
                    },
                };
                Ok(SeqIterState::Source(state))
            }
            SeqPlan::Map { input, f } => Ok(SeqIterState::Map {
                input: Box::new(self.seq_iter_from_plan(input)?),
                f: f.clone(),
            }),
            SeqPlan::Filter { input, f } => Ok(SeqIterState::Filter {
                input: Box::new(self.seq_iter_from_plan(input)?),
                f: f.clone(),
            }),
            SeqPlan::Scan { input, init, f } => Ok(SeqIterState::Scan {
                input: Box::new(self.seq_iter_from_plan(input)?),
                acc: init.clone(),
                f: f.clone(),
                emitted_init: false,
            }),
            SeqPlan::Unfold { seed, step } => Ok(SeqIterState::Unfold {
                state: Some(seed.clone()),
                step: step.clone(),
            }),
            SeqPlan::Enumerate { input } => Ok(SeqIterState::Enumerate {
                input: Box::new(self.seq_iter_from_plan(input)?),
                idx: 0,
                index_name: Name::new(&mut self.interner, "index"),
                value_name: Name::new(&mut self.interner, "value"),
            }),
            SeqPlan::Zip { left, right } => Ok(SeqIterState::Zip {
                left: Box::new(self.seq_iter_from_plan(left)?),
                right: Box::new(self.seq_iter_from_plan(right)?),
                left_name: Name::new(&mut self.interner, "left"),
                right_name: Name::new(&mut self.interner, "right"),
            }),
            SeqPlan::Chunks { input, n } => {
                let chunk_size = usize::try_from(*n).ok().filter(|&size| size > 0).ok_or(
                    RuntimeError::TypeError("seq_chunks: chunk size must be > 0".into()),
                )?;
                Ok(SeqIterState::Chunks {
                    input: Box::new(self.seq_iter_from_plan(input)?),
                    n: chunk_size,
                })
            }
            SeqPlan::Windows { input, n } => {
                let window_size = usize::try_from(*n).ok().filter(|&size| size > 0).ok_or(
                    RuntimeError::TypeError("seq_windows: window size must be > 0".into()),
                )?;
                Ok(SeqIterState::Windows {
                    input: Box::new(self.seq_iter_from_plan(input)?),
                    n: window_size,
                    window: VecDeque::with_capacity(window_size),
                    primed: false,
                })
            }
        }
    }

    fn next_string_split_part(
        s: &str,
        delim: &str,
        next_start: &mut usize,
        emitted_empty_leading: &mut bool,
        emitted_empty_trailing: &mut bool,
    ) -> Option<String> {
        if delim.is_empty() {
            if !*emitted_empty_leading {
                *emitted_empty_leading = true;
                return Some(String::new());
            }
            if *next_start < s.len() {
                let ch = s[*next_start..].chars().next()?;
                *next_start += ch.len_utf8();
                return Some(ch.to_string());
            }
            if !*emitted_empty_trailing {
                *emitted_empty_trailing = true;
                return Some(String::new());
            }
            return None;
        }

        if *next_start > s.len() {
            return None;
        }

        let rest = &s[*next_start..];
        if let Some(rel) = rest.find(delim) {
            let end = *next_start + rel;
            let out = s[*next_start..end].to_string();
            *next_start = end + delim.len();
            Some(out)
        } else {
            let out = s[*next_start..].to_string();
            *next_start = s.len() + 1;
            Some(out)
        }
    }

    fn next_string_line(s: &str, next_start: &mut usize, finished: &mut bool) -> Option<String> {
        if *finished {
            return None;
        }
        if *next_start >= s.len() {
            *finished = true;
            return None;
        }

        let rest = &s[*next_start..];
        if let Some(rel) = rest.find('\n') {
            let line_end = *next_start + rel;
            let trimmed_end = if line_end > *next_start && s.as_bytes()[line_end - 1] == b'\r' {
                line_end - 1
            } else {
                line_end
            };
            let out = s[*next_start..trimmed_end].to_string();
            *next_start = line_end + 1;
            if *next_start >= s.len() {
                *finished = true;
            }
            Some(out)
        } else {
            *finished = true;
            Some(s[*next_start..].to_string())
        }
    }

    fn next_string_char(s: &str, next_start: &mut usize) -> Option<char> {
        if *next_start >= s.len() {
            return None;
        }
        let ch = s[*next_start..].chars().next()?;
        *next_start += ch.len_utf8();
        Some(ch)
    }

    fn seq_source_iter_next(source: &mut SeqSourceIter) -> Option<Value> {
        match source {
            SeqSourceIter::Range { current, end } => {
                if current >= end {
                    None
                } else {
                    let out = Value::Int(*current);
                    *current += 1;
                    Some(out)
                }
            }
            SeqSourceIter::FromList { items, idx } => {
                let out = items.get(*idx).cloned();
                if out.is_some() {
                    *idx += 1;
                }
                out
            }
            SeqSourceIter::BitSetValues {
                bitset,
                word_idx,
                pending_word,
            } => loop {
                let words = bitset.words();
                if *word_idx >= words.len() {
                    return None;
                }
                if *pending_word == 0 {
                    *word_idx += 1;
                    *pending_word = words.get(*word_idx).copied().unwrap_or(0);
                    continue;
                }
                let bit = pending_word.trailing_zeros() as usize;
                *pending_word &= *pending_word - 1;
                let idx = *word_idx * 64 + bit;
                if idx < bitset.size_bits() {
                    return Some(Value::Int(idx as i64));
                }
            },
            SeqSourceIter::FromDeque { items, idx } => {
                let out = items.get(*idx).cloned();
                if out.is_some() {
                    *idx += 1;
                }
                out
            }
            SeqSourceIter::StringSplit {
                s,
                delim,
                next_start,
                emitted_empty_leading,
                emitted_empty_trailing,
            } => Self::next_string_split_part(
                s.as_str(),
                delim.as_str(),
                next_start,
                emitted_empty_leading,
                emitted_empty_trailing,
            )
            .map(Value::String),
            SeqSourceIter::StringLines {
                s,
                next_start,
                finished,
            } => Self::next_string_line(s.as_str(), next_start, finished).map(Value::String),
            SeqSourceIter::StringChars { s, next_start } => {
                Self::next_string_char(s.as_str(), next_start).map(Value::Char)
            }
            SeqSourceIter::MapKeys { entries, idx } => {
                let out = entries.key_at(*idx);
                if out.is_some() {
                    *idx += 1;
                }
                out
            }
            SeqSourceIter::MapValues { entries, idx } => {
                let out = entries.value_at(*idx);
                if out.is_some() {
                    *idx += 1;
                }
                out
            }
            SeqSourceIter::SetValues { entries, idx } => {
                let out = entries.value_at(*idx);
                if out.is_some() {
                    *idx += 1;
                }
                out
            }
        }
    }

    fn seq_iter_next(&mut self, state: &mut SeqIterState) -> Result<Option<Value>, RuntimeError> {
        match state {
            SeqIterState::Source(source) => Ok(Self::seq_source_iter_next(source)),
            SeqIterState::Map { input, f } => {
                let Some(item) = self.seq_iter_next(input)? else {
                    return Ok(None);
                };
                let mapped = self.call_value(f.clone(), smallvec::smallvec![item])?;
                Ok(Some(mapped))
            }
            SeqIterState::Filter { input, f } => loop {
                let Some(item) = self.seq_iter_next(input)? else {
                    return Ok(None);
                };
                let keep = self.call_value(f.clone(), smallvec::smallvec![item.clone()])?;
                match keep {
                    Value::Bool(true) => return Ok(Some(item)),
                    Value::Bool(false) => {}
                    _ => {
                        return Err(RuntimeError::TypeError(
                            "seq_filter: predicate must return Bool".into(),
                        ));
                    }
                }
            },
            SeqIterState::Scan {
                input,
                acc,
                f,
                emitted_init,
            } => {
                if !*emitted_init {
                    *emitted_init = true;
                    return Ok(Some(acc.clone()));
                }

                let Some(item) = self.seq_iter_next(input)? else {
                    return Ok(None);
                };

                let next_acc =
                    self.call_value(f.clone(), smallvec::smallvec![acc.clone(), item])?;
                *acc = next_acc.clone();
                Ok(Some(next_acc))
            }
            SeqIterState::Unfold { state, step } => {
                let Some(current_state) = state.take() else {
                    return Ok(None);
                };

                let step_out = self.call_value(step.clone(), smallvec::smallvec![current_state])?;
                match self.decode_option_some_payload(&step_out, "seq_unfold")? {
                    Some(payload) => {
                        let (value, next_state) = self.decode_unfold_step_payload(payload)?;
                        *state = Some(next_state);
                        Ok(Some(value))
                    }
                    None => Ok(None),
                }
            }
            SeqIterState::Enumerate {
                input,
                idx,
                index_name,
                value_name,
            } => {
                let Some(item) = self.seq_iter_next(input)? else {
                    return Ok(None);
                };
                let current = *idx;
                *idx = idx.checked_add(1).ok_or(RuntimeError::IntegerOverflow)?;
                Ok(Some(Value::Record {
                    fields: vec![(*index_name, Value::Int(current)), (*value_name, item)],
                    type_idx: None,
                }))
            }
            SeqIterState::Zip {
                left,
                right,
                left_name,
                right_name,
            } => {
                let Some(l) = self.seq_iter_next(left)? else {
                    return Ok(None);
                };
                let Some(r) = self.seq_iter_next(right)? else {
                    return Ok(None);
                };
                Ok(Some(Value::Record {
                    fields: vec![(*left_name, l), (*right_name, r)],
                    type_idx: None,
                }))
            }
            SeqIterState::Chunks { input, n } => {
                let mut chunk = Vec::with_capacity(*n);
                for _ in 0..*n {
                    let Some(item) = self.seq_iter_next(input)? else {
                        break;
                    };
                    chunk.push(item);
                }
                if chunk.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(Value::list(chunk)))
                }
            }
            SeqIterState::Windows {
                input,
                n,
                window,
                primed,
            } => {
                if !*primed {
                    while window.len() < *n {
                        let Some(item) = self.seq_iter_next(input)? else {
                            return Ok(None);
                        };
                        window.push_back(item);
                    }
                    *primed = true;
                    return Ok(Some(Value::list(window.iter().cloned().collect())));
                }

                let Some(next_item) = self.seq_iter_next(input)? else {
                    return Ok(None);
                };
                window.pop_front();
                window.push_back(next_item);
                Ok(Some(Value::list(window.iter().cloned().collect())))
            }
        }
    }

    fn seq_for_each_control(
        &mut self,
        plan: &SeqPlan,
        emit: &mut dyn FnMut(&mut Self, Value) -> Result<SeqEmitControl, RuntimeError>,
    ) -> Result<(), RuntimeError> {
        use SeqEmitControl::{Break, Continue};

        match plan {
            SeqPlan::Source(source) => match source {
                SeqSource::Range { start, end } => {
                    if start >= end {
                        return Ok(());
                    }
                    for n in *start..*end {
                        match emit(self, Value::Int(n))? {
                            Continue => {}
                            Break => return Ok(()),
                        }
                    }
                    Ok(())
                }
                SeqSource::FromList(xs) => {
                    for item in xs.iter().cloned() {
                        match emit(self, item)? {
                            Continue => {}
                            Break => return Ok(()),
                        }
                    }
                    Ok(())
                }
                SeqSource::FromDeque(xs) => {
                    for item in xs.iter().cloned() {
                        match emit(self, item)? {
                            Continue => {}
                            Break => return Ok(()),
                        }
                    }
                    Ok(())
                }
                SeqSource::BitSetValues(bitset) => {
                    let words = bitset.words();
                    for (word_idx, word) in words.iter().enumerate() {
                        let mut remaining = *word;
                        while remaining != 0 {
                            let bit = remaining.trailing_zeros() as usize;
                            remaining &= remaining - 1;
                            let idx = word_idx * 64 + bit;
                            if idx >= bitset.size_bits() {
                                continue;
                            }
                            match emit(self, Value::Int(idx as i64))? {
                                Continue => {}
                                Break => return Ok(()),
                            }
                        }
                    }
                    Ok(())
                }
                SeqSource::StringSplit { s, delim } => {
                    for part in s.split(delim.as_str()) {
                        match emit(self, Value::String(part.to_string()))? {
                            Continue => {}
                            Break => return Ok(()),
                        }
                    }
                    Ok(())
                }
                SeqSource::StringLines { s } => {
                    for line in s.lines() {
                        match emit(self, Value::String(line.to_string()))? {
                            Continue => {}
                            Break => return Ok(()),
                        }
                    }
                    Ok(())
                }
                SeqSource::StringChars { s } => {
                    for ch in s.chars() {
                        match emit(self, Value::Char(ch))? {
                            Continue => {}
                            Break => return Ok(()),
                        }
                    }
                    Ok(())
                }
                SeqSource::MapKeys(entries) => {
                    for entry in entries.entries() {
                        match emit(self, entry.key.clone())? {
                            Continue => {}
                            Break => return Ok(()),
                        }
                    }
                    Ok(())
                }
                SeqSource::MapValues(entries) => {
                    for entry in entries.entries() {
                        match emit(self, entry.value.clone())? {
                            Continue => {}
                            Break => return Ok(()),
                        }
                    }
                    Ok(())
                }
                SeqSource::SetValues(entries) => {
                    for entry in entries.entries() {
                        match emit(self, entry.value.clone())? {
                            Continue => {}
                            Break => return Ok(()),
                        }
                    }
                    Ok(())
                }
            },
            SeqPlan::Map { input, f } => {
                let mapper = f.clone();
                self.seq_for_each_control(input, &mut |interp, item| {
                    let mapped = interp.call_value(mapper.clone(), smallvec::smallvec![item])?;
                    emit(interp, mapped)
                })
            }
            SeqPlan::Filter { input, f } => {
                let predicate = f.clone();
                self.seq_for_each_control(input, &mut |interp, item| {
                    let keep =
                        interp.call_value(predicate.clone(), smallvec::smallvec![item.clone()])?;
                    match keep {
                        Value::Bool(true) => emit(interp, item),
                        Value::Bool(false) => Ok(Continue),
                        _ => Err(RuntimeError::TypeError(
                            "seq_filter: predicate must return Bool".into(),
                        )),
                    }
                })
            }
            SeqPlan::Scan { input, init, f } => {
                let mut acc = init.clone();
                match emit(self, acc.clone())? {
                    Continue => {}
                    Break => return Ok(()),
                }

                let folder = f.clone();
                self.seq_for_each_control(input, &mut |interp, item| {
                    acc = interp
                        .call_value(folder.clone(), smallvec::smallvec![acc.clone(), item])?;
                    emit(interp, acc.clone())
                })
            }
            SeqPlan::Unfold { seed, step } => {
                let mut state = Some(seed.clone());
                let step_fn = step.clone();
                loop {
                    let Some(current_state) = state.take() else {
                        return Ok(());
                    };

                    let step_out =
                        self.call_value(step_fn.clone(), smallvec::smallvec![current_state])?;
                    let maybe_payload = self.decode_option_some_payload(&step_out, "seq_unfold")?;
                    let Some(payload) = maybe_payload else {
                        return Ok(());
                    };
                    let (value, next_state) = self.decode_unfold_step_payload(payload)?;
                    state = Some(next_state);
                    match emit(self, value)? {
                        Continue => {}
                        Break => return Ok(()),
                    }
                }
            }
            SeqPlan::Enumerate { input } => {
                let index_name = Name::new(&mut self.interner, "index");
                let value_name = Name::new(&mut self.interner, "value");
                let mut idx: i64 = 0;
                self.seq_for_each_control(input, &mut |interp, item| {
                    let current = idx;
                    idx = idx.checked_add(1).ok_or(RuntimeError::IntegerOverflow)?;
                    emit(
                        interp,
                        Value::Record {
                            fields: vec![(index_name, Value::Int(current)), (value_name, item)],
                            type_idx: None,
                        },
                    )
                })
            }
            SeqPlan::Zip { left, right } => {
                let left_name = Name::new(&mut self.interner, "left");
                let right_name = Name::new(&mut self.interner, "right");
                let mut left_state = self.seq_iter_from_plan(left)?;
                let mut right_state = self.seq_iter_from_plan(right)?;

                loop {
                    let Some(l) = self.seq_iter_next(&mut left_state)? else {
                        return Ok(());
                    };
                    let Some(r) = self.seq_iter_next(&mut right_state)? else {
                        return Ok(());
                    };
                    match emit(
                        self,
                        Value::Record {
                            fields: vec![(left_name, l), (right_name, r)],
                            type_idx: None,
                        },
                    )? {
                        Continue => {}
                        Break => return Ok(()),
                    }
                }
            }
            SeqPlan::Chunks { input, n } => {
                if *n <= 0 {
                    return Err(RuntimeError::TypeError(
                        "seq_chunks: chunk size must be > 0".into(),
                    ));
                }
                let chunk_size = *n as usize;
                let mut chunk = Vec::with_capacity(chunk_size);
                let mut broke = false;
                self.seq_for_each_control(input, &mut |interp, item| {
                    chunk.push(item);
                    if chunk.len() == chunk_size {
                        let out = std::mem::take(&mut chunk);
                        match emit(interp, Value::list(out))? {
                            Continue => Ok(Continue),
                            Break => {
                                broke = true;
                                Ok(Break)
                            }
                        }
                    } else {
                        Ok(Continue)
                    }
                })?;
                if !broke && !chunk.is_empty() {
                    match emit(self, Value::list(chunk))? {
                        Continue | Break => {}
                    }
                }
                Ok(())
            }
            SeqPlan::Windows { input, n } => {
                if *n <= 0 {
                    return Err(RuntimeError::TypeError(
                        "seq_windows: window size must be > 0".into(),
                    ));
                }
                let window_size = *n as usize;
                let mut window: VecDeque<Value> = VecDeque::with_capacity(window_size);
                self.seq_for_each_control(input, &mut |interp, item| {
                    window.push_back(item);
                    if window.len() < window_size {
                        return Ok(Continue);
                    }

                    let out = window.iter().cloned().collect::<Vec<_>>();
                    match emit(interp, Value::list(out))? {
                        Continue => {
                            window.pop_front();
                            Ok(Continue)
                        }
                        Break => Ok(Break),
                    }
                })
            }
        }
    }

    fn seq_for_each(
        &mut self,
        plan: &SeqPlan,
        emit: &mut dyn FnMut(&mut Self, Value) -> Result<(), RuntimeError>,
    ) -> Result<(), RuntimeError> {
        self.seq_for_each_control(plan, &mut |interp, item| {
            emit(interp, item)?;
            Ok(SeqEmitControl::Continue)
        })
    }

    fn eval_seq_to_vec(&mut self, plan: &SeqPlan) -> Result<Vec<Value>, RuntimeError> {
        let mut out = Vec::new();
        self.seq_for_each(plan, &mut |_interp, item| {
            out.push(item);
            Ok(())
        })?;
        Ok(out)
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
                    self.make_some(val.clone())
                } else {
                    self.make_none()
                }
            }
            IntrinsicFn::MutableListGet => {
                let Value::MutableList(xs) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_list_get expects a MutableList".into(),
                    ));
                };
                let Value::Int(i) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_list_get expects an Int index".into(),
                    ));
                };
                let idx = *i as usize;
                if let Some(val) = xs.get_cloned(idx) {
                    self.make_some(val)
                } else {
                    self.make_none()
                }
            }
            IntrinsicFn::MutableListLast => {
                let Value::MutableList(xs) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_list_last expects a MutableList".into(),
                    ));
                };
                if let Some(val) = xs.last_cloned() {
                    self.make_some(val)
                } else {
                    self.make_none()
                }
            }
            IntrinsicFn::MutableListPop => {
                let Value::MutableList(xs) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_list_pop expects a MutableList".into(),
                    ));
                };
                if let Some(val) = xs.pop() {
                    self.make_some(val)
                } else {
                    self.make_none()
                }
            }
            IntrinsicFn::ListHead => {
                let Value::List(xs) = &args[0] else {
                    return Err(RuntimeError::TypeError("list_head expects a List".into()));
                };
                if let Some(val) = xs.first() {
                    self.make_some(val.clone())
                } else {
                    self.make_none()
                }
            }
            IntrinsicFn::ListUpdate => {
                let mut args = args.into_iter();
                let Value::List(mut xs) = args.next().ok_or(RuntimeError::TypeError(
                    "list_update: missing list argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError("list_update expects a List".into()));
                };
                let Value::Int(i) = args.next().ok_or(RuntimeError::TypeError(
                    "list_update: missing index argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError(
                        "list_update expects an Int index".into(),
                    ));
                };
                if i < 0 || i as usize >= xs.len() {
                    return Err(RuntimeError::IndexOutOfBounds {
                        index: i,
                        len: xs.len() as i64,
                    });
                }
                let idx = i as usize;
                let updater = args.next().ok_or(RuntimeError::TypeError(
                    "list_update: missing updater argument".into(),
                ))?;
                let updated = self.call_value(updater, smallvec::smallvec![xs[idx].clone()])?;
                if updated == xs[idx] {
                    return Ok(Value::List(xs));
                }
                Rc::make_mut(&mut xs)[idx] = updated;
                Ok(Value::List(xs))
            }
            IntrinsicFn::MutableListUpdate => {
                let Value::MutableList(xs) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_list_update expects a MutableList".into(),
                    ));
                };
                let Value::Int(i) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_list_update expects an Int index".into(),
                    ));
                };
                let len = xs.len();
                if *i < 0 || *i as usize >= len {
                    return Err(RuntimeError::IndexOutOfBounds {
                        index: *i,
                        len: len as i64,
                    });
                }
                let idx = *i as usize;
                let updater = args[2].clone();
                let current = xs
                    .get_cloned(idx)
                    .expect("bounds checked mutable list element should exist");
                let updated = self.call_value(updater, smallvec::smallvec![current])?;
                xs.set(idx, updated);
                Ok(Value::MutableList(xs.clone()))
            }
            IntrinsicFn::MutablePriorityQueuePush => {
                let Value::MutablePriorityQueue(queue) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_priority_queue_push expects a MutablePriorityQueue".into(),
                    ));
                };
                let priority = args[1].clone();
                let value = args[2].clone();
                let mut queue = queue.borrow_mut();
                let seq = queue.next_seq;
                queue.next_seq += 1;
                queue.entries.push(PriorityQueueEntry {
                    priority,
                    value,
                    seq,
                });
                let idx = queue.entries.len() - 1;
                self.priority_queue_bubble_up(&mut queue, idx)?;
                Ok(args[0].clone())
            }
            IntrinsicFn::MutablePriorityQueuePeek => {
                let Value::MutablePriorityQueue(queue) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_priority_queue_peek expects a MutablePriorityQueue".into(),
                    ));
                };
                let Some(entry) = queue.borrow().entries.first().cloned() else {
                    return self.make_none();
                };
                let payload = self.priority_queue_make_payload(entry);
                self.make_some(payload)
            }
            IntrinsicFn::MutablePriorityQueuePop => {
                let Value::MutablePriorityQueue(queue) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_priority_queue_pop expects a MutablePriorityQueue".into(),
                    ));
                };
                let mut queue = queue.borrow_mut();
                let Some(last) = queue.entries.pop() else {
                    return self.make_none();
                };
                let entry = if queue.entries.is_empty() {
                    last
                } else {
                    let root = std::mem::replace(&mut queue.entries[0], last);
                    self.priority_queue_bubble_down(&mut queue, 0)?;
                    root
                };
                let payload = self.priority_queue_make_payload(entry);
                self.make_some(payload)
            }
            IntrinsicFn::MapInsert => {
                let mut args = args.into_iter();
                let Value::Map(mut entries) = args.next().ok_or(RuntimeError::TypeError(
                    "map_insert: missing map argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError("map_insert expects a Map".into()));
                };
                let key = args.next().ok_or(RuntimeError::TypeError(
                    "map_insert: missing key argument".into(),
                ))?;
                let value = args.next().ok_or(RuntimeError::TypeError(
                    "map_insert: missing value argument".into(),
                ))?;
                if self.map_get_value(&entries, &key)? == Some(value.clone()) {
                    return Ok(Value::Map(entries));
                }
                let hash = self.hash_for_collection_key(&key)?;
                Rc::make_mut(&mut entries).insert_with(hash, key, value, &mut |lhs, rhs| {
                    self.trait_eq_values(lhs, rhs)
                })?;
                Ok(Value::Map(entries))
            }
            IntrinsicFn::MapContains => {
                let Value::Map(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError("map_contains expects a Map".into()));
                };
                Ok(Value::Bool(self.map_contains_value(entries, &args[1])?))
            }
            IntrinsicFn::MapRemove => {
                let mut args = args.into_iter();
                let Value::Map(mut entries) = args.next().ok_or(RuntimeError::TypeError(
                    "map_remove: missing map argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError("map_remove expects a Map".into()));
                };
                let key = args.next().ok_or(RuntimeError::TypeError(
                    "map_remove: missing key argument".into(),
                ))?;
                if !self.map_contains_value(&entries, &key)? {
                    return Ok(Value::Map(entries));
                }
                let hash = self.hash_for_collection_key(&key)?;
                Rc::make_mut(&mut entries)
                    .remove_with(hash, &key, &mut |lhs, rhs| self.trait_eq_values(lhs, rhs))?;
                Ok(Value::Map(entries))
            }
            IntrinsicFn::MapLen => {
                let Value::Map(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError("map_len expects a Map".into()));
                };
                Ok(Value::Int(entries.len() as i64))
            }
            IntrinsicFn::MapKeys => {
                let Value::Map(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError("map_keys expects a Map".into()));
                };
                Ok(Value::seq_source(SeqSource::MapKeys(entries.clone())))
            }
            IntrinsicFn::MapValues => {
                let Value::Map(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError("map_values expects a Map".into()));
                };
                Ok(Value::seq_source(SeqSource::MapValues(entries.clone())))
            }
            IntrinsicFn::MapIsEmpty => {
                let Value::Map(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError("map_is_empty expects a Map".into()));
                };
                Ok(Value::Bool(entries.is_empty()))
            }
            IntrinsicFn::MapGet => {
                let Value::Map(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError("map_get expects a Map".into()));
                };
                if let Some(v) = self.map_get_value(entries, &args[1])? {
                    self.make_some(v.clone())
                } else {
                    self.make_none()
                }
            }
            IntrinsicFn::MutableMapInsert => {
                let Value::MutableMap(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_map_insert expects a MutableMap".into(),
                    ));
                };
                let key = args[1].clone();
                let value = args[2].clone();
                if let Ok(primitive_key) = MapKey::from_value(&key) {
                    entries.borrow_mut().insert_primitive(primitive_key, value);
                } else {
                    let hash = self.hash_for_collection_key(&key)?;
                    entries
                        .borrow_mut()
                        .insert_with(hash, key, value, &mut |lhs, rhs| {
                            self.trait_eq_values(lhs, rhs)
                        })?;
                }
                Ok(Value::MutableMap(entries.clone()))
            }
            IntrinsicFn::MutableMapGet => {
                let Value::MutableMap(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_map_get expects a MutableMap".into(),
                    ));
                };
                let borrowed = entries.borrow();
                if let Some(v) = self.mutable_map_get_value(&borrowed, &args[1])? {
                    self.make_some(v)
                } else {
                    self.make_none()
                }
            }
            IntrinsicFn::MutableMapGetOrInsertWith => {
                let Value::MutableMap(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_map_get_or_insert_with expects a MutableMap".into(),
                    ));
                };
                if let Some(v) = {
                    let borrowed = entries.borrow();
                    self.mutable_map_get_value(&borrowed, &args[1])?
                } {
                    return Ok(v);
                }
                let computed = self.call_value(args[2].clone(), smallvec::smallvec![])?;
                if let Some(existing) = {
                    let borrowed = entries.borrow();
                    self.mutable_map_get_value(&borrowed, &args[1])?
                } {
                    return Ok(existing);
                }
                let key = args[1].clone();
                if let Ok(primitive_key) = MapKey::from_value(&key) {
                    entries
                        .borrow_mut()
                        .insert_primitive(primitive_key, computed.clone());
                } else {
                    let hash = self.hash_for_collection_key(&key)?;
                    entries.borrow_mut().insert_with(
                        hash,
                        key,
                        computed.clone(),
                        &mut |lhs, rhs| self.trait_eq_values(lhs, rhs),
                    )?;
                }
                Ok(computed)
            }
            IntrinsicFn::MutableMapContains => {
                let Value::MutableMap(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_map_contains expects a MutableMap".into(),
                    ));
                };
                let borrowed = entries.borrow();
                Ok(Value::Bool(
                    self.mutable_map_contains_value(&borrowed, &args[1])?,
                ))
            }
            IntrinsicFn::MutableMapRemove => {
                let Value::MutableMap(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_map_remove expects a MutableMap".into(),
                    ));
                };
                if let Ok(primitive_key) = MapKey::from_value(&args[1]) {
                    entries.borrow_mut().remove_primitive(&primitive_key);
                } else {
                    let hash = self.hash_for_collection_key(&args[1])?;
                    entries
                        .borrow_mut()
                        .remove_with(hash, &args[1], &mut |lhs, rhs| {
                            self.trait_eq_values(lhs, rhs)
                        })?;
                }
                Ok(Value::MutableMap(entries.clone()))
            }
            IntrinsicFn::MutableMapLen => {
                let Value::MutableMap(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_map_len expects a MutableMap".into(),
                    ));
                };
                Ok(Value::Int(entries.borrow().len() as i64))
            }
            IntrinsicFn::MutableMapKeys => {
                let Value::MutableMap(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_map_keys expects a MutableMap".into(),
                    ));
                };
                Ok(Value::seq_source(SeqSource::MapKeys(Rc::new(
                    entries.borrow().snapshot(),
                ))))
            }
            IntrinsicFn::MutableMapValues => {
                let Value::MutableMap(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_map_values expects a MutableMap".into(),
                    ));
                };
                Ok(Value::seq_source(SeqSource::MapValues(Rc::new(
                    entries.borrow().snapshot(),
                ))))
            }
            IntrinsicFn::MutableMapIsEmpty => {
                let Value::MutableMap(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_map_is_empty expects a MutableMap".into(),
                    ));
                };
                Ok(Value::Bool(entries.borrow().is_empty()))
            }
            IntrinsicFn::SetInsert => {
                let mut args = args.into_iter();
                let Value::Set(mut entries) = args.next().ok_or(RuntimeError::TypeError(
                    "set_insert: missing set argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError("set_insert expects a Set".into()));
                };
                let value = args.next().ok_or(RuntimeError::TypeError(
                    "set_insert: missing element argument".into(),
                ))?;
                if self.set_contains_value(&entries, &value)? {
                    return Ok(Value::Set(entries));
                }
                let hash = self.hash_for_collection_key(&value)?;
                Rc::make_mut(&mut entries)
                    .insert_with(hash, value, &mut |lhs, rhs| self.trait_eq_values(lhs, rhs))?;
                Ok(Value::Set(entries))
            }
            IntrinsicFn::SetContains => {
                let Value::Set(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError("set_contains expects a Set".into()));
                };
                Ok(Value::Bool(self.set_contains_value(entries, &args[1])?))
            }
            IntrinsicFn::SetRemove => {
                let mut args = args.into_iter();
                let Value::Set(mut entries) = args.next().ok_or(RuntimeError::TypeError(
                    "set_remove: missing set argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError("set_remove expects a Set".into()));
                };
                let value = args.next().ok_or(RuntimeError::TypeError(
                    "set_remove: missing element argument".into(),
                ))?;
                if !self.set_contains_value(&entries, &value)? {
                    return Ok(Value::Set(entries));
                }
                let hash = self.hash_for_collection_key(&value)?;
                Rc::make_mut(&mut entries)
                    .remove_with(hash, &value, &mut |lhs, rhs| self.trait_eq_values(lhs, rhs))?;
                Ok(Value::Set(entries))
            }
            IntrinsicFn::SetLen => {
                let Value::Set(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError("set_len expects a Set".into()));
                };
                Ok(Value::Int(entries.len() as i64))
            }
            IntrinsicFn::SetIsEmpty => {
                let Value::Set(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError("set_is_empty expects a Set".into()));
                };
                Ok(Value::Bool(entries.is_empty()))
            }
            IntrinsicFn::SetValues => {
                let Value::Set(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError("set_values expects a Set".into()));
                };
                Ok(Value::seq_source(SeqSource::SetValues(entries.clone())))
            }
            IntrinsicFn::MutableSetInsert => {
                let Value::MutableSet(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_set_insert expects a MutableSet".into(),
                    ));
                };
                let value = args[1].clone();
                if let Ok(primitive_key) = MapKey::from_value(&value) {
                    entries.borrow_mut().insert_primitive(primitive_key);
                } else {
                    let hash = self.hash_for_collection_key(&value)?;
                    entries
                        .borrow_mut()
                        .insert_with(hash, value, &mut |lhs, rhs| self.trait_eq_values(lhs, rhs))?;
                }
                Ok(Value::MutableSet(entries.clone()))
            }
            IntrinsicFn::MutableSetContains => {
                let Value::MutableSet(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_set_contains expects a MutableSet".into(),
                    ));
                };
                let borrowed = entries.borrow();
                Ok(Value::Bool(
                    self.mutable_set_contains_value(&borrowed, &args[1])?,
                ))
            }
            IntrinsicFn::MutableSetRemove => {
                let Value::MutableSet(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_set_remove expects a MutableSet".into(),
                    ));
                };
                if let Ok(primitive_key) = MapKey::from_value(&args[1]) {
                    entries.borrow_mut().remove_primitive(&primitive_key);
                } else {
                    let hash = self.hash_for_collection_key(&args[1])?;
                    entries
                        .borrow_mut()
                        .remove_with(hash, &args[1], &mut |lhs, rhs| {
                            self.trait_eq_values(lhs, rhs)
                        })?;
                }
                Ok(Value::MutableSet(entries.clone()))
            }
            IntrinsicFn::MutableSetLen => {
                let Value::MutableSet(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_set_len expects a MutableSet".into(),
                    ));
                };
                Ok(Value::Int(entries.borrow().len() as i64))
            }
            IntrinsicFn::MutableSetIsEmpty => {
                let Value::MutableSet(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_set_is_empty expects a MutableSet".into(),
                    ));
                };
                Ok(Value::Bool(entries.borrow().is_empty()))
            }
            IntrinsicFn::MutableSetValues => {
                let Value::MutableSet(entries) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "mutable_set_values expects a MutableSet".into(),
                    ));
                };
                Ok(Value::seq_source(SeqSource::SetValues(Rc::new(
                    entries.borrow().snapshot(),
                ))))
            }
            IntrinsicFn::SeqMap => {
                let plan = self.require_traversal_plan(&args[0], "seq_map")?;
                Ok(Value::seq_plan(SeqPlan::Map {
                    input: plan,
                    f: args[1].clone(),
                }))
            }
            IntrinsicFn::SeqFilter => {
                let plan = self.require_traversal_plan(&args[0], "seq_filter")?;
                Ok(Value::seq_plan(SeqPlan::Filter {
                    input: plan,
                    f: args[1].clone(),
                }))
            }
            IntrinsicFn::SeqFold => {
                let plan = self.require_traversal_plan(&args[0], "seq_fold")?;
                let mut acc = args[1].clone();
                let folder = args[2].clone();
                self.seq_for_each(&plan, &mut |interp, item| {
                    acc = interp
                        .call_value(folder.clone(), smallvec::smallvec![acc.clone(), item])?;
                    Ok(())
                })?;
                Ok(acc)
            }
            IntrinsicFn::SeqScan => {
                let input = self.require_traversal_plan(&args[0], "seq_scan")?;
                Ok(Value::seq_plan(SeqPlan::Scan {
                    input,
                    init: args[1].clone(),
                    f: args[2].clone(),
                }))
            }
            IntrinsicFn::SeqUnfold => Ok(Value::seq_plan(SeqPlan::Unfold {
                seed: args[0].clone(),
                step: args[1].clone(),
            })),
            IntrinsicFn::SeqEnumerate => {
                let plan = self.require_traversal_plan(&args[0], "seq_enumerate")?;
                Ok(Value::seq_plan(SeqPlan::Enumerate { input: plan }))
            }
            IntrinsicFn::SeqZip => {
                let left = self.require_traversal_plan(&args[0], "seq_zip")?;
                let right = self.require_traversal_plan(&args[1], "seq_zip")?;
                Ok(Value::seq_plan(SeqPlan::Zip { left, right }))
            }
            IntrinsicFn::SeqChunks => {
                let input = self.require_traversal_plan(&args[0], "seq_chunks")?;
                let Value::Int(n) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "seq_chunks expects an Int chunk size".into(),
                    ));
                };
                if *n <= 0 {
                    return Err(RuntimeError::TypeError(
                        "seq_chunks: chunk size must be > 0".into(),
                    ));
                }
                Ok(Value::seq_plan(SeqPlan::Chunks { input, n: *n }))
            }
            IntrinsicFn::SeqWindows => {
                let input = self.require_traversal_plan(&args[0], "seq_windows")?;
                let Value::Int(n) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "seq_windows expects an Int window size".into(),
                    ));
                };
                if *n <= 0 {
                    return Err(RuntimeError::TypeError(
                        "seq_windows: window size must be > 0".into(),
                    ));
                }
                Ok(Value::seq_plan(SeqPlan::Windows { input, n: *n }))
            }
            IntrinsicFn::SeqCount => {
                let plan = self.require_traversal_plan(&args[0], "seq_count")?;
                let mut count: i64 = 0;
                self.seq_for_each(&plan, &mut |_interp, _| {
                    count = count.checked_add(1).ok_or(RuntimeError::IntegerOverflow)?;
                    Ok(())
                })?;
                Ok(Value::Int(count))
            }
            IntrinsicFn::SeqCountBy => {
                let plan = self.require_traversal_plan(&args[0], "seq_count_by")?;
                let predicate = args[1].clone();
                let mut count: i64 = 0;
                self.seq_for_each(&plan, &mut |interp, item| {
                    let keep = interp.call_value(predicate.clone(), smallvec::smallvec![item])?;
                    match keep {
                        Value::Bool(true) => {
                            count = count.checked_add(1).ok_or(RuntimeError::IntegerOverflow)?;
                            Ok(())
                        }
                        Value::Bool(false) => Ok(()),
                        _ => Err(RuntimeError::TypeError(
                            "seq_count_by: predicate must return Bool".into(),
                        )),
                    }
                })?;
                Ok(Value::Int(count))
            }
            IntrinsicFn::SeqFrequencies => {
                let plan = self.require_traversal_plan(&args[0], "seq_frequencies")?;
                let mut entries = MapValue::new();
                let mut state = self.seq_iter_from_plan(&plan)?;
                while let Some(item) = self.seq_iter_next(&mut state)? {
                    let next = match self.map_get_value(&entries, &item)? {
                        Some(Value::Int(n)) => {
                            n.checked_add(1).ok_or(RuntimeError::IntegerOverflow)?
                        }
                        Some(_) => {
                            return Err(RuntimeError::TypeError(
                                "seq_frequencies: internal count must be Int".into(),
                            ));
                        }
                        None => 1,
                    };
                    entries = self.map_insert_value(&entries, item, Value::Int(next))?;
                }
                Ok(Value::Map(Rc::new(entries)))
            }
            IntrinsicFn::SeqAny => {
                let plan = self.require_traversal_plan(&args[0], "seq_any")?;
                let predicate = args[1].clone();
                let mut result = false;
                self.seq_for_each_control(&plan, &mut |interp, item| {
                    let keep = interp.call_value(predicate.clone(), smallvec::smallvec![item])?;
                    match keep {
                        Value::Bool(true) => {
                            result = true;
                            Ok(SeqEmitControl::Break)
                        }
                        Value::Bool(false) => Ok(SeqEmitControl::Continue),
                        _ => Err(RuntimeError::TypeError(
                            "seq_any: predicate must return Bool".into(),
                        )),
                    }
                })?;
                Ok(Value::Bool(result))
            }
            IntrinsicFn::SeqAll => {
                let plan = self.require_traversal_plan(&args[0], "seq_all")?;
                let predicate = args[1].clone();
                let mut result = true;
                self.seq_for_each_control(&plan, &mut |interp, item| {
                    let keep = interp.call_value(predicate.clone(), smallvec::smallvec![item])?;
                    match keep {
                        Value::Bool(true) => Ok(SeqEmitControl::Continue),
                        Value::Bool(false) => {
                            result = false;
                            Ok(SeqEmitControl::Break)
                        }
                        _ => Err(RuntimeError::TypeError(
                            "seq_all: predicate must return Bool".into(),
                        )),
                    }
                })?;
                Ok(Value::Bool(result))
            }
            IntrinsicFn::SeqFind => {
                let plan = self.require_traversal_plan(&args[0], "seq_find")?;
                let predicate = args[1].clone();
                let mut found: Option<Value> = None;
                self.seq_for_each_control(&plan, &mut |interp, item| {
                    let keep =
                        interp.call_value(predicate.clone(), smallvec::smallvec![item.clone()])?;
                    match keep {
                        Value::Bool(true) => {
                            found = Some(item);
                            Ok(SeqEmitControl::Break)
                        }
                        Value::Bool(false) => Ok(SeqEmitControl::Continue),
                        _ => Err(RuntimeError::TypeError(
                            "seq_find: predicate must return Bool".into(),
                        )),
                    }
                })?;

                if let Some(value) = found {
                    self.make_some(value)
                } else {
                    self.make_none()
                }
            }
            IntrinsicFn::SeqToList => {
                let plan = self.require_traversal_plan(&args[0], "seq_to_list")?;
                Ok(Value::list(self.eval_seq_to_vec(&plan)?))
            }
            IntrinsicFn::DequePopFront => {
                let mut args = args.into_iter();
                let Value::Deque(mut q) = args.next().ok_or(RuntimeError::TypeError(
                    "deque_pop_front: missing deque argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError(
                        "deque_pop_front expects a Deque".into(),
                    ));
                };
                if q.is_empty() {
                    return self.make_none();
                }
                let value = Rc::make_mut(&mut q)
                    .pop_front()
                    .expect("checked deque is non-empty before pop_front");
                let value_name = Name::new(&mut self.interner, "value");
                let rest_name = Name::new(&mut self.interner, "rest");
                let payload = Value::Record {
                    fields: vec![(value_name, value), (rest_name, Value::Deque(q))],
                    type_idx: None,
                };
                self.make_some(payload)
            }
            IntrinsicFn::DequePopBack => {
                let mut args = args.into_iter();
                let Value::Deque(mut q) = args.next().ok_or(RuntimeError::TypeError(
                    "deque_pop_back: missing deque argument".into(),
                ))?
                else {
                    return Err(RuntimeError::TypeError(
                        "deque_pop_back expects a Deque".into(),
                    ));
                };
                if q.is_empty() {
                    return self.make_none();
                }
                let value = Rc::make_mut(&mut q)
                    .pop_back()
                    .expect("checked deque is non-empty before pop_back");
                let value_name = Name::new(&mut self.interner, "value");
                let rest_name = Name::new(&mut self.interner, "rest");
                let payload = Value::Record {
                    fields: vec![(value_name, value), (rest_name, Value::Deque(q))],
                    type_idx: None,
                };
                self.make_some(payload)
            }
            IntrinsicFn::ListSort => {
                let Value::List(xs) = &args[0] else {
                    return Err(RuntimeError::TypeError("list_sort expects a List".into()));
                };
                let mut cmp = |a: &Value, b: &Value| {
                    let ord = self.trait_compare_values(a, b)?;
                    Ok(ord.cmp(&0))
                };
                let items = stable_merge_sort_by(xs.as_ref(), &mut cmp)?;
                Ok(Value::list(items))
            }
            IntrinsicFn::ListBinarySearch => {
                let Value::List(xs) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "list_binary_search expects a List".into(),
                    ));
                };
                let needle = &args[1];
                let mut lo = 0usize;
                let mut hi = xs.len();
                while lo < hi {
                    let mid = lo + (hi - lo) / 2;
                    let ord = self.trait_compare_values(&xs[mid], needle)?;
                    match ord.cmp(&0) {
                        Ordering::Less => lo = mid + 1,
                        Ordering::Greater => hi = mid,
                        Ordering::Equal => return Ok(Value::Int(mid as i64)),
                    }
                }
                Ok(Value::Int(-(lo as i64) - 1))
            }
            IntrinsicFn::ListSortBy => {
                let Value::List(xs) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "list_sort_by expects a List".into(),
                    ));
                };
                let cmp_fn = args[1].clone();
                let mut cmp = |a: &Value, b: &Value| {
                    let cmp_result =
                        self.call_value(cmp_fn.clone(), smallvec::smallvec![a.clone(), b.clone()])?;
                    match cmp_result {
                        Value::Int(n) if n > 0 => Ok(Ordering::Greater),
                        Value::Int(n) if n < 0 => Ok(Ordering::Less),
                        Value::Int(_) => Ok(Ordering::Equal),
                        _ => Err(RuntimeError::TypeError(
                            "list_sort_by: comparator must return Int".into(),
                        )),
                    }
                };
                let items = stable_merge_sort_by(xs.as_ref(), &mut cmp)?;
                Ok(Value::list(items))
            }
            IntrinsicFn::OptionUnwrapOr => {
                let fallback = args[1].clone();
                match self.decode_option_some_payload(&args[0], "option_unwrap_or")? {
                    Some(v) => Ok(v),
                    None => Ok(fallback),
                }
            }
            IntrinsicFn::OptionMapOr => {
                let fallback = args[1].clone();
                let mapper = args[2].clone();
                match self.decode_option_some_payload(&args[0], "option_map_or")? {
                    Some(v) => self.call_value(mapper, smallvec::smallvec![v]),
                    None => Ok(fallback),
                }
            }
            IntrinsicFn::OptionMap => {
                let mapper = args[1].clone();
                match self.decode_option_some_payload(&args[0], "option_map")? {
                    Some(v) => {
                        let mapped = self.call_value(mapper, smallvec::smallvec![v])?;
                        self.make_some(mapped)
                    }
                    None => self.make_none(),
                }
            }
            IntrinsicFn::OptionAndThen => {
                let mapper = args[1].clone();
                match self.decode_option_some_payload(&args[0], "option_and_then")? {
                    Some(v) => {
                        let mapped = self.call_value(mapper, smallvec::smallvec![v])?;
                        // Enforce mapper contract at runtime even if called dynamically.
                        let _ = self.decode_option_some_payload(&mapped, "option_and_then")?;
                        Ok(mapped)
                    }
                    None => self.make_none(),
                }
            }
            IntrinsicFn::ResultUnwrapOr => {
                let fallback = args[1].clone();
                match self.decode_result_ok_payload(&args[0], "result_unwrap_or")? {
                    Some(v) => Ok(v),
                    None => Ok(fallback),
                }
            }
            IntrinsicFn::ResultMap => {
                let mapper = args[1].clone();
                match self.decode_result_ok_payload(&args[0], "result_map")? {
                    Some(v) => {
                        let mapped = self.call_value(mapper, smallvec::smallvec![v])?;
                        self.make_ok(mapped)
                    }
                    None => Ok(args[0].clone()),
                }
            }
            IntrinsicFn::ResultAndThen => {
                let mapper = args[1].clone();
                match self.decode_result_ok_payload(&args[0], "result_and_then")? {
                    Some(v) => {
                        let mapped = self.call_value(mapper, smallvec::smallvec![v])?;
                        // Enforce mapper contract at runtime even if called dynamically.
                        let _ = self.decode_result_ok_payload(&mapped, "result_and_then")?;
                        Ok(mapped)
                    }
                    None => Ok(args[0].clone()),
                }
            }
            IntrinsicFn::ResultMapErr => {
                let mapper = args[1].clone();
                let (is_ok, payload) = self.decode_result_payload(&args[0], "result_map_err")?;
                if is_ok {
                    Ok(args[0].clone())
                } else {
                    let mapped_err = self.call_value(mapper, smallvec::smallvec![payload])?;
                    self.make_err(mapped_err)
                }
            }
            IntrinsicFn::ResultMapOr => {
                let fallback = args[1].clone();
                let mapper = args[2].clone();
                match self.decode_result_ok_payload(&args[0], "result_map_or")? {
                    Some(v) => self.call_value(mapper, smallvec::smallvec![v]),
                    None => Ok(fallback),
                }
            }
            IntrinsicFn::ParseInt => {
                let Value::String(s) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "parse_int expects a String argument".into(),
                    ));
                };
                match s.parse::<i64>() {
                    Ok(n) => self.make_ok(Value::Int(n)),
                    Err(e) => self.make_err(self.make_invalid_int(format!("{e}"))?),
                }
            }
            IntrinsicFn::ParseFloat => {
                let Value::String(s) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "parse_float expects a String argument".into(),
                    ));
                };
                match s.parse::<f64>() {
                    Ok(n) => self.make_ok(Value::Float(n)),
                    Err(e) => self.make_err(self.make_invalid_float(format!("{e}"))?),
                }
            }
            IntrinsicFn::CharToDecimalDigit => {
                let Value::Char(c) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "char_to_decimal_digit expects a Char".into(),
                    ));
                };
                if let Some(digit) = intrinsics::char_digit_value(*c, 10)? {
                    self.make_some(Value::Int(digit))
                } else {
                    self.make_none()
                }
            }
            IntrinsicFn::CharToDigit => {
                let Value::Char(c) = &args[0] else {
                    return Err(RuntimeError::TypeError(
                        "char_to_digit expects a Char".into(),
                    ));
                };
                let Value::Int(radix) = &args[1] else {
                    return Err(RuntimeError::TypeError(
                        "char_to_digit expects an Int radix".into(),
                    ));
                };
                if let Some(digit) = intrinsics::char_digit_value(*c, *radix)? {
                    self.make_some(Value::Int(digit))
                } else {
                    self.make_none()
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

    fn static_owner_key_for_name(&self, name: Name) -> Option<StaticOwnerKey> {
        let type_idx = *self.module_scope.types.get(&name)?;
        Some(
            self.module_scope
                .core_types
                .kind_for_idx(type_idx)
                .map(StaticOwnerKey::Core)
                .unwrap_or(StaticOwnerKey::User(type_idx)),
        )
    }

    /// Map a runtime value to its dispatch identity for method lookup.
    fn receiver_key_for_value(&self, val: &Value) -> Option<ReceiverKey> {
        match val {
            Value::String(_) => Some(ReceiverKey::Primitive(PrimitiveType::String)),
            Value::Int(_) => Some(ReceiverKey::Primitive(PrimitiveType::Int)),
            Value::Float(_) => Some(ReceiverKey::Primitive(PrimitiveType::Float)),
            Value::Bool(_) => Some(ReceiverKey::Primitive(PrimitiveType::Bool)),
            Value::Char(_) => Some(ReceiverKey::Primitive(PrimitiveType::Char)),
            Value::List(_) => self
                .module_scope
                .core_types
                .get(CoreType::List)
                .map(|_| ReceiverKey::Core(CoreType::List)),
            Value::BitSet(_) => self
                .module_scope
                .core_types
                .get(CoreType::BitSet)
                .map(|_| ReceiverKey::Core(CoreType::BitSet)),
            Value::MutableList(_) => self
                .module_scope
                .core_types
                .get(CoreType::MutableList)
                .map(|_| ReceiverKey::Core(CoreType::MutableList)),
            Value::MutablePriorityQueue(_) => self
                .module_scope
                .core_types
                .get(CoreType::MutablePriorityQueue)
                .map(|_| ReceiverKey::Core(CoreType::MutablePriorityQueue)),
            Value::MutableMap(_) => self
                .module_scope
                .core_types
                .get(CoreType::MutableMap)
                .map(|_| ReceiverKey::Core(CoreType::MutableMap)),
            Value::MutableSet(_) => self
                .module_scope
                .core_types
                .get(CoreType::MutableSet)
                .map(|_| ReceiverKey::Core(CoreType::MutableSet)),
            Value::MutableBitSet(_) => self
                .module_scope
                .core_types
                .get(CoreType::MutableBitSet)
                .map(|_| ReceiverKey::Core(CoreType::MutableBitSet)),
            Value::Deque(_) => self
                .module_scope
                .core_types
                .get(CoreType::Deque)
                .map(|_| ReceiverKey::Core(CoreType::Deque)),
            Value::Seq(_) => self
                .module_scope
                .core_types
                .get(CoreType::Seq)
                .map(|_| ReceiverKey::Core(CoreType::Seq)),
            Value::Map(_) => self
                .module_scope
                .core_types
                .get(CoreType::Map)
                .map(|_| ReceiverKey::Core(CoreType::Map)),
            Value::Set(_) => self
                .module_scope
                .core_types
                .get(CoreType::Set)
                .map(|_| ReceiverKey::Core(CoreType::Set)),
            Value::Adt { type_idx, .. } => Some(
                self.module_scope
                    .core_types
                    .kind_for_idx(*type_idx)
                    .map(ReceiverKey::Core)
                    .unwrap_or(ReceiverKey::User(*type_idx)),
            ),
            Value::Record {
                type_idx: Some(idx),
                ..
            } => Some(
                self.module_scope
                    .core_types
                    .kind_for_idx(*idx)
                    .map(ReceiverKey::Core)
                    .unwrap_or(ReceiverKey::User(*idx)),
            ),
            _ => None,
        }
    }

    fn eval_index(&mut self, base: Value, index: Value) -> Result<Value, RuntimeError> {
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
            (Value::MutableList(items), Value::Int(i)) => {
                let i = *i;
                if i < 0 {
                    return Err(RuntimeError::IndexOutOfBounds {
                        index: i,
                        len: items.len() as i64,
                    });
                }
                let idx = i as usize;
                if let Some(item) = items.get_cloned(idx) {
                    Ok(item)
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
            (Value::Map(entries), key) => self
                .map_get_value(entries, key)?
                .ok_or(RuntimeError::KeyNotFound),
            (Value::MutableMap(entries), key) => {
                let borrowed = entries.borrow();
                self.mutable_map_get_value(&borrowed, key)?
                    .ok_or(RuntimeError::KeyNotFound)
            }
            _ => Err(RuntimeError::TypeError(
                "indexing requires List, MutableList, String, Map, or MutableMap".into(),
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

    fn eval_match_terminal(
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
                let result = self.eval_terminal_expr(env, body, arm.body);
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
        tail_is_terminal: bool,
    ) -> Result<ControlFlow, RuntimeError> {
        env.push_scope();
        for stmt in stmts {
            match stmt {
                Stmt::Let { pat, init, .. } => {
                    let result = match match self
                        .consuming_local_for_shadow_rebind_init(body, *pat, *init)
                    {
                        Some(access) => self
                            .with_consuming_local(access, |this| this.eval_expr(env, body, *init)),
                        None => self.eval_expr(env, body, *init),
                    } {
                        Ok(result) => result,
                        Err(err) => {
                            env.pop_scope();
                            return Err(err);
                        }
                    };
                    if matches!(
                        result,
                        ControlFlow::Return(_) | ControlFlow::Break | ControlFlow::Continue
                    ) {
                        env.pop_scope();
                        return Ok(result);
                    }
                    if let Err(err) = self.bind_pat(body, *pat, &result.into_value(), env) {
                        env.pop_scope();
                        return Err(err);
                    }
                }
                Stmt::Assign { target, value } => {
                    let result = match self.eval_expr(env, body, *value) {
                        Ok(result) => result,
                        Err(err) => {
                            env.pop_scope();
                            return Err(err);
                        }
                    };
                    if matches!(
                        result,
                        ControlFlow::Return(_) | ControlFlow::Break | ControlFlow::Continue
                    ) {
                        env.pop_scope();
                        return Ok(result);
                    }
                    let Some(access) = self.local_access_for_expr(body, *target) else {
                        env.pop_scope();
                        return Err(RuntimeError::TypeError(
                            "assignment target must be a local variable".into(),
                        ));
                    };
                    if env
                        .set_slot(access.depth, access.slot, result.into_value())
                        .is_none()
                    {
                        env.pop_scope();
                        return Err(RuntimeError::TypeError(
                            "internal runtime error: assignment target unavailable".into(),
                        ));
                    }
                }
                Stmt::While {
                    condition,
                    body: loop_body,
                } => {
                    let result = match self.eval_while(env, body, *condition, *loop_body) {
                        Ok(result) => result,
                        Err(err) => {
                            env.pop_scope();
                            return Err(err);
                        }
                    };
                    if !matches!(result, ControlFlow::Value(_)) {
                        env.pop_scope();
                        return Ok(result);
                    }
                }
                Stmt::For {
                    pat,
                    source,
                    body: loop_body,
                } => {
                    let result = match self.eval_for(env, body, *pat, *source, *loop_body) {
                        Ok(result) => result,
                        Err(err) => {
                            env.pop_scope();
                            return Err(err);
                        }
                    };
                    if !matches!(result, ControlFlow::Value(_)) {
                        env.pop_scope();
                        return Ok(result);
                    }
                }
                Stmt::Break => {
                    env.pop_scope();
                    return Ok(ControlFlow::Break);
                }
                Stmt::Continue => {
                    env.pop_scope();
                    return Ok(ControlFlow::Continue);
                }
                Stmt::Expr(idx) => {
                    let result = match self.eval_expr(env, body, *idx) {
                        Ok(result) => result,
                        Err(err) => {
                            env.pop_scope();
                            return Err(err);
                        }
                    };
                    if matches!(
                        result,
                        ControlFlow::Return(_) | ControlFlow::Break | ControlFlow::Continue
                    ) {
                        env.pop_scope();
                        return Ok(result);
                    }
                }
            }
        }
        let result = if let Some(tail_idx) = tail {
            if tail_is_terminal {
                self.eval_terminal_expr(env, body, tail_idx)
            } else {
                self.eval_expr(env, body, tail_idx)
            }
        } else {
            Ok(ControlFlow::Value(Value::Unit))
        };
        env.pop_scope();
        result
    }

    fn eval_while(
        &mut self,
        env: &mut Env,
        body: &Body,
        condition: ExprIdx,
        loop_body: ExprIdx,
    ) -> Result<ControlFlow, RuntimeError> {
        loop {
            let cond = match self.eval_expr(env, body, condition)? {
                ControlFlow::Value(v) => v,
                ControlFlow::Return(v) => return Ok(ControlFlow::Return(v)),
                ControlFlow::Break => return Ok(ControlFlow::Break),
                ControlFlow::Continue => return Ok(ControlFlow::Continue),
            };
            let Value::Bool(cond_bool) = cond else {
                return Err(RuntimeError::TypeError(
                    "while condition must be Bool".into(),
                ));
            };
            if !cond_bool {
                return Ok(ControlFlow::Value(Value::Unit));
            }

            match self.eval_expr(env, body, loop_body)? {
                ControlFlow::Value(_) => {}
                ControlFlow::Continue => continue,
                ControlFlow::Break => return Ok(ControlFlow::Value(Value::Unit)),
                ControlFlow::Return(v) => return Ok(ControlFlow::Return(v)),
            }
        }
    }

    fn eval_for(
        &mut self,
        env: &mut Env,
        body: &Body,
        pat: kyokara_hir_def::expr::PatIdx,
        source: ExprIdx,
        loop_body: ExprIdx,
    ) -> Result<ControlFlow, RuntimeError> {
        let source_val = match self.eval_expr(env, body, source)? {
            ControlFlow::Value(v) => v,
            ControlFlow::Return(v) => return Ok(ControlFlow::Return(v)),
            ControlFlow::Break => return Ok(ControlFlow::Break),
            ControlFlow::Continue => return Ok(ControlFlow::Continue),
        };
        let plan = self.require_traversal_plan(&source_val, "for loop source")?;
        let mut early_return: Option<Value> = None;
        self.seq_for_each_control(&plan, &mut |interp, item| {
            env.push_scope();
            if let Err(err) = interp.bind_pat(body, pat, &item, env) {
                env.pop_scope();
                return Err(err);
            }

            let body_result = match interp.eval_expr(env, body, loop_body) {
                Ok(result) => result,
                Err(err) => {
                    env.pop_scope();
                    return Err(err);
                }
            };
            env.pop_scope();

            match body_result {
                ControlFlow::Value(_) | ControlFlow::Continue => Ok(SeqEmitControl::Continue),
                ControlFlow::Break => Ok(SeqEmitControl::Break),
                ControlFlow::Return(v) => {
                    early_return = Some(v);
                    Ok(SeqEmitControl::Break)
                }
            }
        })?;

        if let Some(v) = early_return {
            Ok(ControlFlow::Return(v))
        } else {
            Ok(ControlFlow::Value(Value::Unit))
        }
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

    fn eval_match_terminal_shared(
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
                let result = self.eval_terminal_expr_shared(body, arm.body);
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
        tail_is_terminal: bool,
    ) -> Result<ControlFlow, RuntimeError> {
        self.env.push_scope();
        for stmt in stmts {
            match stmt {
                Stmt::Let { pat, init, .. } => {
                    let result = match match self
                        .consuming_local_for_shadow_rebind_init(body, *pat, *init)
                    {
                        Some(access) => self.with_consuming_local(access, |this| {
                            this.eval_expr_shared(body, *init)
                        }),
                        None => self.eval_expr_shared(body, *init),
                    } {
                        Ok(result) => result,
                        Err(err) => {
                            self.env.pop_scope();
                            return Err(err);
                        }
                    };
                    if matches!(
                        result,
                        ControlFlow::Return(_) | ControlFlow::Break | ControlFlow::Continue
                    ) {
                        self.env.pop_scope();
                        return Ok(result);
                    }
                    if let Err(err) = self.bind_pat_shared(body, *pat, &result.into_value()) {
                        self.env.pop_scope();
                        return Err(err);
                    }
                }
                Stmt::Assign { target, value } => {
                    let result = match self.eval_expr_shared(body, *value) {
                        Ok(result) => result,
                        Err(err) => {
                            self.env.pop_scope();
                            return Err(err);
                        }
                    };
                    if matches!(
                        result,
                        ControlFlow::Return(_) | ControlFlow::Break | ControlFlow::Continue
                    ) {
                        self.env.pop_scope();
                        return Ok(result);
                    }
                    let Some(access) = self.local_access_for_expr(body, *target) else {
                        self.env.pop_scope();
                        return Err(RuntimeError::TypeError(
                            "assignment target must be a local variable".into(),
                        ));
                    };
                    if self
                        .env
                        .set_slot(access.depth, access.slot, result.into_value())
                        .is_none()
                    {
                        self.env.pop_scope();
                        return Err(RuntimeError::TypeError(
                            "internal runtime error: assignment target unavailable".into(),
                        ));
                    }
                }
                Stmt::While {
                    condition,
                    body: loop_body,
                } => {
                    let result = match self.eval_while_shared(body, *condition, *loop_body) {
                        Ok(result) => result,
                        Err(err) => {
                            self.env.pop_scope();
                            return Err(err);
                        }
                    };
                    if !matches!(result, ControlFlow::Value(_)) {
                        self.env.pop_scope();
                        return Ok(result);
                    }
                }
                Stmt::For {
                    pat,
                    source,
                    body: loop_body,
                } => {
                    let result = match self.eval_for_shared(body, *pat, *source, *loop_body) {
                        Ok(result) => result,
                        Err(err) => {
                            self.env.pop_scope();
                            return Err(err);
                        }
                    };
                    if !matches!(result, ControlFlow::Value(_)) {
                        self.env.pop_scope();
                        return Ok(result);
                    }
                }
                Stmt::Break => {
                    self.env.pop_scope();
                    return Ok(ControlFlow::Break);
                }
                Stmt::Continue => {
                    self.env.pop_scope();
                    return Ok(ControlFlow::Continue);
                }
                Stmt::Expr(idx) => {
                    let result = match self.eval_expr_shared(body, *idx) {
                        Ok(result) => result,
                        Err(err) => {
                            self.env.pop_scope();
                            return Err(err);
                        }
                    };
                    if matches!(
                        result,
                        ControlFlow::Return(_) | ControlFlow::Break | ControlFlow::Continue
                    ) {
                        self.env.pop_scope();
                        return Ok(result);
                    }
                }
            }
        }
        let result = if let Some(tail_idx) = tail {
            if tail_is_terminal {
                self.eval_terminal_expr_shared(body, tail_idx)
            } else {
                self.eval_expr_shared(body, tail_idx)
            }
        } else {
            Ok(ControlFlow::Value(Value::Unit))
        };
        self.env.pop_scope();
        result
    }

    fn eval_while_shared(
        &mut self,
        body: &Body,
        condition: ExprIdx,
        loop_body: ExprIdx,
    ) -> Result<ControlFlow, RuntimeError> {
        loop {
            let cond = match self.eval_expr_shared(body, condition)? {
                ControlFlow::Value(v) => v,
                ControlFlow::Return(v) => return Ok(ControlFlow::Return(v)),
                ControlFlow::Break => return Ok(ControlFlow::Break),
                ControlFlow::Continue => return Ok(ControlFlow::Continue),
            };
            let Value::Bool(cond_bool) = cond else {
                return Err(RuntimeError::TypeError(
                    "while condition must be Bool".into(),
                ));
            };
            if !cond_bool {
                return Ok(ControlFlow::Value(Value::Unit));
            }

            match self.eval_expr_shared(body, loop_body)? {
                ControlFlow::Value(_) => {}
                ControlFlow::Continue => continue,
                ControlFlow::Break => return Ok(ControlFlow::Value(Value::Unit)),
                ControlFlow::Return(v) => return Ok(ControlFlow::Return(v)),
            }
        }
    }

    fn eval_for_shared(
        &mut self,
        body: &Body,
        pat: kyokara_hir_def::expr::PatIdx,
        source: ExprIdx,
        loop_body: ExprIdx,
    ) -> Result<ControlFlow, RuntimeError> {
        let source_val = match self.eval_expr_shared(body, source)? {
            ControlFlow::Value(v) => v,
            ControlFlow::Return(v) => return Ok(ControlFlow::Return(v)),
            ControlFlow::Break => return Ok(ControlFlow::Break),
            ControlFlow::Continue => return Ok(ControlFlow::Continue),
        };
        let plan = self.require_traversal_plan(&source_val, "for loop source")?;
        let mut early_return: Option<Value> = None;
        self.seq_for_each_control(&plan, &mut |interp, item| {
            interp.env.push_scope();
            if let Err(err) = interp.bind_pat_shared(body, pat, &item) {
                interp.env.pop_scope();
                return Err(err);
            }

            let body_result = match interp.eval_expr_shared(body, loop_body) {
                Ok(result) => result,
                Err(err) => {
                    interp.env.pop_scope();
                    return Err(err);
                }
            };
            interp.env.pop_scope();

            match body_result {
                ControlFlow::Value(_) | ControlFlow::Continue => Ok(SeqEmitControl::Continue),
                ControlFlow::Break => Ok(SeqEmitControl::Break),
                ControlFlow::Return(v) => {
                    early_return = Some(v);
                    Ok(SeqEmitControl::Break)
                }
            }
        })?;

        if let Some(v) = early_return {
            Ok(ControlFlow::Return(v))
        } else {
            Ok(ControlFlow::Value(Value::Unit))
        }
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
    use crate::value::MapKey;

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
            derives: Vec::new(),
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
            requires: Vec::new(),
            ensures: Vec::new(),
            invariant: Vec::new(),
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
            checked.type_check.let_bodies,
            FxHashMap::default(),
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
    fn call_fn_by_idx_initializes_top_level_lets() {
        let mut interp = make_checked_interpreter(
            "let off = 1\nfn read() -> Int { off }\nfn main() -> Int { 0 }",
        );
        let read_idx = fn_idx_by_name(&mut interp, "read");

        let value = interp
            .call_fn_by_idx(read_idx, Args::new())
            .expect("top-level lets should be initialized before direct function calls");

        assert_eq!(value, Value::Int(1));
    }

    trait SharedBodyRef {
        fn body_ptr(&self) -> *const Body;
    }

    impl SharedBodyRef for Body {
        fn body_ptr(&self) -> *const Body {
            self as *const Body
        }
    }

    impl SharedBodyRef for Rc<Body> {
        fn body_ptr(&self) -> *const Body {
            Rc::as_ptr(self)
        }
    }

    fn shared_body_ptr<B: SharedBodyRef>(body: &B) -> *const Body {
        body.body_ptr()
    }

    fn lambda_body_ptr(value: &Value) -> *const Body {
        let Value::Fn(fv) = value else {
            panic!("expected function value");
        };
        let FnValue::Lambda { body, .. } = &**fv else {
            panic!("expected lambda function value");
        };
        Rc::as_ptr(body)
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
    fn factory_lambda_reuses_stored_function_body_handle() {
        let mut interp = make_checked_interpreter(
            "fn make() -> fn() -> Int { fn() => 1 } fn main() -> Int { 0 }",
        );
        let make_idx = fn_idx_by_name(&mut interp, "make");
        let stored_body_ptr = shared_body_ptr(
            interp
                .fn_bodies
                .get(&make_idx)
                .expect("factory function body should exist"),
        );

        let lambda = interp
            .call_fn_by_idx(make_idx, Args::new())
            .expect("factory call should return lambda");

        assert_eq!(
            stored_body_ptr,
            lambda_body_ptr(&lambda),
            "returned lambda should reuse the stored function body handle"
        );
    }

    #[test]
    fn repeated_factory_calls_share_body_handle_and_keep_distinct_captures() {
        let mut interp = make_checked_interpreter(
            "fn make(x: Int) -> fn() -> Int { fn() => x } fn main() -> Int { 0 }",
        );
        let make_idx = fn_idx_by_name(&mut interp, "make");

        let first = interp
            .call_fn_by_idx(make_idx, smallvec![Value::Int(1)])
            .expect("first factory call should succeed");
        let second = interp
            .call_fn_by_idx(make_idx, smallvec![Value::Int(2)])
            .expect("second factory call should succeed");

        assert_eq!(
            lambda_body_ptr(&first),
            lambda_body_ptr(&second),
            "factory-produced lambdas should share the same body handle"
        );

        let first_value = interp
            .call_value(first, Args::new())
            .expect("first captured lambda should run");
        let second_value = interp
            .call_value(second, Args::new())
            .expect("second captured lambda should run");
        assert_eq!(first_value, Value::Int(1));
        assert_eq!(second_value, Value::Int(2));
    }

    #[test]
    fn lambda_method_chain_tail_reuses_param_storage_when_unique() {
        let mut interp = make_checked_interpreter(
            "import collections\n\
             fn make() -> fn(List<Int>) -> List<Int> { fn(xs: List<Int>) => xs.push(1).push(2) }\n\
             fn main() -> Unit {}",
        );
        let make_idx = fn_idx_by_name(&mut interp, "make");
        let lambda = interp
            .call_fn_by_idx(make_idx, Args::new())
            .expect("factory should return lambda");
        let original = Value::list(vec![Value::Int(0)]);
        let before_ptr = match &original {
            Value::List(xs) => Rc::as_ptr(xs),
            _ => panic!("expected list"),
        };

        let out = interp
            .call_value(lambda, smallvec![original])
            .expect("lambda tail method chain should succeed");

        let Value::List(updated) = &out else {
            panic!("expected list output");
        };
        assert_eq!(
            updated.as_ref(),
            &[Value::Int(0), Value::Int(1), Value::Int(2)]
        );
        assert!(
            std::ptr::eq(before_ptr, Rc::as_ptr(updated)),
            "lambda tail method chain should reuse uniquely owned param storage"
        );
    }

    #[test]
    fn lambda_tail_method_chain_does_not_consume_duplicate_receiver_use() {
        let mut interp = make_checked_interpreter(
            "import collections\n\
             fn make() -> fn(List<Int>) -> Int {\n\
               fn(xs: List<Int>) => xs.push(xs.len()).len() + xs.len()\n\
             }\n\
             fn main() -> Unit {}",
        );
        let make_idx = fn_idx_by_name(&mut interp, "make");
        let lambda = interp
            .call_fn_by_idx(make_idx, Args::new())
            .expect("factory should return lambda");
        let original = Value::list(vec![Value::Int(10), Value::Int(20)]);

        let out = interp
            .call_value(lambda, smallvec![original])
            .expect("duplicate receiver use in lambda should still succeed");

        assert_eq!(out, Value::Int(5));
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
            "fn bad(x: Int) -> Int contract requires (x / 0 > 0) ensures (old(x) == x) { x }\n\
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
            "fn bad(x: Int) -> Int contract ensures (old(x) == x) { x / 0 }\n\
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
            "fn bad(x: Int) -> Int contract ensures (old(x) == x) invariant (x / 0 > 0) { x }\n\
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
            "fn bad(x: Int) -> Int contract ensures (old(x) / 0 > 0) { x }\n\
             fn main() -> Int { 0 }",
        );
        let bad_idx = fn_idx_by_name(&mut interp, "bad");
        let err = interp
            .call_fn_by_idx(bad_idx, smallvec![Value::Int(1)])
            .expect_err("ensures expression should fail at runtime");
        assert!(matches!(err, RuntimeError::DivisionByZero));
        assert_no_leaked_call_state(&mut interp);
    }

    #[test]
    fn deque_pop_front_reuses_storage_when_unique() {
        let mut interp = make_checked_interpreter("fn main() -> Unit {}");
        let original = Value::deque(VecDeque::from([Value::Int(1), Value::Int(2)]));
        let before_ptr = match &original {
            Value::Deque(q) => Rc::as_ptr(q),
            _ => panic!("expected deque"),
        };

        let out = interp
            .call_complex_intrinsic(IntrinsicFn::DequePopFront, smallvec![original])
            .expect("deque_pop_front should succeed");

        let payload = interp
            .decode_option_some_payload(&out, "deque_pop_front")
            .expect("decode should succeed")
            .expect("expected Some payload");
        let Value::Record { fields, .. } = payload else {
            panic!("expected record payload");
        };
        assert_eq!(fields.len(), 2, "payload should contain value/rest");
        assert_eq!(fields[0].1, Value::Int(1), "front value should be returned");
        let Value::Deque(rest) = &fields[1].1 else {
            panic!("rest should be a deque");
        };
        assert_eq!(rest.len(), 1, "rest should contain one element");
        assert!(
            std::ptr::eq(before_ptr, Rc::as_ptr(rest)),
            "deque_pop_front should not allocate a fresh deque when uniquely owned"
        );
    }

    #[test]
    fn deque_pop_front_detaches_when_storage_is_shared() {
        let mut interp = make_checked_interpreter("fn main() -> Unit {}");
        let original = Value::deque(VecDeque::from([Value::Int(1), Value::Int(2)]));
        let alias = original.clone();
        let alias_ptr = match &alias {
            Value::Deque(q) => Rc::as_ptr(q),
            _ => panic!("expected alias deque"),
        };

        let out = interp
            .call_complex_intrinsic(IntrinsicFn::DequePopFront, smallvec![original])
            .expect("deque_pop_front should succeed");

        let payload = interp
            .decode_option_some_payload(&out, "deque_pop_front")
            .expect("decode should succeed")
            .expect("expected Some payload");
        let Value::Record { fields, .. } = payload else {
            panic!("expected record payload");
        };
        let Value::Deque(rest) = &fields[1].1 else {
            panic!("rest should be a deque");
        };
        let Value::Deque(alias_q) = &alias else {
            panic!("expected alias deque");
        };

        assert_eq!(alias_q.len(), 2, "alias must remain unchanged");
        assert_eq!(rest.len(), 1, "rest should have one element after pop");
        assert!(
            !std::ptr::eq(alias_ptr, Rc::as_ptr(rest)),
            "deque_pop_front must detach when input storage is shared"
        );
    }

    #[test]
    fn deque_pop_front_empty_shared_does_not_mutate_alias() {
        let mut interp = make_checked_interpreter("fn main() -> Unit {}");
        let original = Value::deque(VecDeque::new());
        let alias = original.clone();
        let alias_ptr_before = match &alias {
            Value::Deque(q) => Rc::as_ptr(q),
            _ => panic!("expected alias deque"),
        };

        let out = interp
            .call_complex_intrinsic(IntrinsicFn::DequePopFront, smallvec![original])
            .expect("deque_pop_front should succeed");

        let none_payload = interp
            .decode_option_some_payload(&out, "deque_pop_front")
            .expect("decode should succeed");
        assert!(none_payload.is_none(), "empty deque should return None");

        let Value::Deque(alias_q) = &alias else {
            panic!("expected alias deque");
        };
        assert_eq!(alias_q.len(), 0, "alias must remain empty");
        assert!(
            std::ptr::eq(alias_ptr_before, Rc::as_ptr(alias_q)),
            "empty pop should not mutate alias storage"
        );
    }

    #[test]
    fn deque_pop_back_reuses_storage_when_unique() {
        let mut interp = make_checked_interpreter("fn main() -> Unit {}");
        let original = Value::deque(VecDeque::from([Value::Int(1), Value::Int(2)]));
        let before_ptr = match &original {
            Value::Deque(q) => Rc::as_ptr(q),
            _ => panic!("expected deque"),
        };

        let out = interp
            .call_complex_intrinsic(IntrinsicFn::DequePopBack, smallvec![original])
            .expect("deque_pop_back should succeed");

        let payload = interp
            .decode_option_some_payload(&out, "deque_pop_back")
            .expect("decode should succeed")
            .expect("expected Some payload");
        let Value::Record { fields, .. } = payload else {
            panic!("expected record payload");
        };
        assert_eq!(fields.len(), 2, "payload should contain value/rest");
        assert_eq!(fields[0].1, Value::Int(2), "back value should be returned");
        let Value::Deque(rest) = &fields[1].1 else {
            panic!("rest should be a deque");
        };
        assert_eq!(rest.len(), 1, "rest should contain one element");
        assert!(
            std::ptr::eq(before_ptr, Rc::as_ptr(rest)),
            "deque_pop_back should not allocate a fresh deque when uniquely owned"
        );
    }

    #[test]
    fn deque_pop_back_detaches_when_storage_is_shared() {
        let mut interp = make_checked_interpreter("fn main() -> Unit {}");
        let original = Value::deque(VecDeque::from([Value::Int(1), Value::Int(2)]));
        let alias = original.clone();
        let alias_ptr = match &alias {
            Value::Deque(q) => Rc::as_ptr(q),
            _ => panic!("expected alias deque"),
        };

        let out = interp
            .call_complex_intrinsic(IntrinsicFn::DequePopBack, smallvec![original])
            .expect("deque_pop_back should succeed");

        let payload = interp
            .decode_option_some_payload(&out, "deque_pop_back")
            .expect("decode should succeed")
            .expect("expected Some payload");
        let Value::Record { fields, .. } = payload else {
            panic!("expected record payload");
        };
        let Value::Deque(rest) = &fields[1].1 else {
            panic!("rest should be a deque");
        };
        let Value::Deque(alias_q) = &alias else {
            panic!("expected alias deque");
        };

        assert_eq!(alias_q.len(), 2, "alias must remain unchanged");
        assert_eq!(rest.len(), 1, "rest should have one element after pop");
        assert!(
            !std::ptr::eq(alias_ptr, Rc::as_ptr(rest)),
            "deque_pop_back must detach when input storage is shared"
        );
    }

    #[test]
    fn deque_pop_back_empty_shared_does_not_mutate_alias() {
        let mut interp = make_checked_interpreter("fn main() -> Unit {}");
        let original = Value::deque(VecDeque::new());
        let alias = original.clone();
        let alias_ptr_before = match &alias {
            Value::Deque(q) => Rc::as_ptr(q),
            _ => panic!("expected alias deque"),
        };

        let out = interp
            .call_complex_intrinsic(IntrinsicFn::DequePopBack, smallvec![original])
            .expect("deque_pop_back should succeed");

        let none_payload = interp
            .decode_option_some_payload(&out, "deque_pop_back")
            .expect("decode should succeed");
        assert!(none_payload.is_none(), "empty deque should return None");

        let Value::Deque(alias_q) = &alias else {
            panic!("expected alias deque");
        };
        assert_eq!(alias_q.len(), 0, "alias must remain empty");
        assert!(
            std::ptr::eq(alias_ptr_before, Rc::as_ptr(alias_q)),
            "empty pop should not mutate alias storage"
        );
    }

    #[test]
    fn list_update_reuses_storage_when_unique() {
        let mut interp = make_checked_interpreter(
            "fn inc(x: Int) -> Int { x + 1 }\n\
             fn main() -> Unit {}",
        );
        let inc_idx = fn_idx_by_name(&mut interp, "inc");
        let updater = Value::Fn(Box::new(FnValue::User(inc_idx)));
        let original = Value::list(vec![Value::Int(1), Value::Int(2)]);
        let before_ptr = match &original {
            Value::List(xs) => Rc::as_ptr(xs),
            _ => panic!("expected list"),
        };

        let out = interp
            .call_complex_intrinsic(
                IntrinsicFn::ListUpdate,
                smallvec![original, Value::Int(1), updater],
            )
            .expect("list_update should succeed");

        let Value::List(updated) = &out else {
            panic!("expected list output");
        };
        assert_eq!(updated.as_ref(), &[Value::Int(1), Value::Int(3)]);
        assert!(
            std::ptr::eq(before_ptr, Rc::as_ptr(updated)),
            "list_update should mutate in-place when storage is uniquely owned"
        );
    }

    #[test]
    fn list_update_noop_keeps_shared_storage() {
        let mut interp = make_checked_interpreter(
            "fn identity(x: Int) -> Int { x }\n\
             fn main() -> Unit {}",
        );
        let id_idx = fn_idx_by_name(&mut interp, "identity");
        let updater = Value::Fn(Box::new(FnValue::User(id_idx)));
        let original = Value::list(vec![Value::Int(5), Value::Int(7)]);
        let alias = original.clone();
        let alias_ptr = match &alias {
            Value::List(xs) => Rc::as_ptr(xs),
            _ => panic!("expected list alias"),
        };

        let out = interp
            .call_complex_intrinsic(
                IntrinsicFn::ListUpdate,
                smallvec![original, Value::Int(1), updater],
            )
            .expect("list_update should succeed");

        let Value::List(updated) = &out else {
            panic!("expected list output");
        };
        assert_eq!(updated.as_ref(), &[Value::Int(5), Value::Int(7)]);
        assert!(
            std::ptr::eq(alias_ptr, Rc::as_ptr(updated)),
            "no-op list_update should not detach shared storage"
        );
    }

    #[test]
    fn list_update_detaches_when_shared_and_changed() {
        let mut interp = make_checked_interpreter(
            "fn inc(x: Int) -> Int { x + 1 }\n\
             fn main() -> Unit {}",
        );
        let inc_idx = fn_idx_by_name(&mut interp, "inc");
        let updater = Value::Fn(Box::new(FnValue::User(inc_idx)));
        let original = Value::list(vec![Value::Int(10), Value::Int(20)]);
        let alias = original.clone();
        let alias_ptr = match &alias {
            Value::List(xs) => Rc::as_ptr(xs),
            _ => panic!("expected list alias"),
        };

        let out = interp
            .call_complex_intrinsic(
                IntrinsicFn::ListUpdate,
                smallvec![original, Value::Int(1), updater],
            )
            .expect("list_update should succeed");

        let Value::List(updated) = &out else {
            panic!("expected list output");
        };
        let Value::List(alias_items) = &alias else {
            panic!("expected list alias");
        };
        assert_eq!(alias_items.as_ref(), &[Value::Int(10), Value::Int(20)]);
        assert_eq!(updated.as_ref(), &[Value::Int(10), Value::Int(21)]);
        assert!(
            !std::ptr::eq(alias_ptr, Rc::as_ptr(updated)),
            "list_update must detach when mutation occurs on shared storage"
        );
    }

    #[test]
    fn list_method_chain_tail_reuses_param_storage_when_unique() {
        let mut interp = make_checked_interpreter(
            "import collections\n\
             fn extend(xs: List<Int>) -> List<Int> { xs.push(1).push(2) }\n\
             fn main() -> Unit {}",
        );
        let extend_idx = fn_idx_by_name(&mut interp, "extend");
        let original = Value::list(vec![Value::Int(0)]);
        let before_ptr = match &original {
            Value::List(xs) => Rc::as_ptr(xs),
            _ => panic!("expected list"),
        };

        let out = interp
            .call_fn_by_idx(extend_idx, smallvec![original])
            .expect("tail method chain should succeed");

        let Value::List(updated) = &out else {
            panic!("expected list output");
        };
        assert_eq!(
            updated.as_ref(),
            &[Value::Int(0), Value::Int(1), Value::Int(2)]
        );
        assert!(
            std::ptr::eq(before_ptr, Rc::as_ptr(updated)),
            "tail list method chain should reuse uniquely owned param storage"
        );
    }

    #[test]
    fn map_method_chain_tail_reuses_param_storage_when_unique() {
        let mut interp = make_checked_interpreter(
            "import collections\n\
             fn extend(m: Map<String, Int>) -> Map<String, Int> {\n\
               m.insert(\"a\", 1).insert(\"b\", 2)\n\
             }\n\
             fn main() -> Unit {}",
        );
        let extend_idx = fn_idx_by_name(&mut interp, "extend");
        let original = Value::map(indexmap::IndexMap::new());
        let before_ptr = match &original {
            Value::Map(entries) => Rc::as_ptr(entries),
            _ => panic!("expected map"),
        };

        let out = interp
            .call_fn_by_idx(extend_idx, smallvec![original])
            .expect("tail map method chain should succeed");

        let Value::Map(updated) = &out else {
            panic!("expected map output");
        };
        assert_eq!(updated.len(), 2);
        assert!(
            std::ptr::eq(before_ptr, Rc::as_ptr(updated)),
            "tail map method chain should reuse uniquely owned param storage"
        );
    }

    #[test]
    fn set_method_chain_tail_reuses_param_storage_when_unique() {
        let mut interp = make_checked_interpreter(
            "import collections\n\
             fn extend(s: Set<Int>) -> Set<Int> { s.insert(1).insert(2) }\n\
             fn main() -> Unit {}",
        );
        let extend_idx = fn_idx_by_name(&mut interp, "extend");
        let original = Value::set(indexmap::IndexSet::new());
        let before_ptr = match &original {
            Value::Set(entries) => Rc::as_ptr(entries),
            _ => panic!("expected set"),
        };

        let out = interp
            .call_fn_by_idx(extend_idx, smallvec![original])
            .expect("tail set method chain should succeed");

        let Value::Set(updated) = &out else {
            panic!("expected set output");
        };
        assert_eq!(updated.len(), 2);
        assert!(
            std::ptr::eq(before_ptr, Rc::as_ptr(updated)),
            "tail set method chain should reuse uniquely owned param storage"
        );
    }

    #[test]
    fn deque_pop_front_tail_reuses_param_storage_when_unique() {
        let mut interp = make_checked_interpreter(
            "import collections\n\
             fn pop(q: Deque<Int>) -> Option<{ value: Int, rest: Deque<Int> }> { q.pop_front() }\n\
             fn main() -> Unit {}",
        );
        let pop_idx = fn_idx_by_name(&mut interp, "pop");
        let original = Value::deque(VecDeque::from([Value::Int(1), Value::Int(2)]));
        let before_ptr = match &original {
            Value::Deque(items) => Rc::as_ptr(items),
            _ => panic!("expected deque"),
        };

        let out = interp
            .call_fn_by_idx(pop_idx, smallvec![original])
            .expect("tail deque pop should succeed");

        let payload = interp
            .decode_option_some_payload(&out, "deque pop tail")
            .expect("decode should succeed")
            .expect("expected Some payload");
        let Value::Record { fields, .. } = payload else {
            panic!("expected record payload");
        };
        let Value::Deque(rest) = &fields[1].1 else {
            panic!("expected deque rest");
        };
        assert_eq!(rest.len(), 1);
        assert!(
            std::ptr::eq(before_ptr, Rc::as_ptr(rest)),
            "tail deque pop should reuse uniquely owned param storage"
        );
    }

    #[test]
    fn shadow_rebind_method_chain_reuses_storage_when_old_binding_dies() {
        let mut interp = make_checked_interpreter(
            "import collections\n\
             fn extend(xs: List<Int>) -> List<Int> {\n\
               let xs = xs.push(1)\n\
               let xs = xs.push(2)\n\
               xs\n\
             }\n\
             fn main() -> Unit {}",
        );
        let extend_idx = fn_idx_by_name(&mut interp, "extend");
        let original = Value::list(vec![Value::Int(0)]);
        let before_ptr = match &original {
            Value::List(xs) => Rc::as_ptr(xs),
            _ => panic!("expected list"),
        };

        let out = interp
            .call_fn_by_idx(extend_idx, smallvec![original])
            .expect("shadow rebinding should succeed");

        let Value::List(updated) = &out else {
            panic!("expected list output");
        };
        assert_eq!(
            updated.as_ref(),
            &[Value::Int(0), Value::Int(1), Value::Int(2)]
        );
        assert!(
            std::ptr::eq(before_ptr, Rc::as_ptr(updated)),
            "shadow rebinding chain should preserve unique ownership across updates"
        );
    }

    #[test]
    fn tail_method_chain_does_not_consume_duplicate_receiver_use() {
        let mut interp = make_checked_interpreter(
            "import collections\n\
             fn measure(xs: List<Int>) -> Int { xs.push(xs.len()).len() + xs.len() }\n\
             fn main() -> Unit {}",
        );
        let measure_idx = fn_idx_by_name(&mut interp, "measure");
        let original = Value::list(vec![Value::Int(10), Value::Int(20)]);

        let out = interp
            .call_fn_by_idx(measure_idx, smallvec![original])
            .expect("duplicate receiver use should still succeed");

        assert_eq!(out, Value::Int(5));
    }

    #[test]
    fn shadow_rebind_init_does_not_consume_duplicate_receiver_use() {
        let mut interp = make_checked_interpreter(
            "import collections\n\
             fn extend(xs: List<Int>) -> Int {\n\
               let xs = xs.push(xs.len())\n\
               xs.len()\n\
             }\n\
             fn main() -> Unit {}",
        );
        let extend_idx = fn_idx_by_name(&mut interp, "extend");
        let original = Value::list(vec![Value::Int(10), Value::Int(20)]);

        let out = interp
            .call_fn_by_idx(extend_idx, smallvec![original])
            .expect("duplicate receiver use in rebinding should still succeed");

        assert_eq!(out, Value::Int(3));
    }

    #[test]
    fn stable_merge_sort_by_has_subquadratic_comparison_envelope() {
        let input: Vec<i64> = (0..128).rev().collect();
        let mut comparisons: usize = 0;
        let sorted = stable_merge_sort_by(&input, &mut |a: &i64, b: &i64| {
            comparisons += 1;
            Ok::<_, RuntimeError>(a.cmp(b))
        })
        .expect("sort should succeed");

        assert_eq!(sorted, (0..128).collect::<Vec<_>>());

        let n = input.len();
        let log2_n = usize::BITS as usize - n.leading_zeros() as usize;
        let envelope = n * (log2_n + 2);
        assert!(
            comparisons <= envelope,
            "expected O(n log n)-like comparator calls, got {comparisons} for n={n} (envelope={envelope})"
        );
    }

    #[test]
    fn mutable_list_traversal_plan_snapshots_current_backing_without_vec_clone() {
        let interp = make_checked_interpreter("fn main() -> Int { 0 }");
        let value = Value::mutable_list(vec![Value::Int(1), Value::Int(2)]);
        let Value::MutableList(items) = &value else {
            panic!("expected mutable list value");
        };

        let current_ptr = items.current_backing_ptr();
        let plan = interp
            .require_traversal_plan(&value, "seq_any")
            .expect("mutable list should be a traversal source");
        let SeqPlan::Source(SeqSource::FromList(snapshot)) = plan.as_ref() else {
            panic!("expected mutable list traversal plan to snapshot as FromList");
        };

        assert_eq!(
            current_ptr,
            Rc::as_ptr(snapshot),
            "mutable list traversal plan should reuse the current backing Rc"
        );
    }

    #[test]
    fn seq_iter_from_plan_reuses_string_map_and_set_source_storage() {
        let mut interp = make_checked_interpreter("fn main() -> Int { 0 }");

        let string_source = Rc::new("a\nb".to_string());
        let lines_plan = SeqPlan::Source(SeqSource::StringLines {
            s: string_source.clone(),
        });
        let SeqIterState::Source(SeqSourceIter::StringLines { s, .. }) = interp
            .seq_iter_from_plan(&lines_plan)
            .expect("string lines iterator should build")
        else {
            panic!("expected string-lines source iterator");
        };
        assert!(
            Rc::ptr_eq(&string_source, &s),
            "string lines iterator should reuse the source string"
        );

        let map_entries = Rc::new(MapValue::from_primitive_indexmap(indexmap::IndexMap::from(
            [(MapKey::Int(1), Value::Int(10))],
        )));
        let map_plan = SeqPlan::Source(SeqSource::MapKeys(map_entries.clone()));
        let SeqIterState::Source(SeqSourceIter::MapKeys { entries, .. }) = interp
            .seq_iter_from_plan(&map_plan)
            .expect("map-keys iterator should build")
        else {
            panic!("expected map-keys source iterator");
        };
        assert!(
            Rc::ptr_eq(&map_entries, &entries),
            "map-keys iterator should reuse map storage"
        );

        let set_entries = Rc::new(SetValue::from_primitive_indexset(indexmap::IndexSet::from(
            [MapKey::Int(1)],
        )));
        let set_plan = SeqPlan::Source(SeqSource::SetValues(set_entries.clone()));
        let SeqIterState::Source(SeqSourceIter::SetValues { entries, .. }) = interp
            .seq_iter_from_plan(&set_plan)
            .expect("set-values iterator should build")
        else {
            panic!("expected set-values source iterator");
        };
        assert!(
            Rc::ptr_eq(&set_entries, &entries),
            "set-values iterator should reuse set storage"
        );
    }
}
