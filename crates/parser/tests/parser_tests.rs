//! Parser unit tests — verify event structure for various constructs.
#![allow(clippy::unwrap_used)]

use SyntaxKind::*;
use kyokara_parser::{Event, Input, ParseError, SyntaxKind, parse};

/// Helper: lex token kinds from a list (simulating a lexer), build Input, parse.
fn parse_tokens(kinds: &[SyntaxKind]) -> (Vec<Event>, Vec<ParseError>) {
    let input = Input::new(kinds.to_vec());
    parse(&input)
}

/// Helper: count events of a given variant.
fn count_start_nodes(events: &[Event], kind: SyntaxKind) -> usize {
    events
        .iter()
        .filter(|e| matches!(e, Event::StartNode { kind: k, .. } if *k == kind))
        .count()
}

fn has_node(events: &[Event], kind: SyntaxKind) -> bool {
    count_start_nodes(events, kind) > 0
}

fn has_no_errors(errors: &[ParseError]) -> bool {
    errors.is_empty()
}

// ── Source file ─────────────────────────────────────────────────────

#[test]
fn empty_source_file() {
    let (events, errors) = parse_tokens(&[]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, SourceFile));
}

#[test]
fn module_decl() {
    // module Foo
    let (events, errors) = parse_tokens(&[ModuleKw, Ident]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, SourceFile));
    assert!(has_node(&events, ModuleDecl));
    assert!(has_node(&events, Path));
}

#[test]
fn import_decl() {
    // import Foo.Bar as Baz
    let (events, errors) = parse_tokens(&[ImportKw, Ident, Dot, Ident, AsKw, Ident]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, ImportDecl));
    assert!(has_node(&events, ImportAlias));
}

// ── Let binding ─────────────────────────────────────────────────────

#[test]
fn let_binding_simple() {
    // let x = 42
    let (events, errors) = parse_tokens(&[LetKw, Ident, Eq, IntLiteral]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, LetBinding));
    assert!(has_node(&events, IdentPat));
}

#[test]
fn let_binding_with_type() {
    // let x : Int = 42
    let (events, errors) = parse_tokens(&[LetKw, Ident, Colon, Ident, Eq, IntLiteral]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, LetBinding));
    assert!(has_node(&events, NameType));
}

// ── Type definitions ────────────────────────────────────────────────

#[test]
fn type_alias() {
    // type Name = Int
    let (events, errors) = parse_tokens(&[TypeKw, Ident, Eq, Ident]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, TypeDef));
    assert!(has_node(&events, NameType));
}

#[test]
fn type_with_variants() {
    // type Option = | Some(Int) | None
    let (events, errors) = parse_tokens(&[
        TypeKw, Ident, Eq, Pipe, Ident, LParen, Ident, RParen, Pipe, Ident,
    ]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, TypeDef));
    assert!(has_node(&events, VariantList));
    assert_eq!(count_start_nodes(&events, Variant), 2);
}

#[test]
fn type_with_generics() {
    // type Option<T> = | Some(T) | None
    let (events, errors) = parse_tokens(&[
        TypeKw, Ident, Lt, Ident, Gt, Eq, Pipe, Ident, LParen, Ident, RParen, Pipe, Ident,
    ]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, TypeParamList));
}

// ── Function definitions ────────────────────────────────────────────

#[test]
fn fn_def_simple() {
    // fn foo() { 42 }
    let (events, errors) = parse_tokens(&[FnKw, Ident, LParen, RParen, LBrace, IntLiteral, RBrace]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, FnDef));
    assert!(has_node(&events, ParamList));
    assert!(has_node(&events, BlockExpr));
}

#[test]
fn fn_def_with_params_and_return() {
    // fn add(x: Int, y: Int) -> Int { x }
    let (events, errors) = parse_tokens(&[
        FnKw, Ident, LParen, Ident, Colon, Ident, Comma, Ident, Colon, Ident, RParen, Arrow, Ident,
        LBrace, Ident, RBrace,
    ]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, FnDef));
    assert_eq!(count_start_nodes(&events, Param), 2);
    assert!(has_node(&events, ReturnType));
}

#[test]
fn fn_def_with_contract() {
    // fn foo(x: Int) -> Int requires x { x }
    let (events, errors) = parse_tokens(&[
        FnKw, Ident, LParen, Ident, Colon, Ident, RParen, Arrow, Ident, RequiresKw, Ident, LBrace,
        Ident, RBrace,
    ]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, RequiresClause));
}

#[test]
fn top_level_fn_def_without_body_reports_error() {
    // fn foo() -> Int
    let (events, errors) = parse_tokens(&[FnKw, Ident, LParen, RParen, Arrow, Ident]);
    assert!(!errors.is_empty(), "expected parse error for missing fn body");
    assert!(has_node(&events, FnDef));
}

#[test]
fn pub_top_level_fn_def_without_body_reports_error() {
    // pub fn foo() -> Int
    let (events, errors) =
        parse_tokens(&[PubKw, FnKw, Ident, LParen, RParen, Arrow, Ident]);
    assert!(!errors.is_empty(), "expected parse error for missing pub fn body");
    assert!(has_node(&events, FnDef));
}

#[test]
fn method_def_without_body_reports_error() {
    // fn List.len(self: List) -> Int
    let (events, errors) = parse_tokens(&[
        FnKw, Ident, Dot, Ident, LParen, Ident, Colon, Ident, RParen, Arrow, Ident,
    ]);
    assert!(
        !errors.is_empty(),
        "expected parse error for missing method body"
    );
    assert!(has_node(&events, FnDef));
}

#[test]
fn fn_def_empty_body_is_allowed() {
    // fn noop() -> Unit {}
    let (events, errors) = parse_tokens(&[
        FnKw, Ident, LParen, RParen, Arrow, Ident, LBrace, RBrace,
    ]);
    assert!(
        has_no_errors(&errors),
        "empty function body should parse: {errors:?}"
    );
    assert!(has_node(&events, FnDef));
    assert!(has_node(&events, BlockExpr));
}

// ── Expressions ─────────────────────────────────────────────────────

#[test]
fn binary_expr_add() {
    // let x = 1 + 2
    let (events, errors) = parse_tokens(&[LetKw, Ident, Eq, IntLiteral, Plus, IntLiteral]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, BinaryExpr));
}

#[test]
fn binary_expr_precedence() {
    // let x = 1 + 2 * 3
    let (events, errors) = parse_tokens(&[
        LetKw, Ident, Eq, IntLiteral, Plus, IntLiteral, Star, IntLiteral,
    ]);
    assert!(has_no_errors(&errors));
    // Should have 2 binary exprs
    assert_eq!(count_start_nodes(&events, BinaryExpr), 2);
}

#[test]
fn unary_expr() {
    // let x = !true
    let (events, errors) = parse_tokens(&[LetKw, Ident, Eq, Bang, TrueKw]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, UnaryExpr));
}

#[test]
fn call_expr() {
    // let x = foo(1, 2)
    let (events, errors) = parse_tokens(&[
        LetKw, Ident, Eq, Ident, LParen, IntLiteral, Comma, IntLiteral, RParen,
    ]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, CallExpr));
    assert!(has_node(&events, ArgList));
}

#[test]
fn field_expr() {
    // let x = foo.bar
    let (events, errors) = parse_tokens(&[LetKw, Ident, Eq, Ident, Dot, Ident]);
    // Note: `Ident . Ident` in expr position: the first Ident starts as
    // a path, and `Dot Ident` extends the path (grammar: Path = Ident ('.' Ident)*).
    // Then it's wrapped as PathExpr. This is expected behavior.
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, PathExpr));
}

#[test]
fn pipeline_expr() {
    // let x = a |> b
    let (events, errors) = parse_tokens(&[LetKw, Ident, Eq, Ident, PipeGt, Ident]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, PipelineExpr));
}

#[test]
fn propagate_expr() {
    // let x = foo?
    let (events, errors) = parse_tokens(&[LetKw, Ident, Eq, Ident, Question]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, PropagateExpr));
}

#[test]
fn if_expr() {
    // let x = if true { 1 } else { 2 }
    let (events, errors) = parse_tokens(&[
        LetKw, Ident, Eq, IfKw, TrueKw, LBrace, IntLiteral, RBrace, ElseKw, LBrace, IntLiteral,
        RBrace,
    ]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, IfExpr));
    assert_eq!(count_start_nodes(&events, BlockExpr), 2);
}

#[test]
fn match_expr() {
    // let x = match y { 1 => 2, _ => 3 }
    let (events, errors) = parse_tokens(&[
        LetKw, Ident, Eq, MatchKw, Ident, LBrace, IntLiteral, FatArrow, IntLiteral, Comma,
        Underscore, FatArrow, IntLiteral, RBrace,
    ]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, MatchExpr));
    assert!(has_node(&events, MatchArmList));
    assert_eq!(count_start_nodes(&events, MatchArm), 2);
}

#[test]
fn match_expr_parenthesized_record_scrutinee() {
    // let x = match (Point { x: 1 }) { _ => 0 }
    let (events, errors) = parse_tokens(&[
        LetKw, Ident, Eq, MatchKw, LParen, Ident, LBrace, Ident, Colon, IntLiteral, RBrace, RParen,
        LBrace, Underscore, FatArrow, IntLiteral, RBrace,
    ]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, MatchExpr));
    assert!(has_node(&events, RecordExpr));
}

#[test]
fn if_expr_parenthesized_record_condition() {
    // let x = if (Point { x: 1 }) == (Point { x: 1 }) { 1 } else { 0 }
    let (events, errors) = parse_tokens(&[
        LetKw, Ident, Eq, IfKw, LParen, Ident, LBrace, Ident, Colon, IntLiteral, RBrace, RParen,
        EqEq, LParen, Ident, LBrace, Ident, Colon, IntLiteral, RBrace, RParen, LBrace, IntLiteral,
        RBrace, ElseKw, LBrace, IntLiteral, RBrace,
    ]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, IfExpr));
    assert_eq!(count_start_nodes(&events, RecordExpr), 2);
}

#[test]
fn return_expr() {
    // fn foo() { return 42 }
    let (events, errors) = parse_tokens(&[
        FnKw, Ident, LParen, RParen, LBrace, ReturnKw, IntLiteral, RBrace,
    ]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, ReturnExpr));
}

#[test]
fn lambda_expr() {
    // let f = fn(x) => x
    let (events, errors) = parse_tokens(&[
        LetKw, Ident, Eq, FnKw, LParen, Ident, RParen, FatArrow, Ident,
    ]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, LambdaExpr));
}

#[test]
fn hole_expr() {
    // let x = _
    let (events, errors) = parse_tokens(&[LetKw, Ident, Eq, Underscore]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, HoleExpr));
}

#[test]
fn old_expr() {
    // fn foo(x: Int) ensures old(x) { x }
    let (events, errors) = parse_tokens(&[
        FnKw, Ident, LParen, Ident, Colon, Ident, RParen, EnsuresKw, OldKw, LParen, Ident, RParen,
        LBrace, Ident, RBrace,
    ]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, OldExpr));
    assert!(has_node(&events, EnsuresClause));
}

#[test]
fn block_expr_allows_semicolon_separators() {
    // fn main() -> Int { let x = 1; x; }
    let (events, errors) = parse_tokens(&[
        FnKw, Ident, LParen, RParen, Arrow, Ident, LBrace, LetKw, Ident, Eq, IntLiteral, Semicolon,
        Ident, Semicolon, RBrace,
    ]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, FnDef));
    assert!(has_node(&events, BlockExpr));
}

// ── Patterns ────────────────────────────────────────────────────────

#[test]
fn wildcard_pattern() {
    // match x { _ => 0 }
    let (events, errors) = parse_tokens(&[
        LetKw, Ident, Eq, MatchKw, Ident, LBrace, Underscore, FatArrow, IntLiteral, RBrace,
    ]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, WildcardPat));
}

#[test]
fn constructor_pattern() {
    // match x { Some(y) => y }
    let (events, errors) = parse_tokens(&[
        LetKw, Ident, Eq, MatchKw, Ident, LBrace, Ident, LParen, Ident, RParen, FatArrow, Ident,
        RBrace,
    ]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, ConstructorPat));
}

// ── Type expressions ────────────────────────────────────────────────

#[test]
fn fn_type() {
    // type F = fn(Int) -> Bool
    let (events, errors) =
        parse_tokens(&[TypeKw, Ident, Eq, FnKw, LParen, Ident, RParen, Arrow, Ident]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, FnType));
}

#[test]
fn record_type() {
    // type Point = { x: Int, y: Int }
    let (events, errors) = parse_tokens(&[
        TypeKw, Ident, Eq, LBrace, Ident, Colon, Ident, Comma, Ident, Colon, Ident, RBrace,
    ]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, RecordType));
    assert_eq!(count_start_nodes(&events, RecordField), 2);
}

#[test]
fn refined_type() {
    // type Pos = { x: Int | x > 0 }
    let (events, errors) = parse_tokens(&[
        TypeKw, Ident, Eq, LBrace, Ident, Colon, Ident, Pipe, Ident, Gt, IntLiteral, RBrace,
    ]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, RefinedType));
}

// ── Cap definition ──────────────────────────────────────────────────

#[test]
fn cap_def() {
    // cap IO { fn read() { 0 } }
    let (events, errors) = parse_tokens(&[
        CapKw, Ident, LBrace, FnKw, Ident, LParen, RParen, LBrace, IntLiteral, RBrace, RBrace,
    ]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, CapDef));
    assert!(has_node(&events, FnDef));
}

// ── Modulo, logical, and bitwise operators ──────────────────────────

#[test]
fn binary_expr_modulo() {
    // let x = 10 % 3
    let (events, errors) = parse_tokens(&[LetKw, Ident, Eq, IntLiteral, Percent, IntLiteral]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, BinaryExpr));
}

#[test]
fn binary_expr_logical_and() {
    // let x = true && false
    let (events, errors) = parse_tokens(&[LetKw, Ident, Eq, TrueKw, AmpAmp, FalseKw]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, BinaryExpr));
}

#[test]
fn binary_expr_logical_or() {
    // let x = true || false
    let (events, errors) = parse_tokens(&[LetKw, Ident, Eq, TrueKw, PipePipe, FalseKw]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, BinaryExpr));
}

#[test]
fn binary_expr_bitwise_and() {
    // let x = 3 & 1
    let (events, errors) = parse_tokens(&[LetKw, Ident, Eq, IntLiteral, Amp, IntLiteral]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, BinaryExpr));
}

#[test]
fn binary_expr_bitwise_or() {
    // let x = 3 | 1
    let (events, errors) = parse_tokens(&[LetKw, Ident, Eq, IntLiteral, Pipe, IntLiteral]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, BinaryExpr));
}

#[test]
fn binary_expr_bitwise_xor() {
    // let x = 3 ^ 1
    let (events, errors) = parse_tokens(&[LetKw, Ident, Eq, IntLiteral, Caret, IntLiteral]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, BinaryExpr));
}

#[test]
fn binary_expr_shift_left() {
    // let x = 1 << 3
    let (events, errors) = parse_tokens(&[LetKw, Ident, Eq, IntLiteral, LtLt, IntLiteral]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, BinaryExpr));
}

#[test]
fn binary_expr_shift_right() {
    // let x = 8 >> 2
    let (events, errors) = parse_tokens(&[LetKw, Ident, Eq, IntLiteral, GtGt, IntLiteral]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, BinaryExpr));
}

#[test]
fn unary_expr_bitwise_not() {
    // let x = ~42
    let (events, errors) = parse_tokens(&[LetKw, Ident, Eq, Tilde, IntLiteral]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, UnaryExpr));
}

#[test]
fn bitwise_precedence_and_tighter_than_or() {
    // let x = a & b | c  → should parse as (a & b) | c → 2 BinaryExprs
    let (events, errors) = parse_tokens(&[LetKw, Ident, Eq, Ident, Amp, Ident, Pipe, Ident]);
    assert!(has_no_errors(&errors));
    assert_eq!(count_start_nodes(&events, BinaryExpr), 2);
}

#[test]
fn bitwise_precedence_xor_between_and_or() {
    // let x = a | b ^ c  → should parse as a | (b ^ c) → 2 BinaryExprs
    let (events, errors) = parse_tokens(&[LetKw, Ident, Eq, Ident, Pipe, Ident, Caret, Ident]);
    assert!(has_no_errors(&errors));
    assert_eq!(count_start_nodes(&events, BinaryExpr), 2);
}

#[test]
fn addition_tighter_than_shift() {
    // let x = a + b << c  → should parse as (a + b) << c → 2 BinaryExprs
    let (events, errors) = parse_tokens(&[LetKw, Ident, Eq, Ident, Plus, Ident, LtLt, Ident]);
    assert!(has_no_errors(&errors));
    assert_eq!(count_start_nodes(&events, BinaryExpr), 2);
}

#[test]
fn bitwise_tighter_than_comparison() {
    // let x = a == b & c  → should parse as a == (b & c) → 2 BinaryExprs
    let (events, errors) = parse_tokens(&[LetKw, Ident, Eq, Ident, EqEq, Ident, Amp, Ident]);
    assert!(has_no_errors(&errors));
    assert_eq!(count_start_nodes(&events, BinaryExpr), 2);
}

#[test]
fn logical_looser_than_comparison() {
    // let x = a == b && c == d  → 2 comparisons + 1 logical = 3 BinaryExprs
    let (events, errors) = parse_tokens(&[
        LetKw, Ident, Eq, Ident, EqEq, Ident, AmpAmp, Ident, EqEq, Ident,
    ]);
    assert!(has_no_errors(&errors));
    assert_eq!(count_start_nodes(&events, BinaryExpr), 3);
}

#[test]
fn modulo_same_precedence_as_multiply() {
    // let x = a * b % c → 2 BinaryExprs (left-assoc: (a * b) % c)
    let (events, errors) = parse_tokens(&[LetKw, Ident, Eq, Ident, Star, Ident, Percent, Ident]);
    assert!(has_no_errors(&errors));
    assert_eq!(count_start_nodes(&events, BinaryExpr), 2);
}

#[test]
fn tilde_tighter_than_shift() {
    // let x = ~a << b  → should parse as (~a) << b → 1 UnaryExpr + 1 BinaryExpr
    let (events, errors) = parse_tokens(&[LetKw, Ident, Eq, Tilde, Ident, LtLt, Ident]);
    assert!(has_no_errors(&errors));
    assert!(has_node(&events, UnaryExpr));
    assert!(has_node(&events, BinaryExpr));
}

// ── Error recovery ──────────────────────────────────────────────────

#[test]
fn error_recovery_missing_eq_in_let() {
    // let x 42  (missing =)
    let (events, errors) = parse_tokens(&[LetKw, Ident, IntLiteral]);
    assert!(!has_no_errors(&errors));
    assert!(has_node(&events, LetBinding));
    assert!(has_node(&events, ErrorNode));
}

#[test]
fn error_recovery_unknown_item() {
    // We have an unexpected token at top level
    let (events, errors) = parse_tokens(&[IntLiteral]);
    assert!(!has_no_errors(&errors));
    assert!(has_node(&events, SourceFile));
}

// ── Property syntax ────────────────────────────────────────────────

#[test]
fn property_with_generator_binding() {
    // property p(x: Int <- Gen.auto()) { x > 0 }
    let (events, errors) = parse_tokens(&[
        PropertyKw, Ident, LParen, Ident, Colon, Ident, LeftArrow, Ident, Dot, Ident, LParen,
        RParen, RParen, LBrace, Ident, Gt, IntLiteral, RBrace,
    ]);
    assert!(has_no_errors(&errors), "errors: {errors:?}");
    assert!(has_node(&events, PropertyDef));
    assert!(has_node(&events, PropertyParamList));
    assert!(has_node(&events, PropertyParam));
}

#[test]
fn property_with_where_clause() {
    // property p(x: Int <- Gen.auto()) where x > 0 { x > 0 }
    let (events, errors) = parse_tokens(&[
        PropertyKw, Ident, LParen, Ident, Colon, Ident, LeftArrow, Ident, Dot, Ident, LParen,
        RParen, RParen, WhereKw, Ident, Gt, IntLiteral, LBrace, Ident, Gt, IntLiteral, RBrace,
    ]);
    assert!(has_no_errors(&errors), "errors: {errors:?}");
    assert!(has_node(&events, PropertyDef));
    assert!(has_node(&events, PropertyParamList));
    assert!(has_node(&events, WhereClause));
}

#[test]
fn property_multiple_params() {
    // property p(a: Int <- Gen.auto(), b: Int <- Gen.auto()) { a + b == b + a }
    let (events, errors) = parse_tokens(&[
        PropertyKw, Ident, LParen, Ident, Colon, Ident, LeftArrow, Ident, Dot, Ident, LParen,
        RParen, Comma, Ident, Colon, Ident, LeftArrow, Ident, Dot, Ident, LParen, RParen, RParen,
        LBrace, Ident, Plus, Ident, EqEq, Ident, Plus, Ident, RBrace,
    ]);
    assert!(has_no_errors(&errors), "errors: {errors:?}");
    assert!(has_node(&events, PropertyDef));
    assert_eq!(count_start_nodes(&events, PropertyParam), 2);
}

#[test]
fn property_bare_param_is_error() {
    // property p(x: Int) { true }  — missing <- binding
    let (events, errors) = parse_tokens(&[
        PropertyKw, Ident, LParen, Ident, Colon, Ident, RParen, LBrace, TrueKw, RBrace,
    ]);
    assert!(
        !has_no_errors(&errors),
        "bare property params should produce errors"
    );
    assert!(has_node(&events, PropertyDef));
    assert!(
        errors.iter().any(|e| e.message.contains("<-")),
        "error should mention `<-`: {errors:?}"
    );
}

#[test]
fn left_arrow_outside_property_is_error() {
    // let x = <- b  — LeftArrow where an expression is expected
    let (_events, errors) = parse_tokens(&[LetKw, Ident, Eq, LeftArrow, Ident]);
    assert!(
        !has_no_errors(&errors),
        "LeftArrow in expr should produce error"
    );
    assert!(
        errors.iter().any(|e| e.message.contains("<-")),
        "error should mention `<-`: {errors:?}"
    );
}

#[test]
fn property_without_body() {
    // property p(x: Int <- Gen.auto()) — no body, just declaration
    let (events, errors) = parse_tokens(&[
        PropertyKw, Ident, LParen, Ident, Colon, Ident, LeftArrow, Ident, Dot, Ident, LParen,
        RParen, RParen,
    ]);
    assert!(
        has_no_errors(&errors),
        "bodyless property should parse: {errors:?}"
    );
    assert!(has_node(&events, PropertyDef));
    assert!(has_node(&events, PropertyParamList));
}

#[test]
fn property_trailing_comma() {
    // property p(x: Int <- Gen.auto(),) { x > 0 }
    let (events, errors) = parse_tokens(&[
        PropertyKw, Ident, LParen, Ident, Colon, Ident, LeftArrow, Ident, Dot, Ident, LParen,
        RParen, Comma, RParen, LBrace, Ident, Gt, IntLiteral, RBrace,
    ]);
    assert!(
        has_no_errors(&errors),
        "trailing comma should be ok: {errors:?}"
    );
    assert!(has_node(&events, PropertyDef));
    assert_eq!(count_start_nodes(&events, PropertyParam), 1);
}

#[test]
fn property_empty_params() {
    // property p() { true }
    let (events, errors) =
        parse_tokens(&[PropertyKw, Ident, LParen, RParen, LBrace, TrueKw, RBrace]);
    assert!(
        has_no_errors(&errors),
        "empty param list should parse: {errors:?}"
    );
    assert!(has_node(&events, PropertyDef));
    assert!(has_node(&events, PropertyParamList));
    assert_eq!(count_start_nodes(&events, PropertyParam), 0);
}

#[test]
fn property_where_without_body() {
    // property p(x: Int <- Gen.auto()) where x > 0
    let (events, errors) = parse_tokens(&[
        PropertyKw, Ident, LParen, Ident, Colon, Ident, LeftArrow, Ident, Dot, Ident, LParen,
        RParen, RParen, WhereKw, Ident, Gt, IntLiteral,
    ]);
    assert!(
        has_no_errors(&errors),
        "where without body should parse: {errors:?}"
    );
    assert!(has_node(&events, PropertyDef));
    assert!(has_node(&events, WhereClause));
}

#[test]
fn property_missing_lparen() {
    // property p x: Int <- Gen.auto() { true }  — missing (
    let (events, errors) = parse_tokens(&[
        PropertyKw, Ident, Ident, Colon, Ident, LeftArrow, Ident, Dot, Ident, LParen, RParen,
        LBrace, TrueKw, RBrace,
    ]);
    assert!(!has_no_errors(&errors), "missing ( should error");
    assert!(has_node(&events, PropertyDef));
}

#[test]
fn left_arrow_after_binary_op() {
    // let x = 1 + <- 2  — LeftArrow after a binary operator
    let (_events, errors) =
        parse_tokens(&[LetKw, Ident, Eq, IntLiteral, Plus, LeftArrow, IntLiteral]);
    assert!(
        !has_no_errors(&errors),
        "LeftArrow after + should produce error"
    );
    assert!(
        errors.iter().any(|e| e.message.contains("<-")),
        "error should mention `<-`: {errors:?}"
    );
}

#[test]
fn left_arrow_in_if_branch() {
    // let x = if true { <- 1 } else { 0 }
    let (_events, errors) = parse_tokens(&[
        LetKw, Ident, Eq, IfKw, TrueKw, LBrace, LeftArrow, IntLiteral, RBrace, ElseKw, LBrace,
        IntLiteral, RBrace,
    ]);
    assert!(
        !has_no_errors(&errors),
        "LeftArrow inside if body should produce error"
    );
}
