//! Parser events — the tree-agnostic output of the parser.
//!
//! The parser emits a flat stream of [`Event`]s. A [`TreeSink`]
//! implementation (in the `syntax` crate) consumes the stream and
//! builds the concrete syntax tree.

use crate::SyntaxKind;

/// A single parser event.
#[derive(Debug, Clone)]
pub enum Event {
    /// Begin a new node of the given kind.
    StartNode { kind: SyntaxKind },
    /// Finish the current node.
    FinishNode,
    /// Consume one token from the input.
    Token { kind: SyntaxKind, len: u32 },
    /// Record a parse error at the current position.
    Error { message: String },
}

/// Trait for consumers that build a tree from parser events.
///
/// The `syntax` crate implements this to construct a rowan green tree.
pub trait TreeSink {
    fn start_node(&mut self, kind: SyntaxKind);
    fn finish_node(&mut self);
    fn token(&mut self, kind: SyntaxKind, text: &str);
    fn error(&mut self, message: String);
}
