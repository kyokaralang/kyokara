//! HIR → KIR lowering pass.
//!
//! Walks the typed expression tree and emits flat SSA instructions
//! with explicit control-flow blocks.

mod expr;
mod pat;

use rustc_hash::{FxHashMap, FxHashSet};

use kyokara_hir_def::body::Body;
use kyokara_hir_def::expr::ExprIdx;
use kyokara_hir_def::item_tree::{FnItemIdx, ItemTree};
use kyokara_hir_def::name::Name;
use kyokara_hir_def::pat::Pat;
use kyokara_hir_def::resolver::ModuleScope;
use kyokara_hir_def::type_ref::TypeRef;
use kyokara_hir_ty::TypeCheckResult;
use kyokara_hir_ty::effects::EffectSet;
use kyokara_hir_ty::infer::InferenceResult;
use kyokara_hir_ty::ty::Ty;
use kyokara_intern::Interner;

use crate::KirModule;
use crate::build::KirBuilder;
use crate::function::{KirContracts, KirFunction};
use crate::inst::Inst;
use crate::value::ValueId;

/// Pre-interned block labels for readable output.
pub(crate) struct Labels {
    entry: Name,
    then_: Name,
    else_: Name,
    merge: Name,
    default: Name,
}

impl Labels {
    fn new(interner: &mut Interner) -> Self {
        Self {
            entry: Name::new(interner, "entry"),
            then_: Name::new(interner, "then"),
            else_: Name::new(interner, "else"),
            merge: Name::new(interner, "merge"),
            default: Name::new(interner, "default"),
        }
    }
}

/// Per-function lowering context.
pub(crate) struct LoweringCtx<'a> {
    pub(crate) builder: KirBuilder,
    pub(crate) body: &'a Body,
    pub(crate) infer: &'a InferenceResult,
    pub(crate) item_tree: &'a ItemTree,
    pub(crate) module_scope: &'a ModuleScope,
    pub(crate) interner: &'a Interner,
    pub(crate) locals: Vec<FxHashMap<Name, ValueId>>,
    pub(crate) intrinsics: FxHashSet<Name>,
    pub(crate) hole_counter: u32,
    pub(crate) labels: Labels,
    /// Ensures expression to emit before every return point.
    pub(crate) ensures_expr: Option<ExprIdx>,
    /// Pre-interned "result" name for binding the return value in ensures.
    pub(crate) result_name: Option<Name>,
    /// Collected ensures assertion ValueIds from all return points.
    pub(crate) ensures_vids: Vec<ValueId>,
}

impl<'a> LoweringCtx<'a> {
    // ── Scope management ─────────────────────────────────────────

    pub(crate) fn push_scope(&mut self) {
        self.locals.push(FxHashMap::default());
    }

    pub(crate) fn pop_scope(&mut self) {
        self.locals.pop();
    }

    pub(crate) fn define_local(&mut self, name: Name, vid: ValueId) {
        if let Some(scope) = self.locals.last_mut() {
            scope.insert(name, vid);
        }
    }

    pub(crate) fn lookup_local(&self, name: Name) -> Option<ValueId> {
        for scope in self.locals.iter().rev() {
            if let Some(&vid) = scope.get(&name) {
                return Some(vid);
            }
        }
        None
    }

    // ── Type helpers ─────────────────────────────────────────────

    pub(crate) fn expr_ty(&self, idx: ExprIdx) -> Ty {
        self.infer.expr_types.get(idx).cloned().unwrap_or(Ty::Error)
    }

    pub(crate) fn pat_ty(&self, idx: kyokara_hir_def::expr::PatIdx) -> Ty {
        self.infer.pat_types.get(idx).cloned().unwrap_or(Ty::Error)
    }

    pub(crate) fn next_hole_id(&mut self) -> u32 {
        let id = self.hole_counter;
        self.hole_counter += 1;
        id
    }

    pub(crate) fn block_has_terminator(&self) -> bool {
        self.builder.block_has_terminator()
    }
}

// ── Public API ───────────────────────────────────────────────────

/// Lower an entire module to KIR.
pub fn lower_module(
    item_tree: &ItemTree,
    module_scope: &ModuleScope,
    type_check: &TypeCheckResult,
    interner: &mut Interner,
) -> KirModule {
    let mut module = KirModule::new();
    let main_name = Name::new(interner, "main");

    for (fn_idx, fn_item) in item_tree.functions.iter() {
        if !fn_item.has_body {
            continue;
        }

        let Some(body) = type_check.fn_bodies.get(&fn_idx) else {
            continue;
        };
        let Some(infer) = type_check.fn_results.get(&fn_idx) else {
            continue;
        };

        let func = lower_function(fn_idx, body, infer, item_tree, module_scope, interner);
        let fid = module.functions.alloc(func);

        if fn_item.name == main_name {
            module.entry = Some(fid);
        }
    }

    module
}

/// Lower a single function to KIR.
pub fn lower_function(
    fn_idx: FnItemIdx,
    body: &Body,
    infer: &InferenceResult,
    item_tree: &ItemTree,
    module_scope: &ModuleScope,
    interner: &mut Interner,
) -> KirFunction {
    let labels = Labels::new(interner);
    let fn_item = &item_tree.functions[fn_idx];

    // Build intrinsics set (functions without bodies).
    let mut intrinsics = FxHashSet::default();
    for (_, fi) in item_tree.functions.iter() {
        if !fi.has_body {
            intrinsics.insert(fi.name);
        }
    }

    // Resolve param types from the inference result.
    let param_types = resolve_param_types(fn_item, body, infer);

    // Pre-intern "result" name if there's an ensures clause.
    let (ensures_expr, result_name) = if body.ensures.is_some() {
        let rn = Name::new(interner, "result");
        (body.ensures, Some(rn))
    } else {
        (None, None)
    };

    let mut ctx = LoweringCtx {
        builder: KirBuilder::new(),
        body,
        infer,
        item_tree,
        module_scope,
        interner: &*interner,
        locals: Vec::new(),
        intrinsics,
        hole_counter: 0,
        labels,
        ensures_expr,
        result_name,
        ensures_vids: Vec::new(),
    };

    // Create entry block.
    let entry_block = ctx.builder.new_block(Some(ctx.labels.entry));
    ctx.builder.switch_to(entry_block);
    ctx.push_scope();

    // Create FnParam values for each parameter.
    for (i, param) in fn_item.params.iter().enumerate() {
        let ty = param_types.get(i).cloned().unwrap_or(Ty::Error);
        let vid = ctx
            .builder
            .alloc_value(ty, Inst::FnParam { index: i as u32 });
        ctx.define_local(param.name, vid);
    }

    // Lower requires clause → Assert.
    let mut requires_vids = Vec::new();
    if let Some(req_expr) = body.requires {
        let cond = ctx.lower_expr(req_expr);
        let vid = ctx
            .builder
            .push_assert(cond, "requires".to_string(), Ty::Unit);
        requires_vids.push(vid);
    }

    // Lower root expression.
    let root_val = ctx.lower_expr(body.root);

    // Emit ensures + return for implicit return (not already terminated by `return`).
    if !ctx.block_has_terminator() {
        if let (Some(ens_expr), Some(rn)) = (ctx.ensures_expr, ctx.result_name) {
            // Temporarily clear ensures_expr to avoid re-entrant emission.
            ctx.ensures_expr = None;
            ctx.define_local(rn, root_val);
            let cond = ctx.lower_expr(ens_expr);
            let vid = ctx
                .builder
                .push_assert(cond, "ensures".to_string(), Ty::Unit);
            ctx.ensures_vids.push(vid);
        }
        ctx.builder.set_return(root_val);
    }

    ctx.pop_scope();

    // Build effects from with_caps.
    let effects = build_effects(fn_item);

    // Resolve return type.
    let ret_ty = ctx.expr_ty(body.root);

    ctx.builder.build(
        fn_item.name,
        fn_item
            .params
            .iter()
            .enumerate()
            .map(|(i, p)| (p.name, param_types.get(i).cloned().unwrap_or(Ty::Error)))
            .collect(),
        ret_ty,
        effects,
        entry_block,
        KirContracts {
            requires: requires_vids,
            ensures: ctx.ensures_vids,
        },
    )
}

// ── Helpers ──────────────────────────────────────────────────────

fn resolve_param_types(
    fn_item: &kyokara_hir_def::item_tree::FnItem,
    body: &Body,
    infer: &InferenceResult,
) -> Vec<Ty> {
    fn_item
        .params
        .iter()
        .map(|param| {
            for (pat_idx, _) in &body.pat_scopes {
                if let Pat::Bind { name } = &body.pats[*pat_idx] {
                    if *name == param.name {
                        if let Some(ty) = infer.pat_types.get(*pat_idx) {
                            return ty.clone();
                        }
                    }
                }
            }
            Ty::Error
        })
        .collect()
}

fn build_effects(fn_item: &kyokara_hir_def::item_tree::FnItem) -> EffectSet {
    let mut caps = FxHashSet::default();
    for cap_ref in &fn_item.with_caps {
        if let TypeRef::Path { path, .. } = cap_ref {
            if let Some(name) = path.last() {
                caps.insert(name);
            }
        }
    }
    EffectSet { caps }
}
