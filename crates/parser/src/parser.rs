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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IdentifierRole {
    PathSegment,
    ImportAlias,
    TypeName,
    VariantName,
    FunctionName,
    MethodName,
    EffectName,
    PropertyName,
    TypeParameterName,
    ParameterName,
    LocalBindingName,
    FieldName,
    ArgumentName,
    PatternName,
}

impl IdentifierRole {
    fn description(self) -> &'static str {
        match self {
            Self::PathSegment => "a path segment",
            Self::ImportAlias => "an import alias",
            Self::TypeName => "a type name",
            Self::VariantName => "a variant name",
            Self::FunctionName => "a function name",
            Self::MethodName => "a method name",
            Self::EffectName => "an effect name",
            Self::PropertyName => "a property name",
            Self::TypeParameterName => "a type parameter name",
            Self::ParameterName => "a parameter name",
            Self::LocalBindingName => "a local binding name",
            Self::FieldName => "a field name",
            Self::ArgumentName => "an argument name",
            Self::PatternName => "a pattern name",
        }
    }
}

/// The parser engine. Grammar functions receive `&mut Parser` and call
/// methods to inspect and consume tokens, emitting events.
pub struct Parser<'i> {
    input: &'i Input,
    /// Current non-trivia token index.
    pos: usize,
    /// Virtual `>` token pending from splitting a `>>` token while parsing type args.
    pending_virtual_gt: u8,
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
            pending_virtual_gt: 0,
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
        if self.pending_virtual_gt > 0 {
            if n == 0 {
                SyntaxKind::Gt
            } else {
                self.input.kind(self.pos + n - 1)
            }
        } else {
            self.input.kind(self.pos + n)
        }
    }

    /// Returns `true` if the current token matches `kind`.
    pub fn at(&self, kind: SyntaxKind) -> bool {
        self.current() == kind
    }

    /// Returns `true` when the current token can lexically occupy an
    /// identifier slot (either a true identifier or a reserved keyword).
    pub fn at_ident_like(&self) -> bool {
        self.at(SyntaxKind::Ident) || self.current().is_keyword()
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
        self.pending_virtual_gt == 0 && self.pos >= self.input.len()
    }

    /// Current non-trivia token position.
    pub fn token_pos(&self) -> usize {
        if self.pending_virtual_gt > 0 {
            self.pos.saturating_sub(1)
        } else {
            self.pos
        }
    }

    /// Returns `true` when a newline appears in trivia immediately before the
    /// current token.
    pub fn has_line_break_before_current(&self) -> bool {
        if self.pending_virtual_gt > 0 {
            false
        } else {
            self.input.line_break_before(self.pos)
        }
    }

    // ── Token consumption ───────────────────────────────────────────

    /// Advance past the current token, emitting a `Token` event.
    pub fn bump(&mut self) {
        if self.pending_virtual_gt > 0 {
            self.pending_virtual_gt -= 1;
            return;
        }
        let kind = self.current();
        assert!(!self.at_eof(), "bump at EOF");
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

    /// Consume an identifier or emit a targeted reserved-keyword diagnostic.
    pub fn expect_identifier(&mut self, role: IdentifierRole) -> bool {
        self.expect_identifier_recover(role, TokenSet::EMPTY)
    }

    /// Consume an identifier or emit a targeted reserved-keyword diagnostic,
    /// falling back to generic recovery for non-identifier tokens.
    pub fn expect_identifier_recover(&mut self, role: IdentifierRole, recovery: TokenSet) -> bool {
        if self.eat(SyntaxKind::Ident) {
            return true;
        }
        if self.current().is_keyword() {
            self.error_keyword_as_identifier(role);
            return false;
        }
        self.error_recover("expected Ident", recovery);
        false
    }

    /// Consume one right-angle token for type-argument parsing.
    ///
    /// In type contexts, `>>` should be interpreted as two consecutive `>`
    /// delimiters for nested generic closures.
    pub fn eat_type_arg_rangle(&mut self) -> bool {
        if self.eat(SyntaxKind::Gt) {
            return true;
        }
        if self.at(SyntaxKind::GtGt) {
            self.bump(); // consume raw `>>`
            self.pending_virtual_gt = self.pending_virtual_gt.saturating_add(1);
            return true;
        }
        false
    }

    /// Expect one right-angle token for type-argument parsing.
    pub fn expect_type_arg_rangle(&mut self) -> bool {
        if self.eat_type_arg_rangle() {
            true
        } else {
            self.error_recover("expected Gt", TokenSet::EMPTY);
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

    /// Recovery for missing-parenthesis head-expression sites (`if`, `match`,
    /// and contract/where clauses).
    ///
    /// This behaves like `error_recover_until`, but treats `{ ... }` immediately
    /// following an identifier as record-literal-shaped head content when it
    /// starts with `Ident ':'`, so we skip that brace group instead of stopping
    /// at the first `{`.
    pub fn error_recover_parenthesized_head(&mut self, message: &str, recovery: TokenSet) {
        self.error(message);
        self.recover_parenthesized_head_content(recovery);
    }

    /// Consume tokens for missing-parenthesis head recovery without emitting
    /// a new diagnostic.
    pub fn recover_parenthesized_head_content(&mut self, recovery: TokenSet) {
        if self.at_eof() {
            return;
        }

        let m = self.open();
        let mut consumed_any = false;
        let mut prev_kind: Option<SyntaxKind> = None;

        while !self.at_eof() {
            let current = self.current();
            if recovery.contains(current) {
                if current == SyntaxKind::LBrace
                    && prev_kind == Some(SyntaxKind::Ident)
                    && self.looks_like_record_literal_brace()
                {
                    self.skip_balanced_braces();
                    consumed_any = true;
                    prev_kind = Some(SyntaxKind::RBrace);
                    continue;
                }
                break;
            }

            let before = self.pos;
            prev_kind = Some(current);
            self.bump();
            consumed_any = true;
            if self.pos == before {
                break;
            }
        }

        if consumed_any {
            m.complete(self, SyntaxKind::ErrorNode);
        } else {
            m.abandon(self);
        }
    }

    fn looks_like_record_literal_brace(&self) -> bool {
        self.at(SyntaxKind::LBrace)
            && self.nth(1) == SyntaxKind::Ident
            && self.nth(2) == SyntaxKind::Colon
    }

    fn skip_balanced_braces(&mut self) {
        if !self.at(SyntaxKind::LBrace) {
            return;
        }

        let mut depth = 0usize;
        while !self.at_eof() {
            let before = self.pos;
            match self.current() {
                SyntaxKind::LBrace => {
                    depth += 1;
                    self.bump();
                }
                SyntaxKind::RBrace => {
                    depth = depth.saturating_sub(1);
                    self.bump();
                    if depth == 0 {
                        break;
                    }
                }
                _ => self.bump(),
            }
            if self.pos == before {
                break;
            }
        }
    }

    fn error_keyword_as_identifier(&mut self, role: IdentifierRole) {
        let keyword = self.current();
        let keyword_text = keyword
            .keyword_text()
            .expect("keyword diagnostic should only be used for keyword tokens");
        let err = self.open();
        self.error(&format!(
            "reserved keyword `{keyword_text}` cannot be used as {}",
            role.description()
        ));
        self.bump();
        err.complete(self, SyntaxKind::ErrorNode);
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

#[cfg(test)]
mod tests {
    use super::{IdentifierRole, Parser};
    use crate::{Input, SyntaxKind};

    #[test]
    fn expect_identifier_reports_exact_keyword_lexeme_for_all_roles() {
        let keywords = [
            SyntaxKind::ModuleKw,
            SyntaxKind::ImportKw,
            SyntaxKind::AsKw,
            SyntaxKind::TypeKw,
            SyntaxKind::FnKw,
            SyntaxKind::LetKw,
            SyntaxKind::MatchKw,
            SyntaxKind::CapKw,
            SyntaxKind::EffectKw,
            SyntaxKind::WithKw,
            SyntaxKind::RequiresKw,
            SyntaxKind::EnsuresKw,
            SyntaxKind::InvariantKw,
            SyntaxKind::ContractKw,
            SyntaxKind::PropertyKw,
            SyntaxKind::ForKw,
            SyntaxKind::InKw,
            SyntaxKind::WhileKw,
            SyntaxKind::BreakKw,
            SyntaxKind::ContinueKw,
            SyntaxKind::WhereKw,
            SyntaxKind::OldKw,
            SyntaxKind::TrueKw,
            SyntaxKind::FalseKw,
            SyntaxKind::IfKw,
            SyntaxKind::ElseKw,
            SyntaxKind::ReturnKw,
            SyntaxKind::PubKw,
        ];
        let roles = [
            IdentifierRole::PathSegment,
            IdentifierRole::ImportAlias,
            IdentifierRole::TypeName,
            IdentifierRole::VariantName,
            IdentifierRole::FunctionName,
            IdentifierRole::MethodName,
            IdentifierRole::EffectName,
            IdentifierRole::PropertyName,
            IdentifierRole::TypeParameterName,
            IdentifierRole::ParameterName,
            IdentifierRole::LocalBindingName,
            IdentifierRole::FieldName,
            IdentifierRole::ArgumentName,
            IdentifierRole::PatternName,
        ];

        for keyword in keywords {
            for role in roles {
                let input = Input::new(vec![keyword]);
                let mut parser = Parser::new(&input);

                assert!(
                    parser.at_ident_like(),
                    "keyword should be ident-like: {keyword:?}"
                );
                assert!(
                    !parser.expect_identifier(role),
                    "keyword should not be accepted as identifier: {keyword:?} / {role:?}"
                );
                assert!(
                    parser.at_eof(),
                    "keyword recovery should consume exactly one token: {keyword:?} / {role:?}"
                );

                let (_events, errors) = parser.finish();
                assert_eq!(
                    errors.len(),
                    1,
                    "expected one parse error for {keyword:?} / {role:?}, got: {errors:?}"
                );

                let expected = format!(
                    "reserved keyword `{}` cannot be used as {}",
                    keyword
                        .keyword_text()
                        .expect("keyword_text should exist for every keyword"),
                    role.description()
                );
                assert_eq!(
                    errors[0].message, expected,
                    "unexpected message for {keyword:?} / {role:?}"
                );
                assert_eq!(
                    errors[0].token_pos, 0,
                    "error should point at offending keyword for {keyword:?} / {role:?}"
                );
            }
        }
    }
}
