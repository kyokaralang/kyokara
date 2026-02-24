//! HIR pattern types.

use crate::expr::{Literal, PatIdx};
use crate::name::Name;
use crate::path::Path;

/// An HIR pattern.
#[derive(Debug, Clone, PartialEq)]
pub enum Pat {
    /// Binding pattern: introduces a name into scope.
    Bind { name: Name },

    /// Wildcard pattern: `_`.
    Wildcard,

    /// Literal pattern: `42`, `"hello"`, `true`.
    Literal(Literal),

    /// Constructor pattern: `Some(x)`, `Ok(v)`.
    Constructor { path: Path, args: Vec<PatIdx> },

    /// Record pattern: `{ x, y }`.
    Record {
        path: Option<Path>,
        fields: Vec<Name>,
    },

    /// Placeholder for parse errors.
    Missing,
}
