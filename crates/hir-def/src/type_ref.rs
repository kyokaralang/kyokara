//! Surface-level type references in the HIR.
//!
//! `TypeRef` represents types as written in source code, before type
//! inference resolves them.

use crate::expr::ExprIdx;
use crate::name::Name;
use crate::path::Path;

/// A reference to a type as written in source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeRef {
    /// A named type, possibly with type arguments: `Int`, `Option<T>`.
    Path { path: Path, args: Vec<TypeRef> },

    /// Function type: `fn(A, B) -> C`.
    Fn {
        params: Vec<TypeRef>,
        ret: Box<TypeRef>,
    },

    /// Record type: `{ x: Int, y: String }`.
    Record { fields: Vec<(Name, TypeRef)> },

    /// Refined type: `{ x: Int | x > 0 }`.
    Refined {
        name: Name,
        base: Box<TypeRef>,
        predicate: ExprIdx,
    },

    /// A placeholder for parse errors.
    Error,
}
