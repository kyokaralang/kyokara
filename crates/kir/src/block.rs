//! IR basic blocks with block parameters.

use la_arena::Idx;

use kyokara_hir_def::name::Name;
use kyokara_hir_ty::ty::Ty;

use crate::value::ValueId;

/// Index into a function's block arena.
pub type BlockId = Idx<Block>;

/// A parameter on a block entry (replaces phi nodes).
#[derive(Debug, Clone)]
pub struct BlockParam {
    pub name: Option<Name>,
    pub ty: Ty,
    /// The ValueId that represents this parameter inside the block.
    pub value: ValueId,
}

/// A branch target: a block plus arguments passed to its parameters.
#[derive(Debug, Clone)]
pub struct BranchTarget {
    pub block: BlockId,
    pub args: Vec<ValueId>,
}

/// A case in a `Switch` terminator.
#[derive(Debug, Clone)]
pub struct SwitchCase {
    pub variant: Name,
    pub target: BranchTarget,
}

/// Block terminator — every block must end with exactly one.
#[derive(Debug, Clone)]
pub enum Terminator {
    /// Return a value from the function.
    Return(ValueId),

    /// Unconditional jump to a block.
    Jump(BranchTarget),

    /// Conditional branch.
    Branch {
        condition: ValueId,
        then_target: BranchTarget,
        else_target: BranchTarget,
    },

    /// Multi-way branch on ADT tag.
    Switch {
        scrutinee: ValueId,
        cases: Vec<SwitchCase>,
        default: Option<BranchTarget>,
    },

    /// Marks unreachable code (e.g. after exhaustive match).
    Unreachable,
}

/// A basic block in the IR.
#[derive(Debug, Clone)]
pub struct Block {
    pub label: Option<Name>,
    pub params: Vec<BlockParam>,
    pub body: Vec<ValueId>,
    pub terminator: Option<Terminator>,
}

impl Block {
    pub fn new(label: Option<Name>) -> Self {
        Self {
            label,
            params: Vec::new(),
            body: Vec::new(),
            terminator: None,
        }
    }
}
