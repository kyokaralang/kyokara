//! Type expression parsing.
//!
//! ```peg
//! TypeExpr   <- FnType / RefinedType / RecordType / NameType
//! NameType   <- Path TypeArgList?
//! FnType     <- 'fn' '(' (TypeExpr (',' TypeExpr)*)? ')' '->' TypeExpr
//! RecordType <- '{' RecordField (',' RecordField)* ','? '}'
//! RefinedType <- '{' Ident ':' TypeExpr '|' Expr '}'
//! ```

use crate::SyntaxKind::*;
use crate::parser::{CompletedMarker, Parser};

pub(super) fn type_expr(p: &mut Parser<'_>) -> Option<CompletedMarker> {
    match p.current() {
        FnKw => Some(fn_type(p)),
        LBrace => Some(brace_type(p)),
        Ident => Some(name_type(p)),
        _ => {
            p.error("expected type expression");
            None
        }
    }
}

/// `fn(A, B) -> C`
fn fn_type(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.open();
    p.bump(); // fn
    p.expect(LParen);
    if !p.at(RParen) {
        type_expr(p);
        while p.eat(Comma) {
            if p.at(RParen) {
                break;
            }
            type_expr(p);
        }
    }
    p.expect(RParen);
    p.expect(Arrow);
    type_expr(p);
    m.complete(p, FnType)
}

/// Disambiguate `{ Ident : Type | Expr }` (refined) vs `{ field: Type, ... }` (record).
fn brace_type(p: &mut Parser<'_>) -> CompletedMarker {
    // Both start with `{ Ident : TypeExpr`.
    // Refined has `|` after the first field, record has `,` or `}`.
    let m = p.open();
    p.bump(); // {

    if p.at(RBrace) {
        // Empty record type `{}`
        p.bump();
        return m.complete(p, RecordType);
    }

    // Parse first field: `Ident : TypeExpr`
    if !p.at(Ident) {
        p.error("expected field name");
        p.expect(RBrace);
        return m.complete(p, RecordType);
    }

    // We need lookahead. Parse `Ident : TypeExpr` then check for `|`.
    // Use a nested marker for the first field.
    let field_m = p.open();
    p.bump(); // Ident
    p.expect(Colon);
    type_expr(p);

    if p.at(Pipe) {
        // Refined type: `{ x: Int | x > 0 }`
        field_m.abandon(p);
        p.bump(); // |
        super::expressions::expr(p);
        p.expect(RBrace);
        m.complete(p, RefinedType)
    } else {
        // Record type: complete the first field, parse rest
        field_m.complete(p, RecordField);
        while p.eat(Comma) {
            if p.at(RBrace) {
                break;
            }
            record_field(p);
        }
        p.expect(RBrace);
        m.complete(p, RecordType)
    }
}

fn record_field(p: &mut Parser<'_>) {
    let m = p.open();
    p.expect(Ident);
    p.expect(Colon);
    type_expr(p);
    m.complete(p, RecordField);
}

/// `Path TypeArgList?`
fn name_type(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.open();
    super::parse_path(p);
    if p.at(Lt) {
        type_arg_list(p);
    }
    m.complete(p, NameType)
}

/// `< TypeExpr (',' TypeExpr)* ','? >`
pub(super) fn type_arg_list(p: &mut Parser<'_>) {
    let m = p.open();
    p.bump(); // <
    type_expr(p);
    while p.eat(Comma) {
        if p.at(Gt) || p.at(GtGt) {
            break;
        }
        type_expr(p);
    }
    p.expect_type_arg_rangle();
    m.complete(p, TypeArgList);
}
