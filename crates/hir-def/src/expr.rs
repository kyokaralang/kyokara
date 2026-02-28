//! HIR expression, statement, and operator types.

use la_arena::Idx;

use crate::name::Name;
use crate::pat::Pat;
use crate::path::Path;
use crate::type_ref::TypeRef;

/// Index into the body's expression arena.
pub type ExprIdx = Idx<Expr>;

/// Index into the body's pattern arena.
pub type PatIdx = Idx<Pat>;

/// A literal value.
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Int(i64),
    Float(f64),
    String(String),
    Char(char),
    Bool(bool),
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

impl BinaryOp {
    pub fn is_numeric_arithmetic(self) -> bool {
        matches!(
            self,
            Self::Add | Self::Sub | Self::Mul | Self::Div | Self::Mod
        )
    }

    pub fn is_equality(self) -> bool {
        matches!(self, Self::Eq | Self::NotEq)
    }

    pub fn is_ordering(self) -> bool {
        matches!(self, Self::Lt | Self::Gt | Self::LtEq | Self::GtEq)
    }

    pub fn is_logical(self) -> bool {
        matches!(self, Self::And | Self::Or)
    }

    pub fn is_bitwise_or_shift(self) -> bool {
        matches!(
            self,
            Self::BitAnd | Self::BitOr | Self::BitXor | Self::Shl | Self::Shr
        )
    }

    pub fn returns_bool(self) -> bool {
        self.is_equality() || self.is_ordering() || self.is_logical()
    }
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    Not,
    Neg,
    BitNot,
}

impl UnaryOp {
    pub fn is_numeric_negation(self) -> bool {
        matches!(self, Self::Neg)
    }

    pub fn is_logical_not(self) -> bool {
        matches!(self, Self::Not)
    }

    pub fn is_bitwise_not(self) -> bool {
        matches!(self, Self::BitNot)
    }
}

/// A function call argument (positional or named).
#[derive(Debug, Clone, PartialEq)]
pub enum CallArg {
    Positional(ExprIdx),
    Named { name: Name, value: ExprIdx },
}

/// A match arm in HIR.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pat: PatIdx,
    pub body: ExprIdx,
}

/// A statement inside a block.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// `let pat (: ty)? = init`
    Let {
        pat: PatIdx,
        ty: Option<TypeRef>,
        init: ExprIdx,
    },
    /// An expression statement.
    Expr(ExprIdx),
}

/// An HIR expression.
///
/// Desugared: no `PipelineExpr` or `PropagateExpr`. Pipeline becomes
/// `Call`, propagation becomes `Match` + `Return`.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// A literal value.
    Literal(Literal),

    /// A path expression, resolved during name resolution.
    Path(Path),

    /// Binary operation: `a + b`.
    Binary {
        op: BinaryOp,
        lhs: ExprIdx,
        rhs: ExprIdx,
    },

    /// Unary operation: `!x`, `-x`.
    Unary { op: UnaryOp, operand: ExprIdx },

    /// Function call (also the target of `|>` desugaring).
    Call { callee: ExprIdx, args: Vec<CallArg> },

    /// Field access: `expr.field`.
    Field { base: ExprIdx, field: Name },

    /// `if cond { then } else { else }`.
    If {
        condition: ExprIdx,
        then_branch: ExprIdx,
        else_branch: Option<ExprIdx>,
    },

    /// `match scrutinee { arms }`.
    Match {
        scrutinee: ExprIdx,
        arms: Vec<MatchArm>,
    },

    /// `{ stmts; tail? }`.
    Block {
        stmts: Vec<Stmt>,
        tail: Option<ExprIdx>,
    },

    /// `return expr?`.
    Return(Option<ExprIdx>),

    /// Record literal: `Foo { x: 1, y: 2 }`.
    RecordLit {
        path: Option<Path>,
        fields: Vec<(Name, ExprIdx)>,
    },

    /// Lambda / closure: `fn(x) => body`.
    Lambda {
        params: Vec<(PatIdx, Option<TypeRef>)>,
        body: ExprIdx,
    },

    /// `old(expr)` — preserved for contract checking.
    Old(ExprIdx),

    /// Typed hole `_`.
    Hole,

    /// Placeholder for parse errors.
    Missing,
}

#[cfg(test)]
mod tests {
    use super::{BinaryOp, UnaryOp};

    #[test]
    fn binary_op_categories_are_disjoint_for_core_groups() {
        for op in [
            BinaryOp::Add,
            BinaryOp::Sub,
            BinaryOp::Mul,
            BinaryOp::Div,
            BinaryOp::Mod,
            BinaryOp::Eq,
            BinaryOp::NotEq,
            BinaryOp::Lt,
            BinaryOp::Gt,
            BinaryOp::LtEq,
            BinaryOp::GtEq,
            BinaryOp::And,
            BinaryOp::Or,
            BinaryOp::BitAnd,
            BinaryOp::BitOr,
            BinaryOp::BitXor,
            BinaryOp::Shl,
            BinaryOp::Shr,
        ] {
            let hits = usize::from(op.is_numeric_arithmetic())
                + usize::from(op.is_equality())
                + usize::from(op.is_ordering())
                + usize::from(op.is_logical())
                + usize::from(op.is_bitwise_or_shift());
            assert_eq!(hits, 1, "operator {op:?} should be in exactly one category");
        }
    }

    #[test]
    fn binary_op_returns_bool_matches_semantics() {
        assert!(BinaryOp::Eq.returns_bool());
        assert!(BinaryOp::Gt.returns_bool());
        assert!(BinaryOp::And.returns_bool());
        assert!(!BinaryOp::Add.returns_bool());
        assert!(!BinaryOp::BitAnd.returns_bool());
    }

    #[test]
    fn unary_op_categories_match_variants() {
        assert!(UnaryOp::Neg.is_numeric_negation());
        assert!(UnaryOp::Not.is_logical_not());
        assert!(UnaryOp::BitNot.is_bitwise_not());
    }
}
