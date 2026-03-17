//! Per-function WASM code generation.

use std::collections::VecDeque;

use kyokara_hir_def::name::Name;
use kyokara_hir_def::{
    expr::{BinaryOp, UnaryOp},
    item_tree::TypeDefKind,
    type_ref::TypeRef,
};
use kyokara_hir_ty::ty::{Ty, resolve_builtin};
use kyokara_kir::block::{BlockId, Terminator};
use kyokara_kir::function::KirFunction;
use kyokara_kir::inst::{CallTarget, Constant, Inst};
use kyokara_kir::value::ValueId;
use rustc_hash::{FxHashMap, FxHashSet};
use wasm_encoder::{BlockType, Function, Instruction, MemArg, ValType};

use crate::error::CodegenError;
use crate::wasm::control::reverse_postorder;
use crate::wasm::layout::{self, AdtLayout};
use crate::wasm::ty::{is_i32_type, ty_to_valtype};
use crate::wasm::{FnTypeKey, ModuleCtx, closure_capture_offset, closure_object_size};

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
    /// Scratch i64 local for checked integer codegen.
    scratch_i64: u32,
    scratch_i64_2: u32,
    scratch_i64_3: u32,
    scratch_i64_4: u32,
    /// Scratch f64 local for float runtime helpers.
    scratch_f64: u32,
    /// Scratch i32 local for checked integer codegen.
    scratch_i32: u32,
    /// Additional scratch i32 locals for runtime helpers.
    scratch_i32_2: u32,
    scratch_i32_3: u32,
    scratch_i32_4: u32,
    scratch_i32_5: u32,
    scratch_i32_6: u32,
    scratch_i32_7: u32,
    scratch_i32_8: u32,
    scratch_i32_9: u32,
    scratch_i32_10: u32,
}

impl<'a> FuncCodegen<'a> {
    pub fn new(kir_func: &'a KirFunction, ctx: &'a ModuleCtx<'a>) -> Self {
        Self {
            kir_func,
            ctx,
            local_map: FxHashMap::default(),
            local_types: Vec::new(),
            next_local: kir_func.params.len() as u32,
            scratch_i64: 0,
            scratch_i64_2: 0,
            scratch_i64_3: 0,
            scratch_i64_4: 0,
            scratch_f64: 0,
            scratch_i32: 0,
            scratch_i32_2: 0,
            scratch_i32_3: 0,
            scratch_i32_4: 0,
            scratch_i32_5: 0,
            scratch_i32_6: 0,
            scratch_i32_7: 0,
            scratch_i32_8: 0,
            scratch_i32_9: 0,
            scratch_i32_10: 0,
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

        // Scratch locals for checked integer operations.
        self.scratch_i64 = self.next_local;
        self.next_local += 1;
        self.local_types.push(ValType::I64);

        self.scratch_i64_2 = self.next_local;
        self.next_local += 1;
        self.local_types.push(ValType::I64);

        self.scratch_i64_3 = self.next_local;
        self.next_local += 1;
        self.local_types.push(ValType::I64);

        self.scratch_i64_4 = self.next_local;
        self.next_local += 1;
        self.local_types.push(ValType::I64);

        self.scratch_f64 = self.next_local;
        self.next_local += 1;
        self.local_types.push(ValType::F64);

        self.scratch_i32 = self.next_local;
        self.next_local += 1;
        self.local_types.push(ValType::I32);

        self.scratch_i32_2 = self.next_local;
        self.next_local += 1;
        self.local_types.push(ValType::I32);

        self.scratch_i32_3 = self.next_local;
        self.next_local += 1;
        self.local_types.push(ValType::I32);

        self.scratch_i32_4 = self.next_local;
        self.next_local += 1;
        self.local_types.push(ValType::I32);

        self.scratch_i32_5 = self.next_local;
        self.next_local += 1;
        self.local_types.push(ValType::I32);

        self.scratch_i32_6 = self.next_local;
        self.next_local += 1;
        self.local_types.push(ValType::I32);

        self.scratch_i32_7 = self.next_local;
        self.next_local += 1;
        self.local_types.push(ValType::I32);

        self.scratch_i32_8 = self.next_local;
        self.next_local += 1;
        self.local_types.push(ValType::I32);

        self.scratch_i32_9 = self.next_local;
        self.next_local += 1;
        self.local_types.push(ValType::I32);

        self.scratch_i32_10 = self.next_local;
        self.next_local += 1;
        self.local_types.push(ValType::I32);

        Ok(())
    }

    /// Infer WASM type from the instruction when the HIR type is Error.
    fn infer_wasm_type_from_inst(&self, inst: &Inst) -> ValType {
        match inst {
            Inst::Const(Constant::Int(_)) => ValType::I64,
            Inst::Const(Constant::Float(_)) => ValType::F64,
            Inst::Const(Constant::Bool(_))
            | Inst::Const(Constant::Unit)
            | Inst::Const(Constant::Char(_)) => ValType::I32,
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
            Inst::FnRef { .. } | Inst::ClosureCreate { .. } => ValType::I32,
            _ => ValType::I32, // default
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
        let term = block
            .terminator
            .as_ref()
            .ok_or_else(|| CodegenError::MissingTerminator("block has no terminator".into()))?;

        if let Terminator::Branch {
            condition,
            then_target,
            else_target,
        } = term
            && let Some((body_target, exit_target)) =
                self.classify_loop_branch(block_id, then_target, else_target)
        {
            self.emit_loop(
                func,
                block_id,
                *condition,
                body_target,
                exit_target,
                stop_at,
                emitted,
            )?;
            return Ok(());
        }

        for &vid in &block.body {
            self.emit_inst(func, vid)?;
        }

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

    fn classify_loop_branch<'b>(
        &self,
        header: BlockId,
        then_target: &'b kyokara_kir::block::BranchTarget,
        else_target: &'b kyokara_kir::block::BranchTarget,
    ) -> Option<(
        &'b kyokara_kir::block::BranchTarget,
        &'b kyokara_kir::block::BranchTarget,
    )> {
        if self.kir_func.blocks[header].params.is_empty() {
            return None;
        }
        let then_reaches_header = self
            .reachable_block_distances(then_target.block)
            .contains_key(&header);
        let else_reaches_header = self
            .reachable_block_distances(else_target.block)
            .contains_key(&header);
        match (then_reaches_header, else_reaches_header) {
            (true, false) => Some((then_target, else_target)),
            (false, true) => Some((else_target, then_target)),
            _ => None,
        }
    }

    fn emit_loop(
        &self,
        func: &mut Function,
        header: BlockId,
        condition: ValueId,
        body_target: &kyokara_kir::block::BranchTarget,
        exit_target: &kyokara_kir::block::BranchTarget,
        outer_stop: Option<BlockId>,
        emitted: &mut FxHashMap<BlockId, ()>,
    ) -> Result<(), CodegenError> {
        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));

        let header_block = &self.kir_func.blocks[header];
        for &vid in &header_block.body {
            self.emit_inst(func, vid)?;
        }

        self.emit_get(func, condition);
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_block_param_stores(func, body_target)?;

        let mut loop_emitted = emitted.clone();
        self.emit_loop_block_chain(
            func,
            body_target.block,
            header,
            exit_target.block,
            1,
            2,
            &mut loop_emitted,
        )?;
        func.instruction(&Instruction::Else);
        self.emit_block_param_stores(func, exit_target)?;
        func.instruction(&Instruction::Br(2));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        emitted.extend(loop_emitted);
        self.emit_block_chain(func, exit_target.block, outer_stop, emitted)?;
        Ok(())
    }

    fn emit_loop_block_chain(
        &self,
        func: &mut Function,
        block_id: BlockId,
        header: BlockId,
        exit: BlockId,
        continue_depth: u32,
        break_depth: u32,
        emitted: &mut FxHashMap<BlockId, ()>,
    ) -> Result<(), CodegenError> {
        if block_id == header {
            func.instruction(&Instruction::Br(continue_depth));
            return Ok(());
        }
        if block_id == exit {
            func.instruction(&Instruction::Br(break_depth));
            return Ok(());
        }
        if emitted.contains_key(&block_id) {
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
                self.emit_loop_block_chain(
                    func,
                    target.block,
                    header,
                    exit,
                    continue_depth,
                    break_depth,
                    emitted,
                )?;
            }
            Terminator::Branch {
                condition,
                then_target,
                else_target,
            } => {
                self.emit_loop_branch(
                    func,
                    *condition,
                    then_target,
                    else_target,
                    header,
                    exit,
                    continue_depth,
                    break_depth,
                    emitted,
                )?;
            }
            Terminator::Switch { .. } => {
                if let Terminator::Switch {
                    scrutinee,
                    cases,
                    default,
                } = term
                {
                    self.emit_loop_switch(
                        func,
                        *scrutinee,
                        cases,
                        default.as_ref(),
                        block_id,
                        header,
                        exit,
                        continue_depth,
                        break_depth,
                        emitted,
                    )?;
                }
            }
        }

        Ok(())
    }

    fn emit_loop_branch(
        &self,
        func: &mut Function,
        condition: ValueId,
        then_target: &kyokara_kir::block::BranchTarget,
        else_target: &kyokara_kir::block::BranchTarget,
        header: BlockId,
        exit: BlockId,
        continue_depth: u32,
        break_depth: u32,
        emitted: &mut FxHashMap<BlockId, ()>,
    ) -> Result<(), CodegenError> {
        let merge_id = self.find_loop_branch_merge(then_target, else_target, header, exit);

        self.emit_get(func, condition);
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_loop_branch_arm(
            func,
            then_target,
            merge_id,
            header,
            exit,
            continue_depth + 1,
            break_depth + 1,
            emitted,
        )?;
        func.instruction(&Instruction::Else);
        self.emit_loop_branch_arm(
            func,
            else_target,
            merge_id,
            header,
            exit,
            continue_depth + 1,
            break_depth + 1,
            emitted,
        )?;
        func.instruction(&Instruction::End);

        if let Some(merge_id) = merge_id {
            self.emit_loop_block_chain(
                func,
                merge_id,
                header,
                exit,
                continue_depth,
                break_depth,
                emitted,
            )?;
        }

        Ok(())
    }

    fn find_loop_branch_merge(
        &self,
        then_target: &kyokara_kir::block::BranchTarget,
        else_target: &kyokara_kir::block::BranchTarget,
        header: BlockId,
        exit: BlockId,
    ) -> Option<BlockId> {
        let then_reachable = self.reachable_block_distances(then_target.block);
        let else_reachable = self.reachable_block_distances(else_target.block);

        then_reachable
            .iter()
            .filter_map(|(block, then_dist)| {
                if *block == header || *block == exit {
                    return None;
                }
                else_reachable.get(block).map(|else_dist| {
                    (
                        *block,
                        *then_dist + *else_dist,
                        (*then_dist).max(*else_dist),
                    )
                })
            })
            .min_by_key(|(block, total, max_side)| (*total, *max_side, block.into_raw().into_u32()))
            .map(|(block, _, _)| block)
    }

    fn find_loop_switch_merge(
        &self,
        cases: &[kyokara_kir::block::SwitchCase],
        default: Option<&kyokara_kir::block::BranchTarget>,
        current_block: BlockId,
        header: BlockId,
        exit: BlockId,
    ) -> Option<BlockId> {
        let mut excluded = FxHashSet::default();
        excluded.insert(current_block);
        excluded.insert(header);
        excluded.insert(exit);
        for case in cases {
            excluded.insert(case.target.block);
        }
        if let Some(default) = default {
            excluded.insert(default.block);
        }

        let mut reachable_sets: Vec<FxHashMap<BlockId, usize>> = cases
            .iter()
            .filter(|case| !self.loop_switch_target_exits_directly(case.target.block, header, exit))
            .map(|case| self.reachable_block_distances(case.target.block))
            .collect();
        if let Some(default) = default {
            if !self.loop_switch_target_exits_directly(default.block, header, exit) {
                reachable_sets.push(self.reachable_block_distances(default.block));
            }
        }

        let first = reachable_sets.first()?;
        first
            .iter()
            .filter_map(|(block, first_dist)| {
                if excluded.contains(block) {
                    return None;
                }
                let mut total = *first_dist;
                let mut max_side = *first_dist;
                for reachable in reachable_sets.iter().skip(1) {
                    let dist = *reachable.get(block)?;
                    total += dist;
                    max_side = max_side.max(dist);
                }
                Some((*block, total, max_side))
            })
            .min_by_key(|(block, total, max_side)| (*total, *max_side, block.into_raw().into_u32()))
            .map(|(block, _, _)| block)
    }

    fn loop_switch_target_exits_directly(
        &self,
        block_id: BlockId,
        header: BlockId,
        exit: BlockId,
    ) -> bool {
        let Some(term) = self.kir_func.blocks[block_id].terminator.as_ref() else {
            return false;
        };
        match term {
            Terminator::Jump(target) => target.block == header || target.block == exit,
            Terminator::Return(_) | Terminator::Unreachable => true,
            _ => false,
        }
    }

    fn emit_loop_branch_arm(
        &self,
        func: &mut Function,
        target: &kyokara_kir::block::BranchTarget,
        merge_id: Option<BlockId>,
        header: BlockId,
        exit: BlockId,
        continue_depth: u32,
        break_depth: u32,
        emitted: &mut FxHashMap<BlockId, ()>,
    ) -> Result<(), CodegenError> {
        if Some(target.block) == merge_id {
            self.emit_block_param_stores(func, target)?;
            return Ok(());
        }

        self.emit_block_param_stores(func, target)?;
        self.emit_loop_block_chain(
            func,
            target.block,
            header,
            exit,
            continue_depth,
            break_depth,
            emitted,
        )
    }

    fn emit_loop_switch(
        &self,
        func: &mut Function,
        scrutinee: ValueId,
        cases: &[kyokara_kir::block::SwitchCase],
        default: Option<&kyokara_kir::block::BranchTarget>,
        current_block: BlockId,
        header: BlockId,
        exit: BlockId,
        continue_depth: u32,
        break_depth: u32,
        emitted: &mut FxHashMap<BlockId, ()>,
    ) -> Result<(), CodegenError> {
        let scrutinee_ty = self.value_ty(scrutinee);
        let adt_layout = match scrutinee_ty {
            Ty::Adt { def, .. } => self
                .ctx
                .adt_layouts
                .get(def)
                .ok_or(CodegenError::MissingAdtDef)?,
            _ => return Err(CodegenError::UnsupportedType("switch on non-ADT".into())),
        };

        let merge_block_id =
            self.find_loop_switch_merge(cases, default, current_block, header, exit);
        let max_tag = adt_layout.tag_map.values().copied().max().unwrap_or(0);
        let num_cases = cases.len();
        let has_default = default.is_some();
        let total_blocks = num_cases + usize::from(has_default);

        func.instruction(&Instruction::Block(BlockType::Empty));
        for _ in 0..total_blocks {
            func.instruction(&Instruction::Block(BlockType::Empty));
        }

        self.emit_get(func, scrutinee);
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));

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

        for (case_idx, case) in cases.iter().enumerate() {
            func.instruction(&Instruction::End);
            let depth_to_merge = (num_cases - 1 - case_idx) as u32 + u32::from(has_default);
            self.emit_block_param_stores(func, &case.target)?;

            let case_block = &self.kir_func.blocks[case.target.block];
            emitted.insert(case.target.block, ());
            for &vid in &case_block.body {
                self.emit_inst(func, vid)?;
            }

            if let Some(term) = &case_block.terminator {
                self.emit_loop_switch_arm_terminator(
                    func,
                    term,
                    depth_to_merge,
                    merge_block_id,
                    header,
                    exit,
                    continue_depth,
                    break_depth,
                    emitted,
                )?;
            }
        }

        if let Some(def_target) = default {
            func.instruction(&Instruction::End);
            self.emit_block_param_stores(func, def_target)?;

            let def_block = &self.kir_func.blocks[def_target.block];
            emitted.insert(def_target.block, ());
            for &vid in &def_block.body {
                self.emit_inst(func, vid)?;
            }

            if let Some(term) = &def_block.terminator {
                self.emit_loop_switch_arm_terminator(
                    func,
                    term,
                    0,
                    merge_block_id,
                    header,
                    exit,
                    continue_depth,
                    break_depth,
                    emitted,
                )?;
            }
        }

        func.instruction(&Instruction::End);

        if let Some(merge_id) = merge_block_id {
            self.emit_loop_block_chain(
                func,
                merge_id,
                header,
                exit,
                continue_depth,
                break_depth,
                emitted,
            )?;
        } else {
            func.instruction(&Instruction::Unreachable);
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

        then_chain.iter().find(|t| else_chain.contains(t)).copied()
    }

    fn reachable_block_distances(&self, start: BlockId) -> FxHashMap<BlockId, usize> {
        let mut distances = FxHashMap::default();
        let mut queue = VecDeque::new();
        distances.insert(start, 0);
        queue.push_back(start);

        while let Some(block_id) = queue.pop_front() {
            let distance = distances[&block_id];
            for succ in self.successor_blocks(block_id) {
                if distances.insert(succ, distance + 1).is_none() {
                    queue.push_back(succ);
                }
            }
        }

        distances
    }

    fn follow_chain(&self, start: BlockId) -> Vec<BlockId> {
        let mut chain = Vec::new();
        let mut current = start;
        let mut visited = FxHashSet::default();
        loop {
            if !visited.insert(current) {
                break;
            }
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
                    if let Some(merge) = self.find_branch_merge_deep(then_target, else_target) {
                        chain.push(merge);
                        current = merge;
                        continue;
                    }
                    break;
                }
                Some(Terminator::Switch { cases, default, .. }) => {
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

    fn successor_blocks(&self, block_id: BlockId) -> Vec<BlockId> {
        let Some(term) = self.kir_func.blocks[block_id].terminator.as_ref() else {
            return Vec::new();
        };
        match term {
            Terminator::Return(_) | Terminator::Unreachable => Vec::new(),
            Terminator::Jump(target) => vec![target.block],
            Terminator::Branch {
                then_target,
                else_target,
                ..
            } => vec![then_target.block, else_target.block],
            Terminator::Switch { cases, default, .. } => {
                let mut succs: Vec<_> = cases.iter().map(|case| case.target.block).collect();
                if let Some(default) = default {
                    succs.push(default.block);
                }
                succs
            }
        }
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

    fn emit_loop_switch_arm_terminator(
        &self,
        func: &mut Function,
        term: &Terminator,
        depth_to_merge: u32,
        switch_merge: Option<BlockId>,
        header: BlockId,
        exit: BlockId,
        continue_depth: u32,
        break_depth: u32,
        emitted: &mut FxHashMap<BlockId, ()>,
    ) -> Result<(), CodegenError> {
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
                if Some(target.block) == switch_merge {
                    func.instruction(&Instruction::Br(depth_to_merge));
                } else if target.block == header {
                    func.instruction(&Instruction::Br(continue_depth + depth_to_merge + 1));
                } else if target.block == exit {
                    func.instruction(&Instruction::Br(break_depth + depth_to_merge + 1));
                } else {
                    self.emit_block_chain(func, target.block, switch_merge, emitted)?;
                    func.instruction(&Instruction::Br(depth_to_merge));
                }
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

        let _ = (continue_depth, break_depth);
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
                self.emit_const(func, c)?;
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
                self.emit_call(func, target, args, &vdef.ty)?;
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

            Inst::RecordUpdate { base, updates } => {
                self.emit_record_update(func, *base, updates, &vdef.ty)?;
                func.instruction(&Instruction::LocalSet(local_idx));
            }

            Inst::Hole { .. } => {
                // Typed holes trap at runtime.
                func.instruction(&Instruction::Unreachable);
            }

            Inst::FnRef { name } => {
                self.emit_closure_create(func, *name, &[])?;
                func.instruction(&Instruction::LocalSet(local_idx));
            }

            Inst::ClosureCreate { name, captures } => {
                self.emit_closure_create(func, *name, captures)?;
                func.instruction(&Instruction::LocalSet(local_idx));
            }
        }

        Ok(())
    }

    // ── Constants ─────────────────────────────────────────────────

    fn emit_const(&self, func: &mut Function, c: &Constant) -> Result<(), CodegenError> {
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
            Constant::String(s) => {
                self.emit_string_const(func, s);
            }
            Constant::Char(c) => {
                func.instruction(&Instruction::I32Const(*c as i32));
            }
        }
        Ok(())
    }

    fn emit_string_const(&self, func: &mut Function, s: &str) {
        let bytes = s.as_bytes();
        let byte_len = bytes.len() as i32;
        let char_len = s.chars().count() as i32;
        let total_size = 8_i32
            .checked_add(byte_len)
            .expect("string constant size should fit in i32");

        func.instruction(&Instruction::I32Const(total_size));
        func.instruction(&Instruction::Call(self.ctx.alloc_fn_index));
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Const(byte_len));
        func.instruction(&Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Const(char_len));
        func.instruction(&Instruction::I32Store(MemArg {
            offset: 4,
            align: 2,
            memory_index: 0,
        }));

        for (idx, byte) in bytes.iter().enumerate() {
            func.instruction(&Instruction::LocalGet(self.scratch_i32));
            func.instruction(&Instruction::I32Const(i32::from(*byte)));
            func.instruction(&Instruction::I32Store8(MemArg {
                offset: 8 + idx as u64,
                align: 0,
                memory_index: 0,
            }));
        }

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
    }

    fn emit_trap_if_true(&self, func: &mut Function) {
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::Unreachable);
        func.instruction(&Instruction::End);
    }

    fn emit_checked_int_add(&self, func: &mut Function, lhs: ValueId, rhs: ValueId) {
        self.emit_get(func, lhs);
        self.emit_get(func, rhs);
        func.instruction(&Instruction::I64Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i64));

        // Overflow iff ((lhs ^ result) & (rhs ^ result)) < 0.
        self.emit_get(func, lhs);
        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::I64Xor);
        self.emit_get(func, rhs);
        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::I64Xor);
        func.instruction(&Instruction::I64And);
        func.instruction(&Instruction::I64Const(0));
        func.instruction(&Instruction::I64LtS);
        self.emit_trap_if_true(func);

        func.instruction(&Instruction::LocalGet(self.scratch_i64));
    }

    fn emit_checked_int_sub(&self, func: &mut Function, lhs: ValueId, rhs: ValueId) {
        self.emit_get(func, lhs);
        self.emit_get(func, rhs);
        func.instruction(&Instruction::I64Sub);
        func.instruction(&Instruction::LocalSet(self.scratch_i64));

        // Overflow iff ((lhs ^ rhs) & (lhs ^ result)) < 0.
        self.emit_get(func, lhs);
        self.emit_get(func, rhs);
        func.instruction(&Instruction::I64Xor);
        self.emit_get(func, lhs);
        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::I64Xor);
        func.instruction(&Instruction::I64And);
        func.instruction(&Instruction::I64Const(0));
        func.instruction(&Instruction::I64LtS);
        self.emit_trap_if_true(func);

        func.instruction(&Instruction::LocalGet(self.scratch_i64));
    }

    fn emit_checked_int_mul(&self, func: &mut Function, lhs: ValueId, rhs: ValueId) {
        self.emit_get(func, lhs);
        self.emit_get(func, rhs);
        func.instruction(&Instruction::I64Mul);
        func.instruction(&Instruction::LocalSet(self.scratch_i64));

        // Overflow check:
        // if lhs != 0 && (result / lhs) != rhs => trap
        self.emit_get(func, lhs);
        func.instruction(&Instruction::I64Eqz);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::Else);
        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        self.emit_get(func, lhs);
        func.instruction(&Instruction::I64DivS);
        self.emit_get(func, rhs);
        func.instruction(&Instruction::I64Ne);
        self.emit_trap_if_true(func);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i64));
    }

    fn emit_checked_int_mul_locals(
        &self,
        func: &mut Function,
        lhs_local: u32,
        rhs_local: u32,
        temp_local: u32,
    ) {
        func.instruction(&Instruction::LocalGet(lhs_local));
        func.instruction(&Instruction::LocalGet(rhs_local));
        func.instruction(&Instruction::I64Mul);
        func.instruction(&Instruction::LocalSet(temp_local));

        func.instruction(&Instruction::LocalGet(lhs_local));
        func.instruction(&Instruction::I64Eqz);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::Else);
        func.instruction(&Instruction::LocalGet(temp_local));
        func.instruction(&Instruction::LocalGet(lhs_local));
        func.instruction(&Instruction::I64DivS);
        func.instruction(&Instruction::LocalGet(rhs_local));
        func.instruction(&Instruction::I64Ne);
        self.emit_trap_if_true(func);
        func.instruction(&Instruction::End);
    }

    fn emit_checked_int_mod(&self, func: &mut Function, lhs: ValueId, rhs: ValueId) {
        // Match interpreter semantics: MIN % -1 is overflow.
        self.emit_get(func, lhs);
        func.instruction(&Instruction::I64Const(i64::MIN));
        func.instruction(&Instruction::I64Eq);
        self.emit_get(func, rhs);
        func.instruction(&Instruction::I64Const(-1));
        func.instruction(&Instruction::I64Eq);
        func.instruction(&Instruction::I32And);
        self.emit_trap_if_true(func);

        self.emit_get(func, lhs);
        self.emit_get(func, rhs);
        func.instruction(&Instruction::I64RemS);
    }

    fn emit_checked_int_shift(
        &self,
        func: &mut Function,
        lhs: ValueId,
        rhs: ValueId,
        op: BinaryOp,
    ) {
        // Shift amount must be within 0..63 inclusive.
        self.emit_get(func, rhs);
        func.instruction(&Instruction::I64Const(0));
        func.instruction(&Instruction::I64LtS);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        self.emit_get(func, rhs);
        func.instruction(&Instruction::I64Const(64));
        func.instruction(&Instruction::I64GeS);
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Or);
        self.emit_trap_if_true(func);

        self.emit_get(func, lhs);
        self.emit_get(func, rhs);
        match op {
            BinaryOp::Shl => func.instruction(&Instruction::I64Shl),
            BinaryOp::Shr => func.instruction(&Instruction::I64ShrS),
            _ => unreachable!("emit_checked_int_shift called with non-shift op"),
        };
    }

    fn emit_string_compare(&self, func: &mut Function, lhs: ValueId, rhs: ValueId) {
        self.emit_get(func, lhs);
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        self.emit_get(func, rhs);
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2));

        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        self.emit_get(func, lhs);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        self.emit_get(func, rhs);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32LtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(-1));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));
        func.instruction(&Instruction::Br(2));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        self.emit_get(func, rhs);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32GtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));
        func.instruction(&Instruction::Br(2));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3));
        func.instruction(&Instruction::Br(0));

        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Eqz);
        func.instruction(&Instruction::If(BlockType::Empty));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32LtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(-1));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));
        func.instruction(&Instruction::Else);
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32GtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
    }

    fn emit_string_contains(&self, func: &mut Function, haystack: ValueId, needle: ValueId) {
        self.emit_get(func, haystack);
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        self.emit_get(func, needle);
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2));

        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Eqz);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));
        func.instruction(&Instruction::Else);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32LeU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6));

        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3));

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32GtU);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7));

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        self.emit_get(func, haystack);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_8));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        self.emit_get(func, needle);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Ne);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_8));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
    }

    fn emit_string_starts_with(&self, func: &mut Function, s: ValueId, prefix: ValueId) {
        self.emit_get(func, s);
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        self.emit_get(func, prefix);
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2));

        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32LeU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        self.emit_get(func, s);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        self.emit_get(func, prefix);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Ne);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_8));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
    }

    fn emit_string_starts_with_from(
        &self,
        func: &mut Function,
        s: ValueId,
        prefix: ValueId,
        start: ValueId,
    ) {
        self.emit_get(func, s);
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32)); // byte_len

        self.emit_get(func, s);
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 4,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2)); // char_len

        self.emit_get(func, prefix);
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3)); // prefix byte_len

        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4)); // result

        self.emit_get(func, start);
        func.instruction(&Instruction::I64Const(0));
        func.instruction(&Instruction::I64LtS);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::Else);
        self.emit_get(func, start);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I64ExtendI32U);
        func.instruction(&Instruction::I64LeU);
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_get(func, start);
        func.instruction(&Instruction::I32WrapI64);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5)); // start_char

        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6)); // start_byte
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7)); // char_idx

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        self.emit_get(func, s);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_8)); // lead byte

        func.instruction(&Instruction::I32Const(4));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_8));
        self.emit_get(func, s);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0xF0));
        func.instruction(&Instruction::I32LtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(3));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_8));
        self.emit_get(func, s);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0xE0));
        func.instruction(&Instruction::I32LtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(2));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_8));
        self.emit_get(func, s);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0x80));
        func.instruction(&Instruction::I32LtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_8));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::I32LeU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7)); // compare idx
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4)); // result

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        self.emit_get(func, s);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_8));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        self.emit_get(func, prefix);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Ne);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_8));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
    }

    fn emit_string_ends_with(&self, func: &mut Function, s: ValueId, suffix: ValueId) {
        self.emit_get(func, s);
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        self.emit_get(func, suffix);
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2));

        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32LeU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6));

        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        self.emit_get(func, s);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        self.emit_get(func, suffix);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Ne);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_8));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
    }

    fn emit_clamped_string_index(
        &self,
        func: &mut Function,
        value: ValueId,
        max_local: u32,
        dst_local: u32,
    ) {
        self.emit_get(func, value);
        func.instruction(&Instruction::I64Const(0));
        func.instruction(&Instruction::I64LtS);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(dst_local));
        func.instruction(&Instruction::Else);
        self.emit_get(func, value);
        func.instruction(&Instruction::LocalGet(max_local));
        func.instruction(&Instruction::I64ExtendI32U);
        func.instruction(&Instruction::I64GtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(max_local));
        func.instruction(&Instruction::LocalSet(dst_local));
        func.instruction(&Instruction::Else);
        self.emit_get(func, value);
        func.instruction(&Instruction::I32WrapI64);
        func.instruction(&Instruction::LocalSet(dst_local));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
    }

    fn emit_utf8_char_width(
        &self,
        func: &mut Function,
        s: ValueId,
        byte_idx_local: u32,
        dst_local: u32,
    ) {
        func.instruction(&Instruction::I32Const(4));
        func.instruction(&Instruction::LocalSet(dst_local));

        self.emit_get(func, s);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(byte_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0xF0));
        func.instruction(&Instruction::I32LtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(3));
        func.instruction(&Instruction::LocalSet(dst_local));

        self.emit_get(func, s);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(byte_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0xE0));
        func.instruction(&Instruction::I32LtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(2));
        func.instruction(&Instruction::LocalSet(dst_local));

        self.emit_get(func, s);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(byte_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0x80));
        func.instruction(&Instruction::I32LtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::LocalSet(dst_local));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
    }

    fn emit_utf8_char_width_from_local(
        &self,
        func: &mut Function,
        s_local: u32,
        byte_idx_local: u32,
        dst_local: u32,
    ) {
        func.instruction(&Instruction::I32Const(4));
        func.instruction(&Instruction::LocalSet(dst_local));

        func.instruction(&Instruction::LocalGet(s_local));
        func.instruction(&Instruction::LocalGet(byte_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 8,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0xF0));
        func.instruction(&Instruction::I32LtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(3));
        func.instruction(&Instruction::LocalSet(dst_local));

        func.instruction(&Instruction::LocalGet(s_local));
        func.instruction(&Instruction::LocalGet(byte_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 8,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0xE0));
        func.instruction(&Instruction::I32LtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(2));
        func.instruction(&Instruction::LocalSet(dst_local));

        func.instruction(&Instruction::LocalGet(s_local));
        func.instruction(&Instruction::LocalGet(byte_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 8,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0x80));
        func.instruction(&Instruction::I32LtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::LocalSet(dst_local));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
    }

    fn emit_utf8_codepoint(
        &self,
        func: &mut Function,
        s: ValueId,
        byte_idx_local: u32,
        width_local: u32,
        dst_local: u32,
    ) {
        func.instruction(&Instruction::LocalGet(width_local));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_get(func, s);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(byte_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(dst_local));
        func.instruction(&Instruction::Else);

        func.instruction(&Instruction::LocalGet(width_local));
        func.instruction(&Instruction::I32Const(2));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_get(func, s);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(byte_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0x1F));
        func.instruction(&Instruction::I32And);
        func.instruction(&Instruction::I32Const(6));
        func.instruction(&Instruction::I32Shl);
        self.emit_get(func, s);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(byte_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0x3F));
        func.instruction(&Instruction::I32And);
        func.instruction(&Instruction::I32Or);
        func.instruction(&Instruction::LocalSet(dst_local));
        func.instruction(&Instruction::Else);

        func.instruction(&Instruction::LocalGet(width_local));
        func.instruction(&Instruction::I32Const(3));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_get(func, s);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(byte_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0x0F));
        func.instruction(&Instruction::I32And);
        func.instruction(&Instruction::I32Const(12));
        func.instruction(&Instruction::I32Shl);
        self.emit_get(func, s);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(byte_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0x3F));
        func.instruction(&Instruction::I32And);
        func.instruction(&Instruction::I32Const(6));
        func.instruction(&Instruction::I32Shl);
        func.instruction(&Instruction::I32Or);
        self.emit_get(func, s);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(byte_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Const(2));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0x3F));
        func.instruction(&Instruction::I32And);
        func.instruction(&Instruction::I32Or);
        func.instruction(&Instruction::LocalSet(dst_local));
        func.instruction(&Instruction::Else);

        self.emit_get(func, s);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(byte_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0x07));
        func.instruction(&Instruction::I32And);
        func.instruction(&Instruction::I32Const(18));
        func.instruction(&Instruction::I32Shl);
        self.emit_get(func, s);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(byte_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0x3F));
        func.instruction(&Instruction::I32And);
        func.instruction(&Instruction::I32Const(12));
        func.instruction(&Instruction::I32Shl);
        func.instruction(&Instruction::I32Or);
        self.emit_get(func, s);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(byte_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Const(2));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0x3F));
        func.instruction(&Instruction::I32And);
        func.instruction(&Instruction::I32Const(6));
        func.instruction(&Instruction::I32Shl);
        func.instruction(&Instruction::I32Or);
        self.emit_get(func, s);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(byte_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Const(3));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0x3F));
        func.instruction(&Instruction::I32And);
        func.instruction(&Instruction::I32Or);
        func.instruction(&Instruction::LocalSet(dst_local));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
    }

    fn emit_utf8_codepoint_from_local(
        &self,
        func: &mut Function,
        s_local: u32,
        byte_idx_local: u32,
        width_local: u32,
        dst_local: u32,
    ) {
        func.instruction(&Instruction::LocalGet(width_local));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(s_local));
        func.instruction(&Instruction::LocalGet(byte_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 8,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(dst_local));
        func.instruction(&Instruction::Else);

        func.instruction(&Instruction::LocalGet(width_local));
        func.instruction(&Instruction::I32Const(2));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(s_local));
        func.instruction(&Instruction::LocalGet(byte_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 8,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0x1F));
        func.instruction(&Instruction::I32And);
        func.instruction(&Instruction::I32Const(6));
        func.instruction(&Instruction::I32Shl);
        func.instruction(&Instruction::LocalGet(s_local));
        func.instruction(&Instruction::LocalGet(byte_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 8,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0x3F));
        func.instruction(&Instruction::I32And);
        func.instruction(&Instruction::I32Or);
        func.instruction(&Instruction::LocalSet(dst_local));
        func.instruction(&Instruction::Else);

        func.instruction(&Instruction::LocalGet(width_local));
        func.instruction(&Instruction::I32Const(3));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(s_local));
        func.instruction(&Instruction::LocalGet(byte_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 8,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0x0F));
        func.instruction(&Instruction::I32And);
        func.instruction(&Instruction::I32Const(12));
        func.instruction(&Instruction::I32Shl);
        func.instruction(&Instruction::LocalGet(s_local));
        func.instruction(&Instruction::LocalGet(byte_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 8,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0x3F));
        func.instruction(&Instruction::I32And);
        func.instruction(&Instruction::I32Const(6));
        func.instruction(&Instruction::I32Shl);
        func.instruction(&Instruction::I32Or);
        func.instruction(&Instruction::LocalGet(s_local));
        func.instruction(&Instruction::LocalGet(byte_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Const(2));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 8,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0x3F));
        func.instruction(&Instruction::I32And);
        func.instruction(&Instruction::I32Or);
        func.instruction(&Instruction::LocalSet(dst_local));
        func.instruction(&Instruction::Else);

        func.instruction(&Instruction::LocalGet(s_local));
        func.instruction(&Instruction::LocalGet(byte_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 8,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0x07));
        func.instruction(&Instruction::I32And);
        func.instruction(&Instruction::I32Const(18));
        func.instruction(&Instruction::I32Shl);
        func.instruction(&Instruction::LocalGet(s_local));
        func.instruction(&Instruction::LocalGet(byte_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 8,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0x3F));
        func.instruction(&Instruction::I32And);
        func.instruction(&Instruction::I32Const(12));
        func.instruction(&Instruction::I32Shl);
        func.instruction(&Instruction::I32Or);
        func.instruction(&Instruction::LocalGet(s_local));
        func.instruction(&Instruction::LocalGet(byte_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Const(2));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 8,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0x3F));
        func.instruction(&Instruction::I32And);
        func.instruction(&Instruction::I32Const(6));
        func.instruction(&Instruction::I32Shl);
        func.instruction(&Instruction::I32Or);
        func.instruction(&Instruction::LocalGet(s_local));
        func.instruction(&Instruction::LocalGet(byte_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Const(3));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 8,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0x3F));
        func.instruction(&Instruction::I32And);
        func.instruction(&Instruction::I32Or);
        func.instruction(&Instruction::LocalSet(dst_local));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
    }

    fn emit_trim_whitespace_check(
        &self,
        func: &mut Function,
        s: ValueId,
        byte_idx_local: u32,
        width_local: u32,
        dst_local: u32,
    ) {
        self.emit_utf8_codepoint(func, s, byte_idx_local, width_local, dst_local);

        func.instruction(&Instruction::LocalGet(dst_local));
        func.instruction(&Instruction::I32Const(0x09));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::LocalGet(dst_local));
        func.instruction(&Instruction::I32Const(0x0D));
        func.instruction(&Instruction::I32LeU);
        func.instruction(&Instruction::I32And);
        func.instruction(&Instruction::LocalGet(dst_local));
        func.instruction(&Instruction::I32Const(0x20));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::I32Or);
        func.instruction(&Instruction::LocalGet(dst_local));
        func.instruction(&Instruction::I32Const(0x85));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::I32Or);
        func.instruction(&Instruction::LocalGet(dst_local));
        func.instruction(&Instruction::I32Const(0xA0));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::I32Or);
        func.instruction(&Instruction::LocalGet(dst_local));
        func.instruction(&Instruction::I32Const(0x1680));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::I32Or);
        func.instruction(&Instruction::LocalGet(dst_local));
        func.instruction(&Instruction::I32Const(0x2000));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::LocalGet(dst_local));
        func.instruction(&Instruction::I32Const(0x200A));
        func.instruction(&Instruction::I32LeU);
        func.instruction(&Instruction::I32And);
        func.instruction(&Instruction::I32Or);
        func.instruction(&Instruction::LocalGet(dst_local));
        func.instruction(&Instruction::I32Const(0x2028));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::I32Or);
        func.instruction(&Instruction::LocalGet(dst_local));
        func.instruction(&Instruction::I32Const(0x2029));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::I32Or);
        func.instruction(&Instruction::LocalGet(dst_local));
        func.instruction(&Instruction::I32Const(0x202F));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::I32Or);
        func.instruction(&Instruction::LocalGet(dst_local));
        func.instruction(&Instruction::I32Const(0x205F));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::I32Or);
        func.instruction(&Instruction::LocalGet(dst_local));
        func.instruction(&Instruction::I32Const(0x3000));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::I32Or);
        func.instruction(&Instruction::LocalSet(dst_local));
    }

    fn emit_string_trim(&self, func: &mut Function, s: ValueId) {
        self.emit_get(func, s);
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32)); // byte_len

        self.emit_get(func, s);
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 4,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2)); // char_len

        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3)); // start_byte
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4)); // start_char

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        self.emit_utf8_char_width(func, s, self.scratch_i32_3, self.scratch_i32_5);
        self.emit_trim_whitespace_check(
            func,
            s,
            self.scratch_i32_3,
            self.scratch_i32_5,
            self.scratch_i32_8,
        );

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::I32Eqz);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_string_const(func, "");
        func.instruction(&Instruction::LocalSet(self.scratch_i32_8));
        func.instruction(&Instruction::Else);

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6)); // end_byte
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7)); // end_char

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32LeU);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5)); // prev_byte

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        self.emit_get(func, s);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0xC0));
        func.instruction(&Instruction::I32And);
        func.instruction(&Instruction::I32Const(0x80));
        func.instruction(&Instruction::I32Ne);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        self.emit_utf8_char_width(func, s, self.scratch_i32_5, self.scratch_i32_2);
        self.emit_trim_whitespace_check(
            func,
            s,
            self.scratch_i32_5,
            self.scratch_i32_2,
            self.scratch_i32_8,
        );

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::I32Eqz);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5)); // trimmed byte len

        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2)); // trimmed char len

        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::Call(self.ctx.alloc_fn_index));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_8)); // result ptr

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Store(MemArg {
            offset: 4,
            align: 2,
            memory_index: 0,
        }));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        self.emit_get(func, s);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::MemoryCopy {
            src_mem: 0,
            dst_mem: 0,
        });
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
    }

    fn emit_string_substring(&self, func: &mut Function, s: ValueId, start: ValueId, end: ValueId) {
        self.emit_get(func, s);
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 4,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        self.emit_clamped_string_index(func, start, self.scratch_i32, self.scratch_i32_2);
        self.emit_clamped_string_index(func, end, self.scratch_i32, self.scratch_i32_3);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_string_const(func, "");
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));
        func.instruction(&Instruction::Else);
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5)); // char_idx
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6)); // byte_idx
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7)); // start_byte

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7));
        func.instruction(&Instruction::End);

        self.emit_get(func, s);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_8)); // lead byte

        func.instruction(&Instruction::I32Const(4));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4)); // width

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::I32Const(0xF0));
        func.instruction(&Instruction::I32LtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(3));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::I32Const(0xE0));
        func.instruction(&Instruction::I32LtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(2));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::I32Const(0x80));
        func.instruction(&Instruction::I32LtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));

        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_8)); // substring byte len

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::Call(self.ctx.alloc_fn_index));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4)); // result ptr

        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::I32Store(MemArg {
            offset: 4,
            align: 2,
            memory_index: 0,
        }));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        self.emit_get(func, s);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::MemoryCopy {
            src_mem: 0,
            dst_mem: 0,
        });
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
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

        if matches!(lhs_ty, Ty::Int) {
            match op {
                BinaryOp::Add => {
                    self.emit_checked_int_add(func, lhs, rhs);
                    return Ok(());
                }
                BinaryOp::Sub => {
                    self.emit_checked_int_sub(func, lhs, rhs);
                    return Ok(());
                }
                BinaryOp::Mul => {
                    self.emit_checked_int_mul(func, lhs, rhs);
                    return Ok(());
                }
                BinaryOp::Mod => {
                    self.emit_checked_int_mod(func, lhs, rhs);
                    return Ok(());
                }
                BinaryOp::Shl | BinaryOp::Shr => {
                    self.emit_checked_int_shift(func, lhs, rhs, op);
                    return Ok(());
                }
                _ => {}
            }
        }

        if matches!(lhs_ty, Ty::String)
            && matches!(
                op,
                BinaryOp::Eq
                    | BinaryOp::NotEq
                    | BinaryOp::Lt
                    | BinaryOp::Gt
                    | BinaryOp::LtEq
                    | BinaryOp::GtEq
            )
        {
            self.emit_string_compare(func, lhs, rhs);
            match op {
                BinaryOp::Eq => {
                    func.instruction(&Instruction::I32Eqz);
                }
                BinaryOp::NotEq => {
                    func.instruction(&Instruction::I32Const(0));
                    func.instruction(&Instruction::I32Ne);
                }
                BinaryOp::Lt => {
                    func.instruction(&Instruction::I32Const(0));
                    func.instruction(&Instruction::I32LtS);
                }
                BinaryOp::Gt => {
                    func.instruction(&Instruction::I32Const(0));
                    func.instruction(&Instruction::I32GtS);
                }
                BinaryOp::LtEq => {
                    func.instruction(&Instruction::I32Const(0));
                    func.instruction(&Instruction::I32LeS);
                }
                BinaryOp::GtEq => {
                    func.instruction(&Instruction::I32Const(0));
                    func.instruction(&Instruction::I32GeS);
                }
                _ => unreachable!("filtered above"),
            }
            return Ok(());
        }

        self.emit_get(func, lhs);
        self.emit_get(func, rhs);

        match (op, lhs_ty) {
            (BinaryOp::Div, Ty::Int) => func.instruction(&Instruction::I64DivS),
            (BinaryOp::BitAnd, Ty::Int) => func.instruction(&Instruction::I64And),
            (BinaryOp::BitOr, Ty::Int) => func.instruction(&Instruction::I64Or),
            (BinaryOp::BitXor, Ty::Int) => func.instruction(&Instruction::I64Xor),

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

            (BinaryOp::Eq, Ty::Char) => func.instruction(&Instruction::I32Eq),
            (BinaryOp::NotEq, Ty::Char) => func.instruction(&Instruction::I32Ne),
            (BinaryOp::Lt, Ty::Char) => func.instruction(&Instruction::I32LtU),
            (BinaryOp::Gt, Ty::Char) => func.instruction(&Instruction::I32GtU),
            (BinaryOp::LtEq, Ty::Char) => func.instruction(&Instruction::I32LeU),
            (BinaryOp::GtEq, Ty::Char) => func.instruction(&Instruction::I32GeU),

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
                self.emit_get(func, operand);
                func.instruction(&Instruction::I64Const(i64::MIN));
                func.instruction(&Instruction::I64Eq);
                self.emit_trap_if_true(func);

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
        result_ty: &Ty,
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
            CallTarget::Indirect(callee) => {
                for &arg in args {
                    self.emit_get(func, arg);
                }
                self.emit_get(func, *callee);
                func.instruction(&Instruction::LocalSet(self.scratch_i32));
                func.instruction(&Instruction::LocalGet(self.scratch_i32));
                func.instruction(&Instruction::GlobalSet(
                    self.ctx.current_closure_global_index,
                ));
                func.instruction(&Instruction::LocalGet(self.scratch_i32));
                func.instruction(&Instruction::I32Load(MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }));
                let sig = FnTypeKey::from_ty(self.value_ty(*callee))?;
                let type_index = self.ctx.fn_type_map.get(&sig).ok_or_else(|| {
                    CodegenError::UnsupportedInstruction(format!(
                        "missing wasm function type for indirect callee {:?}",
                        self.value_ty(*callee)
                    ))
                })?;
                func.instruction(&Instruction::CallIndirect {
                    type_index: *type_index,
                    table_index: 0,
                });
                Ok(())
            }
            CallTarget::Intrinsic(name) => match name.as_str() {
                "char_code" => {
                    self.emit_get(func, args[0]);
                    func.instruction(&Instruction::I64ExtendI32U);
                    Ok(())
                }
                "char_is_decimal_digit" => {
                    self.emit_get(func, args[0]);
                    func.instruction(&Instruction::I32Const('0' as i32));
                    func.instruction(&Instruction::I32GeU);
                    self.emit_get(func, args[0]);
                    func.instruction(&Instruction::I32Const('9' as i32));
                    func.instruction(&Instruction::I32LeU);
                    func.instruction(&Instruction::I32And);
                    Ok(())
                }
                "char_to_decimal_digit" => {
                    self.emit_char_to_digit_option(func, args[0], None, result_ty)?;
                    Ok(())
                }
                "char_to_digit" => {
                    self.emit_char_to_digit_option(func, args[0], Some(args[1]), result_ty)?;
                    Ok(())
                }
                "char_to_string" => {
                    self.emit_char_to_string(func, args[0]);
                    Ok(())
                }
                "abs" => {
                    self.emit_int_abs(func, args[0]);
                    Ok(())
                }
                "float_abs" => {
                    self.emit_get(func, args[0]);
                    func.instruction(&Instruction::F64Abs);
                    Ok(())
                }
                "float_max" => {
                    self.emit_float_min_max(func, args[0], args[1], false);
                    Ok(())
                }
                "float_is_finite" => {
                    self.emit_float_is_finite(func, args[0]);
                    Ok(())
                }
                "float_is_infinite" => {
                    self.emit_float_is_infinite(func, args[0]);
                    Ok(())
                }
                "float_is_nan" => {
                    self.emit_float_is_nan(func, args[0]);
                    Ok(())
                }
                "float_min" => {
                    self.emit_float_min_max(func, args[0], args[1], true);
                    Ok(())
                }
                "float_to_int" => {
                    self.emit_get(func, args[0]);
                    func.instruction(&Instruction::I64TruncSatF64S);
                    Ok(())
                }
                "gcd" => {
                    self.emit_gcd(func, args[0], args[1]);
                    Ok(())
                }
                "int_pow" => {
                    self.emit_int_pow(func, args[0], args[1]);
                    Ok(())
                }
                "int_to_float" => {
                    self.emit_get(func, args[0]);
                    func.instruction(&Instruction::F64ConvertI64S);
                    Ok(())
                }
                "lcm" => {
                    self.emit_lcm(func, args[0], args[1]);
                    Ok(())
                }
                "int_to_string" => {
                    self.emit_int_to_string(func, args[0]);
                    Ok(())
                }
                "max" => {
                    self.emit_int_min_max(func, args[0], args[1], false);
                    Ok(())
                }
                "min" => {
                    self.emit_int_min_max(func, args[0], args[1], true);
                    Ok(())
                }
                "seq_range" => {
                    self.emit_seq_range(func, args[0], args[1]);
                    Ok(())
                }
                "seq_any" => self.emit_seq_any(func, args[0], args[1]),
                "seq_all" => self.emit_seq_all(func, args[0], args[1]),
                "seq_count" => self.emit_seq_count(func, args[0]),
                "seq_count_by" => self.emit_seq_count_by(func, args[0], args[1]),
                "seq_contains" => self.emit_seq_contains(func, args[0], args[1]),
                "seq_find" => self.emit_seq_find(func, args[0], args[1], result_ty),
                "seq_fold" => self.emit_seq_fold(func, args[0], args[1], args[2], result_ty),
                "option_unwrap_or" => self.emit_option_unwrap_or(func, args[0], args[1], result_ty),
                "option_map" => self.emit_option_map(func, args[0], args[1], result_ty),
                "option_and_then" => self.emit_option_and_then(func, args[0], args[1], result_ty),
                "option_map_or" => {
                    self.emit_option_map_or(func, args[0], args[1], args[2], result_ty)
                }
                "result_unwrap_or" => self.emit_result_unwrap_or(func, args[0], args[1], result_ty),
                "result_map" => self.emit_result_map(func, args[0], args[1], result_ty),
                "result_and_then" => self.emit_result_and_then(func, args[0], args[1], result_ty),
                "result_map_err" => self.emit_result_map_err(func, args[0], args[1], result_ty),
                "result_map_or" => {
                    self.emit_result_map_or(func, args[0], args[1], args[2], result_ty)
                }
                "parse_float" => self.emit_parse_float_result(func, args[0], result_ty),
                "parse_int" => self.emit_parse_int_result(func, args[0], result_ty),
                "string_contains" => {
                    self.emit_string_contains(func, args[0], args[1]);
                    Ok(())
                }
                "string_chars" => {
                    self.emit_seq_chars(func, args[0]);
                    Ok(())
                }
                "string_concat" => {
                    self.emit_string_concat(func, args[0], args[1]);
                    Ok(())
                }
                "string_ends_with" => {
                    self.emit_string_ends_with(func, args[0], args[1]);
                    Ok(())
                }
                "string_lines" => {
                    self.emit_seq_lines(func, args[0]);
                    Ok(())
                }
                "string_split" => {
                    self.emit_seq_split(func, args[0], args[1]);
                    Ok(())
                }
                "string_md5" => {
                    self.emit_string_host_helper_call(
                        func,
                        args[0],
                        self.ctx
                            .string_md5_fn_index
                            .expect("string_md5 helper should be imported when used"),
                    );
                    Ok(())
                }
                "string_len" => {
                    self.emit_get(func, args[0]);
                    func.instruction(&Instruction::I32Load(MemArg {
                        offset: 4,
                        align: 2,
                        memory_index: 0,
                    }));
                    func.instruction(&Instruction::I64ExtendI32U);
                    Ok(())
                }
                "string_starts_with" => {
                    self.emit_string_starts_with(func, args[0], args[1]);
                    Ok(())
                }
                "string_starts_with_from" => {
                    self.emit_string_starts_with_from(func, args[0], args[1], args[2]);
                    Ok(())
                }
                "string_substring" => {
                    self.emit_string_substring(func, args[0], args[1], args[2]);
                    Ok(())
                }
                "string_to_lower" => {
                    self.emit_string_host_helper_call(
                        func,
                        args[0],
                        self.ctx
                            .string_to_lower_fn_index
                            .expect("string_to_lower helper should be imported when used"),
                    );
                    Ok(())
                }
                "string_to_upper" => {
                    self.emit_string_host_helper_call(
                        func,
                        args[0],
                        self.ctx
                            .string_to_upper_fn_index
                            .expect("string_to_upper helper should be imported when used"),
                    );
                    Ok(())
                }
                "string_trim" => {
                    self.emit_string_trim(func, args[0]);
                    Ok(())
                }
                _ => Err(CodegenError::UnsupportedInstruction(format!(
                    "intrinsic {name} (deferred)"
                ))),
            },
        }
    }

    fn emit_string_host_helper_call(
        &self,
        func: &mut Function,
        value: ValueId,
        helper_fn_index: u32,
    ) {
        self.emit_get(func, value);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        self.emit_get(func, value);
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::Call(helper_fn_index));
    }

    fn emit_seq_range(&self, func: &mut Function, start: ValueId, end: ValueId) {
        func.instruction(&Instruction::I32Const(24));
        func.instruction(&Instruction::Call(self.ctx.alloc_fn_index));
        func.instruction(&Instruction::Drop);

        func.instruction(&Instruction::GlobalGet(0));
        func.instruction(&Instruction::I32Const(24));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        self.emit_get(func, start);
        func.instruction(&Instruction::I64Store(MemArg {
            offset: 8,
            align: 3,
            memory_index: 0,
        }));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        self.emit_get(func, end);
        func.instruction(&Instruction::I64Store(MemArg {
            offset: 16,
            align: 3,
            memory_index: 0,
        }));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
    }

    fn emit_seq_chars(&self, func: &mut Function, value: ValueId) {
        func.instruction(&Instruction::I32Const(24));
        func.instruction(&Instruction::Call(self.ctx.alloc_fn_index));
        func.instruction(&Instruction::Drop);

        func.instruction(&Instruction::GlobalGet(0));
        func.instruction(&Instruction::I32Const(24));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        self.emit_get(func, value);
        func.instruction(&Instruction::I32Store(MemArg {
            offset: 8,
            align: 2,
            memory_index: 0,
        }));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
    }

    fn emit_seq_lines(&self, func: &mut Function, value: ValueId) {
        func.instruction(&Instruction::I32Const(24));
        func.instruction(&Instruction::Call(self.ctx.alloc_fn_index));
        func.instruction(&Instruction::Drop);

        func.instruction(&Instruction::GlobalGet(0));
        func.instruction(&Instruction::I32Const(24));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Const(2));
        func.instruction(&Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        self.emit_get(func, value);
        func.instruction(&Instruction::I32Store(MemArg {
            offset: 8,
            align: 2,
            memory_index: 0,
        }));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
    }

    fn emit_seq_split(&self, func: &mut Function, value: ValueId, delim: ValueId) {
        func.instruction(&Instruction::I32Const(24));
        func.instruction(&Instruction::Call(self.ctx.alloc_fn_index));
        func.instruction(&Instruction::Drop);

        func.instruction(&Instruction::GlobalGet(0));
        func.instruction(&Instruction::I32Const(24));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Const(3));
        func.instruction(&Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        self.emit_get(func, value);
        func.instruction(&Instruction::I32Store(MemArg {
            offset: 8,
            align: 2,
            memory_index: 0,
        }));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        self.emit_get(func, delim);
        func.instruction(&Instruction::I32Store(MemArg {
            offset: 12,
            align: 2,
            memory_index: 0,
        }));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
    }

    fn emit_seq_count(&self, func: &mut Function, seq: ValueId) -> Result<(), CodegenError> {
        self.emit_get(func, seq);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Result(ValType::I64)));
        self.emit_seq_count_range_from_local(func, self.scratch_i32)?;
        func.instruction(&Instruction::Else);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Result(ValType::I64)));
        self.emit_seq_count_chars_from_local(func, self.scratch_i32);
        func.instruction(&Instruction::Else);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Const(2));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Result(ValType::I64)));
        self.emit_seq_count_lines_from_local(func, self.scratch_i32);
        func.instruction(&Instruction::Else);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Const(3));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Result(ValType::I64)));
        self.emit_seq_count_split_from_local(func, self.scratch_i32);
        func.instruction(&Instruction::Else);
        func.instruction(&Instruction::Unreachable);
        func.instruction(&Instruction::I64Const(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
        Ok(())
    }

    fn emit_seq_count_range_from_local(
        &self,
        func: &mut Function,
        seq_local: u32,
    ) -> Result<(), CodegenError> {
        self.emit_seq_load_range_bounds_from_local(func, seq_local)?;
        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::LocalGet(self.scratch_i64_2));
        func.instruction(&Instruction::I64LtS);
        func.instruction(&Instruction::If(BlockType::Result(ValType::I64)));
        func.instruction(&Instruction::LocalGet(self.scratch_i64_2));
        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::I64Sub);
        func.instruction(&Instruction::LocalTee(self.scratch_i64_3));
        func.instruction(&Instruction::I64Const(0));
        func.instruction(&Instruction::I64LtS);
        self.emit_trap_if_true(func);
        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::Else);
        func.instruction(&Instruction::I64Const(0));
        func.instruction(&Instruction::End);
        Ok(())
    }

    fn emit_seq_count_chars_from_local(&self, func: &mut Function, seq_local: u32) {
        func.instruction(&Instruction::LocalGet(seq_local));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 8,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 4,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I64ExtendI32U);
    }

    fn emit_seq_count_lines_from_local(&self, func: &mut Function, seq_local: u32) {
        func.instruction(&Instruction::LocalGet(seq_local));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 8,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::If(BlockType::Result(ValType::I64)));
        func.instruction(&Instruction::I64Const(1));
        func.instruction(&Instruction::LocalSet(self.scratch_i64_3));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));
        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 8,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(10));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32LtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::I64Const(1));
        func.instruction(&Instruction::I64Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i64_3));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::Else);
        func.instruction(&Instruction::I64Const(0));
        func.instruction(&Instruction::End);
    }

    fn emit_seq_count_split_from_local(&self, func: &mut Function, seq_local: u32) {
        func.instruction(&Instruction::LocalGet(seq_local));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 8,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3)); // string ptr
        func.instruction(&Instruction::LocalGet(seq_local));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 12,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4)); // delim ptr
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2)); // delim byte len

        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Eqz);
        func.instruction(&Instruction::If(BlockType::Result(ValType::I64)));
        self.emit_seq_count_empty_delim_split(func);
        func.instruction(&Instruction::Else);
        self.emit_seq_count_nonempty_delim_split(func);
        func.instruction(&Instruction::End);
    }

    fn emit_seq_count_empty_delim_split(&self, func: &mut Function) {
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 4,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I64ExtendI32U);
        func.instruction(&Instruction::I64Const(2));
        func.instruction(&Instruction::I64Add);
    }

    fn emit_seq_count_nonempty_delim_split(&self, func: &mut Function) {
        func.instruction(&Instruction::I64Const(1));
        func.instruction(&Instruction::LocalSet(self.scratch_i64_3));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5)); // string byte len
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6)); // scan index
        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::I32GtU);
        func.instruction(&Instruction::BrIf(1));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_8)); // matched
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7)); // inner offset
        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 8,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 8,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::I32Eqz);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_8));
        func.instruction(&Instruction::Br(2));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::I64Const(i64::MAX));
        func.instruction(&Instruction::I64Eq);
        self.emit_trap_if_true(func);
        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::I64Const(1));
        func.instruction(&Instruction::I64Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i64_3));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6));
        func.instruction(&Instruction::Else);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
    }

    fn emit_seq_count_by(
        &self,
        func: &mut Function,
        seq: ValueId,
        predicate: ValueId,
    ) -> Result<(), CodegenError> {
        self.emit_get(func, seq);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        match self.core_seq_element_ty(seq) {
            Some(Ty::Int) => {
                self.emit_seq_count_by_range_from_local(func, self.scratch_i32, predicate)?;
            }
            Some(Ty::Char) => {
                self.emit_seq_count_by_chars_from_local(func, self.scratch_i32, predicate)?;
            }
            Some(Ty::String) => {
                func.instruction(&Instruction::LocalGet(self.scratch_i32));
                func.instruction(&Instruction::I32Load(MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }));
                func.instruction(&Instruction::LocalSet(self.scratch_i32_2));

                func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
                func.instruction(&Instruction::I32Const(2));
                func.instruction(&Instruction::I32Eq);
                func.instruction(&Instruction::If(BlockType::Result(ValType::I64)));
                self.emit_seq_count_by_lines_from_local(func, self.scratch_i32, predicate)?;
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
                func.instruction(&Instruction::I32Const(3));
                func.instruction(&Instruction::I32Eq);
                func.instruction(&Instruction::If(BlockType::Result(ValType::I64)));
                self.emit_seq_count_by_split_from_local(func, self.scratch_i32, predicate)?;
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::Unreachable);
                func.instruction(&Instruction::I64Const(0));
                func.instruction(&Instruction::End);
                func.instruction(&Instruction::End);
            }
            Some(elem_ty) => {
                return Err(CodegenError::UnsupportedInstruction(format!(
                    "seq_count_by over Wasm seq element type {elem_ty:?} (deferred)"
                )));
            }
            None => {
                return Err(CodegenError::UnsupportedInstruction(format!(
                    "seq_count_by over non-Seq receiver in Wasm: {:?}",
                    self.value_ty(seq)
                )));
            }
        }

        Ok(())
    }

    fn emit_seq_count_by_range_from_local(
        &self,
        func: &mut Function,
        seq_local: u32,
        predicate: ValueId,
    ) -> Result<(), CodegenError> {
        self.emit_seq_load_range_bounds_from_local(func, seq_local)?;

        func.instruction(&Instruction::I64Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i64_3));

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::LocalGet(self.scratch_i64_2));
        func.instruction(&Instruction::I64GeS);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        self.emit_indirect_call_single_arg(func, predicate)?;
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::I64Const(i64::MAX));
        func.instruction(&Instruction::I64Eq);
        self.emit_trap_if_true(func);
        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::I64Const(1));
        func.instruction(&Instruction::I64Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i64_3));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::I64Const(1));
        func.instruction(&Instruction::I64Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i64));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        Ok(())
    }

    fn emit_seq_count_by_chars_from_local(
        &self,
        func: &mut Function,
        seq_local: u32,
        predicate: ValueId,
    ) -> Result<(), CodegenError> {
        func.instruction(&Instruction::LocalGet(seq_local));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 8,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));

        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));
        func.instruction(&Instruction::I64Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i64_3));

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        self.emit_utf8_char_width_from_local(
            func,
            self.scratch_i32_3,
            self.scratch_i32_5,
            self.scratch_i32_7,
        );
        self.emit_utf8_codepoint_from_local(
            func,
            self.scratch_i32_3,
            self.scratch_i32_5,
            self.scratch_i32_7,
            self.scratch_i32_8,
        );

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        self.emit_indirect_call_single_arg(func, predicate)?;
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::I64Const(i64::MAX));
        func.instruction(&Instruction::I64Eq);
        self.emit_trap_if_true(func);
        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::I64Const(1));
        func.instruction(&Instruction::I64Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i64_3));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        Ok(())
    }

    fn emit_seq_count_by_lines_from_local(
        &self,
        func: &mut Function,
        seq_local: u32,
        predicate: ValueId,
    ) -> Result<(), CodegenError> {
        func.instruction(&Instruction::LocalGet(seq_local));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 8,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3)); // string ptr

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4)); // byte len

        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6)); // scan idx
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7)); // segment start
        func.instruction(&Instruction::I64Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i64_3)); // count

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(10));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2)); // segment len

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32GtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(13));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_10));
        self.emit_string_range_alloc_from_locals(
            func,
            self.scratch_i32_3,
            self.scratch_i32_7,
            self.scratch_i32_10,
            self.scratch_i32_5,
            self.scratch_i32_8,
            self.scratch_i32_9,
        );
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        self.emit_indirect_call_single_arg(func, predicate)?;
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::I64Const(i64::MAX));
        func.instruction(&Instruction::I64Eq);
        self.emit_trap_if_true(func);
        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::I64Const(1));
        func.instruction(&Instruction::I64Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i64_3));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32LtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_string_range_alloc_from_locals(
            func,
            self.scratch_i32_3,
            self.scratch_i32_7,
            self.scratch_i32_4,
            self.scratch_i32_5,
            self.scratch_i32_8,
            self.scratch_i32_9,
        );
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        self.emit_indirect_call_single_arg(func, predicate)?;
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::I64Const(i64::MAX));
        func.instruction(&Instruction::I64Eq);
        self.emit_trap_if_true(func);
        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::I64Const(1));
        func.instruction(&Instruction::I64Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i64_3));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        Ok(())
    }

    fn emit_string_range_alloc_from_locals(
        &self,
        func: &mut Function,
        base_string_local: u32,
        slice_start_local: u32,
        slice_end_local: u32,
        result_local: u32,
        char_count_local: u32,
        scan_local: u32,
    ) {
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(char_count_local));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(scan_local));

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(scan_local));
        func.instruction(&Instruction::LocalGet(slice_end_local));
        func.instruction(&Instruction::LocalGet(slice_start_local));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::I32Const(4));
        func.instruction(&Instruction::LocalSet(result_local));

        func.instruction(&Instruction::LocalGet(base_string_local));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(slice_start_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(scan_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0xF0));
        func.instruction(&Instruction::I32LtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(3));
        func.instruction(&Instruction::LocalSet(result_local));
        func.instruction(&Instruction::LocalGet(base_string_local));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(slice_start_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(scan_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0xE0));
        func.instruction(&Instruction::I32LtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(2));
        func.instruction(&Instruction::LocalSet(result_local));
        func.instruction(&Instruction::LocalGet(base_string_local));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(slice_start_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(scan_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0x80));
        func.instruction(&Instruction::I32LtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::LocalSet(result_local));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(scan_local));
        func.instruction(&Instruction::LocalGet(result_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(scan_local));
        func.instruction(&Instruction::LocalGet(char_count_local));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(char_count_local));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(slice_end_local));
        func.instruction(&Instruction::LocalGet(slice_start_local));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::Call(self.ctx.alloc_fn_index));
        func.instruction(&Instruction::LocalSet(result_local));

        func.instruction(&Instruction::LocalGet(result_local));
        func.instruction(&Instruction::LocalGet(slice_end_local));
        func.instruction(&Instruction::LocalGet(slice_start_local));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalGet(result_local));
        func.instruction(&Instruction::LocalGet(char_count_local));
        func.instruction(&Instruction::I32Store(MemArg {
            offset: 4,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalGet(result_local));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(base_string_local));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(slice_start_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(slice_end_local));
        func.instruction(&Instruction::LocalGet(slice_start_local));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::MemoryCopy {
            src_mem: 0,
            dst_mem: 0,
        });
    }

    fn emit_seq_count_by_split_from_local(
        &self,
        func: &mut Function,
        seq_local: u32,
        predicate: ValueId,
    ) -> Result<(), CodegenError> {
        func.instruction(&Instruction::LocalGet(seq_local));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 8,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3)); // string ptr
        func.instruction(&Instruction::LocalGet(seq_local));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4)); // seq ptr copy
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6)); // string byte len
        func.instruction(&Instruction::I64Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i64_3));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 12,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Eqz);
        func.instruction(&Instruction::If(BlockType::Result(ValType::I64)));
        self.emit_seq_count_by_empty_delim_split_from_local(
            func,
            self.scratch_i32_3,
            self.scratch_i32_6,
            predicate,
        )?;
        func.instruction(&Instruction::Else);

        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7)); // scan idx
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2)); // segment start

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 12,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32GtU);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::LocalSet(self.scratch_i32));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 12,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 12,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Ne);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32));
        func.instruction(&Instruction::Br(2));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_string_range_alloc_from_locals(
            func,
            self.scratch_i32_3,
            self.scratch_i32_2,
            self.scratch_i32_7,
            self.scratch_i32_5,
            self.scratch_i32,
            self.scratch_i32_8,
        );
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        self.emit_indirect_call_single_arg(func, predicate)?;
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::I64Const(i64::MAX));
        func.instruction(&Instruction::I64Eq);
        self.emit_trap_if_true(func);
        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::I64Const(1));
        func.instruction(&Instruction::I64Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i64_3));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 12,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2));
        func.instruction(&Instruction::Else);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        self.emit_string_range_alloc_from_locals(
            func,
            self.scratch_i32_3,
            self.scratch_i32_2,
            self.scratch_i32_6,
            self.scratch_i32_5,
            self.scratch_i32,
            self.scratch_i32_8,
        );
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        self.emit_indirect_call_single_arg(func, predicate)?;
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::I64Const(i64::MAX));
        func.instruction(&Instruction::I64Eq);
        self.emit_trap_if_true(func);
        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::I64Const(1));
        func.instruction(&Instruction::I64Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i64_3));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::End);
        Ok(())
    }

    fn emit_seq_count_by_empty_delim_split_from_local(
        &self,
        func: &mut Function,
        string_local: u32,
        string_byte_len_local: u32,
        predicate: ValueId,
    ) -> Result<(), CodegenError> {
        self.emit_string_const(func, "");
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));
        func.instruction(&Instruction::I64Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i64_3));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        self.emit_indirect_call_single_arg(func, predicate)?;
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::I64Const(i64::MAX));
        func.instruction(&Instruction::I64Eq);
        self.emit_trap_if_true(func);
        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::I64Const(1));
        func.instruction(&Instruction::I64Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i64_3));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7)); // byte idx

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(string_byte_len_local));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        self.emit_utf8_char_width_from_local(
            func,
            string_local,
            self.scratch_i32_7,
            self.scratch_i32_2,
        );
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_8));
        self.emit_string_range_alloc_from_locals(
            func,
            string_local,
            self.scratch_i32_7,
            self.scratch_i32_8,
            self.scratch_i32_5,
            self.scratch_i32,
            self.scratch_i32_2,
        );
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        self.emit_indirect_call_single_arg(func, predicate)?;
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::I64Const(i64::MAX));
        func.instruction(&Instruction::I64Eq);
        self.emit_trap_if_true(func);
        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::I64Const(1));
        func.instruction(&Instruction::I64Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i64_3));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        self.emit_string_const(func, "");
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        self.emit_indirect_call_single_arg(func, predicate)?;
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::I64Const(i64::MAX));
        func.instruction(&Instruction::I64Eq);
        self.emit_trap_if_true(func);
        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::I64Const(1));
        func.instruction(&Instruction::I64Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i64_3));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        Ok(())
    }

    fn core_seq_element_ty<'b>(&'b self, seq: ValueId) -> Option<&'b Ty> {
        let Ty::Adt { def, args } = self.value_ty(seq) else {
            return None;
        };
        let type_name = self.ctx.item_tree.types[*def]
            .name
            .resolve(self.ctx.interner);
        if type_name == "$core_Seq" || type_name == "Seq" {
            args.first()
        } else {
            None
        }
    }

    fn emit_seq_any(
        &self,
        func: &mut Function,
        seq: ValueId,
        predicate: ValueId,
    ) -> Result<(), CodegenError> {
        self.emit_get(func, seq);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        match self.core_seq_element_ty(seq) {
            Some(Ty::Int) => {
                self.emit_seq_any_range_from_local(func, self.scratch_i32, predicate)?
            }
            Some(Ty::Char) => {
                self.emit_seq_any_chars_from_local(func, self.scratch_i32, predicate)?
            }
            Some(Ty::String) => {
                func.instruction(&Instruction::LocalGet(self.scratch_i32));
                func.instruction(&Instruction::I32Load(MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }));
                func.instruction(&Instruction::LocalSet(self.scratch_i32_2));
                func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
                func.instruction(&Instruction::I32Const(2));
                func.instruction(&Instruction::I32Eq);
                func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
                self.emit_seq_count_by_lines_from_local(func, self.scratch_i32, predicate)?;
                func.instruction(&Instruction::I64Eqz);
                func.instruction(&Instruction::I32Eqz);
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
                func.instruction(&Instruction::I32Const(3));
                func.instruction(&Instruction::I32Eq);
                func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
                self.emit_seq_count_by_split_from_local(func, self.scratch_i32, predicate)?;
                func.instruction(&Instruction::I64Eqz);
                func.instruction(&Instruction::I32Eqz);
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::Unreachable);
                func.instruction(&Instruction::I32Const(0));
                func.instruction(&Instruction::End);
                func.instruction(&Instruction::End);
            }
            Some(elem_ty) => {
                return Err(CodegenError::UnsupportedInstruction(format!(
                    "seq_any over Wasm seq element type {elem_ty:?} (deferred)"
                )));
            }
            None => {
                return Err(CodegenError::UnsupportedInstruction(format!(
                    "seq_any over non-Seq receiver in Wasm: {:?}",
                    self.value_ty(seq)
                )));
            }
        }

        Ok(())
    }

    fn emit_seq_all(
        &self,
        func: &mut Function,
        seq: ValueId,
        predicate: ValueId,
    ) -> Result<(), CodegenError> {
        self.emit_get(func, seq);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        match self.core_seq_element_ty(seq) {
            Some(Ty::Int) => {
                self.emit_seq_all_range_from_local(func, self.scratch_i32, predicate)?
            }
            Some(Ty::Char) => {
                self.emit_seq_all_chars_from_local(func, self.scratch_i32, predicate)?
            }
            Some(Ty::String) => {
                func.instruction(&Instruction::LocalGet(self.scratch_i32));
                func.instruction(&Instruction::I32Load(MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }));
                func.instruction(&Instruction::LocalSet(self.scratch_i32_2));
                func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
                func.instruction(&Instruction::I32Const(2));
                func.instruction(&Instruction::I32Eq);
                func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
                func.instruction(&Instruction::LocalGet(self.scratch_i32));
                func.instruction(&Instruction::I64ExtendI32U);
                func.instruction(&Instruction::LocalSet(self.scratch_i64_4));
                self.emit_seq_count_by_lines_from_local(func, self.scratch_i32, predicate)?;
                func.instruction(&Instruction::LocalSet(self.scratch_i64_2));
                func.instruction(&Instruction::LocalGet(self.scratch_i64_4));
                func.instruction(&Instruction::I32WrapI64);
                func.instruction(&Instruction::LocalSet(self.scratch_i32_2));
                self.emit_seq_count_lines_from_local(func, self.scratch_i32_2);
                func.instruction(&Instruction::LocalGet(self.scratch_i64_2));
                func.instruction(&Instruction::I64Eq);
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
                func.instruction(&Instruction::I32Const(3));
                func.instruction(&Instruction::I32Eq);
                func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
                func.instruction(&Instruction::LocalGet(self.scratch_i32));
                func.instruction(&Instruction::I64ExtendI32U);
                func.instruction(&Instruction::LocalSet(self.scratch_i64_4));
                self.emit_seq_count_by_split_from_local(func, self.scratch_i32, predicate)?;
                func.instruction(&Instruction::LocalSet(self.scratch_i64_2));
                func.instruction(&Instruction::LocalGet(self.scratch_i64_4));
                func.instruction(&Instruction::I32WrapI64);
                func.instruction(&Instruction::LocalSet(self.scratch_i32_2));
                self.emit_seq_count_split_from_local(func, self.scratch_i32_2);
                func.instruction(&Instruction::LocalGet(self.scratch_i64_2));
                func.instruction(&Instruction::I64Eq);
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::Unreachable);
                func.instruction(&Instruction::I32Const(0));
                func.instruction(&Instruction::End);
                func.instruction(&Instruction::End);
            }
            Some(elem_ty) => {
                return Err(CodegenError::UnsupportedInstruction(format!(
                    "seq_all over Wasm seq element type {elem_ty:?} (deferred)"
                )));
            }
            None => {
                return Err(CodegenError::UnsupportedInstruction(format!(
                    "seq_all over non-Seq receiver in Wasm: {:?}",
                    self.value_ty(seq)
                )));
            }
        }

        Ok(())
    }

    fn emit_seq_any_range_from_local(
        &self,
        func: &mut Function,
        seq_local: u32,
        predicate: ValueId,
    ) -> Result<(), CodegenError> {
        self.emit_seq_load_range_bounds_from_local(func, seq_local)?;
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3));

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::LocalGet(self.scratch_i64_2));
        func.instruction(&Instruction::I64GeS);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        self.emit_indirect_call_single_arg(func, predicate)?;
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3));
        func.instruction(&Instruction::Br(2));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::I64Const(1));
        func.instruction(&Instruction::I64Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i64));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        Ok(())
    }

    fn emit_seq_any_chars_from_local(
        &self,
        func: &mut Function,
        seq_local: u32,
        predicate: ValueId,
    ) -> Result<(), CodegenError> {
        func.instruction(&Instruction::LocalGet(seq_local));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 8,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));

        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6));

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        self.emit_utf8_char_width_from_local(
            func,
            self.scratch_i32_3,
            self.scratch_i32_5,
            self.scratch_i32_7,
        );
        self.emit_utf8_codepoint_from_local(
            func,
            self.scratch_i32_3,
            self.scratch_i32_5,
            self.scratch_i32_7,
            self.scratch_i32_8,
        );

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        self.emit_indirect_call_single_arg(func, predicate)?;
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6));
        func.instruction(&Instruction::Br(2));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        Ok(())
    }

    fn emit_seq_all_range_from_local(
        &self,
        func: &mut Function,
        seq_local: u32,
        predicate: ValueId,
    ) -> Result<(), CodegenError> {
        self.emit_seq_load_range_bounds_from_local(func, seq_local)?;
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3));

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::LocalGet(self.scratch_i64_2));
        func.instruction(&Instruction::I64GeS);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        self.emit_indirect_call_single_arg(func, predicate)?;
        func.instruction(&Instruction::I32Eqz);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3));
        func.instruction(&Instruction::Br(2));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::I64Const(1));
        func.instruction(&Instruction::I64Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i64));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        Ok(())
    }

    fn emit_seq_all_chars_from_local(
        &self,
        func: &mut Function,
        seq_local: u32,
        predicate: ValueId,
    ) -> Result<(), CodegenError> {
        func.instruction(&Instruction::LocalGet(seq_local));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 8,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));

        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6));

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        self.emit_utf8_char_width_from_local(
            func,
            self.scratch_i32_3,
            self.scratch_i32_5,
            self.scratch_i32_7,
        );
        self.emit_utf8_codepoint_from_local(
            func,
            self.scratch_i32_3,
            self.scratch_i32_5,
            self.scratch_i32_7,
            self.scratch_i32_8,
        );

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        self.emit_indirect_call_single_arg(func, predicate)?;
        func.instruction(&Instruction::I32Eqz);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6));
        func.instruction(&Instruction::Br(2));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        Ok(())
    }

    fn emit_seq_fold(
        &self,
        func: &mut Function,
        seq: ValueId,
        init: ValueId,
        folder: ValueId,
        result_ty: &Ty,
    ) -> Result<(), CodegenError> {
        let acc_local = match result_ty {
            Ty::Float => self.scratch_f64,
            Ty::Int => self.scratch_i64_3,
            _ => self.scratch_i32_8,
        };
        self.emit_get(func, init);
        func.instruction(&Instruction::LocalSet(acc_local));

        self.emit_get(func, seq);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        match self.core_seq_element_ty(seq) {
            Some(Ty::Int) => {
                self.emit_seq_fold_range_from_local(func, self.scratch_i32, acc_local, folder)?;
            }
            Some(Ty::Char) => {
                self.emit_seq_fold_chars_from_local(func, self.scratch_i32, acc_local, folder)?;
            }
            Some(Ty::String) => {
                func.instruction(&Instruction::LocalGet(self.scratch_i32));
                func.instruction(&Instruction::I32Load(MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }));
                func.instruction(&Instruction::LocalSet(self.scratch_i32_2));
                func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
                func.instruction(&Instruction::I32Const(2));
                func.instruction(&Instruction::I32Eq);
                func.instruction(&Instruction::If(BlockType::Empty));
                self.emit_seq_fold_lines_from_local(func, self.scratch_i32, acc_local, folder)?;
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
                func.instruction(&Instruction::I32Const(3));
                func.instruction(&Instruction::I32Eq);
                func.instruction(&Instruction::If(BlockType::Empty));
                self.emit_seq_fold_split_from_local(func, self.scratch_i32, acc_local, folder)?;
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::Unreachable);
                func.instruction(&Instruction::End);
                func.instruction(&Instruction::End);
            }
            Some(elem_ty) => {
                return Err(CodegenError::UnsupportedInstruction(format!(
                    "seq_fold over Wasm seq element type {elem_ty:?} (deferred)"
                )));
            }
            None => {
                return Err(CodegenError::UnsupportedInstruction(format!(
                    "seq_fold over non-Seq receiver in Wasm: {:?}",
                    self.value_ty(seq)
                )));
            }
        }

        func.instruction(&Instruction::LocalGet(acc_local));
        Ok(())
    }

    fn emit_seq_fold_range_from_local(
        &self,
        func: &mut Function,
        seq_local: u32,
        acc_local: u32,
        folder: ValueId,
    ) -> Result<(), CodegenError> {
        self.emit_seq_load_range_bounds_from_local(func, seq_local)?;

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::LocalGet(self.scratch_i64_2));
        func.instruction(&Instruction::I64GeS);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::LocalGet(acc_local));
        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        self.emit_indirect_call_two_args(func, folder)?;
        func.instruction(&Instruction::LocalSet(acc_local));

        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::I64Const(1));
        func.instruction(&Instruction::I64Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i64));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        Ok(())
    }

    fn emit_seq_fold_chars_from_local(
        &self,
        func: &mut Function,
        seq_local: u32,
        acc_local: u32,
        folder: ValueId,
    ) -> Result<(), CodegenError> {
        func.instruction(&Instruction::LocalGet(seq_local));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 8,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));

        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        self.emit_utf8_char_width_from_local(
            func,
            self.scratch_i32_3,
            self.scratch_i32_5,
            self.scratch_i32_6,
        );
        self.emit_utf8_codepoint_from_local(
            func,
            self.scratch_i32_3,
            self.scratch_i32_5,
            self.scratch_i32_6,
            self.scratch_i32_7,
        );

        func.instruction(&Instruction::LocalGet(acc_local));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        self.emit_indirect_call_two_args(func, folder)?;
        func.instruction(&Instruction::LocalSet(acc_local));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        Ok(())
    }

    fn emit_seq_fold_lines_from_local(
        &self,
        func: &mut Function,
        seq_local: u32,
        acc_local: u32,
        folder: ValueId,
    ) -> Result<(), CodegenError> {
        func.instruction(&Instruction::LocalGet(seq_local));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 8,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3)); // string ptr

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4)); // byte len

        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6)); // scan idx
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7)); // segment start

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(10));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2)); // segment len

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32GtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(13));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_10));
        self.emit_string_range_alloc_from_locals(
            func,
            self.scratch_i32_3,
            self.scratch_i32_7,
            self.scratch_i32_10,
            self.scratch_i32_5,
            self.scratch_i32_8,
            self.scratch_i32_9,
        );
        func.instruction(&Instruction::LocalGet(acc_local));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        self.emit_indirect_call_two_args(func, folder)?;
        func.instruction(&Instruction::LocalSet(acc_local));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32LtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_string_range_alloc_from_locals(
            func,
            self.scratch_i32_3,
            self.scratch_i32_7,
            self.scratch_i32_4,
            self.scratch_i32_5,
            self.scratch_i32_8,
            self.scratch_i32_9,
        );
        func.instruction(&Instruction::LocalGet(acc_local));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        self.emit_indirect_call_two_args(func, folder)?;
        func.instruction(&Instruction::LocalSet(acc_local));
        func.instruction(&Instruction::End);

        Ok(())
    }

    fn emit_seq_fold_split_from_local(
        &self,
        func: &mut Function,
        seq_local: u32,
        acc_local: u32,
        folder: ValueId,
    ) -> Result<(), CodegenError> {
        func.instruction(&Instruction::LocalGet(seq_local));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 8,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3)); // string ptr
        func.instruction(&Instruction::LocalGet(seq_local));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4)); // seq ptr copy
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6)); // string byte len

        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 12,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Eqz);
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_seq_fold_empty_delim_split_from_local(
            func,
            self.scratch_i32_3,
            self.scratch_i32_6,
            acc_local,
            folder,
        )?;
        func.instruction(&Instruction::Else);

        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7)); // scan idx
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2)); // segment start

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 12,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32GtU);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::LocalSet(self.scratch_i32)); // matched
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5)); // inner offset

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 12,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 12,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Ne);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32));
        func.instruction(&Instruction::Br(2));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_string_range_alloc_from_locals(
            func,
            self.scratch_i32_3,
            self.scratch_i32_2,
            self.scratch_i32_7,
            self.scratch_i32_10,
            self.scratch_i32,
            self.scratch_i32_8,
        );
        func.instruction(&Instruction::LocalGet(acc_local));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_10));
        self.emit_indirect_call_two_args(func, folder)?;
        func.instruction(&Instruction::LocalSet(acc_local));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 12,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2));
        func.instruction(&Instruction::Else);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        self.emit_string_range_alloc_from_locals(
            func,
            self.scratch_i32_3,
            self.scratch_i32_2,
            self.scratch_i32_6,
            self.scratch_i32_10,
            self.scratch_i32,
            self.scratch_i32_8,
        );
        func.instruction(&Instruction::LocalGet(acc_local));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_10));
        self.emit_indirect_call_two_args(func, folder)?;
        func.instruction(&Instruction::LocalSet(acc_local));
        func.instruction(&Instruction::End);

        Ok(())
    }

    fn emit_seq_fold_empty_delim_split_from_local(
        &self,
        func: &mut Function,
        string_local: u32,
        string_byte_len_local: u32,
        acc_local: u32,
        folder: ValueId,
    ) -> Result<(), CodegenError> {
        self.emit_string_const(func, "");
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));
        func.instruction(&Instruction::LocalGet(acc_local));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        self.emit_indirect_call_two_args(func, folder)?;
        func.instruction(&Instruction::LocalSet(acc_local));

        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7)); // byte idx

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(string_byte_len_local));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        self.emit_utf8_char_width_from_local(
            func,
            string_local,
            self.scratch_i32_7,
            self.scratch_i32_2,
        );
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_8));
        self.emit_string_range_alloc_from_locals(
            func,
            string_local,
            self.scratch_i32_7,
            self.scratch_i32_8,
            self.scratch_i32_5,
            self.scratch_i32,
            self.scratch_i32_2,
        );
        func.instruction(&Instruction::LocalGet(acc_local));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        self.emit_indirect_call_two_args(func, folder)?;
        func.instruction(&Instruction::LocalSet(acc_local));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        self.emit_string_const(func, "");
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));
        func.instruction(&Instruction::LocalGet(acc_local));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        self.emit_indirect_call_two_args(func, folder)?;
        func.instruction(&Instruction::LocalSet(acc_local));
        Ok(())
    }

    fn emit_seq_contains(
        &self,
        func: &mut Function,
        seq: ValueId,
        needle: ValueId,
    ) -> Result<(), CodegenError> {
        self.emit_get(func, seq);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        match self.core_seq_element_ty(seq) {
            Some(Ty::Int) => {
                if !matches!(self.value_ty(needle), Ty::Int) {
                    return Err(CodegenError::UnsupportedInstruction(format!(
                        "seq_contains over Wasm Seq<Int> expects Int needle, got {:?}",
                        self.value_ty(needle)
                    )));
                }
                self.emit_seq_contains_range_from_local(func, self.scratch_i32, needle)?;
            }
            Some(Ty::Char) => {
                if !matches!(self.value_ty(needle), Ty::Char) {
                    return Err(CodegenError::UnsupportedInstruction(format!(
                        "seq_contains over Wasm Seq<Char> expects Char needle, got {:?}",
                        self.value_ty(needle)
                    )));
                }
                self.emit_seq_contains_chars_from_local(func, self.scratch_i32, needle)?;
            }
            Some(Ty::String) => {
                if !matches!(self.value_ty(needle), Ty::String) {
                    return Err(CodegenError::UnsupportedInstruction(format!(
                        "seq_contains over Wasm Seq<String> expects String needle, got {:?}",
                        self.value_ty(needle)
                    )));
                }

                func.instruction(&Instruction::LocalGet(self.scratch_i32));
                func.instruction(&Instruction::I32Load(MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }));
                func.instruction(&Instruction::LocalSet(self.scratch_i32_2));

                func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
                func.instruction(&Instruction::I32Const(2));
                func.instruction(&Instruction::I32Eq);
                func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
                self.emit_seq_contains_lines_from_local(func, self.scratch_i32, needle);
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
                func.instruction(&Instruction::I32Const(3));
                func.instruction(&Instruction::I32Eq);
                func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
                self.emit_seq_contains_split_from_local(func, self.scratch_i32, needle);
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::Unreachable);
                func.instruction(&Instruction::I32Const(0));
                func.instruction(&Instruction::End);
                func.instruction(&Instruction::End);
            }
            Some(elem_ty) => {
                return Err(CodegenError::UnsupportedInstruction(format!(
                    "seq_contains over Wasm seq element type {elem_ty:?} (deferred)"
                )));
            }
            None => {
                return Err(CodegenError::UnsupportedInstruction(format!(
                    "seq_contains over non-Seq receiver in Wasm: {:?}",
                    self.value_ty(seq)
                )));
            }
        }

        Ok(())
    }

    fn emit_seq_contains_range_from_local(
        &self,
        func: &mut Function,
        seq_local: u32,
        needle: ValueId,
    ) -> Result<(), CodegenError> {
        self.emit_seq_load_range_bounds_from_local(func, seq_local)?;
        self.emit_get(func, needle);
        func.instruction(&Instruction::LocalSet(self.scratch_i64_3));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3));

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::LocalGet(self.scratch_i64_2));
        func.instruction(&Instruction::I64GeS);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::I64Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3));
        func.instruction(&Instruction::Br(2));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::I64Const(1));
        func.instruction(&Instruction::I64Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i64));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        Ok(())
    }

    fn emit_seq_contains_chars_from_local(
        &self,
        func: &mut Function,
        seq_local: u32,
        needle: ValueId,
    ) -> Result<(), CodegenError> {
        func.instruction(&Instruction::LocalGet(seq_local));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 8,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));

        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));
        self.emit_get(func, needle);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7));

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        self.emit_utf8_char_width_from_local(
            func,
            self.scratch_i32_3,
            self.scratch_i32_5,
            self.scratch_i32_8,
        );
        self.emit_utf8_codepoint_from_local(
            func,
            self.scratch_i32_3,
            self.scratch_i32_5,
            self.scratch_i32_8,
            self.scratch_i32_2,
        );

        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7));
        func.instruction(&Instruction::Br(2));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        Ok(())
    }

    fn emit_string_slice_equals_string_from_locals(
        &self,
        func: &mut Function,
        base_string_local: u32,
        slice_start_local: u32,
        slice_len_local: u32,
        string_local: u32,
        result_local: u32,
        compare_idx_local: u32,
    ) {
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(result_local));

        func.instruction(&Instruction::LocalGet(slice_len_local));
        func.instruction(&Instruction::LocalGet(string_local));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(compare_idx_local));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::LocalSet(result_local));

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(compare_idx_local));
        func.instruction(&Instruction::LocalGet(slice_len_local));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::LocalGet(base_string_local));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(slice_start_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(compare_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalGet(string_local));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(compare_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Ne);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(result_local));
        func.instruction(&Instruction::Br(2));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(compare_idx_local));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(compare_idx_local));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
    }

    fn emit_string_slice_equals_string_value_from_locals(
        &self,
        func: &mut Function,
        base_string_local: u32,
        slice_start_local: u32,
        slice_len_local: u32,
        string: ValueId,
        result_local: u32,
        compare_idx_local: u32,
    ) {
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(result_local));

        func.instruction(&Instruction::LocalGet(slice_len_local));
        self.emit_get(func, string);
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(compare_idx_local));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::LocalSet(result_local));

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(compare_idx_local));
        func.instruction(&Instruction::LocalGet(slice_len_local));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::LocalGet(base_string_local));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(slice_start_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(compare_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        self.emit_get(func, string);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(compare_idx_local));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Ne);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(result_local));
        func.instruction(&Instruction::Br(2));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(compare_idx_local));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(compare_idx_local));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
    }

    fn emit_seq_contains_lines_from_local(
        &self,
        func: &mut Function,
        seq_local: u32,
        needle: ValueId,
    ) {
        func.instruction(&Instruction::LocalGet(seq_local));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 8,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3));
        self.emit_get(func, needle);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));

        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6)); // scan idx
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7)); // segment start
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_8)); // result

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(10));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32GtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(13));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        self.emit_string_slice_equals_string_from_locals(
            func,
            self.scratch_i32_3,
            self.scratch_i32_7,
            self.scratch_i32_2,
            self.scratch_i32_5,
            self.scratch_i32_8,
            self.scratch_i32,
        );
        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::Br(3));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::I32Eqz);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32LtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2));
        self.emit_string_slice_equals_string_from_locals(
            func,
            self.scratch_i32_3,
            self.scratch_i32_7,
            self.scratch_i32_2,
            self.scratch_i32_5,
            self.scratch_i32_8,
            self.scratch_i32,
        );
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
    }

    fn emit_seq_contains_split_from_local(
        &self,
        func: &mut Function,
        seq_local: u32,
        needle: ValueId,
    ) {
        func.instruction(&Instruction::LocalGet(seq_local));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 8,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3)); // string ptr
        func.instruction(&Instruction::LocalGet(seq_local));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4)); // seq ptr copy
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6)); // string byte len

        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 12,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Eqz);
        func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
        self.emit_seq_contains_empty_delim_split_from_local(
            func,
            self.scratch_i32_3,
            self.scratch_i32_6,
            needle,
        );
        func.instruction(&Instruction::Else);
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7)); // scan idx
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2)); // segment start
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_8)); // result

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 12,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32GtU);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::LocalSet(self.scratch_i32));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 12,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 12,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Ne);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32));
        func.instruction(&Instruction::Br(2));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));
        self.emit_string_slice_equals_string_value_from_locals(
            func,
            self.scratch_i32_3,
            self.scratch_i32_2,
            self.scratch_i32,
            needle,
            self.scratch_i32_8,
            self.scratch_i32_5,
        );
        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::Br(3));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 12,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2));
        func.instruction(&Instruction::Else);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::I32Eqz);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));
        self.emit_string_slice_equals_string_value_from_locals(
            func,
            self.scratch_i32_3,
            self.scratch_i32_2,
            self.scratch_i32,
            needle,
            self.scratch_i32_8,
            self.scratch_i32_5,
        );
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::End);
    }

    fn emit_seq_contains_empty_delim_split_from_local(
        &self,
        func: &mut Function,
        string_local: u32,
        string_byte_len_local: u32,
        needle: ValueId,
    ) {
        self.emit_get(func, needle);
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Eqz);
        func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::Else);

        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7)); // byte idx
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_8)); // result

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(string_byte_len_local));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        self.emit_utf8_char_width_from_local(
            func,
            string_local,
            self.scratch_i32_7,
            self.scratch_i32_4,
        );
        self.emit_string_slice_equals_string_value_from_locals(
            func,
            string_local,
            self.scratch_i32_7,
            self.scratch_i32_4,
            needle,
            self.scratch_i32_8,
            self.scratch_i32_5,
        );
        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::Br(2));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        func.instruction(&Instruction::End);
    }

    fn emit_seq_find(
        &self,
        func: &mut Function,
        seq: ValueId,
        predicate: ValueId,
        result_ty: &Ty,
    ) -> Result<(), CodegenError> {
        self.emit_get(func, seq);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        match self.core_seq_element_ty(seq) {
            Some(Ty::Int) => {
                self.emit_seq_find_range_from_local(func, self.scratch_i32, predicate, result_ty)?;
            }
            Some(Ty::Char) => {
                self.emit_seq_find_chars_from_local(func, self.scratch_i32, predicate, result_ty)?;
            }
            Some(Ty::String) => {
                func.instruction(&Instruction::LocalGet(self.scratch_i32));
                func.instruction(&Instruction::I32Load(MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }));
                func.instruction(&Instruction::LocalSet(self.scratch_i32_2));
                func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
                func.instruction(&Instruction::I32Const(2));
                func.instruction(&Instruction::I32Eq);
                func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
                self.emit_seq_find_lines_from_local(func, self.scratch_i32, predicate, result_ty)?;
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
                func.instruction(&Instruction::I32Const(3));
                func.instruction(&Instruction::I32Eq);
                func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
                self.emit_seq_find_split_from_local(func, self.scratch_i32, predicate, result_ty)?;
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::Unreachable);
                func.instruction(&Instruction::I32Const(0));
                func.instruction(&Instruction::End);
                func.instruction(&Instruction::End);
            }
            Some(elem_ty) => {
                return Err(CodegenError::UnsupportedInstruction(format!(
                    "seq_find over Wasm seq element type {elem_ty:?} (deferred)"
                )));
            }
            None => {
                return Err(CodegenError::UnsupportedInstruction(format!(
                    "seq_find over non-Seq receiver in Wasm: {:?}",
                    self.value_ty(seq)
                )));
            }
        }

        Ok(())
    }

    fn emit_seq_find_range_from_local(
        &self,
        func: &mut Function,
        seq_local: u32,
        predicate: ValueId,
        result_ty: &Ty,
    ) -> Result<(), CodegenError> {
        self.emit_seq_load_range_bounds_from_local(func, seq_local)?;
        self.emit_option_none(func, result_ty, self.scratch_i32_3)?;

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::LocalGet(self.scratch_i64_2));
        func.instruction(&Instruction::I64GeS);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        self.emit_indirect_call_single_arg(func, predicate)?;
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_single_field_adt_from_local(
            func,
            result_ty,
            "Some",
            self.scratch_i64,
            &Ty::Int,
            self.scratch_i32_3,
        )?;
        func.instruction(&Instruction::Br(2));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::I64Const(1));
        func.instruction(&Instruction::I64Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i64));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        Ok(())
    }

    fn emit_seq_find_chars_from_local(
        &self,
        func: &mut Function,
        seq_local: u32,
        predicate: ValueId,
        result_ty: &Ty,
    ) -> Result<(), CodegenError> {
        func.instruction(&Instruction::LocalGet(seq_local));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 8,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));

        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));
        self.emit_option_none(func, result_ty, self.scratch_i32_6)?;

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        self.emit_utf8_char_width_from_local(
            func,
            self.scratch_i32_3,
            self.scratch_i32_5,
            self.scratch_i32_7,
        );
        self.emit_utf8_codepoint_from_local(
            func,
            self.scratch_i32_3,
            self.scratch_i32_5,
            self.scratch_i32_7,
            self.scratch_i32_8,
        );

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        self.emit_indirect_call_single_arg(func, predicate)?;
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_single_field_adt_from_local(
            func,
            result_ty,
            "Some",
            self.scratch_i32_8,
            &Ty::Char,
            self.scratch_i32_6,
        )?;
        func.instruction(&Instruction::Br(2));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        Ok(())
    }

    fn emit_seq_find_lines_from_local(
        &self,
        func: &mut Function,
        seq_local: u32,
        predicate: ValueId,
        result_ty: &Ty,
    ) -> Result<(), CodegenError> {
        func.instruction(&Instruction::LocalGet(seq_local));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 8,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3)); // string ptr

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4)); // byte len

        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6)); // scan idx
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7)); // segment start
        self.emit_option_none(func, result_ty, self.scratch_i32_8)?;

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(10));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2)); // segment len

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32GtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(13));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));
        self.emit_string_range_alloc_from_locals(
            func,
            self.scratch_i32_3,
            self.scratch_i32_7,
            self.scratch_i32,
            self.scratch_i32_5,
            self.scratch_i32_9,
            self.scratch_i32_10,
        );
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        self.emit_indirect_call_single_arg(func, predicate)?;
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_single_field_adt_from_local(
            func,
            result_ty,
            "Some",
            self.scratch_i32_5,
            &Ty::String,
            self.scratch_i32_8,
        )?;
        func.instruction(&Instruction::Br(3));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32LtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_string_range_alloc_from_locals(
            func,
            self.scratch_i32_3,
            self.scratch_i32_7,
            self.scratch_i32_4,
            self.scratch_i32_5,
            self.scratch_i32_9,
            self.scratch_i32_10,
        );
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        self.emit_indirect_call_single_arg(func, predicate)?;
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_single_field_adt_from_local(
            func,
            result_ty,
            "Some",
            self.scratch_i32_5,
            &Ty::String,
            self.scratch_i32_8,
        )?;
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        Ok(())
    }

    fn emit_seq_find_split_from_local(
        &self,
        func: &mut Function,
        seq_local: u32,
        predicate: ValueId,
        result_ty: &Ty,
    ) -> Result<(), CodegenError> {
        func.instruction(&Instruction::LocalGet(seq_local));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 8,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3)); // string ptr
        func.instruction(&Instruction::LocalGet(seq_local));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 12,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4)); // delim ptr
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6)); // string byte len
        self.emit_option_none(func, result_ty, self.scratch_i32_8)?;

        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5)); // delim byte len
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::I32Eqz);
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_seq_find_empty_delim_split_from_local(func, predicate, result_ty)?;
        func.instruction(&Instruction::Else);

        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7)); // scan idx
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2)); // segment start

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32GtU);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::LocalSet(self.scratch_i32)); // matched
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_9)); // inner offset

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_9));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_9));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_9));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Load8U(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Ne);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32));
        func.instruction(&Instruction::Br(2));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_9));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_9));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_string_range_alloc_from_locals(
            func,
            self.scratch_i32_3,
            self.scratch_i32_2,
            self.scratch_i32_7,
            self.scratch_i32_10,
            self.scratch_i32,
            self.scratch_i32_9,
        );
        func.instruction(&Instruction::LocalGet(self.scratch_i32_10));
        self.emit_indirect_call_single_arg(func, predicate)?;
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_single_field_adt_from_local(
            func,
            result_ty,
            "Some",
            self.scratch_i32_10,
            &Ty::String,
            self.scratch_i32_8,
        )?;
        func.instruction(&Instruction::Br(3));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2));
        func.instruction(&Instruction::Else);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        self.emit_string_range_alloc_from_locals(
            func,
            self.scratch_i32_3,
            self.scratch_i32_2,
            self.scratch_i32_6,
            self.scratch_i32_10,
            self.scratch_i32,
            self.scratch_i32_9,
        );
        func.instruction(&Instruction::LocalGet(self.scratch_i32_10));
        self.emit_indirect_call_single_arg(func, predicate)?;
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_single_field_adt_from_local(
            func,
            result_ty,
            "Some",
            self.scratch_i32_10,
            &Ty::String,
            self.scratch_i32_8,
        )?;
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_8));
        Ok(())
    }

    fn emit_seq_find_empty_delim_split_from_local(
        &self,
        func: &mut Function,
        predicate: ValueId,
        result_ty: &Ty,
    ) -> Result<(), CodegenError> {
        func.instruction(&Instruction::Block(BlockType::Empty));

        self.emit_string_const(func, "");
        func.instruction(&Instruction::LocalSet(self.scratch_i32_10));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_10));
        self.emit_indirect_call_single_arg(func, predicate)?;
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_single_field_adt_from_local(
            func,
            result_ty,
            "Some",
            self.scratch_i32_10,
            &Ty::String,
            self.scratch_i32_8,
        )?;
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7)); // byte idx

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::BrIf(1));

        self.emit_utf8_char_width_from_local(
            func,
            self.scratch_i32_3,
            self.scratch_i32_7,
            self.scratch_i32_9,
        );
        func.instruction(&Instruction::LocalGet(self.scratch_i32_7));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_9));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));
        self.emit_string_range_alloc_from_locals(
            func,
            self.scratch_i32_3,
            self.scratch_i32_7,
            self.scratch_i32_5,
            self.scratch_i32_10,
            self.scratch_i32,
            self.scratch_i32_2,
        );
        func.instruction(&Instruction::LocalGet(self.scratch_i32_10));
        self.emit_indirect_call_single_arg(func, predicate)?;
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_single_field_adt_from_local(
            func,
            result_ty,
            "Some",
            self.scratch_i32_10,
            &Ty::String,
            self.scratch_i32_8,
        )?;
        func.instruction(&Instruction::Br(2));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_7));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        self.emit_string_const(func, "");
        func.instruction(&Instruction::LocalSet(self.scratch_i32_10));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_10));
        self.emit_indirect_call_single_arg(func, predicate)?;
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_single_field_adt_from_local(
            func,
            result_ty,
            "Some",
            self.scratch_i32_10,
            &Ty::String,
            self.scratch_i32_8,
        )?;
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::End);
        Ok(())
    }

    fn emit_seq_load_range_bounds_from_local(
        &self,
        func: &mut Function,
        seq_local: u32,
    ) -> Result<(), CodegenError> {
        func.instruction(&Instruction::LocalGet(seq_local));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::I32Ne);
        self.emit_trap_if_true(func);

        func.instruction(&Instruction::LocalGet(seq_local));
        func.instruction(&Instruction::I64Load(MemArg {
            offset: 8,
            align: 3,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i64));

        func.instruction(&Instruction::LocalGet(seq_local));
        func.instruction(&Instruction::I64Load(MemArg {
            offset: 16,
            align: 3,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i64_2));
        Ok(())
    }

    fn emit_option_unwrap_or(
        &self,
        func: &mut Function,
        option: ValueId,
        fallback: ValueId,
        result_ty: &Ty,
    ) -> Result<(), CodegenError> {
        let input_option_ty = self.value_ty(option);
        let (some_ty, some_tag, none_tag) =
            self.lookup_option_variant_tags(input_option_ty, "option_unwrap_or")?;
        let temp_local = self.temp_local_for_ty(result_ty);

        self.emit_get(func, option);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Const(some_tag as i32));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_option_payload_load_from_local(func, self.scratch_i32, some_ty);
        func.instruction(&Instruction::LocalSet(temp_local));
        func.instruction(&Instruction::Else);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Const(none_tag as i32));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::I32Eqz);
        self.emit_trap_if_true(func);
        self.emit_get(func, fallback);
        func.instruction(&Instruction::LocalSet(temp_local));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(temp_local));
        Ok(())
    }

    fn emit_option_map(
        &self,
        func: &mut Function,
        option: ValueId,
        mapper: ValueId,
        result_ty: &Ty,
    ) -> Result<(), CodegenError> {
        let input_option_ty = self.value_ty(option);
        let (input_some_ty, input_some_tag, input_none_tag) =
            self.lookup_option_variant_tags(input_option_ty, "option_map")?;
        let (output_some_ty, _output_some_tag, _output_none_tag) =
            self.lookup_option_variant_tags(result_ty, "option_map")?;
        let mapped_local = self.temp_local_for_ty(output_some_ty);

        self.emit_get(func, option);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Const(input_some_tag as i32));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_option_payload_load_from_local(func, self.scratch_i32, input_some_ty);
        self.emit_indirect_call_single_arg(func, mapper)?;
        func.instruction(&Instruction::LocalSet(mapped_local));
        self.emit_single_field_adt_from_local(
            func,
            result_ty,
            "Some",
            mapped_local,
            output_some_ty,
            self.scratch_i32_6,
        )?;
        func.instruction(&Instruction::Else);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Const(input_none_tag as i32));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::I32Eqz);
        self.emit_trap_if_true(func);
        self.emit_option_none(func, result_ty, self.scratch_i32_6)?;
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        Ok(())
    }

    fn emit_option_and_then(
        &self,
        func: &mut Function,
        option: ValueId,
        mapper: ValueId,
        result_ty: &Ty,
    ) -> Result<(), CodegenError> {
        let input_option_ty = self.value_ty(option);
        let (input_some_ty, input_some_tag, input_none_tag) =
            self.lookup_option_variant_tags(input_option_ty, "option_and_then")?;

        self.emit_get(func, option);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Const(input_some_tag as i32));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_option_payload_load_from_local(func, self.scratch_i32, input_some_ty);
        self.emit_indirect_call_single_arg(func, mapper)?;
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6));
        func.instruction(&Instruction::Else);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Const(input_none_tag as i32));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::I32Eqz);
        self.emit_trap_if_true(func);
        self.emit_option_none(func, result_ty, self.scratch_i32_6)?;
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        Ok(())
    }

    fn emit_option_map_or(
        &self,
        func: &mut Function,
        option: ValueId,
        fallback: ValueId,
        mapper: ValueId,
        result_ty: &Ty,
    ) -> Result<(), CodegenError> {
        let input_option_ty = self.value_ty(option);
        let (input_some_ty, input_some_tag, input_none_tag) =
            self.lookup_option_variant_tags(input_option_ty, "option_map_or")?;
        let temp_local = self.temp_local_for_ty(result_ty);

        self.emit_get(func, option);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Const(input_some_tag as i32));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_option_payload_load_from_local(func, self.scratch_i32, input_some_ty);
        self.emit_indirect_call_single_arg(func, mapper)?;
        func.instruction(&Instruction::LocalSet(temp_local));
        func.instruction(&Instruction::Else);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Const(input_none_tag as i32));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::I32Eqz);
        self.emit_trap_if_true(func);
        self.emit_get(func, fallback);
        func.instruction(&Instruction::LocalSet(temp_local));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(temp_local));
        Ok(())
    }

    fn lookup_option_variant_tags<'b>(
        &self,
        option_ty: &'b Ty,
        op: &str,
    ) -> Result<(&'b Ty, u32, u32), CodegenError> {
        let Ty::Adt { def, args } = option_ty else {
            return Err(CodegenError::UnsupportedInstruction(format!(
                "{op} expected Option<_>, got {option_ty:?}"
            )));
        };
        let some_ty = args.first().ok_or_else(|| {
            CodegenError::UnsupportedInstruction(format!(
                "{op} expected Option<_>, got {option_ty:?}"
            ))
        })?;

        let layout = self
            .ctx
            .adt_layouts
            .get(def)
            .ok_or(CodegenError::MissingAdtDef)?;
        let type_item = &self.ctx.item_tree.types[*def];
        let TypeDefKind::Adt { variants } = &type_item.kind else {
            return Err(CodegenError::MissingAdtDef);
        };
        let some_variant = variants
            .iter()
            .find(|variant| variant.name.resolve(self.ctx.interner) == "Some")
            .ok_or(CodegenError::MissingAdtDef)?;
        let none_variant = variants
            .iter()
            .find(|variant| variant.name.resolve(self.ctx.interner) == "None")
            .ok_or(CodegenError::MissingAdtDef)?;
        let some_tag = *layout
            .tag_map
            .get(&some_variant.name)
            .ok_or(CodegenError::MissingAdtDef)?;
        let none_tag = *layout
            .tag_map
            .get(&none_variant.name)
            .ok_or(CodegenError::MissingAdtDef)?;
        Ok((some_ty, some_tag, none_tag))
    }

    fn emit_option_payload_load_from_local(
        &self,
        func: &mut Function,
        option_ptr_local: u32,
        payload_ty: &Ty,
    ) {
        func.instruction(&Instruction::LocalGet(option_ptr_local));
        self.emit_typed_load(func, payload_ty, u64::from(AdtLayout::field_offset(0)));
    }

    fn emit_option_none(
        &self,
        func: &mut Function,
        option_ty: &Ty,
        out_local: u32,
    ) -> Result<(), CodegenError> {
        let Ty::Adt { def, .. } = option_ty else {
            return Err(CodegenError::UnsupportedInstruction(format!(
                "expected Option<_>, got {option_ty:?}"
            )));
        };

        let layout = self
            .ctx
            .adt_layouts
            .get(def)
            .ok_or(CodegenError::MissingAdtDef)?;
        let type_item = &self.ctx.item_tree.types[*def];
        let TypeDefKind::Adt { variants } = &type_item.kind else {
            return Err(CodegenError::MissingAdtDef);
        };
        let none_variant = variants
            .iter()
            .find(|variant| variant.name.resolve(self.ctx.interner) == "None")
            .ok_or(CodegenError::MissingAdtDef)?;
        let none_tag = *layout
            .tag_map
            .get(&none_variant.name)
            .ok_or(CodegenError::MissingAdtDef)?;

        func.instruction(&Instruction::I32Const(layout.size as i32));
        func.instruction(&Instruction::Call(self.ctx.alloc_fn_index));
        func.instruction(&Instruction::Drop);

        let derive_ptr = |func: &mut Function, size: u32| {
            func.instruction(&Instruction::GlobalGet(0));
            func.instruction(&Instruction::I32Const(size as i32));
            func.instruction(&Instruction::I32Sub);
        };

        derive_ptr(func, layout.size);
        func.instruction(&Instruction::I32Const(none_tag as i32));
        func.instruction(&Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));

        derive_ptr(func, layout.size);
        func.instruction(&Instruction::LocalSet(out_local));
        Ok(())
    }

    fn emit_result_unwrap_or(
        &self,
        func: &mut Function,
        result: ValueId,
        fallback: ValueId,
        result_ty: &Ty,
    ) -> Result<(), CodegenError> {
        let input_result_ty = self.value_ty(result);
        let (ok_ty, _err_ty, ok_tag, _err_tag) =
            self.lookup_result_variant_tags(input_result_ty, "result_unwrap_or")?;
        let temp_local = self.temp_local_for_ty(result_ty);

        self.emit_get(func, result);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(ok_tag as i32));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_result_payload_load_from_local(func, self.scratch_i32, ok_ty);
        func.instruction(&Instruction::LocalSet(temp_local));
        func.instruction(&Instruction::Else);
        self.emit_get(func, fallback);
        func.instruction(&Instruction::LocalSet(temp_local));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(temp_local));
        Ok(())
    }

    fn emit_result_map(
        &self,
        func: &mut Function,
        result: ValueId,
        mapper: ValueId,
        result_ty: &Ty,
    ) -> Result<(), CodegenError> {
        let input_result_ty = self.value_ty(result);
        let (input_ok_ty, input_err_ty, input_ok_tag, _input_err_tag) =
            self.lookup_result_variant_tags(input_result_ty, "result_map")?;
        let (output_ok_ty, output_err_ty, _output_ok_tag, _output_err_tag) =
            self.lookup_result_variant_tags(result_ty, "result_map")?;
        let mapped_local = self.temp_local_for_ty(output_ok_ty);
        let err_local = self.temp_local_for_ty(output_err_ty);

        self.emit_get(func, result);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(input_ok_tag as i32));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_result_payload_load_from_local(func, self.scratch_i32, input_ok_ty);
        self.emit_indirect_call_single_arg(func, mapper)?;
        func.instruction(&Instruction::LocalSet(mapped_local));
        self.emit_single_field_adt_from_local(
            func,
            result_ty,
            "Ok",
            mapped_local,
            output_ok_ty,
            self.scratch_i32_6,
        )?;
        func.instruction(&Instruction::Else);
        self.emit_result_payload_load_from_local(func, self.scratch_i32, input_err_ty);
        func.instruction(&Instruction::LocalSet(err_local));
        self.emit_single_field_adt_from_local(
            func,
            result_ty,
            "Err",
            err_local,
            output_err_ty,
            self.scratch_i32_6,
        )?;
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        Ok(())
    }

    fn emit_result_and_then(
        &self,
        func: &mut Function,
        result: ValueId,
        mapper: ValueId,
        result_ty: &Ty,
    ) -> Result<(), CodegenError> {
        let input_result_ty = self.value_ty(result);
        let (input_ok_ty, input_err_ty, input_ok_tag, _input_err_tag) =
            self.lookup_result_variant_tags(input_result_ty, "result_and_then")?;
        let (_output_ok_ty, output_err_ty, _output_ok_tag, _output_err_tag) =
            self.lookup_result_variant_tags(result_ty, "result_and_then")?;
        let err_local = self.temp_local_for_ty(output_err_ty);

        self.emit_get(func, result);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(input_ok_tag as i32));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_result_payload_load_from_local(func, self.scratch_i32, input_ok_ty);
        self.emit_indirect_call_single_arg(func, mapper)?;
        func.instruction(&Instruction::LocalSet(self.scratch_i32_6));
        func.instruction(&Instruction::Else);
        self.emit_result_payload_load_from_local(func, self.scratch_i32, input_err_ty);
        func.instruction(&Instruction::LocalSet(err_local));
        self.emit_single_field_adt_from_local(
            func,
            result_ty,
            "Err",
            err_local,
            output_err_ty,
            self.scratch_i32_6,
        )?;
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        Ok(())
    }

    fn emit_result_map_err(
        &self,
        func: &mut Function,
        result: ValueId,
        mapper: ValueId,
        result_ty: &Ty,
    ) -> Result<(), CodegenError> {
        let input_result_ty = self.value_ty(result);
        let (input_ok_ty, input_err_ty, input_ok_tag, _input_err_tag) =
            self.lookup_result_variant_tags(input_result_ty, "result_map_err")?;
        let (output_ok_ty, output_err_ty, _output_ok_tag, _output_err_tag) =
            self.lookup_result_variant_tags(result_ty, "result_map_err")?;
        let ok_local = self.temp_local_for_ty(output_ok_ty);
        let mapped_err_local = self.temp_local_for_ty(output_err_ty);

        self.emit_get(func, result);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(input_ok_tag as i32));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_result_payload_load_from_local(func, self.scratch_i32, input_ok_ty);
        func.instruction(&Instruction::LocalSet(ok_local));
        self.emit_single_field_adt_from_local(
            func,
            result_ty,
            "Ok",
            ok_local,
            output_ok_ty,
            self.scratch_i32_6,
        )?;
        func.instruction(&Instruction::Else);
        self.emit_result_payload_load_from_local(func, self.scratch_i32, input_err_ty);
        self.emit_indirect_call_single_arg(func, mapper)?;
        func.instruction(&Instruction::LocalSet(mapped_err_local));
        self.emit_single_field_adt_from_local(
            func,
            result_ty,
            "Err",
            mapped_err_local,
            output_err_ty,
            self.scratch_i32_6,
        )?;
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        Ok(())
    }

    fn emit_result_map_or(
        &self,
        func: &mut Function,
        result: ValueId,
        fallback: ValueId,
        mapper: ValueId,
        result_ty: &Ty,
    ) -> Result<(), CodegenError> {
        let input_result_ty = self.value_ty(result);
        let (input_ok_ty, _input_err_ty, input_ok_tag, _input_err_tag) =
            self.lookup_result_variant_tags(input_result_ty, "result_map_or")?;
        let temp_local = self.temp_local_for_ty(result_ty);

        self.emit_get(func, result);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::I32Const(input_ok_tag as i32));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_result_payload_load_from_local(func, self.scratch_i32, input_ok_ty);
        self.emit_indirect_call_single_arg(func, mapper)?;
        func.instruction(&Instruction::LocalSet(temp_local));
        func.instruction(&Instruction::Else);
        self.emit_get(func, fallback);
        func.instruction(&Instruction::LocalSet(temp_local));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(temp_local));
        Ok(())
    }

    fn lookup_result_variant_tags<'b>(
        &self,
        result_ty: &'b Ty,
        op: &str,
    ) -> Result<(&'b Ty, &'b Ty, u32, u32), CodegenError> {
        let Ty::Adt { def, args } = result_ty else {
            return Err(CodegenError::UnsupportedInstruction(format!(
                "{op} expected Result<_, _>, got {result_ty:?}"
            )));
        };
        let ok_ty = args.first().ok_or_else(|| {
            CodegenError::UnsupportedInstruction(format!(
                "{op} expected Result<_, _>, got {result_ty:?}"
            ))
        })?;
        let err_ty = args.get(1).ok_or_else(|| {
            CodegenError::UnsupportedInstruction(format!(
                "{op} expected Result<_, _>, got {result_ty:?}"
            ))
        })?;

        let layout = self
            .ctx
            .adt_layouts
            .get(def)
            .ok_or(CodegenError::MissingAdtDef)?;
        let type_item = &self.ctx.item_tree.types[*def];
        let TypeDefKind::Adt { variants } = &type_item.kind else {
            return Err(CodegenError::MissingAdtDef);
        };
        let ok_variant = variants
            .iter()
            .find(|variant| variant.name.resolve(self.ctx.interner) == "Ok")
            .ok_or(CodegenError::MissingAdtDef)?;
        let err_variant = variants
            .iter()
            .find(|variant| variant.name.resolve(self.ctx.interner) == "Err")
            .ok_or(CodegenError::MissingAdtDef)?;
        let ok_tag = *layout
            .tag_map
            .get(&ok_variant.name)
            .ok_or(CodegenError::MissingAdtDef)?;
        let err_tag = *layout
            .tag_map
            .get(&err_variant.name)
            .ok_or(CodegenError::MissingAdtDef)?;
        Ok((ok_ty, err_ty, ok_tag, err_tag))
    }

    fn emit_result_payload_load_from_local(
        &self,
        func: &mut Function,
        result_ptr_local: u32,
        payload_ty: &Ty,
    ) {
        func.instruction(&Instruction::LocalGet(result_ptr_local));
        self.emit_typed_load(func, payload_ty, u64::from(AdtLayout::field_offset(0)));
    }

    fn emit_indirect_call_single_arg(
        &self,
        func: &mut Function,
        callee: ValueId,
    ) -> Result<(), CodegenError> {
        self.emit_get(func, callee);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::GlobalSet(
            self.ctx.current_closure_global_index,
        ));
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        let sig = FnTypeKey::from_ty(self.value_ty(callee))?;
        let type_index = self.ctx.fn_type_map.get(&sig).ok_or_else(|| {
            CodegenError::UnsupportedInstruction(format!(
                "missing wasm function type for indirect callee {:?}",
                self.value_ty(callee)
            ))
        })?;
        func.instruction(&Instruction::CallIndirect {
            type_index: *type_index,
            table_index: 0,
        });
        Ok(())
    }

    fn emit_indirect_call_two_args(
        &self,
        func: &mut Function,
        callee: ValueId,
    ) -> Result<(), CodegenError> {
        self.emit_get(func, callee);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::GlobalSet(
            self.ctx.current_closure_global_index,
        ));
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        let sig = FnTypeKey::from_ty(self.value_ty(callee))?;
        let type_index = self.ctx.fn_type_map.get(&sig).ok_or_else(|| {
            CodegenError::UnsupportedInstruction(format!(
                "missing wasm function type for indirect callee {:?}",
                self.value_ty(callee)
            ))
        })?;
        func.instruction(&Instruction::CallIndirect {
            type_index: *type_index,
            table_index: 0,
        });
        Ok(())
    }

    fn emit_closure_create(
        &self,
        func: &mut Function,
        name: Name,
        captures: &[ValueId],
    ) -> Result<(), CodegenError> {
        let slot = self.ctx.fn_table_map.get(&name).ok_or_else(|| {
            CodegenError::MissingFunction(name.resolve(self.ctx.interner).to_owned())
        })?;
        let size = closure_object_size(captures.len());

        func.instruction(&Instruction::I32Const(size));
        func.instruction(&Instruction::Call(self.ctx.alloc_fn_index));
        func.instruction(&Instruction::Drop);

        func.instruction(&Instruction::GlobalGet(0));
        func.instruction(&Instruction::I32Const(size));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Const(*slot as i32));
        func.instruction(&Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));

        for (index, capture) in captures.iter().enumerate() {
            func.instruction(&Instruction::LocalGet(self.scratch_i32));
            self.emit_typed_store(
                func,
                *capture,
                self.value_ty(*capture),
                closure_capture_offset(index),
            );
        }

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        Ok(())
    }

    fn temp_local_for_ty(&self, ty: &Ty) -> u32 {
        match ty {
            Ty::Float => self.scratch_f64,
            Ty::Int => self.scratch_i64,
            _ => self.scratch_i32_7,
        }
    }

    fn emit_parse_int_result(
        &self,
        func: &mut Function,
        value: ValueId,
        result_ty: &Ty,
    ) -> Result<(), CodegenError> {
        let Ty::Adt { args, .. } = result_ty else {
            return Err(CodegenError::UnsupportedInstruction(format!(
                "parse_int expected Result<Int, ParseError>, got {result_ty:?}"
            )));
        };
        let ok_ty = args.first().ok_or_else(|| {
            CodegenError::UnsupportedInstruction(format!(
                "parse_int expected Result<Int, ParseError>, got {result_ty:?}"
            ))
        })?;
        let err_ty = args.get(1).ok_or_else(|| {
            CodegenError::UnsupportedInstruction(format!(
                "parse_int expected Result<Int, ParseError>, got {result_ty:?}"
            ))
        })?;

        func.instruction(&Instruction::I32Const(12));
        func.instruction(&Instruction::Call(self.ctx.alloc_fn_index));
        func.instruction(&Instruction::LocalSet(self.scratch_i32)); // parse buffer

        self.emit_get(func, value);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        self.emit_get(func, value);
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::Call(
            self.ctx
                .parse_int_fn_index
                .expect("parse_int helper should be imported when used"),
        ));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2)); // status

        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32GtU);
        self.emit_trap_if_true(func);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 8,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3)); // error message ptr
        self.emit_single_field_adt_from_local(
            func,
            err_ty,
            "InvalidInt",
            self.scratch_i32_3,
            &Ty::String,
            self.scratch_i32_4,
        )?;
        self.emit_single_field_adt_from_local(
            func,
            result_ty,
            "Err",
            self.scratch_i32_4,
            err_ty,
            self.scratch_i32_6,
        )?;
        func.instruction(&Instruction::Else);
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I64Load(MemArg {
            offset: 0,
            align: 3,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i64));
        self.emit_single_field_adt_from_local(
            func,
            result_ty,
            "Ok",
            self.scratch_i64,
            ok_ty,
            self.scratch_i32_6,
        )?;
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        Ok(())
    }

    fn emit_parse_float_result(
        &self,
        func: &mut Function,
        value: ValueId,
        result_ty: &Ty,
    ) -> Result<(), CodegenError> {
        let Ty::Adt { args, .. } = result_ty else {
            return Err(CodegenError::UnsupportedInstruction(format!(
                "parse_float expected Result<Float, ParseError>, got {result_ty:?}"
            )));
        };
        let ok_ty = args.first().ok_or_else(|| {
            CodegenError::UnsupportedInstruction(format!(
                "parse_float expected Result<Float, ParseError>, got {result_ty:?}"
            ))
        })?;
        let err_ty = args.get(1).ok_or_else(|| {
            CodegenError::UnsupportedInstruction(format!(
                "parse_float expected Result<Float, ParseError>, got {result_ty:?}"
            ))
        })?;

        func.instruction(&Instruction::I32Const(12));
        func.instruction(&Instruction::Call(self.ctx.alloc_fn_index));
        func.instruction(&Instruction::LocalSet(self.scratch_i32)); // parse buffer

        self.emit_get(func, value);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        self.emit_get(func, value);
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::Call(
            self.ctx
                .parse_float_fn_index
                .expect("parse_float helper should be imported when used"),
        ));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2)); // status

        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32GtU);
        self.emit_trap_if_true(func);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 8,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3)); // error message ptr
        self.emit_single_field_adt_from_local(
            func,
            err_ty,
            "InvalidFloat",
            self.scratch_i32_3,
            &Ty::String,
            self.scratch_i32_4,
        )?;
        self.emit_single_field_adt_from_local(
            func,
            result_ty,
            "Err",
            self.scratch_i32_4,
            err_ty,
            self.scratch_i32_6,
        )?;
        func.instruction(&Instruction::Else);
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::F64Load(MemArg {
            offset: 0,
            align: 3,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_f64));
        self.emit_single_field_adt_from_local(
            func,
            result_ty,
            "Ok",
            self.scratch_f64,
            ok_ty,
            self.scratch_i32_6,
        )?;
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_6));
        Ok(())
    }

    fn emit_int_abs(&self, func: &mut Function, value: ValueId) {
        self.emit_get(func, value);
        func.instruction(&Instruction::I64Const(i64::MIN));
        func.instruction(&Instruction::I64Eq);
        self.emit_trap_if_true(func);

        self.emit_get(func, value);
        func.instruction(&Instruction::LocalSet(self.scratch_i64));
        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::I64Const(0));
        func.instruction(&Instruction::I64LtS);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I64Const(0));
        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::I64Sub);
        func.instruction(&Instruction::LocalSet(self.scratch_i64));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::LocalGet(self.scratch_i64));
    }

    fn emit_int_min_max(&self, func: &mut Function, lhs: ValueId, rhs: ValueId, pick_min: bool) {
        self.emit_get(func, lhs);
        func.instruction(&Instruction::LocalSet(self.scratch_i64));
        self.emit_get(func, rhs);
        func.instruction(&Instruction::LocalSet(self.scratch_i64_2));

        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::LocalGet(self.scratch_i64_2));
        if pick_min {
            func.instruction(&Instruction::I64LeS);
        } else {
            func.instruction(&Instruction::I64GeS);
        }
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::LocalSet(self.scratch_i64_3));
        func.instruction(&Instruction::Else);
        func.instruction(&Instruction::LocalGet(self.scratch_i64_2));
        func.instruction(&Instruction::LocalSet(self.scratch_i64_3));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
    }

    fn emit_unsigned_abs_to_local(&self, func: &mut Function, value: ValueId, target_local: u32) {
        self.emit_get(func, value);
        func.instruction(&Instruction::LocalSet(target_local));
        func.instruction(&Instruction::LocalGet(target_local));
        func.instruction(&Instruction::I64Const(0));
        func.instruction(&Instruction::I64LtS);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I64Const(0));
        func.instruction(&Instruction::LocalGet(target_local));
        func.instruction(&Instruction::I64Sub);
        func.instruction(&Instruction::LocalSet(target_local));
        func.instruction(&Instruction::End);
    }

    fn emit_unsigned_gcd_loop(
        &self,
        func: &mut Function,
        a_local: u32,
        b_local: u32,
        tmp_local: u32,
    ) {
        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(b_local));
        func.instruction(&Instruction::I64Eqz);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::LocalGet(b_local));
        func.instruction(&Instruction::LocalSet(tmp_local));

        func.instruction(&Instruction::LocalGet(a_local));
        func.instruction(&Instruction::LocalGet(b_local));
        func.instruction(&Instruction::I64RemU);
        func.instruction(&Instruction::LocalSet(b_local));

        func.instruction(&Instruction::LocalGet(tmp_local));
        func.instruction(&Instruction::LocalSet(a_local));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
    }

    fn emit_gcd(&self, func: &mut Function, lhs: ValueId, rhs: ValueId) {
        self.emit_unsigned_abs_to_local(func, lhs, self.scratch_i64);
        self.emit_unsigned_abs_to_local(func, rhs, self.scratch_i64_2);
        self.emit_unsigned_gcd_loop(
            func,
            self.scratch_i64,
            self.scratch_i64_2,
            self.scratch_i64_3,
        );

        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::I64Const(0));
        func.instruction(&Instruction::I64LtS);
        self.emit_trap_if_true(func);
        func.instruction(&Instruction::LocalGet(self.scratch_i64));
    }

    fn emit_lcm(&self, func: &mut Function, lhs: ValueId, rhs: ValueId) {
        self.emit_get(func, lhs);
        func.instruction(&Instruction::I64Eqz);
        self.emit_get(func, rhs);
        func.instruction(&Instruction::I64Eqz);
        func.instruction(&Instruction::I32Or);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I64Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i64_3));
        func.instruction(&Instruction::Else);
        self.emit_unsigned_abs_to_local(func, lhs, self.scratch_i64);
        self.emit_unsigned_abs_to_local(func, rhs, self.scratch_i64_2);
        self.emit_unsigned_gcd_loop(
            func,
            self.scratch_i64,
            self.scratch_i64_2,
            self.scratch_i64_3,
        );

        self.emit_unsigned_abs_to_local(func, lhs, self.scratch_i64);
        self.emit_unsigned_abs_to_local(func, rhs, self.scratch_i64_2);

        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::I64DivU);
        func.instruction(&Instruction::LocalSet(self.scratch_i64));

        func.instruction(&Instruction::I64Const(i64::MAX));
        func.instruction(&Instruction::LocalGet(self.scratch_i64_2));
        func.instruction(&Instruction::I64DivU);
        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::I64LtU);
        self.emit_trap_if_true(func);

        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::LocalGet(self.scratch_i64_2));
        func.instruction(&Instruction::I64Mul);
        func.instruction(&Instruction::LocalSet(self.scratch_i64_3));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
    }

    fn emit_int_pow(&self, func: &mut Function, base: ValueId, exp: ValueId) {
        self.emit_get(func, exp);
        func.instruction(&Instruction::I64Const(0));
        func.instruction(&Instruction::I64LtS);
        self.emit_trap_if_true(func);

        func.instruction(&Instruction::I64Const(1));
        func.instruction(&Instruction::LocalSet(self.scratch_i64));
        self.emit_get(func, base);
        func.instruction(&Instruction::LocalSet(self.scratch_i64_2));
        self.emit_get(func, exp);
        func.instruction(&Instruction::LocalSet(self.scratch_i64_3));

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::I64Eqz);
        func.instruction(&Instruction::BrIf(1));

        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::I64Const(1));
        func.instruction(&Instruction::I64And);
        func.instruction(&Instruction::I64Const(1));
        func.instruction(&Instruction::I64Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        self.emit_checked_int_mul_locals(
            func,
            self.scratch_i64,
            self.scratch_i64_2,
            self.scratch_i64_4,
        );
        func.instruction(&Instruction::LocalGet(self.scratch_i64_4));
        func.instruction(&Instruction::LocalSet(self.scratch_i64));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::I64Const(1));
        func.instruction(&Instruction::I64ShrU);
        func.instruction(&Instruction::LocalSet(self.scratch_i64_3));

        func.instruction(&Instruction::LocalGet(self.scratch_i64_3));
        func.instruction(&Instruction::I64Eqz);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::Else);
        self.emit_checked_int_mul_locals(
            func,
            self.scratch_i64_2,
            self.scratch_i64_2,
            self.scratch_i64_4,
        );
        func.instruction(&Instruction::LocalGet(self.scratch_i64_4));
        func.instruction(&Instruction::LocalSet(self.scratch_i64_2));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i64));
    }

    fn emit_float_is_nan(&self, func: &mut Function, value: ValueId) {
        self.emit_get(func, value);
        self.emit_get(func, value);
        func.instruction(&Instruction::F64Ne);
    }

    fn emit_float_min_max(&self, func: &mut Function, lhs: ValueId, rhs: ValueId, pick_min: bool) {
        self.emit_get(func, lhs);
        self.emit_get(func, lhs);
        func.instruction(&Instruction::F64Eq);
        func.instruction(&Instruction::If(BlockType::Result(ValType::F64)));
        self.emit_get(func, rhs);
        self.emit_get(func, rhs);
        func.instruction(&Instruction::F64Eq);
        func.instruction(&Instruction::If(BlockType::Result(ValType::F64)));
        self.emit_get(func, lhs);
        self.emit_get(func, rhs);
        if pick_min {
            func.instruction(&Instruction::F64Lt);
        } else {
            func.instruction(&Instruction::F64Gt);
        }
        func.instruction(&Instruction::If(BlockType::Result(ValType::F64)));
        self.emit_get(func, lhs);
        func.instruction(&Instruction::Else);
        self.emit_get(func, rhs);
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::Else);
        self.emit_get(func, lhs);
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::Else);
        self.emit_get(func, rhs);
        func.instruction(&Instruction::End);
    }

    fn emit_float_is_infinite(&self, func: &mut Function, value: ValueId) {
        self.emit_get(func, value);
        func.instruction(&Instruction::F64Abs);
        func.instruction(&Instruction::F64Const(f64::INFINITY));
        func.instruction(&Instruction::F64Eq);
    }

    fn emit_float_is_finite(&self, func: &mut Function, value: ValueId) {
        self.emit_get(func, value);
        self.emit_get(func, value);
        func.instruction(&Instruction::F64Eq);
        self.emit_get(func, value);
        func.instruction(&Instruction::F64Abs);
        func.instruction(&Instruction::F64Const(f64::INFINITY));
        func.instruction(&Instruction::F64Ne);
        func.instruction(&Instruction::I32And);
    }

    fn emit_char_to_digit_option(
        &self,
        func: &mut Function,
        ch: ValueId,
        radix: Option<ValueId>,
        result_ty: &Ty,
    ) -> Result<(), CodegenError> {
        if let Some(radix) = radix {
            self.emit_get(func, radix);
            func.instruction(&Instruction::I64Const(2));
            func.instruction(&Instruction::I64LtS);
            self.emit_get(func, radix);
            func.instruction(&Instruction::I64Const(36));
            func.instruction(&Instruction::I64GtS);
            func.instruction(&Instruction::I32Or);
            self.emit_trap_if_true(func);

            self.emit_get(func, radix);
            func.instruction(&Instruction::I32WrapI64);
            func.instruction(&Instruction::LocalSet(self.scratch_i32_2));
        } else {
            func.instruction(&Instruction::I32Const(10));
            func.instruction(&Instruction::LocalSet(self.scratch_i32_2));
        }

        self.emit_char_digit_value(func, ch);
        func.instruction(&Instruction::LocalSet(self.scratch_i64));

        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::I64Const(0));
        func.instruction(&Instruction::I64GeS);
        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I64ExtendI32U);
        func.instruction(&Instruction::I64LtU);
        func.instruction(&Instruction::I32And);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3));

        self.emit_option_int_from_locals(func, result_ty, self.scratch_i32_3, self.scratch_i64)
    }

    fn emit_char_digit_value(&self, func: &mut Function, ch: ValueId) {
        self.emit_get(func, ch);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        func.instruction(&Instruction::I64Const(-1));
        func.instruction(&Instruction::LocalSet(self.scratch_i64));

        self.emit_ascii_range_check(func, self.scratch_i32, '0', '9');
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Const('0' as i32));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::I64ExtendI32U);
        func.instruction(&Instruction::LocalSet(self.scratch_i64));
        func.instruction(&Instruction::Else);
        self.emit_ascii_range_check(func, self.scratch_i32, 'a', 'z');
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Const('a' as i32));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::I32Const(10));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I64ExtendI32U);
        func.instruction(&Instruction::LocalSet(self.scratch_i64));
        func.instruction(&Instruction::Else);
        self.emit_ascii_range_check(func, self.scratch_i32, 'A', 'Z');
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Const('A' as i32));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::I32Const(10));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I64ExtendI32U);
        func.instruction(&Instruction::LocalSet(self.scratch_i64));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i64));
    }

    fn emit_ascii_range_check(&self, func: &mut Function, local: u32, start: char, end: char) {
        func.instruction(&Instruction::LocalGet(local));
        func.instruction(&Instruction::I32Const(start as i32));
        func.instruction(&Instruction::I32GeU);
        func.instruction(&Instruction::LocalGet(local));
        func.instruction(&Instruction::I32Const(end as i32));
        func.instruction(&Instruction::I32LeU);
        func.instruction(&Instruction::I32And);
    }

    fn emit_option_int_from_locals(
        &self,
        func: &mut Function,
        result_ty: &Ty,
        has_value_local: u32,
        value_local: u32,
    ) -> Result<(), CodegenError> {
        let Ty::Adt { def, .. } = result_ty else {
            return Err(CodegenError::UnsupportedInstruction(format!(
                "char digit conversion expected Option<Int>, got {result_ty:?}"
            )));
        };

        let layout = self
            .ctx
            .adt_layouts
            .get(def)
            .ok_or(CodegenError::MissingAdtDef)?;

        let type_item = &self.ctx.item_tree.types[*def];
        let TypeDefKind::Adt { variants } = &type_item.kind else {
            return Err(CodegenError::MissingAdtDef);
        };

        let some_variant = variants
            .iter()
            .find(|variant| variant.name.resolve(self.ctx.interner) == "Some")
            .ok_or(CodegenError::MissingAdtDef)?;
        let none_variant = variants
            .iter()
            .find(|variant| variant.name.resolve(self.ctx.interner) == "None")
            .ok_or(CodegenError::MissingAdtDef)?;

        let some_tag = layout
            .tag_map
            .get(&some_variant.name)
            .ok_or(CodegenError::MissingAdtDef)?;
        let none_tag = layout
            .tag_map
            .get(&none_variant.name)
            .ok_or(CodegenError::MissingAdtDef)?;

        func.instruction(&Instruction::I32Const(layout.size as i32));
        func.instruction(&Instruction::Call(self.ctx.alloc_fn_index));
        func.instruction(&Instruction::Drop);

        let derive_ptr = |func: &mut Function, size: u32| {
            func.instruction(&Instruction::GlobalGet(0));
            func.instruction(&Instruction::I32Const(size as i32));
            func.instruction(&Instruction::I32Sub);
        };

        func.instruction(&Instruction::LocalGet(has_value_local));
        func.instruction(&Instruction::If(BlockType::Empty));
        derive_ptr(func, layout.size);
        func.instruction(&Instruction::I32Const(*some_tag as i32));
        func.instruction(&Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        derive_ptr(func, layout.size);
        func.instruction(&Instruction::LocalGet(value_local));
        func.instruction(&Instruction::I64Store(MemArg {
            offset: u64::from(AdtLayout::field_offset(0)),
            align: 3,
            memory_index: 0,
        }));
        func.instruction(&Instruction::Else);
        derive_ptr(func, layout.size);
        func.instruction(&Instruction::I32Const(*none_tag as i32));
        func.instruction(&Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::End);

        derive_ptr(func, layout.size);
        Ok(())
    }

    fn emit_single_field_adt_from_local(
        &self,
        func: &mut Function,
        adt_ty: &Ty,
        variant_name: &str,
        payload_local: u32,
        payload_ty: &Ty,
        out_local: u32,
    ) -> Result<(), CodegenError> {
        let Ty::Adt { def, .. } = adt_ty else {
            return Err(CodegenError::UnsupportedInstruction(format!(
                "expected ADT for variant `{variant_name}`, got {adt_ty:?}"
            )));
        };

        let layout = self
            .ctx
            .adt_layouts
            .get(def)
            .ok_or(CodegenError::MissingAdtDef)?;
        let type_item = &self.ctx.item_tree.types[*def];
        let TypeDefKind::Adt { variants } = &type_item.kind else {
            return Err(CodegenError::MissingAdtDef);
        };
        let variant = variants
            .iter()
            .find(|variant| variant.name.resolve(self.ctx.interner) == variant_name)
            .ok_or(CodegenError::MissingAdtDef)?;
        let tag = layout
            .tag_map
            .get(&variant.name)
            .ok_or(CodegenError::MissingAdtDef)?;

        func.instruction(&Instruction::I32Const(layout.size as i32));
        func.instruction(&Instruction::Call(self.ctx.alloc_fn_index));
        func.instruction(&Instruction::Drop);

        let derive_ptr = |func: &mut Function, size: u32| {
            func.instruction(&Instruction::GlobalGet(0));
            func.instruction(&Instruction::I32Const(size as i32));
            func.instruction(&Instruction::I32Sub);
        };

        derive_ptr(func, layout.size);
        func.instruction(&Instruction::I32Const(*tag as i32));
        func.instruction(&Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));

        derive_ptr(func, layout.size);
        func.instruction(&Instruction::LocalGet(payload_local));
        self.emit_typed_store_stack(func, payload_ty, u64::from(AdtLayout::field_offset(0)));

        derive_ptr(func, layout.size);
        func.instruction(&Instruction::LocalSet(out_local));
        Ok(())
    }

    fn emit_string_concat(&self, func: &mut Function, lhs: ValueId, rhs: ValueId) {
        self.emit_get(func, lhs);
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        self.emit_get(func, rhs);
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2));

        self.emit_get(func, lhs);
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 4,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3));

        self.emit_get(func, rhs);
        func.instruction(&Instruction::I32Load(MemArg {
            offset: 4,
            align: 2,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::Call(self.ctx.alloc_fn_index));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Store(MemArg {
            offset: 4,
            align: 2,
            memory_index: 0,
        }));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        self.emit_get(func, lhs);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::MemoryCopy {
            src_mem: 0,
            dst_mem: 0,
        });

        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Add);
        self.emit_get(func, rhs);
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::MemoryCopy {
            src_mem: 0,
            dst_mem: 0,
        });

        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
    }

    fn emit_char_to_string(&self, func: &mut Function, value: ValueId) {
        func.instruction(&Instruction::I32Const(4));
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        self.emit_get(func, value);
        func.instruction(&Instruction::I32Const(0x10000));
        func.instruction(&Instruction::I32LtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(3));
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        self.emit_get(func, value);
        func.instruction(&Instruction::I32Const(0x800));
        func.instruction(&Instruction::I32LtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(2));
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        self.emit_get(func, value);
        func.instruction(&Instruction::I32Const(0x80));
        func.instruction(&Instruction::I32LtU);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::LocalSet(self.scratch_i32));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::Call(self.ctx.alloc_fn_index));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Store(MemArg {
            offset: 4,
            align: 2,
            memory_index: 0,
        }));

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        self.emit_get(func, value);
        func.instruction(&Instruction::I32Store8(MemArg {
            offset: 8,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::Else);

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Const(2));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        self.emit_get(func, value);
        func.instruction(&Instruction::I32Const(6));
        func.instruction(&Instruction::I32ShrU);
        func.instruction(&Instruction::I32Const(0xC0));
        func.instruction(&Instruction::I32Or);
        func.instruction(&Instruction::I32Store8(MemArg {
            offset: 8,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        self.emit_get(func, value);
        func.instruction(&Instruction::I32Const(0x3F));
        func.instruction(&Instruction::I32And);
        func.instruction(&Instruction::I32Const(0x80));
        func.instruction(&Instruction::I32Or);
        func.instruction(&Instruction::I32Store8(MemArg {
            offset: 9,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::Else);

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Const(3));
        func.instruction(&Instruction::I32Eq);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        self.emit_get(func, value);
        func.instruction(&Instruction::I32Const(12));
        func.instruction(&Instruction::I32ShrU);
        func.instruction(&Instruction::I32Const(0xE0));
        func.instruction(&Instruction::I32Or);
        func.instruction(&Instruction::I32Store8(MemArg {
            offset: 8,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        self.emit_get(func, value);
        func.instruction(&Instruction::I32Const(6));
        func.instruction(&Instruction::I32ShrU);
        func.instruction(&Instruction::I32Const(0x3F));
        func.instruction(&Instruction::I32And);
        func.instruction(&Instruction::I32Const(0x80));
        func.instruction(&Instruction::I32Or);
        func.instruction(&Instruction::I32Store8(MemArg {
            offset: 9,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        self.emit_get(func, value);
        func.instruction(&Instruction::I32Const(0x3F));
        func.instruction(&Instruction::I32And);
        func.instruction(&Instruction::I32Const(0x80));
        func.instruction(&Instruction::I32Or);
        func.instruction(&Instruction::I32Store8(MemArg {
            offset: 10,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::Else);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        self.emit_get(func, value);
        func.instruction(&Instruction::I32Const(18));
        func.instruction(&Instruction::I32ShrU);
        func.instruction(&Instruction::I32Const(0xF0));
        func.instruction(&Instruction::I32Or);
        func.instruction(&Instruction::I32Store8(MemArg {
            offset: 8,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        self.emit_get(func, value);
        func.instruction(&Instruction::I32Const(12));
        func.instruction(&Instruction::I32ShrU);
        func.instruction(&Instruction::I32Const(0x3F));
        func.instruction(&Instruction::I32And);
        func.instruction(&Instruction::I32Const(0x80));
        func.instruction(&Instruction::I32Or);
        func.instruction(&Instruction::I32Store8(MemArg {
            offset: 9,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        self.emit_get(func, value);
        func.instruction(&Instruction::I32Const(6));
        func.instruction(&Instruction::I32ShrU);
        func.instruction(&Instruction::I32Const(0x3F));
        func.instruction(&Instruction::I32And);
        func.instruction(&Instruction::I32Const(0x80));
        func.instruction(&Instruction::I32Or);
        func.instruction(&Instruction::I32Store8(MemArg {
            offset: 10,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        self.emit_get(func, value);
        func.instruction(&Instruction::I32Const(0x3F));
        func.instruction(&Instruction::I32And);
        func.instruction(&Instruction::I32Const(0x80));
        func.instruction(&Instruction::I32Or);
        func.instruction(&Instruction::I32Store8(MemArg {
            offset: 11,
            align: 0,
            memory_index: 0,
        }));

        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
    }

    fn emit_int_to_string(&self, func: &mut Function, value: ValueId) {
        self.emit_get(func, value);
        func.instruction(&Instruction::LocalSet(self.scratch_i64));

        self.emit_get(func, value);
        func.instruction(&Instruction::I64Const(0));
        func.instruction(&Instruction::I64LtS);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_2));

        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));

        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::I64Const(10));
        func.instruction(&Instruction::I64DivS);
        func.instruction(&Instruction::LocalSet(self.scratch_i64));

        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::I64Eqz);
        func.instruction(&Instruction::BrIf(1));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::Call(self.ctx.alloc_fn_index));
        func.instruction(&Instruction::LocalSet(self.scratch_i32_3));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Store(MemArg {
            offset: 4,
            align: 2,
            memory_index: 0,
        }));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
        func.instruction(&Instruction::I32Const(8));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalGet(self.scratch_i32));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));

        self.emit_get(func, value);
        func.instruction(&Instruction::LocalSet(self.scratch_i64));

        func.instruction(&Instruction::Block(BlockType::Empty));
        func.instruction(&Instruction::Loop(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::I64Const(10));
        func.instruction(&Instruction::I64RemS);
        func.instruction(&Instruction::I32WrapI64);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::I32LtS);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_5));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));

        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_5));
        func.instruction(&Instruction::I32Const(48));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::I32Store8(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));

        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::I64Const(10));
        func.instruction(&Instruction::I64DivS);
        func.instruction(&Instruction::LocalSet(self.scratch_i64));

        func.instruction(&Instruction::LocalGet(self.scratch_i64));
        func.instruction(&Instruction::I64Eqz);
        func.instruction(&Instruction::BrIf(1));
        func.instruction(&Instruction::Br(0));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_2));
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Const(1));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::LocalSet(self.scratch_i32_4));
        func.instruction(&Instruction::LocalGet(self.scratch_i32_4));
        func.instruction(&Instruction::I32Const(45));
        func.instruction(&Instruction::I32Store8(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        func.instruction(&Instruction::End);

        func.instruction(&Instruction::LocalGet(self.scratch_i32_3));
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
            self.emit_typed_store(func, field_vid, field_ty, u64::from(offset));
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
        self.emit_typed_load(func, result_ty, u64::from(offset));

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
            self.emit_typed_store(func, *vid, field_ty, u64::from(offset));
        }

        // Leave ptr on stack.
        derive_ptr(func, size);

        Ok(())
    }

    fn emit_record_update(
        &self,
        func: &mut Function,
        base: ValueId,
        updates: &[(Name, ValueId)],
        result_ty: &Ty,
    ) -> Result<(), CodegenError> {
        let sorted_fields = self.resolve_record_fields(result_ty)?;
        let field_count = sorted_fields.len() as u32;
        let size = layout::record_size(field_count);

        func.instruction(&Instruction::I32Const(size as i32));
        func.instruction(&Instruction::Call(self.ctx.alloc_fn_index));
        func.instruction(&Instruction::Drop);

        let derive_ptr = |func: &mut Function, size: u32| {
            func.instruction(&Instruction::GlobalGet(0));
            func.instruction(&Instruction::I32Const(size as i32));
            func.instruction(&Instruction::I32Sub);
        };

        for (i, (field_name, field_ty)) in sorted_fields.iter().enumerate() {
            let offset = u64::from(layout::record_field_offset(i as u32));
            derive_ptr(func, size);
            if let Some((_, update_vid)) = updates.iter().find(|(name, _)| *name == *field_name) {
                self.emit_typed_store(func, *update_vid, self.value_ty(*update_vid), offset);
            } else {
                self.emit_get(func, base);
                self.emit_typed_load(func, field_ty, offset);
                self.emit_typed_store_stack(func, field_ty, offset);
            }
        }

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
        self.emit_typed_load(func, result_ty, u64::from(offset));

        Ok(())
    }

    fn resolve_record_field_index(&self, base_ty: &Ty, field: Name) -> Result<u32, CodegenError> {
        let field_names: Vec<Name> = match base_ty {
            Ty::Record { fields } => fields.iter().map(|(n, _)| *n).collect(),
            Ty::Adt { def, .. } => {
                let type_item = &self.ctx.item_tree.types[*def];
                match &type_item.kind {
                    kyokara_hir_def::item_tree::TypeDefKind::Record { fields } => {
                        fields.iter().map(|(n, _)| *n).collect()
                    }
                    kyokara_hir_def::item_tree::TypeDefKind::Alias(
                        kyokara_hir_def::type_ref::TypeRef::Record { fields },
                    ) => fields.iter().map(|(n, _)| *n).collect(),
                    _ => {
                        return Err(CodegenError::UnsupportedType(
                            "field access on non-record ADT".into(),
                        ));
                    }
                }
            }
            _ => {
                return Err(CodegenError::UnsupportedType(
                    "field access on non-record".into(),
                ));
            }
        };

        // Sort field names to get deterministic index.
        let mut names = field_names;
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

    fn resolve_record_fields(&self, record_ty: &Ty) -> Result<Vec<(Name, Ty)>, CodegenError> {
        let mut fields: Vec<(Name, Ty)> = match record_ty {
            Ty::Record { fields } => fields.clone(),
            Ty::Adt { def, .. } => {
                let type_item = &self.ctx.item_tree.types[*def];
                match &type_item.kind {
                    TypeDefKind::Record { fields } => fields
                        .iter()
                        .map(|(name, ty_ref)| (*name, self.resolve_record_field_storage_ty(ty_ref)))
                        .collect(),
                    TypeDefKind::Alias(TypeRef::Record { fields }) => fields
                        .iter()
                        .map(|(name, ty_ref)| (*name, self.resolve_record_field_storage_ty(ty_ref)))
                        .collect(),
                    _ => {
                        return Err(CodegenError::UnsupportedType(
                            "record update on non-record ADT".into(),
                        ));
                    }
                }
            }
            _ => {
                return Err(CodegenError::UnsupportedType(
                    "record update on non-record".into(),
                ));
            }
        };

        fields.sort_by(|a, b| {
            let a_str = a.0.resolve(self.ctx.interner);
            let b_str = b.0.resolve(self.ctx.interner);
            a_str.cmp(b_str)
        });
        Ok(fields)
    }

    fn resolve_record_field_storage_ty(&self, type_ref: &TypeRef) -> Ty {
        match type_ref {
            TypeRef::Path { path, args } if path.is_single() && args.is_empty() => {
                let name = path.segments[0].resolve(self.ctx.interner);
                resolve_builtin(name).unwrap_or(Ty::Error)
            }
            _ => Ty::Error,
        }
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

    /// Type-aware store: emit the value and the appropriate store instruction.
    ///
    /// - Float → `f64.store` (8 bytes, align 3)
    /// - Int   → `i64.store` (8 bytes, align 3)
    /// - i32 types (Bool/Unit/Ptr) → extend to i64, then `i64.store`
    ///
    /// All field slots are 8 bytes, so even i32 values are stored as i64
    /// to keep the layout uniform.
    fn emit_typed_store(&self, func: &mut Function, vid: ValueId, ty: &Ty, offset: u64) {
        self.emit_get(func, vid);
        self.emit_typed_store_stack(func, ty, offset);
    }

    fn emit_typed_store_stack(&self, func: &mut Function, ty: &Ty, offset: u64) {
        match ty {
            Ty::Float => {
                func.instruction(&Instruction::F64Store(MemArg {
                    offset,
                    align: 3,
                    memory_index: 0,
                }));
            }
            Ty::Int => {
                func.instruction(&Instruction::I64Store(MemArg {
                    offset,
                    align: 3,
                    memory_index: 0,
                }));
            }
            _ => {
                // i32 types (Bool/Unit/pointers): extend to i64 for uniform slot size.
                if is_i32_type(ty) {
                    func.instruction(&Instruction::I64ExtendI32U);
                }
                func.instruction(&Instruction::I64Store(MemArg {
                    offset,
                    align: 3,
                    memory_index: 0,
                }));
            }
        }
    }

    /// Type-aware load: emit the appropriate load instruction for the field type.
    ///
    /// - Float → `f64.load` (8 bytes)
    /// - Int   → `i64.load` (8 bytes)
    /// - i32 types → `i64.load` + `i32.wrap_i64`
    fn emit_typed_load(&self, func: &mut Function, ty: &Ty, offset: u64) {
        match ty {
            Ty::Float => {
                func.instruction(&Instruction::F64Load(MemArg {
                    offset,
                    align: 3,
                    memory_index: 0,
                }));
            }
            _ if is_i32_type(ty) => {
                func.instruction(&Instruction::I64Load(MemArg {
                    offset,
                    align: 3,
                    memory_index: 0,
                }));
                func.instruction(&Instruction::I32WrapI64);
            }
            _ => {
                // Int and anything else: i64.load
                func.instruction(&Instruction::I64Load(MemArg {
                    offset,
                    align: 3,
                    memory_index: 0,
                }));
            }
        }
    }
}
