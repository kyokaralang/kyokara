//! Integration tests — parse source text, verify CST structure + lossless roundtrip.
#![allow(clippy::unwrap_used)]

use kyokara_syntax::{SyntaxKind, parse};

/// Helper: parse source, check no errors, return CST debug string.
fn parse_ok(src: &str) -> rowan::GreenNode {
    let result = parse(src);
    assert!(
        result.errors.is_empty(),
        "unexpected parse errors for {src:?}: {:?}",
        result.errors
    );
    result.green
}

/// Verify the root node is SourceFile.
fn assert_root_is_source_file(green: &rowan::GreenNode) {
    assert_eq!(
        green.kind(),
        rowan::SyntaxKind(SyntaxKind::SourceFile as u16)
    );
}

/// Reconstruct source text from the green tree (lossless roundtrip).
fn green_text(green: &rowan::GreenNode) -> String {
    use rowan::NodeOrToken;
    let mut out = String::new();
    fn collect(node: &rowan::GreenNodeData, out: &mut String) {
        for child in node.children() {
            match child {
                NodeOrToken::Node(n) => collect(n, out),
                NodeOrToken::Token(t) => out.push_str(t.text()),
            }
        }
    }
    collect(green, &mut out);
    out
}

// ── Lossless roundtrip ──────────────────────────────────────────────

#[test]
fn roundtrip_empty() {
    let green = parse_ok("");
    assert_root_is_source_file(&green);
    assert_eq!(green_text(&green), "");
}

#[test]
fn roundtrip_let_binding() {
    let src = "let x = 42";
    let green = parse_ok(src);
    assert_root_is_source_file(&green);
    assert_eq!(green_text(&green), src);
}

#[test]
fn roundtrip_fn_def() {
    let src = "fn add(x: Int, y: Int) -> Int { x }";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn parse_top_level_fn_without_body_reports_error() {
    let src = "fn foo() -> Int";
    let result = parse(src);
    assert!(
        !result.errors.is_empty(),
        "expected parse error for missing fn body"
    );
    assert_eq!(green_text(&result.green), src);
}

#[test]
fn parse_cap_member_fn_without_body_is_allowed() {
    let src = "cap IO {\n  fn read() -> String\n}";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn parse_top_level_fn_empty_body_is_allowed() {
    let src = "fn noop() -> Unit {}";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn parse_misordered_contract_clause_reports_specific_error() {
    let src = "fn inc(x: Int) -> Int ensures (result > x) requires (x >= 0) { x + 1 }";
    let result = parse(src);
    assert_eq!(
        result.errors.len(),
        1,
        "expected one targeted parse error, got: {:?}",
        result.errors
    );
    assert!(
        result.errors[0]
            .message
            .contains("requires cannot appear after ensures"),
        "expected order-specific message, got: {:?}",
        result.errors
    );
    assert_eq!(green_text(&result.green), src);
}

#[test]
fn roundtrip_type_def_variants() {
    let src = "type Option = Some(Int) | None";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn roundtrip_module_and_import() {
    let src = "module Main\n\nimport Std.IO as IO";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn roundtrip_if_else() {
    let src = "let x = if (true) { 1 } else { 2 }";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn roundtrip_match() {
    let src = "let x = match (y) { 1 => 2, _ => 3 }";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn parse_match_with_leading_pipe_arm_reports_targeted_error() {
    let src = "let x = match (y) { | _ => 0 }";
    let result = parse(src);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.message.contains("match arms do not use a leading `|`")),
        "expected leading-pipe match-arm diagnostic, got: {:?}",
        result.errors
    );
    assert_eq!(green_text(&result.green), src);
}

#[test]
fn parse_match_with_repeated_leading_pipes_reports_each() {
    let src = "let x = match (y) { | | 1 => 2, _ => 3 }";
    let result = parse(src);
    assert!(
        result
            .errors
            .iter()
            .filter(|e| e.message.contains("match arms do not use a leading `|`"))
            .count()
            >= 2,
        "expected repeated leading-pipe diagnostics, got: {:?}",
        result.errors
    );
    assert_eq!(green_text(&result.green), src);
}

#[test]
fn roundtrip_binary_ops() {
    let src = "let x = 1 + 2 * 3";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn roundtrip_pipeline() {
    let src = "let x = a |> b |> c";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn roundtrip_lambda() {
    let src = "let f = fn(x) => x";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn roundtrip_record_type() {
    let src = "type Point = { x: Int, y: Int }";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn roundtrip_refined_type() {
    let src = "type Pos = { x: Int | x > 0 }";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn roundtrip_fn_type() {
    let src = "type F = fn(Int) -> Bool";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn roundtrip_cap_def() {
    let src = "cap IO {\n  fn read() { 0 }\n}";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn roundtrip_with_comments() {
    let src = "// a comment\nlet x = 42 /* inline */";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn roundtrip_generics() {
    let src = "type Option<T> = Some(T) | None";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn parse_type_variant_leading_pipe_reports_error() {
    let src = "type Option = | Some(Int) | None";
    let result = parse(src);
    assert!(
        result.errors.iter().any(|e| e
            .message
            .contains("leading `|` is not allowed in type variants")),
        "expected leading-pipe rejection, got: {:?}",
        result.errors
    );
    assert_eq!(green_text(&result.green), src);
}

#[test]
fn parse_pub_property_reports_error_and_recovers() {
    let src = "pub property p(x: Int <- Gen.int()) { true }\nfn ok() { 1 }";
    let result = parse(src);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.message.contains("expected item")),
        "expected pub property error, got: {:?}",
        result.errors
    );
    assert_eq!(green_text(&result.green), src);
}

#[test]
fn parse_pub_let_reports_error_and_recovers() {
    let src = "pub let x = 1\nfn ok() { 1 }";
    let result = parse(src);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.message.contains("expected item")),
        "expected pub let error, got: {:?}",
        result.errors
    );
    assert_eq!(green_text(&result.green), src);
}

#[test]
fn roundtrip_generic_fn() {
    let src = "fn id<T>(x: T) -> T { x }";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn roundtrip_call_with_args() {
    let src = "let x = foo(1, 2, 3)";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn roundtrip_named_arg() {
    let src = "let x = foo(name: 42)";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn roundtrip_unary() {
    let src = "let x = !true";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn roundtrip_propagate() {
    let src = "let x = foo?";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn roundtrip_return() {
    let src = "fn foo() { return 42 }";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn roundtrip_old_expr() {
    let src = "fn foo(x: Int) ensures (old(x)) { x }";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn roundtrip_wildcard() {
    let src = "let _ = 42";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn roundtrip_hole_expr() {
    let src = "let x = _";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn roundtrip_property_def() {
    let src = "property commutative(x: Int <- Gen.auto(), y: Int <- Gen.auto()) { x }";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn roundtrip_property_with_where() {
    let src = "property p(x: Int <- Gen.auto()) where (x > 0) { x > 0 }";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn parse_if_without_parenthesized_condition_reports_targeted_error() {
    let src = "let x = if true { 1 } else { 2 }";
    let result = parse(src);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.message.contains("if condition must be parenthesized")),
        "expected parenthesized-if diagnostic, got: {:?}",
        result.errors
    );
    assert_eq!(green_text(&result.green), src);
}

#[test]
fn parse_match_without_parenthesized_scrutinee_reports_targeted_error() {
    let src = "let x = match y { 1 => 2, _ => 3 }";
    let result = parse(src);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.message.contains("match scrutinee must be parenthesized")),
        "expected parenthesized-match diagnostic, got: {:?}",
        result.errors
    );
    assert_eq!(green_text(&result.green), src);
}

#[test]
fn parse_requires_without_parenthesized_expr_reports_targeted_error() {
    let src = "fn f(x: Int) -> Int requires x > 0 { x }";
    let result = parse(src);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.message.contains("requires clause expression must be parenthesized")),
        "expected parenthesized-requires diagnostic, got: {:?}",
        result.errors
    );
    assert_eq!(green_text(&result.green), src);
}

#[test]
fn parse_where_without_parenthesized_expr_reports_targeted_error() {
    let src = "property p(x: Int <- Gen.auto()) where x > 0 { x > 0 }";
    let result = parse(src);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.message.contains("where clause expression must be parenthesized")),
        "expected parenthesized-where diagnostic, got: {:?}",
        result.errors
    );
    assert_eq!(green_text(&result.green), src);
}

#[test]
fn roundtrip_property_gen_int_range() {
    let src = "property p(x: Int <- Gen.int_range(1, 100)) { true }";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn roundtrip_property_gen_list() {
    let src = "property p(xs: List<Int> <- Gen.list(Gen.int())) { true }";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn roundtrip_property_trailing_comma() {
    let src = "property p(x: Int <- Gen.auto(),) { true }";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn roundtrip_property_no_body() {
    let src = "property p(x: Int <- Gen.auto())";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn roundtrip_record_expr() {
    let src = "let p = Point { x: 1, y: 2 }";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn roundtrip_full_program() {
    let src = r#"module Main

import Std.IO as IO

type Option<T> =
    Some(T)
    | None

fn map<T, U>(opt: Option<T>, f: fn(T) -> U) -> Option<U> {
    match (opt) {
        Some(x) => f(x),
        None => None,
    }
}

let result = Some(42)"#;
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

// ── Error recovery ──────────────────────────────────────────────────

#[test]
fn error_recovery_does_not_panic() {
    // Malformed: missing = in let
    let result = parse("let x 42");
    assert!(!result.errors.is_empty());
    // Should still produce a tree (with error nodes)
    assert_root_is_source_file(&result.green);
    // Lossless: all text is preserved
    assert_eq!(green_text(&result.green), "let x 42");
}

#[test]
fn error_recovery_multiple_items() {
    // First item is broken, second is fine
    let src = "let x\nlet y = 1";
    let result = parse(src);
    // Should have errors from first let
    assert!(!result.errors.is_empty());
    // But all text preserved
    assert_eq!(green_text(&result.green), src);
}

// ── Underscore lexer test ───────────────────────────────────────────

#[test]
fn lexer_emits_underscore() {
    let tokens = kyokara_syntax::lex("_");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::Underscore);
    assert_eq!(tokens[0].text, "_");
}

#[test]
fn lexer_underscore_prefix_is_ident() {
    let tokens = kyokara_syntax::lex("_foo");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::Ident);
}

// ── LeftArrow lexer tests ───────────────────────────────────────────

#[test]
fn lexer_left_arrow() {
    let tokens = kyokara_syntax::lex("<-");
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, SyntaxKind::LeftArrow);
    assert_eq!(tokens[0].text, "<-");
}

#[test]
fn lexer_left_arrow_in_context() {
    // "x: Int <- Gen.auto()" should tokenize <- as LeftArrow
    let tokens = kyokara_syntax::lex("x <- y");
    let arrow = tokens.iter().find(|t| t.kind == SyntaxKind::LeftArrow);
    assert!(arrow.is_some(), "should have LeftArrow token: {tokens:?}");
}

#[test]
fn lexer_lt_space_minus_stays_separate() {
    // "a < -b" should NOT produce LeftArrow
    let tokens = kyokara_syntax::lex("a < -b");
    let has_arrow = tokens.iter().any(|t| t.kind == SyntaxKind::LeftArrow);
    assert!(!has_arrow, "< -b should not produce LeftArrow: {tokens:?}");
}

#[test]
fn lexer_left_arrow_no_spaces() {
    // "x<-y" should tokenize as Ident LeftArrow Ident
    let tokens = kyokara_syntax::lex("x<-y");
    let kinds: Vec<_> = tokens
        .iter()
        .filter(|t| t.kind != SyntaxKind::Whitespace)
        .map(|t| t.kind)
        .collect();
    assert_eq!(
        kinds,
        vec![SyntaxKind::Ident, SyntaxKind::LeftArrow, SyntaxKind::Ident],
        "x<-y should be [Ident, LeftArrow, Ident]: {tokens:?}"
    );
}

#[test]
fn lexer_left_arrow_in_property_context() {
    let tokens = kyokara_syntax::lex("x: Int <- Gen.auto()");
    let arrow = tokens.iter().find(|t| t.kind == SyntaxKind::LeftArrow);
    assert!(arrow.is_some(), "should have LeftArrow: {tokens:?}");
    assert_eq!(arrow.unwrap().text, "<-");
}

#[test]
fn lexer_multiple_left_arrows() {
    let tokens = kyokara_syntax::lex("a <- b, c <- d");
    let arrow_count = tokens
        .iter()
        .filter(|t| t.kind == SyntaxKind::LeftArrow)
        .count();
    assert_eq!(arrow_count, 2, "should have 2 LeftArrows: {tokens:?}");
}
