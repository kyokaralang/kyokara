//! Builder API for constructing KIR programmatically.

use la_arena::Arena;

use kyokara_hir_def::expr::{BinaryOp, UnaryOp};
use kyokara_hir_def::item_tree::TypeItemIdx;
use kyokara_hir_def::name::Name;
use kyokara_hir_ty::effects::EffectSet;
use kyokara_hir_ty::ty::Ty;

use crate::block::{Block, BlockId, BlockParam, BranchTarget, Terminator};
use crate::function::{KirContracts, KirFunction};
use crate::inst::{CallTarget, Constant, HoleConstraint, Inst};
use crate::value::{ValueDef, ValueId};

/// Convenience builder for constructing a [`KirFunction`].
pub struct KirBuilder {
    blocks: Arena<Block>,
    values: Arena<ValueDef>,
    current_block: Option<BlockId>,
}

impl KirBuilder {
    pub fn new() -> Self {
        Self {
            blocks: Arena::default(),
            values: Arena::default(),
            current_block: None,
        }
    }

    /// Create a new block with an optional label. Does NOT switch to it.
    pub fn new_block(&mut self, label: Option<Name>) -> BlockId {
        self.blocks.alloc(Block::new(label))
    }

    /// Switch the builder's insertion point to `block`.
    pub fn switch_to(&mut self, block: BlockId) {
        self.current_block = Some(block);
    }

    /// Add a block parameter, returning the ValueId that represents it.
    pub fn add_block_param(&mut self, block: BlockId, name: Option<Name>, ty: Ty) -> ValueId {
        let index = self.blocks[block].params.len() as u32;
        let value = self.values.alloc(ValueDef {
            ty: ty.clone(),
            inst: Inst::BlockParam { block, index },
        });
        self.blocks[block]
            .params
            .push(BlockParam { name, ty, value });
        value
    }

    // ── Instruction helpers ──────────────────────────────────────

    /// Allocate a value without pushing it to any block body.
    /// Used for `FnParam` values that don't belong to a specific block.
    pub fn alloc_value(&mut self, ty: Ty, inst: Inst) -> ValueId {
        self.values.alloc(ValueDef { ty, inst })
    }

    fn push_value(&mut self, ty: Ty, inst: Inst) -> ValueId {
        let vid = self.values.alloc(ValueDef { ty, inst });
        let block = self
            .current_block
            .expect("no current block — call switch_to first");
        self.blocks[block].body.push(vid);
        vid
    }

    pub fn push_const(&mut self, c: Constant, ty: Ty) -> ValueId {
        self.push_value(ty, Inst::Const(c))
    }

    pub fn push_binary(&mut self, op: BinaryOp, lhs: ValueId, rhs: ValueId, ty: Ty) -> ValueId {
        self.push_value(ty, Inst::Binary { op, lhs, rhs })
    }

    pub fn push_unary(&mut self, op: UnaryOp, operand: ValueId, ty: Ty) -> ValueId {
        self.push_value(ty, Inst::Unary { op, operand })
    }

    pub fn push_record_create(&mut self, fields: Vec<(Name, ValueId)>, ty: Ty) -> ValueId {
        self.push_value(ty, Inst::RecordCreate { fields })
    }

    pub fn push_field_get(&mut self, base: ValueId, field: Name, ty: Ty) -> ValueId {
        self.push_value(ty, Inst::FieldGet { base, field })
    }

    pub fn push_record_update(
        &mut self,
        base: ValueId,
        updates: Vec<(Name, ValueId)>,
        ty: Ty,
    ) -> ValueId {
        self.push_value(ty, Inst::RecordUpdate { base, updates })
    }

    pub fn push_adt_construct(
        &mut self,
        type_def: TypeItemIdx,
        variant: Name,
        fields: Vec<ValueId>,
        ty: Ty,
    ) -> ValueId {
        self.push_value(
            ty,
            Inst::AdtConstruct {
                type_def,
                variant,
                fields,
            },
        )
    }

    pub fn push_call(&mut self, target: CallTarget, args: Vec<ValueId>, ty: Ty) -> ValueId {
        self.push_value(ty, Inst::Call { target, args })
    }

    pub fn push_assert(&mut self, condition: ValueId, message: String, ty: Ty) -> ValueId {
        self.push_value(ty, Inst::Assert { condition, message })
    }

    pub fn push_hole(&mut self, id: u32, constraints: Vec<HoleConstraint>, ty: Ty) -> ValueId {
        self.push_value(ty, Inst::Hole { id, constraints })
    }

    pub fn push_adt_field_get(&mut self, base: ValueId, field_index: u32, ty: Ty) -> ValueId {
        self.push_value(ty, Inst::AdtFieldGet { base, field_index })
    }

    pub fn push_fn_ref(&mut self, name: Name, ty: Ty) -> ValueId {
        self.push_value(ty, Inst::FnRef { name })
    }

    pub fn push_closure_create(&mut self, name: Name, captures: Vec<ValueId>, ty: Ty) -> ValueId {
        self.push_value(ty, Inst::ClosureCreate { name, captures })
    }

    // ── Query helpers ─────────────────────────────────────────────

    pub fn current_block(&self) -> Option<BlockId> {
        self.current_block
    }

    pub fn block_has_terminator(&self) -> bool {
        self.current_block
            .map(|bid| self.blocks[bid].terminator.is_some())
            .unwrap_or(false)
    }

    /// Get the type of an already-allocated value.
    pub fn value_ty(&self, vid: ValueId) -> &Ty {
        &self.values[vid].ty
    }

    // ── Terminators ──────────────────────────────────────────────

    pub fn set_terminator(&mut self, term: Terminator) {
        let block = self
            .current_block
            .expect("no current block — call switch_to first");
        self.blocks[block].terminator = Some(term);
    }

    pub fn set_return(&mut self, value: ValueId) {
        self.set_terminator(Terminator::Return(value));
    }

    pub fn set_jump(&mut self, target: BranchTarget) {
        self.set_terminator(Terminator::Jump(target));
    }

    pub fn set_branch(
        &mut self,
        condition: ValueId,
        then_target: BranchTarget,
        else_target: BranchTarget,
    ) {
        self.set_terminator(Terminator::Branch {
            condition,
            then_target,
            else_target,
        });
    }

    pub fn set_unreachable(&mut self) {
        self.set_terminator(Terminator::Unreachable);
    }

    // ── Finalization ─────────────────────────────────────────────

    /// Consume the builder and produce a [`KirFunction`].
    pub fn build(
        self,
        name: Name,
        params: Vec<(Name, Ty)>,
        ret_ty: Ty,
        effects: EffectSet,
        entry_block: BlockId,
        contracts: KirContracts,
    ) -> KirFunction {
        self.build_with_captures(
            name,
            params,
            Vec::new(),
            ret_ty,
            effects,
            entry_block,
            contracts,
        )
    }

    pub fn build_with_captures(
        self,
        name: Name,
        params: Vec<(Name, Ty)>,
        closure_capture_tys: Vec<Ty>,
        ret_ty: Ty,
        effects: EffectSet,
        entry_block: BlockId,
        contracts: KirContracts,
    ) -> KirFunction {
        KirFunction {
            name,
            params,
            closure_capture_tys,
            ret_ty,
            effects,
            blocks: self.blocks,
            values: self.values,
            entry_block,
            contracts,
            source_map: Default::default(),
        }
    }
}

impl Default for KirBuilder {
    fn default() -> Self {
        Self::new()
    }
}
