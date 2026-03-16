//! IR instructions — the operations that produce values.

use kyokara_hir_def::expr::{BinaryOp, UnaryOp};
use kyokara_hir_def::item_tree::TypeItemIdx;
use kyokara_hir_def::name::Name;
use kyokara_hir_ty::ty::Ty;

use crate::block::BlockId;
use crate::value::ValueId;

/// A constant literal value.
#[derive(Debug, Clone, PartialEq)]
pub enum Constant {
    Int(i64),
    Float(f64),
    String(String),
    Char(char),
    Bool(bool),
    Unit,
}

/// The target of a call instruction.
#[derive(Debug, Clone)]
pub enum CallTarget {
    /// Direct call to a named function.
    Direct(Name),
    /// Indirect call through a value (closure / fn pointer).
    Indirect(ValueId),
    /// Call to a built-in intrinsic by name.
    Intrinsic(String),
}

/// Constraint information attached to a typed hole.
#[derive(Debug, Clone)]
pub struct HoleConstraint {
    pub expected_ty: Ty,
    pub context: String,
}

/// An SSA instruction that produces a value.
#[derive(Debug, Clone)]
pub enum Inst {
    /// Load a constant literal.
    Const(Constant),

    /// Binary operation.
    Binary {
        op: BinaryOp,
        lhs: ValueId,
        rhs: ValueId,
    },

    /// Unary operation.
    Unary { op: UnaryOp, operand: ValueId },

    /// Create a record value from fields.
    RecordCreate { fields: Vec<(Name, ValueId)> },

    /// Access a field on a record value.
    FieldGet { base: ValueId, field: Name },

    /// Functional record update: `{ base with field = value }`.
    RecordUpdate {
        base: ValueId,
        updates: Vec<(Name, ValueId)>,
    },

    /// Construct a tagged ADT variant.
    AdtConstruct {
        type_def: TypeItemIdx,
        variant: Name,
        fields: Vec<ValueId>,
    },

    /// Function call (direct, indirect, or intrinsic).
    Call {
        target: CallTarget,
        args: Vec<ValueId>,
    },

    /// Runtime assertion (lowered from contracts).
    Assert { condition: ValueId, message: String },

    /// A typed hole — placeholder for incomplete code.
    Hole {
        id: u32,
        constraints: Vec<HoleConstraint>,
    },

    /// Reference to a block parameter. The value is the `index`-th
    /// parameter of `block`.
    BlockParam { block: BlockId, index: u32 },

    /// Reference to a function parameter by position.
    /// Not pushed into any block body — just lives in the value arena.
    FnParam { index: u32 },

    /// Extract a positional field from an ADT value (for destructuring).
    AdtFieldGet { base: ValueId, field_index: u32 },

    /// Reference to a top-level function as a first-class value.
    FnRef { name: Name },

    /// Create a closure value for a lifted function plus captured locals.
    ClosureCreate { name: Name, captures: Vec<ValueId> },
}
