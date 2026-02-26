//! IR value definitions.

use la_arena::Idx;

use kyokara_hir_ty::ty::Ty;

use crate::inst::Inst;

/// Index into a function's value arena.
pub type ValueId = Idx<ValueDef>;

/// A value produced by an instruction. Every value has a type and
/// the instruction that produces it.
#[derive(Debug, Clone)]
pub struct ValueDef {
    pub ty: Ty,
    pub inst: Inst,
}
