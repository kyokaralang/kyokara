//! Per-function WASM code generation.

use kyokara_hir_def::expr::{BinaryOp, UnaryOp};
use kyokara_hir_def::name::Name;
use kyokara_hir_ty::ty::Ty;
use kyokara_kir::block::{BlockId, Terminator};
use kyokara_kir::function::KirFunction;
use kyokara_kir::inst::{CallTarget, Constant, Inst};
use kyokara_kir::value::ValueId;
use rustc_hash::FxHashMap;
use wasm_encoder::{BlockType, Function, Instruction, MemArg, ValType};

use crate::error::CodegenError;
use crate::wasm::ModuleCtx;
use crate::wasm::control::reverse_postorder;
use crate::wasm::layout::{self, AdtLayout};
use crate::wasm::ty::{is_i32_type, ty_to_valtype};

/// Per-function codegen state.
pub struct FuncCodegen<'a> {
    kir_func: &'a KirFunction,
    ctx: &'a ModuleCtx<'a>,
    /// ValueId -> WASM local index.
    local_map: FxHashMap<ValueId, u32>,
    /// Types of non-param locals (for WASM local declarations).
    local_types: Vec<ValType>,
    /// Next available local index.
    next_local: u32,
}

impl<'a> FuncCodegen<'a> {
    pub fn new(kir_func: &'a KirFunction, ctx: &'a ModuleCtx<'a>) -> Self {
        Self {
            kir_func,
            ctx,
            local_map: FxHashMap::default(),
            local_types: Vec::new(),
            next_local: kir_func.params.len() as u32,
        }
    }

    /// Allocate locals for all ValueIds and emit the function body.
    pub fn emit(mut self) -> Result<Function, CodegenError> {
        // Phase 1: Allocate locals for all non-param values.
        self.allocate_locals()?;

        // Phase 2: Emit instructions.
        let locals: Vec<(u32, ValType)> = self.local_types.iter().map(|&t| (1, t)).collect();
        let mut func = Function::new(locals);

        self.emit_body(&mut func)?;

        func.instruction(&Instruction::End);
        Ok(func)
    }

    // ── Local allocation ──────────────────────────────────────────

    fn allocate_locals(&mut self) -> Result<(), CodegenError> {
        // Map FnParam values to WASM param locals 0..N-1.
        for (vid, vdef) in self.kir_func.values.iter() {
            if let Inst::FnParam { index } = &vdef.inst {
                self.local_map.insert(vid, *index);
            }
        }

        // Allocate locals for all other values.
        for (vid, vdef) in self.kir_func.values.iter() {
            if matches!(&vdef.inst, Inst::FnParam { .. }) {
                continue;
            }
            // When the HIR type is Error/Never (from contract clauses),
            // infer the WASM type from the instruction itself.
            let wasm_ty = if vdef.ty.is_poison() {
                self.infer_wasm_type_from_inst(&vdef.inst)
            } else {
                ty_to_valtype(&vdef.ty)?
            };
            let local_idx = self.next_local;
            self.next_local += 1;
            self.local_types.push(wasm_ty);
            self.local_map.insert(vid, local_idx);
        }

        Ok(())
    }

    /// Infer WASM type from the instruction when the HIR type is Error.
    fn infer_wasm_type_from_inst(&self, inst: &Inst) -> ValType {
        match inst {
            Inst::Const(Constant::Int(_)) => ValType::I64,
            Inst::Const(Constant::Float(_)) => ValType::F64,
            Inst::Const(Constant::Bool(_)) | Inst::Const(Constant::Unit) => ValType::I32,
            Inst::Binary { op, .. } => {
                if op.returns_bool() {
                    ValType::I32
                } else {
                    // Arithmetic/bitwise ops are represented as i64 at this stage.
                    ValType::I64
                }
            }
            Inst::Unary {
                op: UnaryOp::Not, ..
            } => ValType::I32,
            Inst::Unary {
                op: UnaryOp::Neg, ..
            }
            | Inst::Unary {
                op: UnaryOp::BitNot,
                ..
            } => ValType::I64,
            Inst::Assert { .. } => ValType::I32, // Unit
            _ => ValType::I32,                   // default
        }
    }

    fn local_for(&self, vid: ValueId) -> u32 {
        self.local_map[&vid]
    }

    fn value_ty(&self, vid: ValueId) -> &Ty {
        &self.kir_func.values[vid].ty
    }

    // ── Body emission ─────────────────────────────────────────────

    fn emit_body(&self, func: &mut Function) -> Result<(), CodegenError> {
        let rpo = reverse_postorder(self.kir_func);

        if rpo.is_empty() {
            return Err(CodegenError::MissingEntryBlock);
        }

        // For simple functions (single block or linear chain), emit directly.
        // For branching, use structured control flow.
        self.emit_structured(func, &rpo)?;

        Ok(())
    }

    /// Emit structured WASM control flow for the function.
    fn emit_structured(&self, func: &mut Function, _rpo: &[BlockId]) -> Result<(), CodegenError> {
        let mut emitted = FxHashMap::default();
        self.emit_block_chain(func, self.kir_func.entry_block, None, &mut emitted)?;
        Ok(())
    }

    /// Emit a block and its successors, stopping at `stop_at` (a merge block
    /// owned by an outer branch/switch).
    fn emit_block_chain(
        &self,
        func: &mut Function,
        block_id: BlockId,
        stop_at: Option<BlockId>,
        emitted: &mut FxHashMap<BlockId, ()>,
    ) -> Result<(), CodegenError> {
        if Some(block_id) == stop_at || emitted.contains_key(&block_id) {
            return Ok(());
        }
        emitted.insert(block_id, ());

        let block = &self.kir_func.blocks[block_id];

        for &vid in &block.body {
            self.emit_inst(func, vid)?;
        }

        let term = block
            .terminator
            .as_ref()
            .ok_or_else(|| CodegenError::MissingTerminator("block has no terminator".into()))?;

        match term {
            Terminator::Return(val) => {
                self.emit_get(func, *val);
                func.instruction(&Instruction::Return);
            }
            Terminator::Unreachable => {
                func.instruction(&Instruction::Unreachable);
            }
            Terminator::Jump(target) => {
                self.emit_block_param_stores(func, target)?;
                self.emit_block_chain(func, target.block, stop_at, emitted)?;
            }
            Terminator::Branch {
                condition,
                then_target,
                else_target,
            } => {
                self.emit_branch(func, *condition, then_target, else_target, stop_at, emitted)?;
            }
            Terminator::Switch {
                scrutinee,
                cases,
                default,
            } => {
                self.emit_switch(func, *scrutinee, cases, default.as_ref(), stop_at, emitted)?;
            }
        }

        Ok(())
    }

    fn emit_branch(
        &self,
        func: &mut Function,
        condition: ValueId,
        then_target: &kyokara_kir::block::BranchTarget,
        else_target: &kyokara_kir::block::BranchTarget,
        outer_stop: Option<BlockId>,
        emitted: &mut FxHashMap<BlockId, ()>,
    ) -> Result<(), CodegenError> {
        // Find the merge block: the block both arms eventually converge to.
        let merge_id = self.find_branch_merge_deep(then_target, else_target);

        // Use BlockType::Empty always — store results via locals.
        self.emit_get(func, condition);
        func.instruction(&Instruction::If(BlockType::Empty));

        // Then arm: emit with merge_id as stop point.
        self.emit_branch_arm(func, then_target, merge_id, emitted)?;

        func.instruction(&Instruction::Else);

        // Else arm: emit with merge_id as stop point.
        self.emit_branch_arm(func, else_target, merge_id, emitted)?;

        func.instruction(&Instruction::End);

        // After if/else, emit the merge block (it wasn't emitted by either arm).
        if let Some(mid) = merge_id {
            self.emit_block_chain(func, mid, outer_stop, emitted)?;
        } else {
            // Both arms diverge (e.g., both return) — no merge block.
            // Emit unreachable to satisfy WASM type checking.
            func.instruction(&Instruction::Unreachable);
        }

        Ok(())
    }

    fn emit_branch_arm(
        &self,
        func: &mut Function,
        target: &kyokara_kir::block::BranchTarget,
        merge_id: Option<BlockId>,
        emitted: &mut FxHashMap<BlockId, ()>,
    ) -> Result<(), CodegenError> {
        // If branching directly to the merge block, just store params.
        if Some(target.block) == merge_id {
            self.emit_block_param_stores(func, target)?;
            return Ok(());
        }

        // Emit the arm's block chain, stopping at merge_id.
        self.emit_block_param_stores(func, target)?;
        emitted.insert(target.block, ());

        let target_block = &self.kir_func.blocks[target.block];
        for &vid in &target_block.body {
            self.emit_inst(func, vid)?;
        }

        if let Some(term) = &target_block.terminator {
            match term {
                Terminator::Return(val) => {
                    self.emit_get(func, *val);
                    func.instruction(&Instruction::Return);
                }
                Terminator::Jump(inner) => {
                    // Store params for the jump target.
                    self.emit_block_param_stores(func, inner)?;
                    // If jumping to merge, stop. Otherwise continue chain.
                    if Some(inner.block) != merge_id {
                        self.emit_block_chain(func, inner.block, merge_id, emitted)?;
                    }
                }
                Terminator::Unreachable => {
                    func.instruction(&Instruction::Unreachable);
                }
                Terminator::Branch {
                    condition,
                    then_target,
                    else_target,
                } => {
                    self.emit_branch(
                        func,
                        *condition,
                        then_target,
                        else_target,
                        merge_id,
                        emitted,
                    )?;
                }
                Terminator::Switch {
                    scrutinee,
                    cases,
                    default,
                } => {
                    self.emit_switch(func, *scrutinee, cases, default.as_ref(), merge_id, emitted)?;
                }
            }
        }

        Ok(())
    }

    /// Find the merge block for a branch by following jump chains.
    fn find_branch_merge_deep(
        &self,
        then_target: &kyokara_kir::block::BranchTarget,
        else_target: &kyokara_kir::block::BranchTarget,
    ) -> Option<BlockId> {
        if then_target.block == else_target.block {
            return Some(then_target.block);
        }

        let then_chain = self.follow_chain(then_target.block);
        let else_chain = self.follow_chain(else_target.block);

        // Find the first block in then_chain that also appears in else_chain.
        then_chain.iter().find(|t| else_chain.contains(t)).copied()
    }

    /// Follow a chain of blocks until we reach a Return/Unreachable/Branch.
    /// Returns the ordered list of blocks visited via Jump terminators.
    fn follow_chain(&self, start: BlockId) -> Vec<BlockId> {
        let mut chain = Vec::new();
        let mut current = start;
        for _ in 0..50 {
            // guard against pathological input
            let block = &self.kir_func.blocks[current];
            match &block.terminator {
                Some(Terminator::Jump(target)) => {
                    chain.push(target.block);
                    current = target.block;
                }
                Some(Terminator::Branch {
                    then_target,
                    else_target,
                    ..
                }) => {
                    // Look through the branch to find its merge.
                    if let Some(merge) = self.find_branch_merge_deep(then_target, else_target) {
                        chain.push(merge);
                        current = merge;
                        continue;
                    }
                    break;
                }
                Some(Terminator::Switch { cases, default, .. }) => {
                    // Look through the switch to find its merge.
                    if let Some(merge) = self.find_switch_merge(cases, default.as_ref()) {
                        chain.push(merge);
                        current = merge;
                        continue;
                    }
                    break;
                }
                _ => break,
            }
        }
        chain
    }

    fn emit_switch(
        &self,
        func: &mut Function,
        scrutinee: ValueId,
        cases: &[kyokara_kir::block::SwitchCase],
        default: Option<&kyokara_kir::block::BranchTarget>,
        outer_stop: Option<BlockId>,
        emitted: &mut FxHashMap<BlockId, ()>,
    ) -> Result<(), CodegenError> {
        // Load the ADT tag from the scrutinee pointer.
        let scrutinee_ty = self.value_ty(scrutinee);
        let adt_layout = match scrutinee_ty {
            Ty::Adt { def, .. } => self
                .ctx
                .adt_layouts
                .get(def)
                .ok_or(CodegenError::MissingAdtDef)?,
            _ => return Err(CodegenError::UnsupportedType("switch on non-ADT".into())),
        };

        // Determine the merge block (all cases should jump to same merge).
        let merge_block_id = self.find_switch_merge(cases, default);

        // Always use Empty block type — results go via locals.
        let result_type = BlockType::Empty;

        // Build a tag -> case index map for br_table.
        let max_tag = adt_layout.tag_map.values().copied().max().unwrap_or(0);

        // We emit: block $merge { block $default { block $case_n { ... block $case_0 {
        //   br_table [0,1,...n, default]
        // } case_0_body } case_1_body ... } default_body } merge_body

        let num_cases = cases.len();
        let has_default = default.is_some();
        // Total nesting = cases + (1 if default) + (1 for merge)
        let total_blocks = num_cases + usize::from(has_default);

        // Open merge block.
        func.instruction(&Instruction::Block(result_type));

        // Open case/default blocks (innermost = lowest tag).
        for _ in 0..total_blocks {
            func.instruction(&Instruction::Block(BlockType::Empty));
        }

        // Emit br_table dispatch.
        // Load tag from scrutinee pointer.
        self.emit_get(func, scrutinee);
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));

        // Build the br_table targets array.
        // For each tag value 0..=max_tag, map to the correct block depth.
        let mut tag_to_case: FxHashMap<u32, usize> = FxHashMap::default();
        for (case_idx, case) in cases.iter().enumerate() {
            if let Some(&tag) = adt_layout.tag_map.get(&case.variant) {
                tag_to_case.insert(tag, case_idx);
            }
        }

        let default_depth = if has_default { num_cases } else { 0 };
        let targets: Vec<u32> = (0..=max_tag)
            .map(|tag| {
                if let Some(&case_idx) = tag_to_case.get(&tag) {
                    case_idx as u32
                } else {
                    default_depth as u32
                }
            })
            .collect();

        func.instruction(&Instruction::BrTable(targets.into(), default_depth as u32));

        // Emit case bodies (innermost block first = case 0).
        for (case_idx, case) in cases.iter().enumerate() {
            func.instruction(&Instruction::End); // close the block for this case

            // Depth to merge block from this case body:
            // remaining cases + (1 if default) = depth
            let depth_to_merge = (num_cases - 1 - case_idx) as u32 + u32::from(has_default);

            // Store block params.
            self.emit_block_param_stores(func, &case.target)?;

            // Emit the case block's body.
            let case_block = &self.kir_func.blocks[case.target.block];
            emitted.insert(case.target.block, ());

            for &vid in &case_block.body {
                self.emit_inst(func, vid)?;
            }

            // Handle terminator.
            if let Some(term) = &case_block.terminator {
                self.emit_switch_arm_terminator(
                    func,
                    term,
                    depth_to_merge,
                    merge_block_id,
                    emitted,
                )?;
            }
        }

        // Emit default body if present.
        if let Some(def_target) = default {
            func.instruction(&Instruction::End); // close default block

            self.emit_block_param_stores(func, def_target)?;

            let def_block = &self.kir_func.blocks[def_target.block];
            emitted.insert(def_target.block, ());

            for &vid in &def_block.body {
                self.emit_inst(func, vid)?;
            }

            if let Some(term) = &def_block.terminator {
                self.emit_switch_arm_terminator(func, term, 0, merge_block_id, emitted)?;
            }
        }

        // Close merge block.
        func.instruction(&Instruction::End);

        // Emit merge block body if it exists.
        if let Some(merge_id) = merge_block_id {
            self.emit_block_chain(func, merge_id, outer_stop, emitted)?;
        } else {
            // All arms diverge — emit unreachable.
            func.instruction(&Instruction::Unreachable);
        }

        Ok(())
    }

    /// Emit a terminator for a switch case/default arm.
    fn emit_switch_arm_terminator(
        &self,
        func: &mut Function,
        term: &Terminator,
        depth_to_merge: u32,
        switch_merge: Option<BlockId>,
        emitted: &mut FxHashMap<BlockId, ()>,
    ) -> Result<(), CodegenError> {
        match term {
            Terminator::Return(val) => {
                self.emit_get(func, *val);
                func.instruction(&Instruction::Return);
            }
            Terminator::Jump(target) => {
                self.emit_block_param_stores(func, target)?;
                func.instruction(&Instruction::Br(depth_to_merge));
            }
            Terminator::Unreachable => {
                func.instruction(&Instruction::Unreachable);
            }
            Terminator::Branch {
                condition,
                then_target,
                else_target,
            } => {
                // Nested if/else inside a switch arm. Emit the branch with the
                // switch merge as the stop point, then br out to the merge block.
                self.emit_branch(
                    func,
                    *condition,
                    then_target,
                    else_target,
                    switch_merge,
                    emitted,
                )?;
                func.instruction(&Instruction::Br(depth_to_merge));
            }
            Terminator::Switch {
                scrutinee,
                cases,
                default,
            } => {
                // Nested match inside a switch arm.
                self.emit_switch(
                    func,
                    *scrutinee,
                    cases,
                    default.as_ref(),
                    switch_merge,
                    emitted,
                )?;
                func.instruction(&Instruction::Br(depth_to_merge));
            }
        }
        Ok(())
    }

    /// Find the merge block for a switch: the block that all cases converge to.
    /// Follows full chains (through nested Branch/Switch) to find the common
    /// merge point, similar to `find_branch_merge_deep`.
    fn find_switch_merge(
        &self,
        cases: &[kyokara_kir::block::SwitchCase],
        default: Option<&kyokara_kir::block::BranchTarget>,
    ) -> Option<BlockId> {
        let mut chains: Vec<Vec<BlockId>> = Vec::new();

        for case in cases {
            chains.push(self.follow_chain(case.target.block));
        }
        if let Some(def) = default {
            chains.push(self.follow_chain(def.block));
        }

        let first_chain = chains.first()?;

        // Find the first block in the first chain that appears in all others.
        for block in first_chain {
            if chains[1..].iter().all(|c| c.contains(block)) {
                return Some(*block);
            }
        }

        None
    }

    // ── Block param stores ────────────────────────────────────────

    fn emit_block_param_stores(
        &self,
        func: &mut Function,
        target: &kyokara_kir::block::BranchTarget,
    ) -> Result<(), CodegenError> {
        let block = &self.kir_func.blocks[target.block];
        for (arg, param) in target.args.iter().zip(block.params.iter()) {
            self.emit_get(func, *arg);
            func.instruction(&Instruction::LocalSet(self.local_for(param.value)));
        }
        Ok(())
    }

    // ── Instruction emission ──────────────────────────────────────

    fn emit_inst(&self, func: &mut Function, vid: ValueId) -> Result<(), CodegenError> {
        let vdef = &self.kir_func.values[vid];
        let local_idx = self.local_for(vid);

        match &vdef.inst {
            Inst::FnParam { .. } | Inst::BlockParam { .. } => {
                // Already mapped to locals, no emission needed.
            }

            Inst::Const(c) => {
                self.emit_const(func, c);
                func.instruction(&Instruction::LocalSet(local_idx));
            }

            Inst::Binary { op, lhs, rhs } => {
                self.emit_binary(func, *op, *lhs, *rhs, &vdef.ty)?;
                func.instruction(&Instruction::LocalSet(local_idx));
            }

            Inst::Unary { op, operand } => {
                self.emit_unary(func, *op, *operand)?;
                func.instruction(&Instruction::LocalSet(local_idx));
            }

            Inst::Call { target, args } => {
                self.emit_call(func, target, args)?;
                func.instruction(&Instruction::LocalSet(local_idx));
            }

            Inst::AdtConstruct {
                type_def,
                variant,
                fields,
            } => {
                self.emit_adt_construct(func, *type_def, *variant, fields)?;
                func.instruction(&Instruction::LocalSet(local_idx));
            }

            Inst::AdtFieldGet { base, field_index } => {
                self.emit_adt_field_get(func, *base, *field_index, &vdef.ty)?;
                func.instruction(&Instruction::LocalSet(local_idx));
            }

            Inst::RecordCreate { fields } => {
                self.emit_record_create(func, fields)?;
                func.instruction(&Instruction::LocalSet(local_idx));
            }

            Inst::FieldGet { base, field } => {
                self.emit_field_get(func, *base, *field, &vdef.ty)?;
                func.instruction(&Instruction::LocalSet(local_idx));
            }

            Inst::Assert { condition, .. } => {
                self.emit_assert(func, *condition);
                // Assert produces Unit (i32 0).
                func.instruction(&Instruction::I32Const(0));
                func.instruction(&Instruction::LocalSet(local_idx));
            }

            Inst::RecordUpdate { .. } => {
                return Err(CodegenError::UnsupportedInstruction(
                    "RecordUpdate (deferred)".into(),
                ));
            }

            Inst::Hole { .. } => {
                // Typed holes trap at runtime.
                func.instruction(&Instruction::Unreachable);
            }

            Inst::FnRef { .. } => {
                return Err(CodegenError::UnsupportedInstruction(
                    "FnRef (closures deferred)".into(),
                ));
            }
        }

        Ok(())
    }

    // ── Constants ─────────────────────────────────────────────────

    fn emit_const(&self, func: &mut Function, c: &Constant) {
        match c {
            Constant::Int(v) => {
                func.instruction(&Instruction::I64Const(*v));
            }
            Constant::Float(v) => {
                func.instruction(&Instruction::F64Const(*v));
            }
            Constant::Bool(b) => {
                func.instruction(&Instruction::I32Const(i32::from(*b)));
            }
            Constant::Unit => {
                func.instruction(&Instruction::I32Const(0));
            }
            Constant::String(_) | Constant::Char(_) => {
                // Deferred: emit unreachable for now.
                func.instruction(&Instruction::Unreachable);
            }
        }
    }

    // ── Binary operations ─────────────────────────────────────────

    fn emit_binary(
        &self,
        func: &mut Function,
        op: BinaryOp,
        lhs: ValueId,
        rhs: ValueId,
        result_ty: &Ty,
    ) -> Result<(), CodegenError> {
        // Use lhs type, falling back to rhs type if lhs is Error.
        let lhs_ty = self.value_ty(lhs);
        let lhs_ty = if lhs_ty.is_poison() {
            self.value_ty(rhs)
        } else {
            lhs_ty
        };

        if matches!((op, lhs_ty), (BinaryOp::Mod, Ty::Float)) {
            // Float modulo is lowered as: lhs - trunc(lhs / rhs) * rhs
            // (Rust `%` semantics for floats).
            self.emit_get(func, lhs);
            self.emit_get(func, lhs);
            self.emit_get(func, rhs);
            func.instruction(&Instruction::F64Div);
            func.instruction(&Instruction::F64Trunc);
            self.emit_get(func, rhs);
            func.instruction(&Instruction::F64Mul);
            func.instruction(&Instruction::F64Sub);
            return Ok(());
        }

        self.emit_get(func, lhs);
        self.emit_get(func, rhs);

        match (op, lhs_ty) {
            (BinaryOp::Add, Ty::Int) => func.instruction(&Instruction::I64Add),
            (BinaryOp::Sub, Ty::Int) => func.instruction(&Instruction::I64Sub),
            (BinaryOp::Mul, Ty::Int) => func.instruction(&Instruction::I64Mul),
            (BinaryOp::Div, Ty::Int) => func.instruction(&Instruction::I64DivS),
            (BinaryOp::Mod, Ty::Int) => func.instruction(&Instruction::I64RemS),
            (BinaryOp::BitAnd, Ty::Int) => func.instruction(&Instruction::I64And),
            (BinaryOp::BitOr, Ty::Int) => func.instruction(&Instruction::I64Or),
            (BinaryOp::BitXor, Ty::Int) => func.instruction(&Instruction::I64Xor),
            (BinaryOp::Shl, Ty::Int) => func.instruction(&Instruction::I64Shl),
            (BinaryOp::Shr, Ty::Int) => func.instruction(&Instruction::I64ShrS),

            (BinaryOp::Add, Ty::Float) => func.instruction(&Instruction::F64Add),
            (BinaryOp::Sub, Ty::Float) => func.instruction(&Instruction::F64Sub),
            (BinaryOp::Mul, Ty::Float) => func.instruction(&Instruction::F64Mul),
            (BinaryOp::Div, Ty::Float) => func.instruction(&Instruction::F64Div),

            (BinaryOp::Eq, Ty::Int) => func.instruction(&Instruction::I64Eq),
            (BinaryOp::NotEq, Ty::Int) => func.instruction(&Instruction::I64Ne),
            (BinaryOp::Lt, Ty::Int) => func.instruction(&Instruction::I64LtS),
            (BinaryOp::Gt, Ty::Int) => func.instruction(&Instruction::I64GtS),
            (BinaryOp::LtEq, Ty::Int) => func.instruction(&Instruction::I64LeS),
            (BinaryOp::GtEq, Ty::Int) => func.instruction(&Instruction::I64GeS),

            (BinaryOp::Eq, Ty::Float) => func.instruction(&Instruction::F64Eq),
            (BinaryOp::NotEq, Ty::Float) => func.instruction(&Instruction::F64Ne),
            (BinaryOp::Lt, Ty::Float) => func.instruction(&Instruction::F64Lt),
            (BinaryOp::Gt, Ty::Float) => func.instruction(&Instruction::F64Gt),
            (BinaryOp::LtEq, Ty::Float) => func.instruction(&Instruction::F64Le),
            (BinaryOp::GtEq, Ty::Float) => func.instruction(&Instruction::F64Ge),

            (BinaryOp::Eq, Ty::Bool) => func.instruction(&Instruction::I32Eq),
            (BinaryOp::NotEq, Ty::Bool) => func.instruction(&Instruction::I32Ne),
            (BinaryOp::And, Ty::Bool) => func.instruction(&Instruction::I32And),
            (BinaryOp::Or, Ty::Bool) => func.instruction(&Instruction::I32Or),

            _ => {
                return Err(CodegenError::UnsupportedInstruction(format!(
                    "binary {op:?} on {lhs_ty:?}"
                )));
            }
        };

        // Comparison ops produce i32 in WASM but we might need i64 wrapping.
        // Actually comparisons always produce Bool (i32), so no wrapping needed.
        let _ = result_ty;

        Ok(())
    }

    // ── Unary operations ──────────────────────────────────────────

    fn emit_unary(
        &self,
        func: &mut Function,
        op: UnaryOp,
        operand: ValueId,
    ) -> Result<(), CodegenError> {
        let operand_ty = self.value_ty(operand);

        match (op, operand_ty) {
            (UnaryOp::Neg, Ty::Int) => {
                func.instruction(&Instruction::I64Const(0));
                self.emit_get(func, operand);
                func.instruction(&Instruction::I64Sub);
            }
            (UnaryOp::Neg, Ty::Float) => {
                self.emit_get(func, operand);
                func.instruction(&Instruction::F64Neg);
            }
            (UnaryOp::Not, Ty::Bool) => {
                self.emit_get(func, operand);
                func.instruction(&Instruction::I32Eqz);
            }
            (UnaryOp::BitNot, Ty::Int) => {
                self.emit_get(func, operand);
                func.instruction(&Instruction::I64Const(-1));
                func.instruction(&Instruction::I64Xor);
            }
            _ => {
                return Err(CodegenError::UnsupportedInstruction(format!(
                    "unary {op:?} on {operand_ty:?}"
                )));
            }
        }

        Ok(())
    }

    // ── Function calls ────────────────────────────────────────────

    fn emit_call(
        &self,
        func: &mut Function,
        target: &CallTarget,
        args: &[ValueId],
    ) -> Result<(), CodegenError> {
        match target {
            CallTarget::Direct(name) => {
                // Push args.
                for &arg in args {
                    self.emit_get(func, arg);
                }
                let fn_idx = self.ctx.fn_name_map.get(name).ok_or_else(|| {
                    CodegenError::MissingFunction(name.resolve(self.ctx.interner).to_owned())
                })?;
                func.instruction(&Instruction::Call(*fn_idx));
                Ok(())
            }
            CallTarget::Indirect(_) => Err(CodegenError::UnsupportedInstruction(
                "indirect calls (closures deferred)".into(),
            )),
            CallTarget::Intrinsic(name) => Err(CodegenError::UnsupportedInstruction(format!(
                "intrinsic {name} (deferred)"
            ))),
        }
    }

    // ── ADT construction ──────────────────────────────────────────

    fn emit_adt_construct(
        &self,
        func: &mut Function,
        type_def: kyokara_hir_def::item_tree::TypeItemIdx,
        variant: Name,
        fields: &[ValueId],
    ) -> Result<(), CodegenError> {
        let layout = self
            .ctx
            .adt_layouts
            .get(&type_def)
            .ok_or(CodegenError::MissingAdtDef)?;

        let tag = layout.tag_map.get(&variant).ok_or_else(|| {
            CodegenError::UnknownVariant(variant.resolve(self.ctx.interner).to_owned())
        })?;

        // Allocate memory: call $alloc(size).
        // Drop the alloc return value immediately — we re-derive the pointer
        // from $heap_ptr each time we need it (ptr = heap_ptr - size).
        // This keeps the value stack clean so ADT construction works inside
        // nested blocks (if/else arms, match arms, etc.).
        func.instruction(&Instruction::I32Const(layout.size as i32));
        func.instruction(&Instruction::Call(self.ctx.alloc_fn_index));
        func.instruction(&Instruction::Drop);

        // ptr = heap_ptr - size
        let derive_ptr = |func: &mut Function, size: u32| {
            func.instruction(&Instruction::GlobalGet(0));
            func.instruction(&Instruction::I32Const(size as i32));
            func.instruction(&Instruction::I32Sub);
        };

        // Store tag.
        derive_ptr(func, layout.size);
        func.instruction(&Instruction::I32Const(*tag as i32));
        func.instruction(&Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));

        // Store fields.
        for (i, &field_vid) in fields.iter().enumerate() {
            let offset = AdtLayout::field_offset(i as u32);
            derive_ptr(func, layout.size);
            let field_ty = self.value_ty(field_vid);
            self.emit_get_as_i64(func, field_vid, field_ty);
            func.instruction(&Instruction::I64Store(MemArg {
                offset: u64::from(offset),
                align: 3,
                memory_index: 0,
            }));
        }

        // Leave ptr on stack for the caller's local.set.
        derive_ptr(func, layout.size);

        Ok(())
    }

    // ── ADT field get ─────────────────────────────────────────────

    fn emit_adt_field_get(
        &self,
        func: &mut Function,
        base: ValueId,
        field_index: u32,
        result_ty: &Ty,
    ) -> Result<(), CodegenError> {
        let offset = AdtLayout::field_offset(field_index);

        self.emit_get(func, base);
        func.instruction(&Instruction::I64Load(MemArg {
            offset: u64::from(offset),
            align: 3,
            memory_index: 0,
        }));

        // If the result type is i32 (Bool/Unit/Ptr), wrap down from i64.
        if is_i32_type(result_ty) {
            func.instruction(&Instruction::I32WrapI64);
        }

        Ok(())
    }

    // ── Record construction ───────────────────────────────────────

    fn emit_record_create(
        &self,
        func: &mut Function,
        fields: &[(Name, ValueId)],
    ) -> Result<(), CodegenError> {
        // Sort fields by name for deterministic layout.
        let mut sorted_fields: Vec<(Name, ValueId)> = fields.to_vec();
        sorted_fields.sort_by(|a, b| {
            let a_str = a.0.resolve(self.ctx.interner);
            let b_str = b.0.resolve(self.ctx.interner);
            a_str.cmp(b_str)
        });

        let field_count = sorted_fields.len() as u32;
        let size = layout::record_size(field_count);

        // Allocate memory.
        func.instruction(&Instruction::I32Const(size as i32));
        func.instruction(&Instruction::Call(self.ctx.alloc_fn_index));
        func.instruction(&Instruction::Drop);

        let derive_ptr = |func: &mut Function, size: u32| {
            func.instruction(&Instruction::GlobalGet(0));
            func.instruction(&Instruction::I32Const(size as i32));
            func.instruction(&Instruction::I32Sub);
        };

        // Store fields.
        for (i, (_, vid)) in sorted_fields.iter().enumerate() {
            let offset = layout::record_field_offset(i as u32);
            derive_ptr(func, size);
            let field_ty = self.value_ty(*vid);
            self.emit_get_as_i64(func, *vid, field_ty);
            func.instruction(&Instruction::I64Store(MemArg {
                offset: u64::from(offset),
                align: 3,
                memory_index: 0,
            }));
        }

        // Leave ptr on stack.
        derive_ptr(func, size);

        Ok(())
    }

    // ── Record field get ──────────────────────────────────────────

    fn emit_field_get(
        &self,
        func: &mut Function,
        base: ValueId,
        field: Name,
        result_ty: &Ty,
    ) -> Result<(), CodegenError> {
        // Resolve field name to index based on the base type's sorted fields.
        let base_ty = self.value_ty(base);
        let field_index = self.resolve_record_field_index(base_ty, field)?;
        let offset = layout::record_field_offset(field_index);

        self.emit_get(func, base);
        func.instruction(&Instruction::I64Load(MemArg {
            offset: u64::from(offset),
            align: 3,
            memory_index: 0,
        }));

        if is_i32_type(result_ty) {
            func.instruction(&Instruction::I32WrapI64);
        }

        Ok(())
    }

    fn resolve_record_field_index(&self, base_ty: &Ty, field: Name) -> Result<u32, CodegenError> {
        let fields = match base_ty {
            Ty::Record { fields } => fields,
            _ => {
                return Err(CodegenError::UnsupportedType(
                    "field access on non-record".into(),
                ));
            }
        };

        // Sort field names to get deterministic index.
        let mut names: Vec<Name> = fields.iter().map(|(n, _)| *n).collect();
        names.sort_by(|a, b| {
            let a_str = a.resolve(self.ctx.interner);
            let b_str = b.resolve(self.ctx.interner);
            a_str.cmp(b_str)
        });

        names
            .iter()
            .position(|n| *n == field)
            .map(|i| i as u32)
            .ok_or_else(|| {
                CodegenError::UnsupportedType(format!(
                    "field {} not found in record",
                    field.resolve(self.ctx.interner)
                ))
            })
    }

    // ── Assert ────────────────────────────────────────────────────

    fn emit_assert(&self, func: &mut Function, condition: ValueId) {
        self.emit_get(func, condition);
        func.instruction(&Instruction::I32Eqz);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::Unreachable);
        func.instruction(&Instruction::End);
    }

    // ── Helpers ───────────────────────────────────────────────────

    /// Emit `local.get` for a value.
    fn emit_get(&self, func: &mut Function, vid: ValueId) {
        func.instruction(&Instruction::LocalGet(self.local_for(vid)));
    }

    /// Emit a value as i64 for storing in linear memory.
    /// If the value is i32 (Bool/Unit/Ptr), extend to i64.
    fn emit_get_as_i64(&self, func: &mut Function, vid: ValueId, ty: &Ty) {
        self.emit_get(func, vid);
        match ty {
            Ty::Int => {} // already i64
            Ty::Float => {
                // reinterpret f64 bits as i64 for uniform storage
                func.instruction(&Instruction::I64ReinterpretF64);
            }
            _ if is_i32_type(ty) => {
                func.instruction(&Instruction::I64ExtendI32U);
            }
            _ => {} // best effort
        }
    }
}
