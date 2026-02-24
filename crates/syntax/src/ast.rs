//! Typed AST wrappers over the lossless CST.
//!
//! Each wrapper type corresponds to a `SyntaxKind` node variant and
//! provides typed accessor methods for its children.

pub mod nodes;
pub mod support;
pub mod traits;

use kyokara_parser::SyntaxKind;

use crate::language::SyntaxNode;

/// Trait implemented by all typed AST node wrappers.
pub trait AstNode: Sized {
    /// Returns `true` if a node of the given `SyntaxKind` can be cast
    /// to this type.
    fn can_cast(kind: SyntaxKind) -> bool;

    /// Try to cast a `SyntaxNode` to this type.
    fn cast(node: SyntaxNode) -> Option<Self>;

    /// Access the underlying `SyntaxNode`.
    fn syntax(&self) -> &SyntaxNode;
}
