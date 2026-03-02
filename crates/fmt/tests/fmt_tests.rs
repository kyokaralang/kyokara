//! Formatter integration tests — snapshot tests, idempotency,
//! comment preservation, import sorting.
#![allow(clippy::unwrap_used)]

use kyokara_fmt::format_source;

/// Assert formatting produces expected output, AND is idempotent.
fn assert_fmt(input: &str, expected: &str) {
    let actual = format_source(input);
    assert_eq!(
        actual, expected,
        "\n--- input ---\n{input}\n--- expected ---\n{expected}\n--- actual ---\n{actual}"
    );
    // Idempotency check
    let again = format_source(&actual);
    assert_eq!(again, actual, "formatter is not idempotent");
}

/// Assert that formatting doesn't change the input (already canonical).
fn assert_unchanged(input: &str) {
    assert_fmt(input, input);
}

// ── Simple constructs ───────────────────────────────────────────────

#[test]
fn fmt_empty_source() {
    assert_fmt("", "");
}

#[test]
fn fmt_module_decl() {
    assert_fmt("module  Main", "module Main\n");
}

#[test]
fn fmt_import_decl() {
    assert_fmt("import  Std.IO", "import Std.IO\n");
}

#[test]
fn fmt_import_with_alias() {
    assert_fmt("import  Std.IO  as  IO", "import Std.IO as IO\n");
}

#[test]
fn fmt_import_sorting() {
    assert_fmt(
        "import Std.IO\nimport Std.Collections\nimport Std.Base",
        "import Std.Base\nimport Std.Collections\nimport Std.IO\n",
    );
}

#[test]
fn fmt_module_and_imports() {
    assert_fmt(
        "module Main\n\nimport Std.IO\nimport Std.Base",
        "module Main\n\nimport Std.Base\nimport Std.IO\n",
    );
}

// ── Let bindings ────────────────────────────────────────────────────

#[test]
fn fmt_let_binding_simple() {
    assert_fmt("let  x  =  42", "let x = 42\n");
}

#[test]
fn fmt_let_binding_with_type() {
    assert_fmt("let  x :  Int  =  42", "let x: Int = 42\n");
}

// ── Function definitions ────────────────────────────────────────────

#[test]
fn fmt_fn_no_params() {
    assert_fmt(
        "fn  main()  ->  Int  {  42  }",
        "fn main() -> Int {\n  42\n}\n",
    );
}

#[test]
fn fmt_fn_with_params() {
    assert_fmt(
        "fn  add( x : Int , y : Int ) -> Int { x + y }",
        "fn add(x: Int, y: Int) -> Int {\n  x + y\n}\n",
    );
}

#[test]
fn fmt_fn_multiple() {
    assert_fmt(
        "fn foo() -> Int { 1 }\nfn bar() -> Int { 2 }",
        "fn foo() -> Int {\n  1\n}\n\nfn bar() -> Int {\n  2\n}\n",
    );
}

// ── Type definitions ────────────────────────────────────────────────

#[test]
fn fmt_type_alias() {
    assert_fmt("type  Id  =  Int", "type Id = Int\n");
}

#[test]
fn fmt_type_adt() {
    assert_fmt(
        "type Option = Some(Int) | None",
        "type Option =\n  | Some(Int)\n  | None\n",
    );
}

#[test]
fn fmt_type_adt_multiple_fields() {
    assert_fmt(
        "type Result = Ok(Int) | Err(String)",
        "type Result =\n  | Ok(Int)\n  | Err(String)\n",
    );
}

// ── Expressions ─────────────────────────────────────────────────────

#[test]
fn fmt_binary_expr() {
    assert_fmt(
        "fn main() -> Int { 1+2 }",
        "fn main() -> Int {\n  1 + 2\n}\n",
    );
}

#[test]
fn fmt_unary_expr() {
    assert_fmt(
        "fn main() -> Bool { !true }",
        "fn main() -> Bool {\n  !true\n}\n",
    );
}

#[test]
fn fmt_modulo_expr() {
    assert_fmt(
        "fn main() -> Int { 10%3 }",
        "fn main() -> Int {\n  10 % 3\n}\n",
    );
}

#[test]
fn fmt_logical_and_expr() {
    assert_fmt(
        "fn main() -> Bool { true&&false }",
        "fn main() -> Bool {\n  true && false\n}\n",
    );
}

#[test]
fn fmt_logical_or_expr() {
    assert_fmt(
        "fn main() -> Bool { true||false }",
        "fn main() -> Bool {\n  true || false\n}\n",
    );
}

#[test]
fn fmt_bitwise_and_expr() {
    assert_fmt(
        "fn main() -> Int { 3&1 }",
        "fn main() -> Int {\n  3 & 1\n}\n",
    );
}

#[test]
fn fmt_bitwise_or_expr() {
    assert_fmt(
        "fn main() -> Int { 3|1 }",
        "fn main() -> Int {\n  3 | 1\n}\n",
    );
}

#[test]
fn fmt_bitwise_xor_expr() {
    assert_fmt(
        "fn main() -> Int { 3^1 }",
        "fn main() -> Int {\n  3 ^ 1\n}\n",
    );
}

#[test]
fn fmt_shift_left_expr() {
    assert_fmt(
        "fn main() -> Int { 1<<3 }",
        "fn main() -> Int {\n  1 << 3\n}\n",
    );
}

#[test]
fn fmt_shift_right_expr() {
    assert_fmt(
        "fn main() -> Int { 8>>2 }",
        "fn main() -> Int {\n  8 >> 2\n}\n",
    );
}

#[test]
fn fmt_bitwise_not_expr() {
    assert_fmt("fn main() -> Int { ~42 }", "fn main() -> Int {\n  ~42\n}\n");
}

#[test]
fn fmt_call_expr() {
    assert_fmt(
        "fn main() -> Int { add(1,2) }",
        "fn main() -> Int {\n  add(1, 2)\n}\n",
    );
}

#[test]
fn fmt_if_expr() {
    assert_fmt(
        "fn main() -> Int { if (true) { 1 } else { 2 } }",
        "fn main() -> Int {\n  if (true) {\n    1\n  } else {\n    2\n  }\n}\n",
    );
}

#[test]
fn fmt_match_expr() {
    assert_fmt(
        "fn main() -> Int { match (x) { 1 => 2, _ => 3 } }",
        "fn main() -> Int {\n  match (x) {\n    1 => 2,\n    _ => 3,\n  }\n}\n",
    );
}

#[test]
fn fmt_block_multiple_stmts() {
    assert_fmt(
        "fn main() -> Int { let x = 1\nlet y = 2\nx + y }",
        "fn main() -> Int {\n  let x = 1\n  let y = 2\n  x + y\n}\n",
    );
}

#[test]
fn fmt_return_expr() {
    assert_fmt(
        "fn main() -> Int { return 42 }",
        "fn main() -> Int {\n  return 42\n}\n",
    );
}

#[test]
fn fmt_paren_expr() {
    assert_fmt(
        "fn main() -> Int { (1 + 2) }",
        "fn main() -> Int {\n  (1 + 2)\n}\n",
    );
}

#[test]
fn fmt_pipeline_expr() {
    assert_fmt(
        "fn main() -> Int { x |> f }",
        "fn main() -> Int {\n  x |> f\n}\n",
    );
}

#[test]
fn fmt_field_expr() {
    assert_fmt("fn main() -> Int { x.y }", "fn main() -> Int {\n  x.y\n}\n");
}

#[test]
fn fmt_propagate_expr() {
    assert_fmt("fn main() -> Int { x? }", "fn main() -> Int {\n  x?\n}\n");
}

#[test]
fn fmt_hole_expr() {
    assert_fmt("fn main() -> Int { _ }", "fn main() -> Int {\n  _\n}\n");
}

#[test]
fn fmt_record_expr() {
    assert_fmt(
        "fn main() -> Point { Point { x: 1, y: 2 } }",
        "fn main() -> Point {\n  Point { x: 1, y: 2 }\n}\n",
    );
}

// ── Patterns ────────────────────────────────────────────────────────

#[test]
fn fmt_wildcard_pattern() {
    assert_fmt(
        "fn main() -> Int { match (x) { _ => 0 } }",
        "fn main() -> Int {\n  match (x) {\n    _ => 0,\n  }\n}\n",
    );
}

#[test]
fn fmt_constructor_pattern() {
    assert_fmt(
        "fn main() -> Int { match (x) { Some(v) => v, None => 0 } }",
        "fn main() -> Int {\n  match (x) {\n    Some(v) => v,\n    None => 0,\n  }\n}\n",
    );
}

// ── Comments ────────────────────────────────────────────────────────

#[test]
fn fmt_leading_comment_on_fn() {
    assert_unchanged("// A helper function\nfn add(x: Int, y: Int) -> Int {\n  x + y\n}\n");
}

#[test]
fn fmt_trailing_comment() {
    assert_unchanged("fn main() -> Int {\n  let x = 42 // the answer\n  x\n}\n");
}

#[test]
fn fmt_comment_between_items() {
    assert_unchanged("fn foo() -> Int {\n  1\n}\n\n// Helper\nfn bar() -> Int {\n  2\n}\n");
}

#[test]
fn fmt_comments_preserved_count() {
    let input = "// comment one\nfn foo() -> Int { 1 }\n// comment two\nfn bar() -> Int { 2 }";
    let output = format_source(input);
    let comment_count = output.matches("//").count();
    assert_eq!(
        comment_count, 2,
        "expected 2 comments, got {comment_count} in:\n{output}"
    );
}

// ── Idempotency on complex programs ─────────────────────────────────

#[test]
fn fmt_idempotency_full_program() {
    let input = r#"module Main

import Std.IO
import Std.Collections

type Option = Some(Int) | None

fn add(x:Int,y:Int) -> Int { x + y }

fn main() -> Int {
  let result = add(1, 2)
  match (result) {
    0 => 42,
    _ => result + 1
  }
}
"#;
    let first = format_source(input);
    let second = format_source(&first);
    assert_eq!(first, second, "formatter is not idempotent");
}

// ── Generics ────────────────────────────────────────────────────────

#[test]
fn fmt_type_with_generics() {
    assert_fmt("type  Box< T >  =  T", "type Box<T> = T\n");
}

#[test]
fn fmt_fn_with_type_params() {
    assert_fmt(
        "fn  identity< T >( x : T ) -> T { x }",
        "fn identity<T>(x: T) -> T {\n  x\n}\n",
    );
}

// ── Effect definitions ──────────────────────────────────────────────

#[test]
fn fmt_effect_def() {
    assert_fmt("effect  IO", "effect IO\n");
}

#[test]
fn fmt_effect_with_body_is_non_lossy() {
    assert_fmt(
        "effect IO { fn read() -> String }",
        "effect IO { fn read() -> String }\n",
    );
}

#[test]
fn fmt_effect_with_type_params_is_non_lossy() {
    assert_fmt("effect IO<T>", "effect IO<T>\n");
}

// ── Let binding with compound expressions ───────────────────────────

#[test]
fn fmt_let_match() {
    assert_fmt(
        "fn main() -> Int { let y = match (x) { 0 => 1, _ => 2 }\ny }",
        "fn main() -> Int {\n  let y = match (x) {\n    0 => 1,\n    _ => 2,\n  }\n  y\n}\n",
    );
}

#[test]
fn fmt_let_if() {
    assert_fmt(
        "fn main() -> Int { let y = if (true) { 1 } else { 2 }\ny }",
        "fn main() -> Int {\n  let y = if (true) {\n    1\n  } else {\n    2\n  }\n  y\n}\n",
    );
}

// ── Lambda ──────────────────────────────────────────────────────────

#[test]
fn fmt_lambda_expr() {
    assert_fmt(
        "fn main() -> Int { fn(x: Int) => x + 1 }",
        "fn main() -> Int {\n  fn(x: Int) => x + 1\n}\n",
    );
}

// ── Empty match arm list ────────────────────────────────────────────

#[test]
fn fmt_empty_match_arms() {
    // Empty match arm list should not produce a trailing comma.
    assert_fmt(
        "fn main() -> Int { match (x) {} }",
        "fn main() -> Int {\n  match (x) {}\n}\n",
    );
}

// ── Error recovery ──────────────────────────────────────────────────

#[test]
fn fmt_error_node_verbatim() {
    // Source with parse errors should still produce output (verbatim for errors)
    let input = "fn main( { }";
    let output = format_source(input);
    // Should not panic and should produce some output
    assert!(!output.is_empty());
}

#[test]
fn fmt_top_level_error_node_preserved() {
    // Top-level parse-error nodes should be preserved verbatim, not dropped.
    let input = "match (x) {}";
    let output = format_source(input);
    assert!(
        output.contains("match"),
        "top-level error node text should be preserved, got: {:?}",
        output
    );
}

#[test]
fn fmt_trailing_comment_preserved() {
    let input = "fn main() -> Int { 1 }\n// EOF comment";
    let output = format_source(input);
    assert!(
        output.contains("// EOF comment"),
        "trailing comment should be preserved, got: {:?}",
        output
    );
}

#[test]
fn fmt_comment_only_file_preserved() {
    let input = "// just a comment";
    let output = format_source(input);
    assert!(
        output.contains("// just a comment"),
        "comment-only file should not be emptied, got: {:?}",
        output
    );
}

// ── Comment-only match arm list (#160) ─────────────────────────────

#[test]
fn fmt_match_comment_only_no_dangling_comma() {
    // Bug test: comment-only match arm list should NOT emit a dangling comma.
    let input = "fn main() -> Int { match (x) { // keep\n } }";
    let output = format_source(input);
    assert!(
        !output.contains(","),
        "comment-only match arm list should not produce a comma, got: {:?}",
        output
    );
    assert!(
        output.contains("// keep"),
        "comment should be preserved, got: {:?}",
        output
    );
}

#[test]
fn fmt_match_arms_with_comment_still_has_commas() {
    // Guard test: real arms with a comment should still get commas.
    let input = "fn main() -> Int { match (x) { // note\n 1 => 2, _ => 3 } }";
    let output = format_source(input);
    assert!(
        output.contains(","),
        "match with real arms should still have commas, got: {:?}",
        output
    );
    assert!(
        output.contains("// note"),
        "leading comment should be preserved, got: {:?}",
        output
    );
}

#[test]
fn fmt_match_trailing_comment_after_arms() {
    // Edge case: comment after last arm should be preserved, commas present.
    let input = "fn main() -> Int { match (x) { 1 => 2 // trailing\n } }";
    let output = format_source(input);
    assert!(
        output.contains(","),
        "match with arms should have commas, got: {:?}",
        output
    );
    assert!(
        output.contains("// trailing"),
        "trailing comment should be preserved, got: {:?}",
        output
    );
}
