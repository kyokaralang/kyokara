//! Integration tests — parse source text, verify CST structure + lossless roundtrip.

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
                NodeOrToken::Node(n) => collect(&n, out),
                NodeOrToken::Token(t) => out.push_str(t.text()),
            }
        }
    }
    collect(&green, &mut out);
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
fn roundtrip_type_def_variants() {
    let src = "type Option = | Some(Int) | None";
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
    let src = "let x = if true { 1 } else { 2 }";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
}

#[test]
fn roundtrip_match() {
    let src = "let x = match y { 1 => 2, _ => 3 }";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
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
    let src = "type Option<T> = | Some(T) | None";
    let green = parse_ok(src);
    assert_eq!(green_text(&green), src);
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
    let src = "fn foo(x: Int) ensures old(x) { x }";
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
    let src = "property commutative(x: Int, y: Int) { x }";
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
    | Some(T)
    | None

fn map<T, U>(opt: Option<T>, f: fn(T) -> U) -> Option<U> {
    match opt {
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
