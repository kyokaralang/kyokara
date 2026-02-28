//! Human-readable text format for KIR.
//!
//! Produces output like:
//! ```text
//! fn @abs(x: Int) -> Int effects {} {
//!   entry:
//!     %0 = const 0 : Int
//!     %1 = lt x, %0 : Bool
//!     branch %1 -> then(), else()
//!
//!   then:
//!     %2 = neg x : Int
//!     jump -> merge(%2)
//!
//!   else:
//!     jump -> merge(x)
//!
//!   merge(%3: Int):
//!     return %3
//! }
//! ```

use std::fmt::{self, Write};

use kyokara_hir_def::expr::{BinaryOp, UnaryOp};
use kyokara_hir_def::item_tree::ItemTree;
use kyokara_hir_def::name::Name;
use kyokara_hir_ty::ty::{Ty, display_ty_with_tree};
use kyokara_intern::Interner;

use crate::KirModule;
use crate::block::{Block, BlockId, BranchTarget, Terminator};
use crate::function::KirFunction;
use crate::inst::{CallTarget, Constant, Inst};
use crate::value::ValueId;

/// Display context carrying the interner and item tree.
pub struct DisplayCtx<'a> {
    pub interner: &'a Interner,
    pub tree: &'a ItemTree,
}

impl<'a> DisplayCtx<'a> {
    pub fn new(interner: &'a Interner, tree: &'a ItemTree) -> Self {
        Self { interner, tree }
    }

    fn fmt_ty(&self, ty: &Ty) -> String {
        display_ty_with_tree(ty, self.interner, self.tree)
    }

    fn fmt_name(&self, name: Name) -> String {
        name.resolve(self.interner).to_owned()
    }

    fn fmt_value(&self, vid: ValueId, func: &KirFunction) -> String {
        match &func.values[vid].inst {
            // Block params use the param name if available.
            Inst::BlockParam { block, index } => {
                let param = &func.blocks[*block].params[*index as usize];
                if let Some(n) = param.name {
                    return self.fmt_name(n);
                }
                format!("%{}", vid.into_raw().into_u32())
            }
            // Function params use the function's parameter name.
            Inst::FnParam { index } => {
                if let Some((name, _)) = func.params.get(*index as usize) {
                    return self.fmt_name(*name);
                }
                format!("%{}", vid.into_raw().into_u32())
            }
            _ => format!("%{}", vid.into_raw().into_u32()),
        }
    }

    fn fmt_block_label(&self, bid: BlockId, func: &KirFunction) -> String {
        let block = &func.blocks[bid];
        if let Some(label) = block.label {
            self.fmt_name(label)
        } else {
            format!("bb{}", bid.into_raw().into_u32())
        }
    }
}

/// Format a module to a string.
pub fn display_module(module: &KirModule, ctx: &DisplayCtx<'_>) -> String {
    let mut out = String::new();
    for (id, func) in module.functions.iter() {
        if id.into_raw().into_u32() > 0 {
            out.push('\n');
        }
        display_function(func, ctx, &mut out).expect("write to String cannot fail");
    }
    out
}

/// Format a single function.
pub fn display_function(func: &KirFunction, ctx: &DisplayCtx<'_>, out: &mut String) -> fmt::Result {
    // Header: fn @name(params) -> ret_ty effects {caps} {
    write!(out, "fn @{}(", ctx.fmt_name(func.name))?;
    for (i, (name, ty)) in func.params.iter().enumerate() {
        if i > 0 {
            write!(out, ", ")?;
        }
        write!(out, "{}: {}", ctx.fmt_name(*name), ctx.fmt_ty(ty))?;
    }
    write!(out, ") -> {} effects {{", ctx.fmt_ty(&func.ret_ty))?;

    let mut caps: Vec<_> = func.effects.caps.iter().map(|c| ctx.fmt_name(*c)).collect();
    caps.sort();
    if !caps.is_empty() {
        write!(out, "{}", caps.join(", "))?;
    }
    writeln!(out, "}} {{")?;

    // Blocks
    let mut first = true;
    for (bid, block) in func.blocks.iter() {
        if !first {
            writeln!(out)?;
        }
        first = false;
        display_block(bid, block, func, ctx, out)?;
    }

    writeln!(out, "}}")?;
    Ok(())
}

fn display_block(
    bid: BlockId,
    block: &Block,
    func: &KirFunction,
    ctx: &DisplayCtx<'_>,
    out: &mut String,
) -> fmt::Result {
    // Block header: "  label(params):"
    write!(out, "  {}", ctx.fmt_block_label(bid, func))?;
    if !block.params.is_empty() {
        write!(out, "(")?;
        for (i, param) in block.params.iter().enumerate() {
            if i > 0 {
                write!(out, ", ")?;
            }
            write!(
                out,
                "{}: {}",
                ctx.fmt_value(param.value, func),
                ctx.fmt_ty(&param.ty)
            )?;
        }
        write!(out, ")")?;
    }
    writeln!(out, ":")?;

    // Body instructions
    for &vid in &block.body {
        display_instruction(vid, func, ctx, out)?;
    }

    // Terminator
    if let Some(term) = &block.terminator {
        display_terminator(term, func, ctx, out)?;
    }

    Ok(())
}

fn display_instruction(
    vid: ValueId,
    func: &KirFunction,
    ctx: &DisplayCtx<'_>,
    out: &mut String,
) -> fmt::Result {
    let vdef = &func.values[vid];
    let val_name = ctx.fmt_value(vid, func);
    let ty_str = ctx.fmt_ty(&vdef.ty);

    write!(out, "    {} = ", val_name)?;

    match &vdef.inst {
        Inst::Const(c) => {
            write!(out, "const ")?;
            display_constant(c, out)?;
        }
        Inst::Binary { op, lhs, rhs } => {
            write!(
                out,
                "{} {}, {}",
                binary_op_name(*op),
                ctx.fmt_value(*lhs, func),
                ctx.fmt_value(*rhs, func)
            )?;
        }
        Inst::Unary { op, operand } => {
            write!(
                out,
                "{} {}",
                unary_op_name(*op),
                ctx.fmt_value(*operand, func)
            )?;
        }
        Inst::RecordCreate { fields } => {
            write!(out, "record_create {{")?;
            for (i, (name, val)) in fields.iter().enumerate() {
                if i > 0 {
                    write!(out, ",")?;
                }
                write!(
                    out,
                    " {}: {}",
                    ctx.fmt_name(*name),
                    ctx.fmt_value(*val, func)
                )?;
            }
            write!(out, " }}")?;
        }
        Inst::FieldGet { base, field } => {
            write!(
                out,
                "field_get {}, {}",
                ctx.fmt_value(*base, func),
                ctx.fmt_name(*field)
            )?;
        }
        Inst::RecordUpdate { base, updates } => {
            write!(out, "record_update {}, {{", ctx.fmt_value(*base, func))?;
            for (i, (name, val)) in updates.iter().enumerate() {
                if i > 0 {
                    write!(out, ",")?;
                }
                write!(
                    out,
                    " {}: {}",
                    ctx.fmt_name(*name),
                    ctx.fmt_value(*val, func)
                )?;
            }
            write!(out, " }}")?;
        }
        Inst::AdtConstruct {
            variant, fields, ..
        } => {
            write!(out, "adt_construct {}(", ctx.fmt_name(*variant))?;
            for (i, val) in fields.iter().enumerate() {
                if i > 0 {
                    write!(out, ", ")?;
                }
                write!(out, "{}", ctx.fmt_value(*val, func))?;
            }
            write!(out, ")")?;
        }
        Inst::Call { target, args } => {
            write!(out, "call ")?;
            match target {
                CallTarget::Direct(name) => write!(out, "@{}", ctx.fmt_name(*name))?,
                CallTarget::Indirect(val) => write!(out, "{}", ctx.fmt_value(*val, func))?,
                CallTarget::Intrinsic(name) => write!(out, "intrinsic:{}", name)?,
            }
            write!(out, "(")?;
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    write!(out, ", ")?;
                }
                write!(out, "{}", ctx.fmt_value(*arg, func))?;
            }
            write!(out, ")")?;
        }
        Inst::Assert { condition, message } => {
            write!(
                out,
                "assert {}, {:?}",
                ctx.fmt_value(*condition, func),
                message
            )?;
        }
        Inst::Hole { id, .. } => {
            write!(out, "hole #{}", id)?;
        }
        Inst::BlockParam { .. } => {
            // Block params are not emitted as instructions in the body.
            // This shouldn't appear here, but handle gracefully.
            write!(out, "block_param")?;
        }
        Inst::FnParam { index } => {
            write!(out, "fn_param {}", index)?;
        }
        Inst::AdtFieldGet { base, field_index } => {
            write!(
                out,
                "adt_field_get {}, {}",
                ctx.fmt_value(*base, func),
                field_index
            )?;
        }
        Inst::FnRef { name } => {
            write!(out, "fn_ref @{}", name.resolve(ctx.interner))?;
        }
    }

    writeln!(out, " : {}", ty_str)?;
    Ok(())
}

fn display_terminator(
    term: &Terminator,
    func: &KirFunction,
    ctx: &DisplayCtx<'_>,
    out: &mut String,
) -> fmt::Result {
    write!(out, "    ")?;
    match term {
        Terminator::Return(val) => {
            writeln!(out, "return {}", ctx.fmt_value(*val, func))?;
        }
        Terminator::Jump(target) => {
            write!(out, "jump -> ")?;
            display_branch_target(target, func, ctx, out)?;
            writeln!(out)?;
        }
        Terminator::Branch {
            condition,
            then_target,
            else_target,
        } => {
            write!(out, "branch {} -> ", ctx.fmt_value(*condition, func))?;
            display_branch_target(then_target, func, ctx, out)?;
            write!(out, ", ")?;
            display_branch_target(else_target, func, ctx, out)?;
            writeln!(out)?;
        }
        Terminator::Switch {
            scrutinee,
            cases,
            default,
        } => {
            writeln!(out, "switch {} {{", ctx.fmt_value(*scrutinee, func))?;
            for case in cases {
                write!(out, "      {} -> ", ctx.fmt_name(case.variant))?;
                display_branch_target(&case.target, func, ctx, out)?;
                writeln!(out)?;
            }
            if let Some(def) = default {
                write!(out, "      _ -> ")?;
                display_branch_target(def, func, ctx, out)?;
                writeln!(out)?;
            }
            writeln!(out, "    }}")?;
        }
        Terminator::Unreachable => {
            writeln!(out, "unreachable")?;
        }
    }
    Ok(())
}

fn display_branch_target(
    target: &BranchTarget,
    func: &KirFunction,
    ctx: &DisplayCtx<'_>,
    out: &mut String,
) -> fmt::Result {
    write!(out, "{}(", ctx.fmt_block_label(target.block, func))?;
    for (i, arg) in target.args.iter().enumerate() {
        if i > 0 {
            write!(out, ", ")?;
        }
        write!(out, "{}", ctx.fmt_value(*arg, func))?;
    }
    write!(out, ")")?;
    Ok(())
}

fn display_constant(c: &Constant, out: &mut String) -> fmt::Result {
    match c {
        Constant::Int(v) => write!(out, "{}", v),
        Constant::Float(v) => {
            if v.fract() == 0.0 {
                write!(out, "{:.1}", v)
            } else {
                write!(out, "{}", v)
            }
        }
        Constant::String(s) => write!(out, "{:?}", s),
        Constant::Char(c) => write!(out, "'{}'", c),
        Constant::Bool(b) => write!(out, "{}", b),
        Constant::Unit => write!(out, "()"),
    }
}

fn binary_op_name(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "add",
        BinaryOp::Sub => "sub",
        BinaryOp::Mul => "mul",
        BinaryOp::Div => "div",
        BinaryOp::Mod => "rem",
        BinaryOp::Eq => "eq",
        BinaryOp::NotEq => "neq",
        BinaryOp::Lt => "lt",
        BinaryOp::Gt => "gt",
        BinaryOp::LtEq => "lte",
        BinaryOp::GtEq => "gte",
        BinaryOp::And => "and",
        BinaryOp::Or => "or",
    }
}

fn unary_op_name(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Not => "not",
        UnaryOp::Neg => "neg",
    }
}
