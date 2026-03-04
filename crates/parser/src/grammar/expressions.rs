//! Expression parsing with Pratt precedence climbing.
//!
//! Binding powers (left_bp, right_bp), all left-associative:
//! - `|>`              : (1, 2)
//! - `||`              : (3, 4)
//! - `&&`              : (5, 6)
//! - `==` `!=`         : (7, 8)
//! - `<` `>` `<=` `>=` : (9, 10)
//! - `|` (bitwise OR)  : (11, 12)
//! - `^` (bitwise XOR) : (13, 14)
//! - `&` (bitwise AND) : (15, 16)
//! - `<<` `>>`         : (17, 18)
//! - `+` `-`           : (19, 20)
//! - `*` `/` `%`       : (21, 22)
//! - Prefix `!` `-` `~`: right_bp 23
//! - Postfix `?` `.` `()` `[]` : left_bp 25

use crate::SyntaxKind::*;
use crate::parser::{CompletedMarker, Parser};
use crate::token_set::TokenSet;

/// Tokens that signal we should stop parsing an expression (recovery).
const EXPR_RECOVERY: TokenSet = TokenSet::new(&[
    LetKw, RBrace, Semicolon, RParen, Comma, FatArrow, TypeKw, FnKw, CapKw, PropertyKw, LeftArrow,
]);
const IF_HEAD_RECOVERY: TokenSet = TokenSet::new(&[LBrace, ElseKw, Semicolon, RBrace]);
const MATCH_HEAD_RECOVERY: TokenSet = TokenSet::new(&[LBrace, Semicolon, RBrace]);
const RANGE_RHS_RECOVERY: TokenSet = TokenSet::new(&[
    LetKw, RBrace, Semicolon, RParen, Comma, FatArrow, TypeKw, FnKw, CapKw, PropertyKw, LeftArrow,
    PipeGt,
]);

/// Entry point: parse an expression.
pub(super) fn expr(p: &mut Parser<'_>) -> Option<CompletedMarker> {
    expr_bp(p, 0)
}

/// Pratt parser: parse expression with minimum binding power `min_bp`.
fn expr_bp(p: &mut Parser<'_>, min_bp: u8) -> Option<CompletedMarker> {
    let mut lhs = lhs(p)?;

    loop {
        // Postfix operators (bp 25).
        lhs = match p.current() {
            Question if 25 >= min_bp => {
                let m = lhs.precede(p);
                p.bump(); // ?
                m.complete(p, PropagateExpr)
            }
            Dot if 25 >= min_bp => {
                let m = lhs.precede(p);
                p.bump(); // .
                p.expect(Ident);
                m.complete(p, FieldExpr)
            }
            LParen if 25 >= min_bp && !p.has_line_break_before_current() => {
                let m = lhs.precede(p);
                arg_list(p);
                m.complete(p, CallExpr)
            }
            LBracket if 25 >= min_bp && !p.has_line_break_before_current() => {
                let m = lhs.precede(p);
                p.bump(); // [
                expr(p);
                p.expect(RBracket);
                m.complete(p, IndexExpr)
            }
            _ => break,
        };
    }

    loop {
        let current = p.current();
        let (left_bp, right_bp) = match current.infix_binding_power() {
            Some(bp) => bp,
            None => break,
        };
        let op_kind = if current == PipeGt {
            PipelineExpr
        } else {
            BinaryExpr
        };

        if left_bp < min_bp {
            break;
        }

        let m = lhs.precede(p);
        p.bump(); // operator
        if current == DotDotLt && !can_start_expr(p.current()) {
            p.error_recover("expected expression after `..<`", RANGE_RHS_RECOVERY);
        } else {
            expr_bp(p, right_bp);
        }
        lhs = m.complete(p, op_kind);
    }

    Some(lhs)
}

/// Parse a left-hand side (prefix unary or primary expression).
fn lhs(p: &mut Parser<'_>) -> Option<CompletedMarker> {
    if p.current().is_unary_prefix_operator() {
        let m = p.open();
        p.bump(); // prefix operator
        expr_bp(p, 23); // prefix bp
        Some(m.complete(p, UnaryExpr))
    } else {
        primary(p)
    }
}

/// Parse a primary expression.
fn primary(p: &mut Parser<'_>) -> Option<CompletedMarker> {
    let cm = match p.current() {
        IntLiteral | FloatLiteral | StringLiteral | CharLiteral => literal_expr(p),
        TrueKw | FalseKw => literal_expr(p),
        Underscore => hole_expr(p),
        LParen => paren_expr(p),
        LBrace => brace_expr(p),
        IfKw => if_expr(p),
        MatchKw => match_expr(p),
        ReturnKw => return_expr(p),
        OldKw => old_expr(p),
        FnKw => lambda_expr(p),
        Ident => ident_or_path_or_record(p),
        LeftArrow => {
            p.error_recover(
                "unexpected `<-` outside property parameter; \
                 did you mean `< -` (comparison with negative) \
                 or `name: Type <- Gen...` in a property parameter?",
                EXPR_RECOVERY,
            );
            return None;
        }
        _ => {
            p.error_recover("expected expression", EXPR_RECOVERY);
            return None;
        }
    };
    Some(cm)
}

fn brace_expr(p: &mut Parser<'_>) -> CompletedMarker {
    if p.nth(1) == Ident && p.nth(2) == Colon {
        let m = p.open();
        record_expr_field_list(p);
        m.complete(p, RecordExpr)
    } else {
        block_expr(p)
    }
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
    expr(p);
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

/// `if '(' Expr ')' BlockExpr ('else' (IfExpr / BlockExpr))?`
fn if_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.open();
    p.bump(); // if
    if p.eat(LParen) {
        expr(p);
        p.expect(RParen);
    } else {
        p.error_recover_parenthesized_head("if condition must be parenthesized", IF_HEAD_RECOVERY);
    }
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

/// `match '(' Expr ')' MatchArmList`
fn match_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.open();
    p.bump(); // match
    if p.eat(LParen) {
        expr(p);
        p.expect(RParen);
    } else {
        p.error_recover_parenthesized_head(
            "match scrutinee must be parenthesized",
            MATCH_HEAD_RECOVERY,
        );
    }
    match_arm_list(p);
    m.complete(p, MatchExpr)
}

fn match_arm_list(p: &mut Parser<'_>) {
    let m = p.open();
    p.expect(LBrace);
    while !p.at(RBrace) && !p.at_eof() {
        let start_pos = p.token_pos();
        if super::patterns::PATTERN_START.contains(p.current()) {
            match_arm(p);
            // Optional comma between arms
            p.eat(Comma);
        } else if p.eat(Pipe) {
            p.error("match arms do not use a leading `|`");
        } else {
            p.error_recover("expected match arm", TokenSet::new(&[RBrace, Comma]));
        }

        if p.token_pos() == start_pos && !p.at_eof() {
            p.bump();
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
        LBrace => {
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
    ) || kind.is_unary_prefix_operator()
}
