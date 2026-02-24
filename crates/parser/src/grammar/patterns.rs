//! Pattern parsing.
//!
//! ```peg
//! Pattern        <- ConstructorPat / RecordPat / LiteralPat / WildcardPat / IdentPat
//! IdentPat       <- Ident
//! ConstructorPat <- Path '(' PatList ')'
//! PatList        <- (Pattern (',' Pattern)* ','?)?
//! WildcardPat    <- '_'
//! LiteralPat     <- IntLiteral / FloatLiteral / StringLiteral / CharLiteral / 'true' / 'false'
//! RecordPat      <- Path? '{' (Ident (',' Ident)* ','?)? '}'
//! ```

use crate::SyntaxKind::*;
use crate::parser::{CompletedMarker, Parser};
use crate::token_set::TokenSet;

/// Tokens that can start a pattern.
pub(super) const PATTERN_START: TokenSet = TokenSet::new(&[
    Ident,
    Underscore,
    IntLiteral,
    FloatLiteral,
    StringLiteral,
    CharLiteral,
    TrueKw,
    FalseKw,
    LBrace,
]);

pub(super) fn pattern(p: &mut Parser<'_>) -> Option<CompletedMarker> {
    let cm = match p.current() {
        Underscore => wildcard_pat(p),
        IntLiteral | FloatLiteral | StringLiteral | CharLiteral | TrueKw | FalseKw => {
            literal_pat(p)
        }
        LBrace => record_pat(p, None),
        Ident => ident_or_constructor_pat(p),
        _ => {
            p.error("expected pattern");
            return None;
        }
    };
    Some(cm)
}

fn wildcard_pat(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.open();
    p.bump(); // _
    m.complete(p, WildcardPat)
}

fn literal_pat(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.open();
    p.bump(); // literal or true/false
    m.complete(p, LiteralPat)
}

/// Parse `Ident` and then decide: constructor `Path(...)`, record `Path { }`,
/// or plain identifier pattern.
fn ident_or_constructor_pat(p: &mut Parser<'_>) -> CompletedMarker {
    let path = super::parse_path(p);

    match p.current() {
        LParen => {
            // ConstructorPat <- Path '(' PatList ')'
            let m = path.precede(p);
            p.bump(); // (
            pat_list(p);
            p.expect(RParen);
            m.complete(p, ConstructorPat)
        }
        LBrace => {
            // RecordPat <- Path '{' ... '}'
            record_pat(p, Some(path))
        }
        _ => {
            // IdentPat (single ident) or PathExpr-as-pattern (multi-segment path)
            // For the pattern grammar, we wrap the path node as IdentPat.
            let m = path.precede(p);
            m.complete(p, IdentPat)
        }
    }
}

fn record_pat(p: &mut Parser<'_>, path: Option<CompletedMarker>) -> CompletedMarker {
    let m = match path {
        Some(cm) => cm.precede(p),
        None => p.open(),
    };
    p.expect(LBrace);
    if p.at(Ident) {
        p.bump();
        while p.eat(Comma) {
            if p.at(RBrace) {
                break; // trailing comma
            }
            p.expect(Ident);
        }
    }
    p.expect(RBrace);
    m.complete(p, RecordPat)
}

fn pat_list(p: &mut Parser<'_>) {
    if p.at(RParen) {
        return;
    }
    pattern(p);
    while p.eat(Comma) {
        if p.at(RParen) {
            break; // trailing comma
        }
        pattern(p);
    }
}
