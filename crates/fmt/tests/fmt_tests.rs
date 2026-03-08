//! Formatter integration tests — snapshot tests, idempotency,
//! comment preservation, import sorting.
#![allow(clippy::unwrap_used)]

use kyokara_fmt::format_source;
use kyokara_syntax::parse;

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

/// Assert formatting output parses without syntax errors.
fn assert_fmt_parse_ok(input: &str, expected: &str) {
    assert_fmt(input, expected);
    let parsed = parse(expected);
    assert!(
        parsed.errors.is_empty(),
        "expected formatted output to parse cleanly, got: {:?}\nsource:\n{}",
        parsed.errors,
        expected
    );
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

#[test]
fn fmt_pub_fn_preserved() {
    assert_fmt_parse_ok(
        "pub fn add(x: Int, y: Int) -> Int { x + y }",
        "pub fn add(x: Int, y: Int) -> Int {\n  x + y\n}\n",
    );
}

#[test]
fn fmt_fn_with_contract_section() {
    assert_fmt(
        "fn inc(x: Int) -> Int contract requires (x > 0) ensures (result > x) { x + 1 }",
        "fn inc(x: Int) -> Int\ncontract\n  requires (x > 0)\n  ensures (result > x)\n{\n  x + 1\n}\n",
    );
}

#[test]
fn fmt_fn_with_with_and_contract_section() {
    assert_fmt(
        "fn run() -> Int with IO contract requires (true) invariant (true) { 1 }",
        "fn run() -> Int\nwith IO\ncontract\n  requires (true)\n  invariant (true)\n{\n  1\n}\n",
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
        "type Option =\n  Some(Int)\n  | None\n",
    );
}

#[test]
fn fmt_type_adt_multiple_fields() {
    assert_fmt(
        "type Result = Ok(Int) | Err(String)",
        "type Result =\n  Ok(Int)\n  | Err(String)\n",
    );
}

#[test]
fn fmt_pub_type_preserved() {
    assert_fmt_parse_ok(
        "pub type Result = Ok(Int) | Err(String)",
        "pub type Result =\n  Ok(Int)\n  | Err(String)\n",
    );
}

#[test]
fn fmt_pub_effect_preserved() {
    assert_fmt_parse_ok("pub effect Net", "pub effect Net\n");
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
fn fmt_zero_arg_call_expr() {
    assert_fmt_parse_ok(
        "fn main(xs: List<Int>) -> List<Int> { xs.to_list() }",
        "fn main(xs: List<Int>) -> List<Int> {\n  xs.to_list()\n}\n",
    );
}

#[test]
fn fmt_index_expr_simple_parse_ok() {
    assert_fmt_parse_ok(
        "fn main(xs: List<Int>) -> Int { xs[0] }",
        "fn main(xs: List<Int>) -> Int {\n  xs[0]\n}\n",
    );
}

#[test]
fn fmt_method_receiver_with_index_expr_preserved() {
    assert_fmt_parse_ok(
        "fn main() -> String { let out = collections.MutableList.new().push(\"\") let _o = out.set(0, out[0].concat(\"x\")) out[0] }",
        "fn main() -> String {\n  let out = collections.MutableList.new().push(\"\")\n  let _o = out.set(0, out[0].concat(\"x\"))\n  out[0]\n}\n",
    );
}

#[test]
fn fmt_call_arg_index_expr_preserved() {
    assert_fmt_parse_ok(
        "fn main() -> Int { let best = collections.MutableList.new().push(0) let cost = collections.MutableList.new().push(42) let _b = best.set(0, cost[0]) best[0] }",
        "fn main() -> Int {\n  let best = collections.MutableList.new().push(0)\n  let cost = collections.MutableList.new().push(42)\n  let _b = best.set(0, cost[0])\n  best[0]\n}\n",
    );
}

#[test]
fn fmt_lambda_body_index_expr_preserved() {
    assert_fmt_parse_ok(
        "fn keep(xs: List<Int>, ys: MutableList<Bool>, v: Int, n: Int) -> List<Int> { xs.filter(fn(x: Int) => ys[v * n + x]).to_list() }",
        "fn keep(xs: List<Int>, ys: MutableList<Bool>, v: Int, n: Int) -> List<Int> {\n  xs.filter(fn(x: Int) => ys[v * n + x]).to_list()\n}\n",
    );
}

#[test]
fn fmt_zero_arg_call_after_wrapped_chain_preserved() {
    assert_fmt_parse_ok(
        "fn main(input: String) -> List<String> { input.lines().map(fn(line: String) => line.trim()).filter(fn(line: String) => line.len() > 0).to_list() }",
        "fn main(input: String) -> List<String> {\n  input.lines().map(fn(line: String) => line.trim()).filter(fn(line: String) => line.len() > 0).to_list()\n}\n",
    );
}

#[test]
fn fmt_unary_operand_index_expr_preserved() {
    assert_fmt_parse_ok(
        "fn main(feeds: MutableList<Bool>) -> Bool { !feeds[0] }",
        "fn main(feeds: MutableList<Bool>) -> Bool {\n  !feeds[0]\n}\n",
    );
}

#[test]
fn fmt_record_field_index_expr_preserved() {
    assert_fmt_parse_ok(
        "fn main(part1: MutableList<Int>) -> Totals { Totals { part1: part1[0], part2: 0 } }",
        "fn main(part1: MutableList<Int>) -> Totals {\n  Totals { part1: part1[0], part2: 0 }\n}\n",
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
fn fmt_while_stmt_canonical_single_line_head() {
    assert_fmt(
        "fn main() -> Int { while (x<10) { x } }",
        "fn main() -> Int {\n  while (x < 10) {\n    x\n  }\n}\n",
    );
}

#[test]
fn fmt_for_stmt_canonical_single_line_head() {
    assert_fmt(
        "fn main() -> Int { for (x in 0 ..< 10) { x } }",
        "fn main() -> Int {\n  for (x in 0 ..< 10) {\n    x\n  }\n}\n",
    );
}

#[test]
fn fmt_break_continue_statements() {
    assert_fmt(
        "fn main() -> Int { while (true) { continue; break } }",
        "fn main() -> Int {\n  while (true) {\n    continue\n    break\n  }\n}\n",
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
fn fmt_range_until_expr_uses_canonical_spacing() {
    assert_fmt(
        "fn main() -> Int { (0..<5).count() }",
        "fn main() -> Int {\n  (0 ..< 5).count()\n}\n",
    );
}

#[test]
fn fmt_range_until_nested_pipeline_is_idempotent() {
    assert_fmt(
        "fn main() -> Int { (0..<5).map(fn(n: Int) => n + 1) |> count }",
        "fn main() -> Int {\n  (0 ..< 5).map(fn(n: Int) => n + 1) |> count\n}\n",
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

#[test]
fn fmt_anonymous_record_expr_has_no_leading_space() {
    assert_fmt(
        "fn main() -> { x: Int, y: Int } { { x: 1, y: 2 } }",
        "fn main() -> { x: Int, y: Int } {\n  { x: 1, y: 2 }\n}\n",
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
fn fmt_nested_type_args_canonicalize_and_parse() {
    assert_fmt_parse_ok(
        "fn f(xs: List< List< Int > >) -> List< List< Int > > { xs }",
        "fn f(xs: List<List<Int>>) -> List<List<Int>> {\n  xs\n}\n",
    );
}

#[test]
fn fmt_deep_nested_type_args_canonicalize_and_parse() {
    assert_fmt_parse_ok(
        "fn f(xs: Map< String, Option< List< List< Int > > > >) -> Int { xs.len() }",
        "fn f(xs: Map<String, Option<List<List<Int>>>>) -> Int {\n  xs.len()\n}\n",
    );
}

#[test]
fn fmt_nested_type_args_in_alias_canonicalize_and_parse() {
    assert_fmt_parse_ok(
        "type T = Map< String, List< List< Int > > >",
        "type T = Map<String, List<List<Int>>>\n",
    );
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

#[test]
fn fmt_lambda_with_if_body_uses_stable_multiline_layout() {
    assert_fmt(
        "fn main() -> Int { fn(acc: Int, n: Int) => if (n > 1) { acc + n } else { acc } }",
        "fn main() -> Int {\n  fn(acc: Int, n: Int) =>\n    if (n > 1) {\n      acc + n\n    } else {\n      acc\n    }\n}\n",
    );
}

#[test]
fn fmt_fold_lambda_in_call_uses_stable_multiline_layout() {
    assert_fmt(
        "fn main() -> Int { (0..<3).fold(0, fn(acc: Int, n: Int) => if (n > 1) { acc + n } else { acc }) }",
        "fn main() -> Int {\n  (0 ..< 3).fold(\n    0,\n    fn(acc: Int, n: Int) =>\n      if (n > 1) {\n        acc + n\n      } else {\n        acc\n      },\n  )\n}\n",
    );
}

#[test]
fn fmt_direct_call_multiline_lambda_argument_uses_block_arg_layout() {
    assert_fmt_parse_ok(
        "fn main() -> Int { apply(1, fn(acc: Int, n: Int) => if (n > 1) { acc + n } else { acc }) }",
        "fn main() -> Int {\n  apply(\n    1,\n    fn(acc: Int, n: Int) =>\n      if (n > 1) {\n        acc + n\n      } else {\n        acc\n      },\n  )\n}\n",
    );
}

#[test]
fn fmt_unfold_lambda_with_block_body_uses_block_arg_layout() {
    assert_fmt_parse_ok(
        "fn main() -> Int { 0.unfold(fn(state: Int) => { let next = state + 1\nSome({ value: state, state: next }) }).count() }",
        "fn main() -> Int {\n  0.unfold(\n    fn(state: Int) =>\n      {\n        let next = state + 1\n        Some({ value: state, state: next })\n      },\n  ).count()\n}\n",
    );
}

#[test]
fn fmt_named_arg_multiline_lambda_uses_block_arg_layout() {
    assert_fmt_parse_ok(
        "fn main() -> Int { apply(seed: 0, step: fn(state: Int) => if (state > 0) { state - 1 } else { state }).count() }",
        "fn main() -> Int {\n  apply(\n    seed: 0,\n    step: fn(state: Int) =>\n      if (state > 0) {\n        state - 1\n      } else {\n        state\n      },\n  ).count()\n}\n",
    );
}

#[test]
fn fmt_if_expression_argument_uses_block_arg_layout() {
    assert_fmt_parse_ok(
        "fn main() -> Int { choose(if (ready) { 1 } else { 2 }, 3) }",
        "fn main() -> Int {\n  choose(\n    if (ready) {\n      1\n    } else {\n      2\n    },\n    3,\n  )\n}\n",
    );
}

#[test]
fn fmt_simple_lambda_argument_stays_compact() {
    assert_fmt(
        "fn main() -> Int { (0..<3).fold(0, fn(acc: Int, n: Int) => acc + n) }",
        "fn main() -> Int {\n  (0 ..< 3).fold(0, fn(acc: Int, n: Int) => acc + n)\n}\n",
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
