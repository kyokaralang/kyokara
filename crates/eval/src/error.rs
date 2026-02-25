//! Runtime errors for the tree-walking interpreter.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("division by zero")]
    DivisionByZero,

    #[error("pattern match failure: no arm matched")]
    PatternMatchFailure,

    #[error("unresolved name: {0}")]
    UnresolvedName(String),

    #[error("encountered a typed hole — program is incomplete")]
    HoleEncountered,

    #[error("type error at runtime: {0}")]
    TypeError(String),

    #[error("no `main` function found")]
    NoMainFunction,

    #[error("missing expression")]
    MissingExpr,

    #[error("precondition failed: {0}")]
    PreconditionFailed(String),

    #[error("postcondition failed: {0}")]
    PostconditionFailed(String),

    #[error("invariant violated: {0}")]
    InvariantViolated(String),
}
