//! Well-formedness validation for KIR.
//!
//! Checks structural invariants that must hold for any valid IR:
//! - Every referenced ValueId exists in the function's arena
//! - Block parameter counts match branch argument counts
//! - Every block has a terminator
//! - Entry block has zero parameters
//! - Return value types match function return type
//! - BlockParam instructions match their block's parameter declarations

use rustc_hash::FxHashSet;

use kyokara_diagnostics::{Diagnostic, Severity};
use kyokara_hir_ty::ty::Ty;
use kyokara_intern::Interner;
use kyokara_span::{FileId, Span, TextRange};

use crate::block::{BlockId, BranchTarget, Terminator};
use crate::function::KirFunction;
use crate::inst::Inst;
use crate::value::ValueId;

/// A dummy span used for KIR validation diagnostics (no source location).
fn dummy_span() -> Span {
    Span {
        file: FileId::new(0),
        range: TextRange::default(),
    }
}

/// Validate a function and return any diagnostics.
pub fn validate_function(func: &KirFunction, interner: &Interner) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let fn_name = func.name.resolve(interner);

    // Validate entry block exists
    if !is_valid_block(func.entry_block, func) {
        diags.push(error(format!(
            "fn {}: invalid entry block (index out of bounds)",
            fn_name
        )));
        return diags;
    }

    // Check entry block has no parameters
    let entry = &func.blocks[func.entry_block];
    if !entry.params.is_empty() {
        diags.push(error(format!(
            "fn {}: entry block must have zero parameters, found {}",
            fn_name,
            entry.params.len()
        )));
    }

    // Validate contract value references
    for (i, vid) in func.contracts.requires.iter().enumerate() {
        if !is_valid_value(*vid, func) {
            diags.push(error(format!(
                "fn {}: requires[{}] references invalid value %{}",
                fn_name,
                i,
                vid.into_raw().into_u32()
            )));
        }
    }
    for (i, vid) in func.contracts.ensures.iter().enumerate() {
        if !is_valid_value(*vid, func) {
            diags.push(error(format!(
                "fn {}: ensures[{}] references invalid value %{}",
                fn_name,
                i,
                vid.into_raw().into_u32()
            )));
        }
    }

    // Check each block
    for (bid, block) in func.blocks.iter() {
        let block_label = block
            .label
            .map(|n| n.resolve(interner).to_owned())
            .unwrap_or_else(|| format!("bb{}", bid.into_raw().into_u32()));

        // Every block must have a terminator
        if block.terminator.is_none() {
            diags.push(error(format!(
                "fn {}: block {} has no terminator",
                fn_name, block_label
            )));
            continue;
        }

        // Validate block parameter ValueDefs
        for (i, param) in block.params.iter().enumerate() {
            if !is_valid_value(param.value, func) {
                diags.push(error(format!(
                    "fn {}: block {} param {} has invalid ValueId",
                    fn_name, block_label, i
                )));
                continue;
            }
            match &func.values[param.value].inst {
                Inst::BlockParam {
                    block: bp_block,
                    index,
                } => {
                    if *bp_block != bid {
                        diags.push(error(format!(
                            "fn {}: block {} param {} references wrong block",
                            fn_name, block_label, i
                        )));
                    }
                    if *index != i as u32 {
                        diags.push(error(format!(
                            "fn {}: block {} param {} has mismatched index (expected {}, got {})",
                            fn_name, block_label, i, i, index
                        )));
                    }
                }
                _ => {
                    diags.push(error(format!(
                        "fn {}: block {} param {} value is not a BlockParam instruction",
                        fn_name, block_label, i
                    )));
                }
            }
        }

        // Validate body value references
        for &vid in &block.body {
            if !is_valid_value(vid, func) {
                diags.push(error(format!(
                    "fn {}: block {} references invalid value %{}",
                    fn_name,
                    block_label,
                    vid.into_raw().into_u32()
                )));
                continue;
            }
            validate_inst_operands(
                &func.values[vid].inst,
                func,
                &block_label,
                fn_name,
                &mut diags,
            );
        }

        // Validate terminator references
        if let Some(term) = &block.terminator {
            validate_terminator(term, func, &block_label, fn_name, interner, &mut diags);
        }
    }

    // Check that every block with parameters has at least one predecessor edge.
    let mut has_predecessor = FxHashSet::<BlockId>::default();
    for (_bid, block) in func.blocks.iter() {
        if let Some(term) = &block.terminator {
            collect_branch_targets(term, &mut has_predecessor);
        }
    }
    for (bid, block) in func.blocks.iter() {
        if bid == func.entry_block {
            continue;
        }
        // Skip blocks explicitly marked unreachable (dead merge blocks).
        let is_unreachable = matches!(block.terminator, Some(Terminator::Unreachable));
        if !block.params.is_empty() && !has_predecessor.contains(&bid) && !is_unreachable {
            let block_label = block
                .label
                .map(|n| n.resolve(interner).to_owned())
                .unwrap_or_else(|| format!("bb{}", bid.into_raw().into_u32()));
            diags.push(error(format!(
                "fn {}: block {} has {} params but no predecessor edges",
                fn_name,
                block_label,
                block.params.len()
            )));
        }
    }

    diags
}

fn validate_terminator(
    term: &Terminator,
    func: &KirFunction,
    block_label: &str,
    fn_name: &str,
    interner: &Interner,
    diags: &mut Vec<Diagnostic>,
) {
    match term {
        Terminator::Return(val) => {
            check_value_exists(*val, func, block_label, fn_name, "return", diags);
            if is_valid_value(*val, func) {
                let val_ty = &func.values[*val].ty;
                if !types_compatible(val_ty, &func.ret_ty) {
                    diags.push(error(format!(
                        "fn {}: block {} return type mismatch",
                        fn_name, block_label
                    )));
                }
            }
        }
        Terminator::Jump(target) => {
            validate_branch_target(target, func, block_label, fn_name, "jump", diags);
        }
        Terminator::Branch {
            condition,
            then_target,
            else_target,
        } => {
            check_value_exists(
                *condition,
                func,
                block_label,
                fn_name,
                "branch condition",
                diags,
            );
            validate_branch_target(
                then_target,
                func,
                block_label,
                fn_name,
                "branch then",
                diags,
            );
            validate_branch_target(
                else_target,
                func,
                block_label,
                fn_name,
                "branch else",
                diags,
            );
        }
        Terminator::Switch {
            scrutinee,
            cases,
            default,
        } => {
            check_value_exists(
                *scrutinee,
                func,
                block_label,
                fn_name,
                "switch scrutinee",
                diags,
            );
            // Check for duplicate case variants.
            let mut seen_variants = FxHashSet::default();
            for case in cases {
                let variant_str = case.variant.resolve(interner);
                if !seen_variants.insert(case.variant) {
                    diags.push(error(format!(
                        "fn {}: block {} switch has duplicate case for variant `{}`",
                        fn_name, block_label, variant_str
                    )));
                }
                validate_branch_target(
                    &case.target,
                    func,
                    block_label,
                    fn_name,
                    "switch case",
                    diags,
                );
            }
            if let Some(def) = default {
                validate_branch_target(def, func, block_label, fn_name, "switch default", diags);
            }
        }
        Terminator::Unreachable => {}
    }
}

/// Collect all block IDs that appear as branch targets in a terminator.
fn collect_branch_targets(term: &Terminator, targets: &mut FxHashSet<BlockId>) {
    match term {
        Terminator::Return(_) | Terminator::Unreachable => {}
        Terminator::Jump(t) => {
            targets.insert(t.block);
        }
        Terminator::Branch {
            then_target,
            else_target,
            ..
        } => {
            targets.insert(then_target.block);
            targets.insert(else_target.block);
        }
        Terminator::Switch { cases, default, .. } => {
            for case in cases {
                targets.insert(case.target.block);
            }
            if let Some(def) = default {
                targets.insert(def.block);
            }
        }
    }
}

fn validate_branch_target(
    target: &BranchTarget,
    func: &KirFunction,
    block_label: &str,
    fn_name: &str,
    context: &str,
    diags: &mut Vec<Diagnostic>,
) {
    // Check target block exists
    if !is_valid_block(target.block, func) {
        diags.push(error(format!(
            "fn {}: block {} {} references invalid block",
            fn_name, block_label, context
        )));
        return;
    }

    let target_block = &func.blocks[target.block];
    if target.args.len() != target_block.params.len() {
        diags.push(error(format!(
            "fn {}: block {} {} passes {} args but target block expects {} params",
            fn_name,
            block_label,
            context,
            target.args.len(),
            target_block.params.len()
        )));
    }

    for arg in &target.args {
        check_value_exists(*arg, func, block_label, fn_name, context, diags);
    }

    for (arg, param) in target.args.iter().zip(target_block.params.iter()) {
        if is_valid_value(*arg, func) {
            let arg_ty = &func.values[*arg].ty;
            if !types_compatible(arg_ty, &param.ty) {
                diags.push(error(format!(
                    "fn {}: block {} {} argument type mismatch",
                    fn_name, block_label, context
                )));
            }
        }
    }
}

fn validate_inst_operands(
    inst: &Inst,
    func: &KirFunction,
    block_label: &str,
    fn_name: &str,
    diags: &mut Vec<Diagnostic>,
) {
    match inst {
        Inst::Const(_) => {}
        Inst::Binary { lhs, rhs, .. } => {
            check_value_exists(*lhs, func, block_label, fn_name, "binary lhs", diags);
            check_value_exists(*rhs, func, block_label, fn_name, "binary rhs", diags);
        }
        Inst::Unary { operand, .. } => {
            check_value_exists(*operand, func, block_label, fn_name, "unary operand", diags);
        }
        Inst::RecordCreate { fields } => {
            for (_, val) in fields {
                check_value_exists(*val, func, block_label, fn_name, "record field", diags);
            }
        }
        Inst::FieldGet { base, .. } => {
            check_value_exists(*base, func, block_label, fn_name, "field_get base", diags);
        }
        Inst::RecordUpdate { base, updates } => {
            check_value_exists(
                *base,
                func,
                block_label,
                fn_name,
                "record_update base",
                diags,
            );
            for (_, val) in updates {
                check_value_exists(
                    *val,
                    func,
                    block_label,
                    fn_name,
                    "record_update field",
                    diags,
                );
            }
        }
        Inst::AdtConstruct { fields, .. } => {
            for val in fields {
                check_value_exists(*val, func, block_label, fn_name, "adt field", diags);
            }
        }
        Inst::Call { target, args } => {
            if let crate::inst::CallTarget::Indirect(val) = target {
                check_value_exists(*val, func, block_label, fn_name, "call target", diags);
            }
            for arg in args {
                check_value_exists(*arg, func, block_label, fn_name, "call arg", diags);
            }
        }
        Inst::Assert { condition, .. } => {
            check_value_exists(
                *condition,
                func,
                block_label,
                fn_name,
                "assert condition",
                diags,
            );
        }
        Inst::Hole { .. } => {}
        Inst::BlockParam { block, index } => {
            if is_valid_block(*block, func) {
                let b = &func.blocks[*block];
                if (*index as usize) >= b.params.len() {
                    diags.push(error(format!(
                        "fn {}: block {} block_param index {} out of range",
                        fn_name, block_label, index
                    )));
                }
            } else {
                diags.push(error(format!(
                    "fn {}: block {} block_param references invalid block",
                    fn_name, block_label
                )));
            }
        }
        Inst::FnParam { index } => {
            if (*index as usize) >= func.params.len() {
                diags.push(error(format!(
                    "fn {}: block {} fn_param index {} out of range (function has {} params)",
                    fn_name,
                    block_label,
                    index,
                    func.params.len()
                )));
            }
        }
        Inst::AdtFieldGet { base, .. } => {
            check_value_exists(
                *base,
                func,
                block_label,
                fn_name,
                "adt_field_get base",
                diags,
            );
        }
        Inst::FnRef { .. } => {}
    }
}

fn check_value_exists(
    vid: ValueId,
    func: &KirFunction,
    block_label: &str,
    fn_name: &str,
    context: &str,
    diags: &mut Vec<Diagnostic>,
) {
    if !is_valid_value(vid, func) {
        diags.push(error(format!(
            "fn {}: block {} {} references invalid value %{}",
            fn_name,
            block_label,
            context,
            vid.into_raw().into_u32()
        )));
    }
}

fn is_valid_value(vid: ValueId, func: &KirFunction) -> bool {
    (vid.into_raw().into_u32() as usize) < func.values.len()
}

fn is_valid_block(bid: crate::block::BlockId, func: &KirFunction) -> bool {
    (bid.into_raw().into_u32() as usize) < func.blocks.len()
}

/// Type compatibility check — exact match or either side is Error/Never.
fn types_compatible(a: &Ty, b: &Ty) -> bool {
    a == b || a.is_poison() || b.is_poison()
}

fn error(message: String) -> Diagnostic {
    Diagnostic {
        message,
        severity: Severity::Error,
        span: dummy_span(),
        fixes: Vec::new(),
    }
}
