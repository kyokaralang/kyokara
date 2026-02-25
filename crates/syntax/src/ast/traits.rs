//! Shared accessor traits for AST nodes.

use kyokara_parser::SyntaxKind;

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

/// Implemented by nodes that can be preceded by a `pub` keyword.
pub trait HasVisibility: super::AstNode {
    fn pub_token(&self) -> Option<SyntaxToken> {
        support::token(self.syntax(), SyntaxKind::PubKw)
    }

    fn is_pub(&self) -> bool {
        self.pub_token().is_some()
    }
}
