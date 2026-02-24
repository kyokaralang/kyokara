//! Shared accessor traits for AST nodes.

use crate::ast::support;
use crate::language::SyntaxToken;

/// Implemented by nodes that have an `Ident` name token.
pub trait HasName: super::AstNode {
    fn name_token(&self) -> Option<SyntaxToken> {
        support::name_token(self.syntax())
    }
}

/// Implemented by nodes that have a `TypeParamList` child.
pub trait HasTypeParams: super::AstNode {
    fn type_param_list(&self) -> Option<super::nodes::TypeParamList> {
        support::child(self.syntax())
    }
}
