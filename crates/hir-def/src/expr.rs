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
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    Not,
    Neg,
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
