//! Parser events — the tree-agnostic output of the parser.
//!
//! The parser emits a flat `Vec<Event>`. The syntax crate's bridge
//! processes events directly to build a rowan green tree.

use crate::SyntaxKind;

/// A single parser event.
#[derive(Debug, Clone)]
pub enum Event {
    /// Begin a new node of the given kind.
    ///
    /// `forward_parent` is used by `CompletedMarker::precede()` to
    /// retroactively wrap an already-completed node. When set, it points
    /// (as an offset from this event's index) to the *real* parent
    /// `StartNode` event. The bridge resolves these forward pointers
    /// before building the tree.
    StartNode {
        kind: SyntaxKind,
        forward_parent: Option<u32>,
    },
    /// Finish the current node.
    FinishNode,
    /// Consume `n_raw_tokens` raw tokens from the input.
    ///
    /// `n_raw_tokens` is typically 1, but may be > 1 for composed tokens.
    Token { kind: SyntaxKind, n_raw_tokens: u8 },
    /// Record a parse error at the current position.
    Error { message: String },
    /// Placeholder for abandoned or moved markers.
    ///
    /// When a `Marker` is abandoned or its `StartNode` is moved via
    /// `precede()`, the original event slot becomes a `Tombstone`.
    Tombstone,
}
