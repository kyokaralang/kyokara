//! The recursive-descent parser engine.
//!
//! Provides [`Parser`], [`Marker`], and [`CompletedMarker`] — the core
//! API used by grammar modules to build the event stream.

use crate::SyntaxKind;
use crate::event::Event;
use crate::input::Input;
use crate::token_set::TokenSet;

/// A parse error message with location info.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    /// Byte start offset of the error in the source text (filled in by syntax crate).
    pub range_start: u32,
    /// Byte end offset (exclusive) of the error in the source text.
    pub range_end: u32,
    /// Non-trivia token index at error time (used by syntax crate to compute byte offsets).
    pub token_pos: usize,
}

/// The parser engine. Grammar functions receive `&mut Parser` and call
/// methods to inspect and consume tokens, emitting events.
pub struct Parser<'i> {
    input: &'i Input,
    /// Current non-trivia token index.
    pos: usize,
    /// Accumulated events.
    events: Vec<Event>,
    /// Accumulated errors.
    errors: Vec<ParseError>,
}

impl<'i> Parser<'i> {
    pub fn new(input: &'i Input) -> Parser<'i> {
        Parser {
            input,
            pos: 0,
            events: Vec::new(),
            errors: Vec::new(),
        }
    }

    /// Consume the parser and return the event stream and errors.
    pub fn finish(self) -> (Vec<Event>, Vec<ParseError>) {
        (self.events, self.errors)
    }

    // ── Token inspection ────────────────────────────────────────────

    /// The kind of the current non-trivia token.
    pub fn current(&self) -> SyntaxKind {
        self.nth(0)
    }

    /// The kind of the non-trivia token `n` positions ahead.
    pub fn nth(&self, n: usize) -> SyntaxKind {
        self.input.kind(self.pos + n)
    }

    /// Returns `true` if the current token matches `kind`.
    pub fn at(&self, kind: SyntaxKind) -> bool {
        self.current() == kind
    }

    /// Peek at the token after the current one (used to look past `pub`).
    pub fn current_after_pub(&self) -> SyntaxKind {
        self.nth(1)
    }

    /// Returns `true` if the current token is in `set`.
    #[allow(dead_code)]
    pub fn at_set(&self, set: TokenSet) -> bool {
        set.contains(self.current())
    }

    /// Returns `true` if we've reached the end of input.
    pub fn at_eof(&self) -> bool {
        self.pos >= self.input.len()
    }

    /// Current non-trivia token position.
    pub fn token_pos(&self) -> usize {
        self.pos
    }

    // ── Token consumption ───────────────────────────────────────────

    /// Advance past the current token, emitting a `Token` event.
    pub fn bump(&mut self) {
        let kind = self.current();
        assert!(!self.at_eof(), "bump at EOF");
        self.do_bump(kind);
    }

    /// Advance past the current token, remapping its kind.
    #[allow(dead_code)]
    pub fn bump_remap(&mut self, kind: SyntaxKind) {
        assert!(!self.at_eof(), "bump_remap at EOF");
        self.do_bump(kind);
    }

    fn do_bump(&mut self, kind: SyntaxKind) {
        self.events.push(Event::Token {
            kind,
            n_raw_tokens: 1,
        });
        self.pos += 1;
    }

    /// If the current token is `kind`, consume it and return `true`.
    pub fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// Consume `kind` or emit an error.
    pub fn expect(&mut self, kind: SyntaxKind) -> bool {
        if self.eat(kind) {
            true
        } else {
            self.error_recover(&format!("expected {kind:?}"), TokenSet::EMPTY);
            false
        }
    }

    // ── Markers ─────────────────────────────────────────────────────

    /// Open a new node. Returns a `Marker` that must be either
    /// completed or abandoned.
    pub fn open(&mut self) -> Marker {
        let pos = self.events.len() as u32;
        self.events.push(Event::StartNode {
            kind: SyntaxKind::ErrorNode,
            forward_parent: None,
        });
        Marker { pos }
    }

    // ── Error handling ──────────────────────────────────────────────

    /// Record a parse error at the current position.
    pub fn error(&mut self, message: &str) {
        self.events.push(Event::Error {
            message: message.to_owned(),
        });
        self.errors.push(ParseError {
            message: message.to_owned(),
            range_start: 0,
            range_end: 0,
            token_pos: self.pos,
        });
    }

    /// Emit an error and skip tokens until we find one in `recovery`.
    pub fn error_recover(&mut self, message: &str, recovery: TokenSet) {
        if self.at_eof() || self.at(SyntaxKind::RBrace) || recovery.contains(self.current()) {
            self.error(message);
            return;
        }
        let m = self.open();
        self.error(message);
        self.bump();
        m.complete(self, SyntaxKind::ErrorNode);
    }

    /// Skip tokens wrapping them in an ErrorNode until we hit something
    /// in `recovery` or EOF.
    #[allow(dead_code)]
    pub fn error_recover_until(&mut self, message: &str, recovery: TokenSet) {
        if self.at_eof() || recovery.contains(self.current()) {
            self.error(message);
            return;
        }
        let m = self.open();
        self.error(message);
        while !self.at_eof() && !recovery.contains(self.current()) {
            self.bump();
        }
        m.complete(self, SyntaxKind::ErrorNode);
    }
}

// ── Marker ──────────────────────────────────────────────────────────

/// A handle to a `StartNode` event that has been pushed but not yet
/// completed. Must be either [`complete`](Marker::complete)d or
/// [`abandon`](Marker::abandon)ed.
pub struct Marker {
    pos: u32,
}

impl Marker {
    /// Complete this node with the given `kind`. Pushes a `FinishNode`
    /// event and returns a `CompletedMarker` that can be used with
    /// `precede()`.
    pub fn complete(self, p: &mut Parser<'_>, kind: SyntaxKind) -> CompletedMarker {
        match &mut p.events[self.pos as usize] {
            Event::StartNode { kind: k, .. } => *k = kind,
            _ => unreachable!("Marker::complete on non-StartNode"),
        }
        p.events.push(Event::FinishNode);
        CompletedMarker { pos: self.pos }
    }

    /// Abandon this marker — converts its `StartNode` event to a
    /// `Tombstone`.
    pub fn abandon(self, p: &mut Parser<'_>) {
        if self.pos as usize == p.events.len() - 1 {
            // Last event — just pop it.
            match p.events.pop() {
                Some(Event::StartNode { .. }) => {}
                _ => unreachable!("Marker::abandon on non-StartNode"),
            }
        } else {
            p.events[self.pos as usize] = Event::Tombstone;
        }
    }
}

/// A completed node marker. Can be used with [`precede()`](CompletedMarker::precede)
/// to retroactively wrap the node in a parent.
pub struct CompletedMarker {
    pos: u32,
}

impl CompletedMarker {
    /// Create a new parent marker that wraps this completed node.
    ///
    /// This is essential for Pratt parsing: after parsing `a`, we
    /// discover `+`, so we need to wrap `a` in a `BinaryExpr` node
    /// that also contains `+` and `b`.
    pub fn precede(self, p: &mut Parser<'_>) -> Marker {
        let new_pos = p.events.len() as u32;
        p.events.push(Event::StartNode {
            kind: SyntaxKind::ErrorNode,
            forward_parent: None,
        });
        // Point the original StartNode's forward_parent to the new one.
        match &mut p.events[self.pos as usize] {
            Event::StartNode { forward_parent, .. } => {
                *forward_parent = Some(new_pos - self.pos);
            }
            _ => unreachable!("CompletedMarker::precede on non-StartNode"),
        }
        Marker { pos: new_pos }
    }
}
