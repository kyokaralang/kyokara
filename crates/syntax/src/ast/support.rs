//! Helper functions for extracting children from `SyntaxNode`.

use kyokara_parser::SyntaxKind;

use crate::language::{SyntaxNode, SyntaxToken};

/// Find the first child node that can be cast to `N`.
pub fn child<N: super::AstNode>(parent: &SyntaxNode) -> Option<N> {
    parent.children().find_map(N::cast)
}

/// Iterate over all children that can be cast to `N`.
pub fn children<'a, N: super::AstNode + 'a>(
    parent: &'a SyntaxNode,
) -> impl Iterator<Item = N> + 'a {
    parent.children().filter_map(N::cast)
}

/// Find the first child token with the given kind.
pub fn token(parent: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxToken> {
    parent
        .children_with_tokens()
        .filter_map(|it| it.into_token())
        .find(|it| it.kind() == kind)
}

/// Find the first `Ident` token in the node.
pub fn name_token(parent: &SyntaxNode) -> Option<SyntaxToken> {
    token(parent, SyntaxKind::Ident)
}
