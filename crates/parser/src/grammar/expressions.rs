//! Expression parsing with Pratt precedence climbing.
//!
//! Binding powers (left_bp, right_bp), all left-associative:
//! - `|>`              : (1, 2)
//! - `||`              : (3, 4)
//! - `&&`              : (5, 6)
//! - `==` `!=`         : (7, 8)
//! - `<` `>` `<=` `>=` : (9, 10)
//! - `+` `-`           : (11, 12)
//! - `*` `/` `%`       : (13, 14)
//! - Prefix `!` `-`    : right_bp 15
//! - Postfix `?` `.` `()` : left_bp 17

use crate::SyntaxKind::*;
use crate::parser::{CompletedMarker, Parser};
use crate::token_set::TokenSet;

/// Tokens that signal we should stop parsing an expression (recovery).
const EXPR_RECOVERY: TokenSet = TokenSet::new(&[
    LetKw, RBrace, Semicolon, RParen, Comma, FatArrow, TypeKw, FnKw, CapKw, PropertyKw,
]);

/// Entry point: parse an expression.
pub(super) fn expr(p: &mut Parser<'_>) -> Option<CompletedMarker> {
    expr_bp(p, 0)
}

/// Parse an expression without allowing `Path {` as RecordExpr.
/// Used in match scrutinees, if conditions, and contract clauses.
pub(super) fn expr_no_record(p: &mut Parser<'_>) -> Option<CompletedMarker> {
    let old = p.no_record_expr;
    p.no_record_expr = true;
    let result = expr_bp(p, 0);
    p.no_record_expr = old;
    result
}

/// Pratt parser: parse expression with minimum binding power `min_bp`.
fn expr_bp(p: &mut Parser<'_>, min_bp: u8) -> Option<CompletedMarker> {
    let mut lhs = lhs(p)?;

    loop {
        // Postfix operators (bp 17).
        lhs = match p.current() {
            Question if 17 >= min_bp => {
                let m = lhs.precede(p);
                p.bump(); // ?
                m.complete(p, PropagateExpr)
            }
            Dot if 17 >= min_bp => {
                let m = lhs.precede(p);
                p.bump(); // .
                p.expect(Ident);
                m.complete(p, FieldExpr)
            }
            LParen if 17 >= min_bp => {
                let m = lhs.precede(p);
                arg_list(p);
                m.complete(p, CallExpr)
            }
            _ => break,
        };
    }

    loop {
        let (op_kind, left_bp, right_bp) = match p.current() {
            PipeGt => (PipelineExpr, 1, 2),
            PipePipe => (BinaryExpr, 3, 4),
            AmpAmp => (BinaryExpr, 5, 6),
            EqEq | BangEq => (BinaryExpr, 7, 8),
            Lt | Gt | LtEq | GtEq => (BinaryExpr, 9, 10),
            Plus | Minus => (BinaryExpr, 11, 12),
            Star | Slash | Percent => (BinaryExpr, 13, 14),
            _ => break,
        };

        if left_bp < min_bp {
            break;
        }

        let m = lhs.precede(p);
        p.bump(); // operator
        expr_bp(p, right_bp);
        lhs = m.complete(p, op_kind);
    }

    Some(lhs)
}

/// Parse a left-hand side (prefix unary or primary expression).
fn lhs(p: &mut Parser<'_>) -> Option<CompletedMarker> {
    match p.current() {
        Bang | Minus => {
            let m = p.open();
            p.bump(); // prefix operator
            expr_bp(p, 15); // prefix bp
            Some(m.complete(p, UnaryExpr))
        }
        _ => primary(p),
    }
}

/// Parse a primary expression.
fn primary(p: &mut Parser<'_>) -> Option<CompletedMarker> {
    let cm = match p.current() {
        IntLiteral | FloatLiteral | StringLiteral | CharLiteral => literal_expr(p),
        TrueKw | FalseKw => literal_expr(p),
        Underscore => hole_expr(p),
        LParen => paren_expr(p),
        LBrace => block_expr(p),
        IfKw => if_expr(p),
        MatchKw => match_expr(p),
        ReturnKw => return_expr(p),
        OldKw => old_expr(p),
        FnKw => lambda_expr(p),
        Ident => ident_or_path_or_record(p),
        _ => {
            p.error_recover("expected expression", EXPR_RECOVERY);
            return None;
        }
    };
    Some(cm)
}

fn literal_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.open();
    p.bump();
    m.complete(p, LiteralExpr)
}

fn hole_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.open();
    p.bump(); // _
    m.complete(p, HoleExpr)
}

fn paren_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.open();
    p.bump(); // (
    // `expr_no_record` disables `Path { .. }` at the immediate parse site to
    // avoid ambiguity with blocks, but parenthesized expressions should still
    // be able to contain record literals.
    let old = p.no_record_expr;
    p.no_record_expr = false;
    expr(p);
    p.no_record_expr = old;
    p.expect(RParen);
    m.complete(p, ParenExpr)
}

/// `{ (LetBinding / Expr)* Expr? }`
pub(super) fn block_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.open();
    p.bump(); // {
    while !p.at(RBrace) && !p.at_eof() {
        if p.at(LetKw) {
            super::items::let_binding(p);
            while p.eat(Semicolon) {}
        } else if expr(p).is_some() {
            // Expression statement — optionally followed by semicolons
            // (we don't require semicolons in the grammar)
            while p.eat(Semicolon) {}
        } else {
            // Couldn't parse expression or let — skip one token for recovery
            break;
        }
    }
    p.expect(RBrace);
    m.complete(p, BlockExpr)
}

/// `if Expr BlockExpr ('else' (IfExpr / BlockExpr))?`
fn if_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.open();
    p.bump(); // if
    expr_no_record(p);
    if p.at(LBrace) {
        block_expr(p);
    } else {
        p.error("expected block after if condition");
    }
    if p.eat(ElseKw) {
        if p.at(IfKw) {
            if_expr(p);
        } else if p.at(LBrace) {
            block_expr(p);
        } else {
            p.error("expected block or if after else");
        }
    }
    m.complete(p, IfExpr)
}

/// `match Expr MatchArmList`
fn match_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.open();
    p.bump(); // match
    expr_no_record(p);
    match_arm_list(p);
    m.complete(p, MatchExpr)
}

fn match_arm_list(p: &mut Parser<'_>) {
    let m = p.open();
    p.expect(LBrace);
    while !p.at(RBrace) && !p.at_eof() {
        if super::patterns::PATTERN_START.contains(p.current()) {
            match_arm(p);
            // Optional comma between arms
            p.eat(Comma);
        } else {
            p.error_recover("expected match arm", TokenSet::new(&[RBrace, Pipe]));
        }
    }
    p.expect(RBrace);
    m.complete(p, MatchArmList);
}

fn match_arm(p: &mut Parser<'_>) {
    let m = p.open();
    super::patterns::pattern(p);
    p.expect(FatArrow);
    expr(p);
    m.complete(p, MatchArm);
}

/// `return Expr?`
fn return_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.open();
    p.bump(); // return
    // Parse expression if the next token can start one.
    if can_start_expr(p.current()) {
        expr(p);
    }
    m.complete(p, ReturnExpr)
}

/// `old(Expr)`
fn old_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.open();
    p.bump(); // old
    p.expect(LParen);
    expr(p);
    p.expect(RParen);
    m.complete(p, OldExpr)
}

/// `fn '(' (Param (',' Param)*)? ')' '=>' Expr`
fn lambda_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.open();
    p.bump(); // fn

    // param list
    let pm = p.open();
    p.expect(LParen);
    if !p.at(RParen) {
        lambda_param(p);
        while p.eat(Comma) {
            if p.at(RParen) {
                break;
            }
            lambda_param(p);
        }
    }
    p.expect(RParen);
    pm.complete(p, ParamList);

    p.expect(FatArrow);
    expr(p);
    m.complete(p, LambdaExpr)
}

fn lambda_param(p: &mut Parser<'_>) {
    let m = p.open();
    p.expect(Ident);
    if p.eat(Colon) {
        super::types::type_expr(p);
    }
    m.complete(p, Param);
}

/// Parse ident, then decide: path expr, record expr, or plain ident expr.
///
/// Uses `parse_single_path` (single ident) so that a trailing `.field`
/// is handled by the postfix-dot loop in `expr_bp` as `FieldExpr`,
/// rather than being greedily consumed into a multi-segment `Path`.
fn ident_or_path_or_record(p: &mut Parser<'_>) -> CompletedMarker {
    let path_cm = super::parse_single_path(p);

    match p.current() {
        LBrace if !p.no_record_expr => {
            // Record expression: `Path { field: value, ... }`
            let m = path_cm.precede(p);
            record_expr_field_list(p);
            m.complete(p, RecordExpr)
        }
        _ => {
            // Just a path/ident expr.
            let m = path_cm.precede(p);
            m.complete(p, PathExpr)
        }
    }
}

fn record_expr_field_list(p: &mut Parser<'_>) {
    let m = p.open();
    p.bump(); // {
    if !p.at(RBrace) {
        record_expr_field(p);
        while p.eat(Comma) {
            if p.at(RBrace) {
                break;
            }
            record_expr_field(p);
        }
    }
    p.expect(RBrace);
    m.complete(p, RecordExprFieldList);
}

fn record_expr_field(p: &mut Parser<'_>) {
    let m = p.open();
    p.expect(Ident);
    p.expect(Colon);
    expr(p);
    m.complete(p, RecordExprField);
}

fn arg_list(p: &mut Parser<'_>) {
    let m = p.open();
    p.bump(); // (
    if !p.at(RParen) {
        arg(p);
        while p.eat(Comma) {
            if p.at(RParen) {
                break;
            }
            arg(p);
        }
    }
    p.expect(RParen);
    m.complete(p, ArgList);
}

/// Parse an argument: either `Ident ':' Expr` (named) or `Expr`.
fn arg(p: &mut Parser<'_>) {
    // Lookahead for named argument: `Ident ':'`
    if p.at(Ident) && p.nth(1) == Colon {
        let m = p.open();
        p.bump(); // ident
        p.bump(); // :
        expr(p);
        m.complete(p, NamedArg);
    } else {
        expr(p);
    }
}

/// Returns true if `kind` can start an expression.
fn can_start_expr(kind: crate::SyntaxKind) -> bool {
    matches!(
        kind,
        IntLiteral
            | FloatLiteral
            | StringLiteral
            | CharLiteral
            | TrueKw
            | FalseKw
            | Ident
            | Underscore
            | LParen
            | LBrace
            | IfKw
            | MatchKw
            | ReturnKw
            | OldKw
            | FnKw
            | Bang
            | Minus
    )
}
