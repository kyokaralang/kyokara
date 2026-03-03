//! End-to-end API tests: source → `check()` → verify structured output.
#![allow(clippy::unwrap_used)]

use kyokara_api::{check, check_project, refactor, refactor_project};
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn check_clean_program_no_diagnostics() {
    let output = check("fn add(x: Int, y: Int) -> Int { x + y }", "test.ky");
    assert!(
        output.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn check_type_mismatch_has_code() {
    // Return a String where (Int is expected.)
    let output = check(r#"fn bad() -> Int { "hello" }"#, "test.ky");
    assert!(
        !output.diagnostics.is_empty(),
        "expected at least one diagnostic"
    );
    let diag = &output.diagnostics[0];
    assert_eq!(diag.code, "E0001");
    assert_eq!(diag.severity, "error");
    assert!(
        diag.expected_type.is_some(),
        "expected_type should be present"
    );
    assert!(diag.actual_type.is_some(), "actual_type should be present");
    assert_eq!(diag.expected_type.as_deref(), Some("Int"));
    assert_eq!(diag.actual_type.as_deref(), Some("String"));
}

#[test]
fn check_else_if_expression_form_typechecks_like_nested_form() {
    let src = "fn main() -> Int { let x = if (true) { 1 } else if (false) { 2 } else { 3 }\n x }";
    let output = check(src, "test.ky");
    assert!(
        output.diagnostics.is_empty(),
        "expected no diagnostics for else-if expression form, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn check_else_if_missing_final_else_matches_nested_form_diagnostics() {
    let else_if = "fn main() -> Int { if (true) { 1 } else if (false) { 2 } }";
    let nested = "fn main() -> Int { if (true) { 1 } else { if (false) { 2 } } }";

    let else_if_output = check(else_if, "test.ky");
    let nested_output = check(nested, "test.ky");

    let else_if_codes: BTreeSet<String> = else_if_output
        .diagnostics
        .iter()
        .map(|d| d.code.clone())
        .collect();
    let nested_codes: BTreeSet<String> = nested_output
        .diagnostics
        .iter()
        .map(|d| d.code.clone())
        .collect();

    assert_eq!(
        else_if_codes, nested_codes,
        "expected else-if and nested forms to produce same diagnostic code set, got else-if={:?}, nested={:?}",
        else_if_output.diagnostics, nested_output.diagnostics
    );
    assert!(
        else_if_codes.contains("E0001"),
        "expected type mismatch E0001 for missing final else, got: {:?}",
        else_if_output.diagnostics
    );
}

#[test]
fn check_hole_produces_spec() {
    let output = check("fn foo() -> Int { _ }", "test.ky");
    assert_eq!(output.holes.len(), 1);
    let hole = &output.holes[0];
    assert_eq!(hole.expected_type.as_deref(), Some("Int"));
    assert_eq!(hole.span.file, "test.ky");
}

#[test]
fn check_hole_with_available_locals() {
    let output = check("fn foo(x: Int) -> Int { _ }", "test.ky");
    assert_eq!(output.holes.len(), 1);
    let hole = &output.holes[0];
    assert_eq!(hole.expected_type.as_deref(), Some("Int"));
    // The param `x` should be in available inputs.
    assert!(
        hole.inputs.iter().any(|v| v.name == "x" && v.ty == "Int"),
        "expected input `x: Int`, got: {:?}",
        hole.inputs
    );
}

#[test]
fn check_hole_inputs_no_duplicate_names() {
    // When a name is shadowed, hole inputs should not contain duplicates.
    let src = "fn main() -> Int {\n  let x = 1\n  let y = {\n    let x = true\n    0\n  }\n  _\n}";
    let output = check(src, "test.ky");
    assert_eq!(output.holes.len(), 1);
    let hole = &output.holes[0];
    let x_inputs: Vec<_> = hole.inputs.iter().filter(|v| v.name == "x").collect();
    assert!(
        x_inputs.len() <= 1,
        "expected at most one `x` in hole inputs, got {}: {:?}",
        x_inputs.len(),
        x_inputs
    );
}

#[test]
fn check_hole_inputs_exclude_out_of_scope_inner_binding_issue_132() {
    let src = r#"
fn main(x: Int) -> Int {
  if (true) {
    let z = 1
    0
  } else {
    0
  }
  _
}
"#;
    let output = check(src, "test.ky");
    assert_eq!(output.holes.len(), 1);
    let hole = &output.holes[0];
    assert!(
        hole.inputs.iter().any(|v| v.name == "x" && v.ty == "Int"),
        "expected in-scope param `x: Int`, got: {:?}",
        hole.inputs
    );
    assert!(
        hole.inputs.iter().all(|v| v.name != "z"),
        "out-of-scope branch local `z` must not appear in hole inputs: {:?}",
        hole.inputs
    );
}

#[test]
fn check_hole_inputs_include_in_scope_branch_binding_issue_132_guard() {
    let src = r#"
fn main() -> Int {
  if (true) {
    let z = 1
    _
  } else {
    0
  }
}
"#;
    let output = check(src, "test.ky");
    assert_eq!(output.holes.len(), 1);
    let hole = &output.holes[0];
    assert!(
        hole.inputs.iter().any(|v| v.name == "z" && v.ty == "Int"),
        "expected in-scope branch local `z: Int`, got: {:?}",
        hole.inputs
    );
}

#[test]
fn check_effect_violation_code() {
    let src = r#"
        effect Console
        fn effectful() -> Unit with Console { () }
        fn pure_caller() -> Unit { effectful() }
    "#;
    let output = check(src, "test.ky");
    let effect_diags: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.code == "E0011")
        .collect();
    assert!(
        !effect_diags.is_empty(),
        "expected effect violation diagnostic (E0011), got: {:?}",
        output.diagnostics
    );
}

#[test]
fn check_parse_error_code() {
    let output = check("fn {}", "test.ky");
    let parse_diags: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.code == "E0100")
        .collect();
    assert!(
        !parse_diags.is_empty(),
        "expected parse error (E0100), got codes: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| &d.code)
            .collect::<Vec<_>>()
    );
}

#[test]
fn check_parse_damaged_function_reports_parse_only_diagnostics() {
    let src = "fn main() -> Int { match value { _ => 0 } }";
    let output = check(src, "test.ky");
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code == "E0100"
                && d.message.contains("match scrutinee must be parenthesized")),
        "expected targeted match parse diagnostic, got: {:?}",
        output.diagnostics
    );
    assert!(
        output.diagnostics.iter().all(|d| d.code == "E0100"),
        "parse-damaged function should report parse-only diagnostics, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn check_if_record_like_head_near_miss_reports_targeted_parse_without_cascade_noise() {
    let src = "fn main() -> Int { let x = if Point { x: 1 } == Point { x: 1 } { 1 } else { 0 } x }";
    let output = check(src, "test.ky");
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code == "E0100" && d.message.contains("if condition must be parenthesized")),
        "expected targeted if parse diagnostic, got: {:?}",
        output.diagnostics
    );
    assert!(
        output.diagnostics.iter().all(|d| d.code == "E0100"),
        "expected parse-only diagnostics for damaged function, got: {:?}",
        output.diagnostics
    );
    assert!(
        output.diagnostics.iter().all(|d| {
            !d.message.contains("expected item")
                && !d.message.contains("expected RBrace")
                && !d.message.contains("expected FatArrow")
        }),
        "did not expect cascade-style parser noise, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn check_match_record_like_head_near_miss_reports_targeted_parse_without_cascade_noise() {
    let src = "fn main() -> Int { let x = match Point { x: 1 } { _ => 0 } x }";
    let output = check(src, "test.ky");
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code == "E0100"
                && d.message.contains("match scrutinee must be parenthesized")),
        "expected targeted match parse diagnostic, got: {:?}",
        output.diagnostics
    );
    assert!(
        output.diagnostics.iter().all(|d| d.code == "E0100"),
        "expected parse-only diagnostics for damaged function, got: {:?}",
        output.diagnostics
    );
    assert!(
        output.diagnostics.iter().all(|d| {
            !d.message.contains("expected item")
                && !d.message.contains("expected RBrace")
                && !d.message.contains("expected FatArrow")
        }),
        "did not expect cascade-style parser noise, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn check_requires_record_like_head_near_miss_reports_targeted_parse_without_cascade_noise() {
    let src = "fn main() -> Int contract requires Point { x: 1 } { 1 }";
    let output = check(src, "test.ky");
    assert!(
        output.diagnostics.iter().any(|d| d.code == "E0100"
            && d.message
                .contains("requires clause expression must be parenthesized")),
        "expected targeted requires parse diagnostic, got: {:?}",
        output.diagnostics
    );
    assert!(
        output.diagnostics.iter().all(|d| d.code == "E0100"),
        "expected parse-only diagnostics for damaged function, got: {:?}",
        output.diagnostics
    );
    assert!(
        output.diagnostics.iter().all(|d| {
            !d.message.contains("expected item")
                && !d.message.contains("expected RBrace")
                && !d.message.contains("expected FatArrow")
        }),
        "did not expect cascade-style parser noise, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn check_where_record_like_head_near_miss_reports_targeted_parse_without_cascade_noise() {
    let src = "property p(x: Int <- Gen.auto()) where Point { x: 1 } { true }";
    let output = check(src, "test.ky");
    assert!(
        output.diagnostics.iter().any(|d| d.code == "E0100"
            && d.message
                .contains("where clause expression must be parenthesized")),
        "expected targeted where parse diagnostic, got: {:?}",
        output.diagnostics
    );
    assert!(
        output.diagnostics.iter().all(|d| d.code == "E0100"),
        "expected parse-only diagnostics for damaged property, got: {:?}",
        output.diagnostics
    );
    assert!(
        output.diagnostics.iter().all(|d| {
            !d.message.contains("expected item")
                && !d.message.contains("expected RBrace")
                && !d.message.contains("expected FatArrow")
        }),
        "did not expect cascade-style parser noise, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn check_mixed_file_keeps_unaffected_function_semantic_diagnostics() {
    let src = r#"
fn broken() -> Int {
  match value {
    _ => 0
  }
}

fn typed_bad() -> Int {
  "oops"
}
"#;
    let output = check(src, "test.ky");
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code == "E0100"
                && d.message.contains("match scrutinee must be parenthesized")),
        "expected parse diagnostic for broken fn, got: {:?}",
        output.diagnostics
    );
    assert!(
        output.diagnostics.iter().any(|d| d.code == "E0001"),
        "expected unaffected function type mismatch diagnostic, got: {:?}",
        output.diagnostics
    );
    assert!(
        output
            .diagnostics
            .iter()
            .all(|d| d.code != "E0009" && d.code != "E0101"),
        "did not expect cascade diagnostics from parse-damaged function, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn check_rejects_leading_pipe_type_variant_syntax() {
    let src = "type Option = | Some(Int) | None\nfn main() -> Int { 1 }";
    let output = check(src, "test.ky");
    let parse_diags: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.code == "E0100")
        .collect();
    assert!(
        parse_diags.iter().any(|d| d
            .message
            .contains("leading `|` is not allowed in type variants")),
        "expected targeted leading-pipe parse diagnostic, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn check_rejects_leading_pipe_match_arm_syntax() {
    let src = "fn main() -> Int { match (1) { | _ => 0 } }";
    let output = check(src, "test.ky");
    let parse_diags: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.code == "E0100")
        .collect();
    assert!(
        parse_diags
            .iter()
            .any(|d| d.message.contains("match arms do not use a leading `|`")),
        "expected targeted leading-pipe match-arm diagnostic, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn check_rejects_pub_property_without_hanging() {
    let src = "pub property p(x: Int <- Gen.int()) { true }\nfn main() -> Int { 1 }";
    let output = check(src, "test.ky");
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code == "E0100" && d.message.contains("expected item")),
        "expected parse diagnostic for pub property, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn check_rejects_pub_let_without_hanging() {
    let src = "pub let x = 1\nfn main() -> Int { 1 }";
    let output = check(src, "test.ky");
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code == "E0100" && d.message.contains("expected item")),
        "expected parse diagnostic for pub let, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn check_rejects_top_level_bodyless_fn_declaration() {
    let src = "fn foo() -> Int\nfn main() -> Int { foo() }";
    let output = check(src, "test.ky");
    assert!(
        !output.diagnostics.is_empty(),
        "expected diagnostics for bodyless top-level fn, got none"
    );
    let parse_diags: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.code == "E0100")
        .collect();
    assert!(
        !parse_diags.is_empty(),
        "expected parse error diagnostics (E0100), got: {:?}",
        output.diagnostics
    );
}

#[test]
fn check_allows_label_only_effect_declaration() {
    let src = "effect IO\nfn main() -> Int { 1 }";
    let output = check(src, "test.ky");
    assert!(
        output.diagnostics.is_empty(),
        "expected no diagnostics for label-only effect declaration, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn check_allows_empty_unit_body() {
    let output = check("fn noop() -> Unit {}", "test.ky");
    assert!(
        output.diagnostics.is_empty(),
        "expected empty Unit body to type-check, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn check_rejects_empty_non_unit_body() {
    let output = check("fn bad() -> Int {}", "test.ky");
    assert!(
        output.diagnostics.iter().any(|d| d.code == "E0001"),
        "expected type mismatch for empty non-Unit body, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn check_misordered_contract_clause_reports_targeted_parse_error() {
    let src = "fn inc(x: Int) -> Int contract ensures (result > x) requires (x >= 0) { x + 1 }\nfn main() -> Int { inc(1) }";
    let output = check(src, "test.ky");
    let parse_diags: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.code == "E0100")
        .collect();
    assert_eq!(
        parse_diags.len(),
        1,
        "expected one parse diagnostic, got: {:?}",
        output.diagnostics
    );
    assert!(
        parse_diags[0]
            .message
            .contains("requires cannot appear after ensures"),
        "expected targeted clause-order message, got: {:?}",
        parse_diags[0]
    );
}

#[test]
fn check_json_roundtrip() {
    let output = check(r#"fn bad() -> Int { "hello" }"#, "test.ky");
    let json = serde_json::to_string_pretty(&output).expect("serialization failed");
    assert!(json.contains("E0001"));
    assert!(json.contains("\"diagnostics\""));
    assert!(json.contains("\"holes\""));

    // Verify it's valid JSON by parsing it back.
    let _: serde_json::Value = serde_json::from_str(&json).expect("invalid JSON");
}

#[test]
fn check_arg_count_mismatch_code() {
    let src = "fn add(x: Int, y: Int) -> Int { x + y }\nfn caller() -> Int { add(1) }";
    let output = check(src, "test.ky");
    let diags: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.code == "E0007")
        .collect();
    assert!(
        !diags.is_empty(),
        "expected arg count mismatch (E0007), got: {:?}",
        output.diagnostics
    );
}

#[test]
fn check_clean_program_no_holes() {
    let output = check("fn id(x: Int) -> Int { x }", "test.ky");
    assert!(output.diagnostics.is_empty());
    assert!(output.holes.is_empty());
}

#[test]
fn check_hole_effect_constraints() {
    let src = r#"
        effect IO
        fn with_io() -> Int with IO { _ }
    "#;
    let output = check(src, "test.ky");
    assert_eq!(output.holes.len(), 1);
    let hole = &output.holes[0];
    assert!(
        hole.effects.contains(&"IO".to_string()),
        "expected IO effect, got: {:?}",
        hole.effects
    );
}

// ── Symbol graph tests ──────────────────────────────────────────────

#[test]
fn symbol_graph_contains_functions() {
    let src = r#"
        fn foo(x: Int) -> Int { x }
        fn bar(y: Int) -> Int { y }
    "#;
    let output = check(src, "test.ky");
    let names: Vec<&str> = output
        .symbol_graph
        .functions
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    assert_eq!(
        output.symbol_graph.functions.len(),
        2,
        "expected 2 functions, got: {names:?}"
    );
    assert!(names.contains(&"foo"), "missing 'foo' in {names:?}");
    assert!(names.contains(&"bar"), "missing 'bar' in {names:?}");
}

#[test]
fn symbol_graph_contains_types() {
    let src = "type Color = Red | Green | Blue
        fn id(x: Int) -> Int { x }";
    let output = check(src, "test.ky");
    // 8 types: Color (user-defined) + Option + Result + List + Seq + Map + Set + ParseError
    assert_eq!(output.symbol_graph.types.len(), 8);
    let color = output
        .symbol_graph
        .types
        .iter()
        .find(|t| t.name == "Color")
        .expect("Color type should be in symbol graph");
    assert_eq!(color.kind, "adt");
    let variant_names: Vec<&str> = color.variants.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(variant_names, vec!["Red", "Green", "Blue"]);
}

#[test]
fn symbol_graph_alias_to_record_emits_record_kind_with_fields() {
    let src = "type Point = { x: Int, y: Int }\nfn main() -> Int { 1 }\n";
    let output = check(src, "test.ky");
    let point = output
        .symbol_graph
        .types
        .iter()
        .find(|t| t.name == "Point")
        .expect("should contain Point type");

    assert_eq!(
        point.kind, "record",
        "alias-to-record should be represented as a record node"
    );
    assert_eq!(
        point.fields.len(),
        2,
        "record fields should be preserved for alias-to-record types"
    );
    assert_eq!(point.fields[0].name, "x");
    assert_eq!(point.fields[0].ty, "Int");
    assert_eq!(point.fields[1].name, "y");
    assert_eq!(point.fields[1].ty, "Int");
}

#[test]
fn symbol_graph_contains_capabilities() {
    let src = r#"
        effect IO
        fn noop() -> Unit { () }
    "#;
    let output = check(src, "test.ky");
    assert_eq!(output.symbol_graph.capabilities.len(), 1);
    let cap = &output.symbol_graph.capabilities[0];
    assert_eq!(cap.name, "IO");
    assert!(
        cap.functions.is_empty(),
        "label-only effect should not carry member fn refs, got: {:?}",
        cap.functions
    );
}

#[test]
fn symbol_graph_call_edges() {
    let src = r#"
        fn callee() -> Int { 42 }
        fn caller() -> Int { callee() }
    "#;
    let output = check(src, "test.ky");
    let caller_node = output
        .symbol_graph
        .functions
        .iter()
        .find(|f| f.name == "caller")
        .expect("should have 'caller' function node");
    assert!(
        caller_node.calls.contains(&"fn::callee".to_string()),
        "expected caller to call fn::callee, got: {:?}",
        caller_node.calls
    );
}

#[test]
fn symbol_graph_repeated_direct_calls_are_deduped() {
    let src = r#"
        fn callee() -> Int { 42 }
        fn caller() -> Int {
            callee()
            callee()
        }
    "#;
    let output = check(src, "test.ky");
    let caller_node = output
        .symbol_graph
        .functions
        .iter()
        .find(|f| f.name == "caller")
        .expect("should have 'caller' function node");
    let callee_edges = caller_node
        .calls
        .iter()
        .filter(|c| c.as_str() == "fn::callee")
        .count();
    assert_eq!(
        callee_edges, 1,
        "repeated direct calls should dedupe to one edge, got: {:?}",
        caller_node.calls
    );
}

#[test]
fn symbol_graph_call_rewrite_avoids_per_edge_qualified_vec_allocation() {
    let src = include_str!("../src/lib.rs");
    assert!(
        !src.contains("let qualified: Vec<&String>"),
        "call rewrite loop should not allocate per-edge qualified Vec"
    );
}

#[test]
fn symbol_graph_effect_annotations() {
    let src = r#"
        effect IO
        fn effectful() -> String with IO { "" }
    "#;
    let output = check(src, "test.ky");
    let fn_node = output
        .symbol_graph
        .functions
        .iter()
        .find(|f| f.name == "effectful")
        .expect("should have 'effectful' function node");
    assert!(
        fn_node.effects.contains(&"IO".to_string()),
        "expected IO in effects, got: {:?}",
        fn_node.effects
    );
}

// ── Patch suggestion tests ──────────────────────────────────────────

#[test]
fn patch_missing_match_arm() {
    let src = "type Color = Red | Green | Blue
        fn describe(c: Color) -> Int {
            match (c) {
                Red => 1
            }
        }";
    let output = check(src, "test.ky");
    let diag = output
        .diagnostics
        .iter()
        .find(|d| d.code == "E0009")
        .expect("expected E0009 (MissingMatchArms) diagnostic");
    assert!(!diag.fixes.is_empty(), "expected non-empty fixes for E0009");
    let fix = &diag.fixes[0];
    assert!(
        fix.replacement.contains("Green"),
        "fix should mention Green: {}",
        fix.replacement
    );
    assert!(
        fix.replacement.contains("Blue"),
        "fix should mention Blue: {}",
        fix.replacement
    );
}

#[test]
fn patch_effect_violation() {
    let src = r#"
        effect Console
        fn effectful() -> Unit with Console { () }
        fn pure_caller() -> Unit { effectful() }
    "#;
    let output = check(src, "test.ky");
    let diag = output
        .diagnostics
        .iter()
        .find(|d| d.code == "E0011")
        .expect("expected E0011 (EffectViolation) diagnostic");
    assert!(!diag.fixes.is_empty(), "expected non-empty fixes for E0011");
    let fix = &diag.fixes[0];
    assert!(
        fix.replacement.contains("Console"),
        "fix should mention Console: {}",
        fix.replacement
    );
}

#[test]
fn patch_apply_missing_arm_fixes_error() {
    // Source with all arms present should have no E0009 errors.
    let src = "type Color = Red | Green | Blue
        fn describe(c: Color) -> Int {
            match (c) {
                Red => 1
                Green => 2
                Blue => 3
            }
        }";
    let output = check(src, "test.ky");
    let e0009: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.code == "E0009")
        .collect();
    assert!(
        e0009.is_empty(),
        "expected no E0009 after adding all arms, got: {e0009:?}"
    );
}

#[test]
fn patch_apply_effect_fix_fixes_error() {
    // Source with correct `with` clause should have no E0011 errors.
    let src = r#"
        effect Console
        fn effectful() -> Unit with Console { () }
        fn caller() -> Unit with Console { effectful() }
    "#;
    let output = check(src, "test.ky");
    let e0011: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.code == "E0011")
        .collect();
    assert!(
        e0011.is_empty(),
        "expected no E0011 after adding capability, got: {e0011:?}"
    );
}

// ── Unresolved name diagnostic tests ────────────────────────────────

#[test]
fn check_unresolved_name_produces_diagnostic() {
    let output = check("fn main() -> Int { foo }", "test.ky");
    assert!(
        !output.diagnostics.is_empty(),
        "expected at least one diagnostic for unresolved name `foo`, got none"
    );
    let has_unresolved = output
        .diagnostics
        .iter()
        .any(|d| d.message.contains("unresolved name"));
    assert!(
        has_unresolved,
        "expected 'unresolved name' diagnostic, got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn check_unresolved_name_in_expression_produces_diagnostic() {
    let output = check("fn main() -> Int { foo + 1 }", "test.ky");
    let has_unresolved = output
        .diagnostics
        .iter()
        .any(|d| d.message.contains("unresolved name"));
    assert!(
        has_unresolved,
        "expected 'unresolved name' diagnostic, got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn check_duplicate_definition_maps_to_e0102() {
    let src = "fn foo() -> Int { 1 }\nfn foo() -> Int { 2 }\nfn main() -> Int { foo() }";
    let output = check(src, "test.ky");
    assert!(
        output.diagnostics.iter().any(|d| d.code == "E0102"),
        "expected duplicate-definition diagnostic code E0102, got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| (&d.code, &d.message))
            .collect::<Vec<_>>()
    );
}

// ── Stable symbol ID tests ──────────────────────────────────────────

#[test]
fn stable_id_fn_nodes_have_ids() {
    let src = "fn foo(x: Int) -> Int { x }\nfn bar(y: Int) -> Int { y }";
    let output = check(src, "test.ky");
    for f in &output.symbol_graph.functions {
        assert!(
            f.id.starts_with("fn::"),
            "function id should start with 'fn::', got: {}",
            f.id
        );
    }
}

#[test]
fn stable_id_type_nodes_have_ids() {
    let src = "type Color = Red | Green\nfn id(x: Int) -> Int { x }";
    let output = check(src, "test.ky");
    for t in &output.symbol_graph.types {
        assert!(
            t.id.starts_with("type::"),
            "type id should start with 'type::', got: {}",
            t.id
        );
    }
}

#[test]
fn stable_id_variant_nodes_have_ids() {
    let src = "type Color = Red | Green | Blue\nfn id(x: Int) -> Int { x }";
    let output = check(src, "test.ky");
    let color = output
        .symbol_graph
        .types
        .iter()
        .find(|t| t.name == "Color")
        .expect("Color type should exist");
    for v in &color.variants {
        assert!(
            v.id.starts_with("type::Color::"),
            "variant id should start with 'type::Color::', got: {}",
            v.id
        );
    }
}

#[test]
fn stable_id_cap_nodes_have_ids() {
    let src = r#"
        effect IO
        fn noop() -> Unit { () }
    "#;
    let output = check(src, "test.ky");
    for c in &output.symbol_graph.capabilities {
        assert!(
            c.id.starts_with("cap::"),
            "capability id should start with 'cap::', got: {}",
            c.id
        );
    }
}

#[test]
fn stable_id_cap_function_refs_use_ids() {
    let src = r#"
        effect IO
        fn noop() -> Unit { () }
    "#;
    let output = check(src, "test.ky");
    let cap = &output.symbol_graph.capabilities[0];
    assert!(
        cap.functions.is_empty(),
        "label-only effect should not emit function refs, got: {:?}",
        cap.functions
    );
}

#[test]
fn stable_id_call_edges_use_fn_ids() {
    let src = r#"
        fn callee() -> Int { 42 }
        fn caller() -> Int { callee() }
    "#;
    let output = check(src, "test.ky");
    let caller = output
        .symbol_graph
        .functions
        .iter()
        .find(|f| f.name == "caller")
        .expect("caller should exist");
    for call in &caller.calls {
        assert!(
            call.starts_with("fn::"),
            "call edge should start with 'fn::', got: {call}"
        );
    }
}

#[test]
fn stable_id_uniqueness() {
    let src = r#"
        type Color = Red | Green | Blue
        effect IO
        fn foo(x: Int) -> Int { x }
    "#;
    let output = check(src, "test.ky");
    let mut ids: Vec<String> = Vec::new();

    for f in &output.symbol_graph.functions {
        ids.push(f.id.clone());
    }
    for t in &output.symbol_graph.types {
        ids.push(t.id.clone());
        for v in &t.variants {
            ids.push(v.id.clone());
        }
    }
    for c in &output.symbol_graph.capabilities {
        ids.push(c.id.clone());
    }

    let count = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(
        ids.len(),
        count,
        "all symbol IDs should be unique, found duplicates"
    );
}

#[test]
fn stable_id_fn_format() {
    let src = "fn add(x: Int, y: Int) -> Int { x + y }";
    let output = check(src, "test.ky");
    let add = output
        .symbol_graph
        .functions
        .iter()
        .find(|f| f.name == "add")
        .expect("add function should exist");
    assert_eq!(add.id, "fn::add");
}

#[test]
fn stable_id_variant_format() {
    let src = "type Color = Red | Green | Blue\nfn id(x: Int) -> Int { x }";
    let output = check(src, "test.ky");
    let color = output
        .symbol_graph
        .types
        .iter()
        .find(|t| t.name == "Color")
        .expect("Color type should exist");
    let red = color
        .variants
        .iter()
        .find(|v| v.name == "Red")
        .expect("Red variant should exist");
    assert_eq!(red.id, "type::Color::Red");
}

// ── Project-mode symbol graph tests ──────────────────────────────────

/// Helper: create a temp dir with .ky files and return the path to main.ky.
fn write_project(files: &[(&str, &str)]) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    for (name, content) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, content).unwrap();
    }
    let main_path = dir.path().join("main.ky");
    (dir, main_path)
}

fn check_project_from_files(files: &[(&str, &str)]) -> kyokara_api::CheckOutput {
    let (_dir, main_path) = write_project(files);
    check_project(&main_path)
}

fn find_function_by_id<'a>(
    output: &'a kyokara_api::CheckOutput,
    id: &str,
) -> &'a kyokara_api::FnNodeDto {
    output
        .symbol_graph
        .functions
        .iter()
        .find(|f| f.id == id)
        .unwrap_or_else(|| {
            panic!(
                "expected function id `{id}` in symbol graph, got: {:?}",
                output
                    .symbol_graph
                    .functions
                    .iter()
                    .map(|f| f.id.as_str())
                    .collect::<Vec<_>>()
            )
        })
}

fn sorted_calls(fn_node: &kyokara_api::FnNodeDto) -> Vec<String> {
    let mut calls = fn_node.calls.clone();
    calls.sort();
    calls
}

fn call_edges_by_caller_id(output: &kyokara_api::CheckOutput) -> BTreeMap<String, Vec<String>> {
    output
        .symbol_graph
        .functions
        .iter()
        .map(|f| (f.id.clone(), sorted_calls(f)))
        .collect()
}

fn diagnostic_signatures(output: &kyokara_api::CheckOutput) -> Vec<(String, String)> {
    let mut diags: Vec<(String, String)> = output
        .diagnostics
        .iter()
        .map(|d| (d.code.clone(), d.message.clone()))
        .collect();
    diags.sort();
    diags
}

fn assert_call_edges_target_existing_functions(output: &kyokara_api::CheckOutput) {
    let function_ids: BTreeSet<&str> = output
        .symbol_graph
        .functions
        .iter()
        .map(|f| f.id.as_str())
        .collect();

    for caller in &output.symbol_graph.functions {
        for callee in &caller.calls {
            assert!(
                function_ids.contains(callee.as_str()),
                "dangling call edge: caller `{}` ({}) -> `{}`; known IDs: {:?}",
                caller.name,
                caller.id,
                callee,
                function_ids
            );
        }
    }
}

fn assert_no_duplicate_call_edges_per_caller(output: &kyokara_api::CheckOutput) {
    for caller in &output.symbol_graph.functions {
        let mut seen = BTreeSet::new();
        let mut duplicates = Vec::new();
        for callee in &caller.calls {
            if !seen.insert(callee.clone()) {
                duplicates.push(callee.clone());
            }
        }
        assert!(
            duplicates.is_empty(),
            "duplicate call edges for caller `{}` ({}) -> {:?}; full calls: {:?}",
            caller.name,
            caller.id,
            duplicates,
            caller.calls
        );
    }
}

fn assert_metamorphic_equivalent(original_src: &str, transformed_src: &str) {
    let original = check(original_src, "test.ky");
    let transformed = check(transformed_src, "test.ky");
    let original_edges = call_edges_by_caller_id(&original);
    let transformed_edges = call_edges_by_caller_id(&transformed);
    assert_eq!(
        original_edges, transformed_edges,
        "metamorphic call-edge mismatch\n--- original source ---\n{}\n--- transformed source ---\n{}\n--- original edges ---\n{:?}\n--- transformed edges ---\n{:?}",
        original_src, transformed_src, original_edges, transformed_edges
    );

    let original_diags = diagnostic_signatures(&original);
    let transformed_diags = diagnostic_signatures(&transformed);
    assert_eq!(
        original_diags, transformed_diags,
        "metamorphic diagnostics mismatch\n--- original source ---\n{}\n--- transformed source ---\n{}\n--- original diagnostics ---\n{:?}\n--- transformed diagnostics ---\n{:?}",
        original_src, transformed_src, original_diags, transformed_diags
    );
}

fn render_project_sources(files: &[(&str, &str)]) -> String {
    let mut entries: Vec<(&str, &str)> = files.to_vec();
    entries.sort_by_key(|(path, _)| *path);
    entries
        .into_iter()
        .map(|(path, src)| format!("--- {path} ---\n{src}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_project_metamorphic_equivalent(
    original_files: &[(&str, &str)],
    transformed_files: &[(&str, &str)],
) {
    let (_dir_a, main_a) = write_project(original_files);
    let (_dir_b, main_b) = write_project(transformed_files);
    let original = check_project(&main_a);
    let transformed = check_project(&main_b);

    let original_edges = call_edges_by_caller_id(&original);
    let transformed_edges = call_edges_by_caller_id(&transformed);
    assert_eq!(
        original_edges,
        transformed_edges,
        "project metamorphic call-edge mismatch\n--- original project ---\n{}\n--- transformed project ---\n{}\n--- original edges ---\n{:?}\n--- transformed edges ---\n{:?}",
        render_project_sources(original_files),
        render_project_sources(transformed_files),
        original_edges,
        transformed_edges
    );

    let original_diags = diagnostic_signatures(&original);
    let transformed_diags = diagnostic_signatures(&transformed);
    assert_eq!(
        original_diags,
        transformed_diags,
        "project metamorphic diagnostics mismatch\n--- original project ---\n{}\n--- transformed project ---\n{}\n--- original diagnostics ---\n{:?}\n--- transformed diagnostics ---\n{:?}",
        render_project_sources(original_files),
        render_project_sources(transformed_files),
        original_diags,
        transformed_diags
    );
}

fn diagnostic_code_counts(output: &kyokara_api::CheckOutput) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for diag in &output.diagnostics {
        *counts.entry(diag.code.clone()).or_insert(0) += 1;
    }
    counts
}

fn code_count_delta(
    base: &BTreeMap<String, usize>,
    transformed: &BTreeMap<String, usize>,
) -> BTreeMap<String, isize> {
    let mut keys = BTreeSet::new();
    keys.extend(base.keys().cloned());
    keys.extend(transformed.keys().cloned());

    let mut delta = BTreeMap::new();
    for code in keys {
        let before = *base.get(&code).unwrap_or(&0) as isize;
        let after = *transformed.get(&code).unwrap_or(&0) as isize;
        let diff = after - before;
        if diff != 0 {
            delta.insert(code, diff);
        }
    }
    delta
}

fn assert_diagnostic_code_delta(
    original_src: &str,
    transformed_src: &str,
    expected_delta: &[(&str, isize)],
) {
    let original = check(original_src, "test.ky");
    let transformed = check(transformed_src, "test.ky");
    let original_counts = diagnostic_code_counts(&original);
    let transformed_counts = diagnostic_code_counts(&transformed);
    let actual_delta = code_count_delta(&original_counts, &transformed_counts);
    let expected: BTreeMap<String, isize> = expected_delta
        .iter()
        .map(|(code, delta)| ((*code).to_string(), *delta))
        .collect();

    assert_eq!(
        actual_delta,
        expected,
        "diagnostic delta mismatch\n--- original source ---\n{}\n--- transformed source ---\n{}\n--- original diagnostics ---\n{:?}\n--- transformed diagnostics ---\n{:?}\n--- original counts ---\n{:?}\n--- transformed counts ---\n{:?}\n--- actual delta ---\n{:?}\n--- expected delta ---\n{:?}",
        original_src,
        transformed_src,
        diagnostic_signatures(&original),
        diagnostic_signatures(&transformed),
        original_counts,
        transformed_counts,
        actual_delta,
        expected
    );
}

fn assert_check_no_diagnostics(src: &str, context: &str) {
    let output = check(src, "test.ky");
    assert!(
        output.diagnostics.is_empty(),
        "{context}: expected no diagnostics, got: {:?}",
        output.diagnostics
    );
}

fn assert_check_has_diagnostics(src: &str, context: &str) {
    let output = check(src, "test.ky");
    assert!(
        !output.diagnostics.is_empty(),
        "{context}: expected at least one diagnostic"
    );
}

#[test]
fn project_symbol_ids_are_module_qualified() {
    let output = check_project_from_files(&[
        ("main.ky", "fn helper() -> Int { 1 }"),
        ("math.ky", "pub fn helper() -> Int { 2 }"),
    ]);
    let helpers: Vec<_> = output
        .symbol_graph
        .functions
        .iter()
        .filter(|f| f.name == "helper")
        .collect();
    assert_eq!(
        helpers.len(),
        2,
        "expected 2 helper functions, got {}",
        helpers.len()
    );
    let ids: Vec<&str> = helpers.iter().map(|f| f.id.as_str()).collect();
    assert!(ids.contains(&"fn::helper"), "missing fn::helper in {ids:?}");
    assert!(
        ids.contains(&"fn::math::helper"),
        "missing fn::math::helper in {ids:?}"
    );
    assert_ne!(helpers[0].id, helpers[1].id, "IDs must be unique");
}

#[test]
fn project_symbol_graph_no_duplicate_builtins() {
    let output = check_project_from_files(&[
        ("main.ky", "fn foo() -> Int { 1 }"),
        ("math.ky", "pub fn bar() -> Int { 2 }"),
    ]);
    for builtin in &["Option", "Result", "List", "Map"] {
        let count = output
            .symbol_graph
            .types
            .iter()
            .filter(|t| t.name == *builtin)
            .count();
        assert_eq!(count, 1, "expected exactly 1 {builtin} type, got {count}");
    }
}

#[test]
fn project_symbol_graph_imported_fn_not_duplicated_as_local_alias() {
    let output = check_project_from_files(&[
        ("main.ky", "import a\nfn main() -> Int { foo() }\n"),
        ("a.ky", "pub fn foo() -> Int { 1 }\n"),
    ]);

    let foo_nodes: Vec<_> = output
        .symbol_graph
        .functions
        .iter()
        .filter(|f| f.name == "foo")
        .collect();

    assert_eq!(
        foo_nodes.len(),
        1,
        "imported function should appear once in project symbol graph, got: {:?}",
        foo_nodes.iter().map(|f| &f.id).collect::<Vec<_>>()
    );
    assert_eq!(
        foo_nodes[0].id, "fn::a::foo",
        "imported function should keep source-module-qualified ID"
    );
}

#[test]
fn project_symbol_id_uniqueness() {
    let output = check_project_from_files(&[
        (
            "main.ky",
            "type Color = Red | Green\ncap IO { fn read() -> String }\nfn foo() -> Int { 1 }",
        ),
        (
            "math.ky",
            "pub fn add(x: Int, y: Int) -> Int { x + y }\npub type Point = { x: Int, y: Int }",
        ),
    ]);

    let mut ids: Vec<String> = Vec::new();
    for f in &output.symbol_graph.functions {
        ids.push(f.id.clone());
    }
    for t in &output.symbol_graph.types {
        ids.push(t.id.clone());
        for v in &t.variants {
            ids.push(v.id.clone());
        }
    }
    for c in &output.symbol_graph.capabilities {
        ids.push(c.id.clone());
    }

    let count = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(
        ids.len(),
        count,
        "all symbol IDs should be unique, found duplicates in: {ids:?}"
    );
}

#[test]
fn project_symbol_graph_duplicate_fn_defs_use_unique_ids() {
    let output = check_project_from_files(&[
        ("main.ky", "import math\nfn main() -> Int { add(1, 2) }\n"),
        (
            "math.ky",
            "pub fn add(x: Int, y: Int) -> Int { x + y }\npub fn add(x: Int, y: Int) -> Int { x - y }\n",
        ),
    ]);

    let mut ids = std::collections::HashSet::new();
    let mut dups = Vec::new();
    for f in &output.symbol_graph.functions {
        if !ids.insert(f.id.clone()) {
            dups.push(f.id.clone());
        }
    }
    assert!(
        dups.is_empty(),
        "duplicate function IDs should be disambiguated even in invalid programs, got: {dups:?}"
    );
}

#[test]
fn project_call_edges_use_qualified_ids() {
    let output = check_project_from_files(&[
        ("main.ky", "import math\nfn caller() -> Int { add(1, 2) }"),
        ("math.ky", "pub fn add(x: Int, y: Int) -> Int { x + y }"),
    ]);
    let caller = output
        .symbol_graph
        .functions
        .iter()
        .find(|f| f.name == "caller")
        .expect("should have 'caller' function");
    assert!(
        caller.calls.contains(&"fn::math::add".to_string()),
        "expected caller to call fn::math::add, got: {:?}",
        caller.calls
    );
}

#[test]
fn project_symbol_graph_repeated_import_calls_are_deduped() {
    let output = check_project_from_files(&[
        (
            "main.ky",
            "import math\nfn caller() -> Int {\n  add(1, 2)\n  add(3, 4)\n}\n",
        ),
        ("math.ky", "pub fn add(x: Int, y: Int) -> Int { x + y }"),
    ]);
    let caller = output
        .symbol_graph
        .functions
        .iter()
        .find(|f| f.name == "caller")
        .expect("should have 'caller' function");
    let add_edges = caller
        .calls
        .iter()
        .filter(|c| c.as_str() == "fn::math::add")
        .count();
    assert_eq!(
        add_edges, 1,
        "repeated imported calls should dedupe to one edge, got: {:?}",
        caller.calls
    );
}

#[test]
fn project_root_module_uses_bare_ids() {
    let output = check_project_from_files(&[
        ("main.ky", "fn foo() -> Int { 1 }"),
        ("math.ky", "pub fn bar() -> Int { 2 }"),
    ]);
    let foo = output
        .symbol_graph
        .functions
        .iter()
        .find(|f| f.name == "foo")
        .expect("should have 'foo' function");
    assert_eq!(
        foo.id, "fn::foo",
        "root module function should use bare fn::name, got: {}",
        foo.id
    );
}

#[test]
fn project_variant_ids_are_module_qualified() {
    let output = check_project_from_files(&[
        ("main.ky", "fn foo() -> Int { 1 }"),
        ("math.ky", "pub type Color = Red | Green"),
    ]);
    let color = output
        .symbol_graph
        .types
        .iter()
        .find(|t| t.name == "Color")
        .expect("Color type should exist");
    assert_eq!(color.id, "type::math::Color");
    let red = color
        .variants
        .iter()
        .find(|v| v.name == "Red")
        .expect("Red variant should exist");
    assert_eq!(
        red.id, "type::math::Color::Red",
        "variant ID should be module-qualified"
    );
}

#[test]
fn project_capability_ids_are_module_qualified() {
    let output = check_project_from_files(&[
        ("main.ky", "fn foo() -> Int { 1 }"),
        ("math.ky", "pub effect IO"),
    ]);
    let io = output
        .symbol_graph
        .capabilities
        .iter()
        .find(|c| c.name == "IO")
        .expect("IO capability should exist");
    assert_eq!(
        io.id, "cap::math::IO",
        "capability ID should be module-qualified"
    );
    assert!(
        io.functions.is_empty(),
        "label-only effect should not emit member function refs, got: {:?}",
        io.functions
    );
}

#[test]
fn project_builtin_type_ids_are_unqualified() {
    let output = check_project_from_files(&[
        ("main.ky", "fn foo() -> Int { 1 }"),
        ("math.ky", "pub fn bar() -> Int { 2 }"),
    ]);
    for builtin in &["Option", "Result", "List", "Map"] {
        let t = output
            .symbol_graph
            .types
            .iter()
            .find(|t| t.name == *builtin)
            .unwrap_or_else(|| panic!("{builtin} should exist"));
        assert_eq!(
            t.id,
            format!("type::{builtin}"),
            "builtin {builtin} should have unqualified ID"
        );
    }
}

#[test]
fn single_file_ids_unchanged() {
    let output = check("fn foo(x: Int) -> Int { x }", "test.ky");
    let foo = output
        .symbol_graph
        .functions
        .iter()
        .find(|f| f.name == "foo")
        .expect("foo should exist");
    assert_eq!(
        foo.id, "fn::foo",
        "single-file IDs should remain fn::name format"
    );
}

// ── Verification diagnostic tests ────────────────────────────────────

#[test]
fn refactor_verified_has_empty_verification_diagnostics() {
    // A clean rename that passes verification.
    let src = "fn foo() -> Int { 1 }\nfn caller() -> Int { foo() }";
    let action = kyokara_refactor::RefactorAction::RenameSymbol {
        old_name: "foo".into(),
        new_name: "bar".into(),
        kind: kyokara_refactor::SymbolKind::Function,
        target_file: None,
    };
    let output = refactor(src, "test.ky", action, false);
    assert_eq!(output.status, "typechecked");
    assert!(
        output.verification_diagnostics.is_empty(),
        "verified refactor should have empty verification_diagnostics, got: {:?}",
        output.verification_diagnostics
    );
}

#[test]
fn refactor_skipped_has_empty_verification_diagnostics() {
    let src = "fn foo() -> Int { 1 }";
    let action = kyokara_refactor::RefactorAction::RenameSymbol {
        old_name: "foo".into(),
        new_name: "bar".into(),
        kind: kyokara_refactor::SymbolKind::Function,
        target_file: None,
    };
    let output = refactor(src, "test.ky", action, true);
    assert_eq!(output.status, "skipped");
    assert!(
        output.verification_diagnostics.is_empty(),
        "skipped verification should have empty verification_diagnostics"
    );
}

#[test]
fn refactor_error_has_empty_verification_diagnostics() {
    // Nonexistent symbol → RefactorError, not verification failure.
    let src = "fn foo() -> Int { 1 }";
    let action = kyokara_refactor::RefactorAction::RenameSymbol {
        old_name: "nonexistent".into(),
        new_name: "bar".into(),
        kind: kyokara_refactor::SymbolKind::Function,
        target_file: None,
    };
    let output = refactor(src, "test.ky", action, false);
    assert_eq!(output.status, "error");
    assert!(output.error.is_some());
    assert!(
        output.verification_diagnostics.is_empty(),
        "error status should have empty verification_diagnostics"
    );
}

#[test]
fn refactor_status_failed_when_body_lowering_diagnostics_exist() {
    // Refactor should report Failed (not typechecked) when unresolved names remain.
    let src = "fn foo() -> Int { 1 }\nfn main() -> Int { foo() + missing }";
    let action = kyokara_refactor::RefactorAction::RenameSymbol {
        old_name: "foo".into(),
        new_name: "bar".into(),
        kind: kyokara_refactor::SymbolKind::Function,
        target_file: None,
    };
    let output = refactor(src, "test.ky", action, false);

    assert_eq!(output.status, "failed");
    assert!(
        !output.verified,
        "failed verification must set verified=false"
    );
    assert!(
        !output.verification_diagnostics.is_empty(),
        "failed verification should include diagnostics"
    );
    assert!(
        output
            .verification_diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("E0101")),
        "expected unresolved-name style diagnostic code E0101, got: {:?}",
        output
            .verification_diagnostics
            .iter()
            .map(|d| (&d.code, &d.message))
            .collect::<Vec<_>>()
    );
}

#[test]
fn refactor_project_status_failed_when_body_lowering_diagnostics_exist() {
    let (_dir, main_path) = write_project(&[
        (
            "main.ky",
            "import math\nfn main() -> Int { add(1, 2) + missing }\n",
        ),
        ("math.ky", "pub fn add(x: Int, y: Int) -> Int { x + y }\n"),
    ]);
    let action = kyokara_refactor::RefactorAction::RenameSymbol {
        old_name: "add".into(),
        new_name: "sum".into(),
        kind: kyokara_refactor::SymbolKind::Function,
        target_file: None,
    };
    let output = refactor_project(&main_path, action, false);

    assert_eq!(output.status, "failed");
    assert!(
        !output.verified,
        "failed verification must set verified=false"
    );
    assert!(
        !output.edits.is_empty(),
        "refactor should still produce edits before verification"
    );
    assert!(
        output
            .verification_diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("E0101")),
        "expected unresolved-name style diagnostic code E0101, got: {:?}",
        output
            .verification_diagnostics
            .iter()
            .map(|d| (&d.code, &d.message))
            .collect::<Vec<_>>()
    );
}

#[test]
fn refactor_json_has_verification_diagnostics_field() {
    // Verify the JSON output uses "verification_diagnostics" (not "warnings").
    let src = "fn foo() -> Int { 1 }\nfn caller() -> Int { foo() }";
    let action = kyokara_refactor::RefactorAction::RenameSymbol {
        old_name: "foo".into(),
        new_name: "bar".into(),
        kind: kyokara_refactor::SymbolKind::Function,
        target_file: None,
    };
    let output = refactor(src, "test.ky", action, false);
    let json = serde_json::to_string_pretty(&output).expect("serialization failed");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("invalid JSON");

    assert!(
        parsed.get("verification_diagnostics").is_some(),
        "JSON should contain 'verification_diagnostics' key"
    );
    assert!(
        parsed.get("warnings").is_none(),
        "JSON should NOT contain old 'warnings' key"
    );
    let diags = parsed["verification_diagnostics"].as_array().unwrap();
    assert!(
        diags.is_empty(),
        "verified refactor should serialize as empty array"
    );
}

#[test]
fn refactor_verification_diagnostics_dto_structure() {
    // Test that VerificationDiagnostic carries structured data (span, code).
    // Manually apply an edit that introduces a type mismatch, then re-check.
    //   Original: fn foo() -> Int { 1 }
    //   Patched:  fn foo() -> Int { "hello" }  (replace "1" with "\"hello\"")
    let src = "fn foo() -> Int { 1 }";
    // "1" is at offset 18 (fn foo() -> Int { 1 })
    //  0123456789012345678901
    let bad_edits = vec![kyokara_refactor::TextEdit {
        file_id: kyokara_span::FileId(0),
        range: kyokara_span::TextRange::new(18.into(), 19.into()),
        new_text: "\"hello\"".into(),
    }];
    let patched = kyokara_refactor::apply_edits(src, &bad_edits);
    assert!(
        patched.contains("\"hello\""),
        "patched should have string literal, got: {patched}"
    );

    // Re-check the broken source — should have a type mismatch.
    let check = kyokara_hir::check_file(&patched);
    assert!(
        !check.type_check.raw_diagnostics.is_empty(),
        "patched source should have type errors"
    );

    // Verify the diagnostic data has code and span.
    let (data, span) = &check.type_check.raw_diagnostics[0];
    assert_eq!(data.code(), "E0001", "should be type mismatch error");
    assert!(
        span.range.start() <= span.range.end(),
        "span should have valid range"
    );

    // Verify VerificationDiagnostic correctly carries the enriched data.
    let diag = data
        .clone()
        .into_diagnostic(*span, &check.interner, &check.item_tree);
    let vd = kyokara_refactor::transaction::VerificationDiagnostic {
        message: diag.message.clone(),
        span: Some(*span),
        code: Some(data.code().into()),
    };
    assert!(vd.span.is_some(), "diagnostic should have span");
    assert_eq!(vd.code.as_deref(), Some("E0001"));
    assert!(
        vd.message.contains("mismatch"),
        "message should mention mismatch: {}",
        vd.message
    );
}

#[test]
fn transaction_verification_failure_has_structured_spans() {
    // Apply edits that introduce a type mismatch, verify the re-check produces
    // diagnostics with spans and codes.
    let src = "fn foo() -> Int { 1 }";
    let bad_edits = vec![kyokara_refactor::TextEdit {
        file_id: kyokara_span::FileId(0),
        range: kyokara_span::TextRange::new(18.into(), 19.into()),
        new_text: "\"broken\"".into(),
    }];
    let patched = kyokara_refactor::apply_edits(src, &bad_edits);

    let check = kyokara_hir::check_file(&patched);
    let has_errors =
        !check.lowering_diagnostics.is_empty() || !check.type_check.raw_diagnostics.is_empty();
    assert!(has_errors, "broken source should have type errors");

    // Type check diagnostics should have valid spans.
    for (data, span) in &check.type_check.raw_diagnostics {
        assert!(
            span.range.start() <= span.range.end(),
            "type diagnostic should have valid span range"
        );
        assert!(
            !data.code().is_empty(),
            "diagnostic should have an error code"
        );
    }
}

#[test]
fn refactor_project_verified_json_structure() {
    let (_dir, main_path) = write_project(&[
        ("main.ky", "import math\nfn caller() -> Int { add(1, 2) }"),
        ("math.ky", "pub fn add(x: Int, y: Int) -> Int { x + y }"),
    ]);
    let action = kyokara_refactor::RefactorAction::RenameSymbol {
        old_name: "add".into(),
        new_name: "sum".into(),
        kind: kyokara_refactor::SymbolKind::Function,
        target_file: None,
    };
    let output = refactor_project(&main_path, action, false);
    assert_eq!(output.status, "typechecked");

    let json = serde_json::to_string_pretty(&output).expect("serialization failed");
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("invalid JSON");

    assert!(
        parsed.get("verification_diagnostics").is_some(),
        "project refactor JSON should have 'verification_diagnostics'"
    );
    assert!(
        parsed.get("warnings").is_none(),
        "project refactor JSON should NOT have old 'warnings' field"
    );
}

#[test]
fn verification_diagnostic_dto_serializes_all_fields() {
    // Directly test the DTO serialization structure.
    let dto = kyokara_api::VerificationDiagnosticDto {
        message: "type mismatch".into(),
        code: Some("E0001".into()),
        span: Some(kyokara_api::SpanDto {
            file: "test.ky".into(),
            start: 10,
            end: 20,
        }),
    };
    let json = serde_json::to_value(&dto).expect("serialization failed");
    assert_eq!(json["message"], "type mismatch");
    assert_eq!(json["code"], "E0001");
    assert_eq!(json["span"]["file"], "test.ky");
    assert_eq!(json["span"]["start"], 10);
    assert_eq!(json["span"]["end"], 20);
}

#[test]
fn verification_diagnostic_dto_nullable_fields() {
    // Code and span can be null.
    let dto = kyokara_api::VerificationDiagnosticDto {
        message: "parse error".into(),
        code: None,
        span: None,
    };
    let json = serde_json::to_value(&dto).expect("serialization failed");
    assert_eq!(json["message"], "parse error");
    assert!(json["code"].is_null());
    assert!(json["span"].is_null());
}

// ── File-qualified quickfix tests (#44) ──────────────────────────────

#[test]
fn api_refactor_project_quickfix_with_target_file() {
    // Set up a project where (two modules have match exhaustiveness errors.)
    // The API should accept target_file to disambiguate.
    let (_dir, main_path) = write_project(&[
        (
            "main.ky",
            "type A = X | Y\nfn check_a(a: A) -> Int {\n    match (a) {\n        X => 1\n    }\n}",
        ),
        (
            "math.ky",
            "pub type B = P | Q\npub fn check_b(b: B) -> Int {\n    match (b) {\n        P => 1\n    }\n}",
        ),
    ]);

    // Get the diagnostics to find the offset.
    let check_output = check_project(&main_path);
    let match_diag = check_output
        .diagnostics
        .iter()
        .find(|d| d.code == "E0009" && d.span.file.contains("math"))
        .expect("math.ky should have E0009 (MissingMatchArms)");

    let math_file = &match_diag.span.file;
    let offset = match_diag.span.start;

    let action = kyokara_refactor::RefactorAction::AddMissingMatchCases {
        offset,
        target_file: Some(math_file.clone()),
    };
    let output = refactor_project(&main_path, action, false);
    assert_ne!(
        output.status, "error",
        "quickfix with target_file should succeed: {:?}",
        output.error
    );
    // The edit should mention Q (the missing variant from math.ky's type B).
    let has_q = output.edits.iter().any(|e| e.new_text.contains("Q"));
    assert!(
        has_q,
        "should add missing arm Q from math.ky, got edits: {:?}",
        output.edits
    );
}

#[test]
fn api_refactor_project_quickfix_wrong_target_file_gives_error() {
    let (_dir, main_path) = write_project(&[
        (
            "main.ky",
            "type A = X | Y\nfn check_a(a: A) -> Int {\n    match (a) {\n        X => 1\n    }\n}",
        ),
        ("math.ky", "pub fn add(x: Int, y: Int) -> Int { x + y }"),
    ]);

    // Get the offset of the match diagnostic in main.ky.
    let check_output = check_project(&main_path);
    let match_diag = check_output
        .diagnostics
        .iter()
        .find(|d| d.code == "E0009")
        .expect("should have E0009 diagnostic");

    let offset = match_diag.span.start;
    let math_file = _dir.path().join("math.ky").display().to_string();

    // Point target_file to math.ky (which has no match diagnostic).
    let action = kyokara_refactor::RefactorAction::AddMissingMatchCases {
        offset,
        target_file: Some(math_file),
    };
    let output = refactor_project(&main_path, action, false);
    assert_eq!(
        output.status, "error",
        "quickfix with wrong target_file should fail"
    );
}

// ── IoError handling tests ───────────────────────────────────────────

#[test]
fn api_refactor_project_io_error_surfaces_in_dto() {
    // A valid project refactor should succeed — regression test that
    // filesystem operations work and don't silently fail.
    let (_dir, main_path) = write_project(&[
        ("main.ky", "import math\nfn caller() -> Int { add(1, 2) }"),
        ("math.ky", "pub fn add(x: Int, y: Int) -> Int { x + y }"),
    ]);
    let action = kyokara_refactor::RefactorAction::RenameSymbol {
        old_name: "add".into(),
        new_name: "sum".into(),
        kind: kyokara_refactor::SymbolKind::Function,
        target_file: None,
    };
    let output = refactor_project(&main_path, action, false);
    // Should be verified, not an error (filesystem ops should succeed).
    assert_ne!(
        output.status, "error",
        "valid project refactor should not produce an error: {:?}",
        output.error
    );
    assert_eq!(output.status, "typechecked");
}

#[test]
fn api_io_error_produces_error_status() {
    // Construct an IoError directly and verify the Display message
    // is what the API's error_dto would serialize.
    let err = kyokara_refactor::RefactorError::IoError {
        message: "failed to create temp directory".into(),
    };
    let msg = err.to_string();
    // The message should clearly indicate an I/O error, not a symbol error.
    assert!(
        !msg.contains("not found in module scope"),
        "IoError should NOT look like SymbolNotFound: {msg}"
    );
    assert!(
        msg.contains("failed to create temp directory"),
        "IoError message should include the original error: {msg}"
    );
}

// ── Import resolution fixes (#64, #65) ───────────────────────────────

#[test]
fn check_project_reports_unresolved_import() {
    let output = check_project_from_files(&[("main.ky", "import nope\nfn main() -> Int { 1 }\n")]);
    let diag = output
        .diagnostics
        .iter()
        .find(|d| d.message.contains("nope"))
        .expect("expected unresolved import diagnostic for `nope`");
    assert!(
        diag.span.end > diag.span.start,
        "expected unresolved import span to target source range, got: {:?}",
        diag.span
    );
}

#[test]
fn check_project_surfaces_module_read_io_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let main_path = dir.path().join("main.ky");
    std::fs::write(&main_path, "import bad\nfn main() -> Int { 1 }\n").expect("write main");

    let bad_path = dir.path().join("bad.ky");
    std::fs::write(&bad_path, vec![0xff, 0xfe, 0xfd]).expect("write invalid utf8 module");

    let output = check_project(&main_path);
    let io_diag = output
        .diagnostics
        .iter()
        .find(|d| d.message.contains("failed to read module"))
        .expect("expected module read I/O diagnostic");

    assert!(
        io_diag.message.contains("bad.ky"),
        "I/O diagnostic should mention failing module path, got: {}",
        io_diag.message
    );
}

#[test]
fn check_project_aliased_import_resolves_by_path_not_alias() {
    // `import math as M` should resolve the "math" module, not look for a module named "M".
    let output = check_project_from_files(&[
        (
            "main.ky",
            "import math as M\nfn main() -> Int { add(1, 2) }\n",
        ),
        ("math.ky", "pub fn add(x: Int, y: Int) -> Int { x + y }\n"),
    ]);
    // Should have no "unresolved import" errors.
    let import_errors: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("unresolved import"))
        .collect();
    assert!(
        import_errors.is_empty(),
        "aliased import should resolve correctly, got errors: {:?}",
        import_errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn check_project_reports_ambiguous_import_last_segment() {
    let output = check_project_from_files(&[
        ("main.ky", "import math\nfn main() -> Int { value() }\n"),
        ("a/math.ky", "pub fn value() -> Int { 1 }\n"),
        ("b/math.ky", "pub fn value() -> Int { 2 }\n"),
    ]);
    let diag = output
        .diagnostics
        .iter()
        .find(|d| d.message.contains("ambiguous import"))
        .expect("expected ambiguous import diagnostic");
    assert!(
        diag.span.end > diag.span.start,
        "expected ambiguous import span to target source range, got: {:?}",
        diag.span
    );
}

#[test]
fn check_project_qualified_import_resolves_duplicate_leaf_modules() {
    // `import a.math` should resolve exactly `a/math.ky` even when `b/math.ky` exists.
    let output = check_project_from_files(&[
        ("main.ky", "import a.math\nfn main() -> Int { value() }\n"),
        ("a/math.ky", "pub fn value() -> Int { 1 }\n"),
        ("b/math.ky", "pub fn value() -> Int { 2 }\n"),
    ]);
    let import_diags: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("import"))
        .collect();
    assert!(
        import_diags.is_empty(),
        "qualified import should resolve without import diagnostics, got: {:?}",
        import_diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn check_project_qualified_import_missing_path_does_not_match_by_leaf() {
    // `import c.math` should not fall back to any `*.math` leaf modules.
    let output = check_project_from_files(&[
        ("main.ky", "import c.math\nfn main() -> Int { value() }\n"),
        ("a/math.ky", "pub fn value() -> Int { 1 }\n"),
        ("b/math.ky", "pub fn value() -> Int { 2 }\n"),
    ]);
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("unresolved import")),
        "expected unresolved import diagnostic for qualified missing path, got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ── Lowering diagnostic file path (#66) ──────────────────────────────

#[test]
fn check_project_lowering_diagnostic_has_real_file_path() {
    // Lowering diagnostics should report the actual file path, not "<project>".
    let output = check_project_from_files(&[("main.ky", "import nope\nfn main() -> Int { 1 }\n")]);
    let diag = output
        .diagnostics
        .iter()
        .find(|d| d.message.contains("unresolved import"))
        .expect("expected unresolved import diagnostic");
    assert!(
        !diag.span.file.contains("<project>"),
        "diagnostic file should not be '<project>', got: {}",
        diag.span.file
    );
    assert!(
        diag.span.file.contains("main.ky"),
        "diagnostic file should reference main.ky, got: {}",
        diag.span.file
    );
}

// ── Constructor pattern binding scope (#100) ─────────────────────────

#[test]
fn constructor_pattern_binding_is_in_scope() {
    // `Some(x) => x` should not produce "unresolved name x".
    let src = "fn main() -> Int { match (Some(1)) { Some(x) => x, None => 0 } }";
    let output = check(src, "test.ky");
    let unresolved: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("unresolved"))
        .collect();
    assert!(
        unresolved.is_empty(),
        "constructor pattern binding `x` should be in scope, got: {:?}",
        unresolved.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn constructor_pattern_arity_mismatch_produces_diagnostic() {
    // `Some(_, _)` has 2 args but Some expects 1.
    let src = "fn main() -> Int { match (Some(1)) { Some(_, _) => 0, None => 1 } }";
    let output = check(src, "test.ky");
    let arity_errors: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("expected") && d.message.contains("argument"))
        .collect();
    assert!(
        !arity_errors.is_empty(),
        "expected arity mismatch diagnostic for Some(_, _), got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn nested_constructor_pattern_binding_is_in_scope() {
    // `Some(Some(x)) => x` — nested constructor bindings should also work.
    let src = "fn main() -> Int { match (Some(Some(1))) { Some(Some(x)) => x, _ => 0 } }";
    let output = check(src, "test.ky");
    let unresolved: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("unresolved"))
        .collect();
    assert!(
        unresolved.is_empty(),
        "nested constructor pattern binding `x` should be in scope, got: {:?}",
        unresolved.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn duplicate_record_field_in_type_alias_produces_diagnostic() {
    let src = "type P = { x: Int, x: Int }\nfn main() -> Int { 1 }";
    let output = check(src, "test.ky");
    let dups: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("duplicate") && d.message.contains("field"))
        .collect();
    assert!(
        !dups.is_empty(),
        "expected duplicate field diagnostic, got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert!(
        dups[0].message.contains("x"),
        "should mention field name `x`"
    );
}

#[test]
fn unknown_constructor_pattern_emits_diagnostic() {
    // `Nope` is not a constructor in scope — should produce E0013.
    let src = "fn main() -> Int { match (Some(1)) { Nope(x) => x, _ => 0 } }";
    let output = check(src, "test.ky");
    let unresolved: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.code == "E0013")
        .collect();
    assert!(
        !unresolved.is_empty(),
        "expected E0013 for unknown constructor `Nope`, got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert!(
        unresolved[0].message.contains("Nope"),
        "diagnostic should mention the unknown constructor name, got: {}",
        unresolved[0].message
    );
}

#[test]
fn unknown_constructor_pattern_diagnostic_uses_pattern_span() {
    let src = "fn main() -> Int { match (Some(1)) { Nope(x) => x, _ => 0 } }";
    let output = check(src, "test.ky");
    let diag = output
        .diagnostics
        .iter()
        .find(|d| d.code == "E0013")
        .expect("expected E0013 for unknown constructor");

    let pat = "Nope(x)";
    let pat_start = src.find(pat).expect("pattern should exist in source") as u32;
    let pat_end = pat_start + pat.len() as u32;
    assert!(
        diag.span.start >= pat_start && diag.span.end <= pat_end,
        "expected E0013 span within pattern `{}` [{}..{}], got [{}..{}]",
        pat,
        pat_start,
        pat_end,
        diag.span.start,
        diag.span.end
    );
}

#[test]
fn constructor_pattern_arity_mismatch_diagnostic_uses_pattern_span() {
    let src = "fn main() -> Int { match (Some(1)) { Some(_, _) => 0, None => 1 } }";
    let output = check(src, "test.ky");
    let diag = output
        .diagnostics
        .iter()
        .find(|d| d.code == "E0007")
        .expect("expected E0007 for constructor arity mismatch");

    let pat = "Some(_, _)";
    let pat_start = src.find(pat).expect("pattern should exist in source") as u32;
    let pat_end = pat_start + pat.len() as u32;
    assert!(
        diag.span.start >= pat_start && diag.span.end <= pat_end,
        "expected E0007 span within pattern `{}` [{}..{}], got [{}..{}]",
        pat,
        pat_start,
        pat_end,
        diag.span.start,
        diag.span.end
    );
}

#[test]
fn duplicate_function_params_produce_diagnostic() {
    let src = "fn f(x: Int, x: Int) -> Int { x }\nfn main() -> Int { f(1, 2) }";
    let output = check(src, "test.ky");
    let dups: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("duplicate") && d.message.contains("parameter"))
        .collect();
    assert!(
        !dups.is_empty(),
        "expected duplicate parameter diagnostic, got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert!(
        dups[0].message.contains("x"),
        "should mention param name `x`"
    );
}

#[test]
fn duplicate_type_params_produce_diagnostic() {
    let src = "fn id<T, T>(x: T) -> T { x }\nfn main() -> Int { id(1) }";
    let output = check(src, "test.ky");
    let dups: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("duplicate") && d.message.contains("type parameter"))
        .collect();
    assert!(
        !dups.is_empty(),
        "expected duplicate type parameter diagnostic, got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert!(
        dups[0].message.contains("T"),
        "should mention type param `T`"
    );
}

#[test]
fn duplicate_fields_in_record_literal_produce_diagnostic() {
    let src = "type Point = { x: Int }\nfn main() -> Int { let p = Point { x: 1, x: 2 }\n p.x }";
    let output = check(src, "test.ky");
    let dups: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("duplicate") && d.message.contains("field"))
        .collect();
    assert!(
        !dups.is_empty(),
        "expected duplicate field diagnostic in record literal, got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert!(
        dups[0].message.contains("x"),
        "should mention field name `x`"
    );
}

#[test]
fn old_outside_contract_produces_diagnostic() {
    let src = "fn f(x: Int) -> Int { old(x) }\nfn main() -> Int { f(1) }";
    let output = check(src, "test.ky");
    let old_errs: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("old") && d.message.contains("contract"))
        .collect();
    assert!(
        !old_errs.is_empty(),
        "expected `old()` outside contract diagnostic, got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn dotted_constructor_pattern_produces_diagnostic() {
    let src = "fn main() -> Int { match (Some(1)) { A.B(_) => 0, _ => 1 } }";
    let output = check(src, "test.ky");
    let errs: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.code == "E0013")
        .collect();
    assert!(
        !errs.is_empty(),
        "expected diagnostic for dotted constructor pattern, got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn record_pattern_invalid_field_produces_diagnostic() {
    let src = "type Point = { x: Int }\nfn f(p: Point) -> Int { match (p) { Point { y } => 0, _ => 1 } }\nfn main() -> Int { f(Point { x: 1 }) }";
    let output = check(src, "test.ky");
    let errs: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("field") && d.message.contains("y"))
        .collect();
    assert!(
        !errs.is_empty(),
        "expected invalid field diagnostic for `y`, got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn record_pattern_on_non_record_scrutinee_produces_diagnostic() {
    let src = "fn main() -> Int { match (1) { { x } => 0, _ => 1 } }";
    let output = check(src, "test.ky");
    let errs: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("mismatch") || d.message.contains("record"))
        .collect();
    assert!(
        !errs.is_empty(),
        "expected type mismatch for record pattern on Int, got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn duplicate_fields_in_record_pattern_produce_diagnostic() {
    let src = "type Point = { x: Int }\nfn f(p: Point) -> Int { match (p) { Point { x, x } => x } }\nfn main() -> Int { f(Point { x: 1 }) }";
    let output = check(src, "test.ky");
    let dups: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("duplicate") && d.message.contains("field"))
        .collect();
    assert!(
        !dups.is_empty(),
        "expected duplicate field diagnostic in record pattern, got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn record_pattern_binding_is_typed_for_not_a_function_issue_133() {
    let src = r#"
type Point = { x: Int }
fn f(p: Point) -> Int { match (p) { { x } => x("oops"), _ => 0 } }
fn main() -> Int { f(Point { x: 1 }) }
"#;
    let output = check(src, "test.ky");
    let errs: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.code == "E0006")
        .collect();
    assert!(
        !errs.is_empty(),
        "expected NotAFunction diagnostic (E0006) for record-bound `x`, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn record_pattern_binding_typed_field_still_allows_valid_use_issue_133_guard() {
    let src = r#"
type Point = { x: Int }
fn f(p: Point) -> Int { match (p) { { x } => x + 1, _ => 0 } }
fn main() -> Int { f(Point { x: 1 }) }
"#;
    let output = check(src, "test.ky");
    assert!(
        output.diagnostics.is_empty(),
        "expected valid typed record binding usage, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn duplicate_lambda_params_produce_diagnostic() {
    let src = "fn main() -> Int { let f = fn(x: Int, x: Int) => x\n f(1, 2) }";
    let output = check(src, "test.ky");
    let dups: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("duplicate") && d.message.contains("parameter"))
        .collect();
    assert!(
        !dups.is_empty(),
        "expected duplicate lambda parameter diagnostic, got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn named_record_literal_unknown_field_produces_diagnostic() {
    let src = "type Point = { x: Int }\nfn main() -> Int { let p = Point { y: 1 }\n p.x }";
    let output = check(src, "test.ky");
    let errs: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("field") && d.message.contains("y"))
        .collect();
    assert!(
        !errs.is_empty(),
        "expected unknown field diagnostic for `y`, got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn capitalized_unknown_pattern_produces_diagnostic() {
    // `Smoe` looks like a constructor but isn't — should warn, not silently bind.
    let src = "fn main() -> Int { match (Some(1)) { Smoe => 0, _ => 1 } }";
    let output = check(src, "test.ky");
    let errs: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("Smoe"))
        .collect();
    assert!(
        !errs.is_empty(),
        "expected diagnostic for unknown capitalized pattern `Smoe`, got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn duplicate_bindings_in_constructor_pattern_produce_diagnostic() {
    let src = "type Pair = Pair(Int, Int)\nfn f(p: Pair) -> Int { match (p) { Pair(x, x) => x } }\nfn main() -> Int { f(Pair(1, 2)) }";
    let output = check(src, "test.ky");
    let dups: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("duplicate") && d.message.contains("binding"))
        .collect();
    assert!(
        !dups.is_empty(),
        "expected duplicate binding diagnostic, got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn duplicate_binding_detection_is_local_to_each_match_arm_pattern() {
    let src = "type Pair = Pair(Int, Int)\nfn f(p: Pair) -> Int { match (p) { Pair(x, x) => x, Pair(x, y) => x } }\nfn main() -> Int { f(Pair(1, 2)) }";
    let output = check(src, "test.ky");
    let dup_binding_diags: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("duplicate binding"))
        .collect();
    assert_eq!(
        dup_binding_diags.len(),
        1,
        "duplicate-binding detection should not leak across match (arms, got:) {:?}",
        dup_binding_diags
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn duplicate_binding_detection_is_local_to_each_let_pattern() {
    let src = "type Pair = Pair(Int, Int)\nfn main() -> Int { let Pair(x, x) = Pair(1, 2)\n let Pair(x, y) = Pair(3, 4)\n x + y }";
    let output = check(src, "test.ky");
    let dup_binding_diags: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("duplicate binding"))
        .collect();
    assert_eq!(
        dup_binding_diags.len(),
        1,
        "duplicate-binding detection should not leak across let patterns, got: {:?}",
        dup_binding_diags
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn unknown_capability_in_with_clause_produces_diagnostic() {
    let src = "fn main() -> Int with Nope { 1 }";
    let output = check(src, "test.ky");
    let errs: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("Nope") || d.message.contains("unresolved"))
        .collect();
    assert!(
        !errs.is_empty(),
        "expected diagnostic for unknown capability `Nope`, got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn refined_type_produces_unsupported_diagnostic() {
    let src = "fn f(x: { v: Int | false }) -> Int { x }\nfn main() -> Int { f(0) }";
    let output = check(src, "test.ky");
    let errs: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("refined") && d.message.contains("not yet supported"))
        .collect();
    assert!(
        !errs.is_empty(),
        "expected unsupported refined type diagnostic, got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn let_constructor_pattern_bindings_in_scope() {
    // `let Some(x) = Some(1)` — x should be in scope after the let binding.
    let src = "fn main() -> Int { let Some(x) = Some(1)\n x }";
    let output = check(src, "test.ky");
    let unresolved: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("unresolved"))
        .collect();
    assert!(
        unresolved.is_empty(),
        "constructor pattern binding `x` should be in scope, got: {:?}",
        unresolved.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn project_import_collision_produces_diagnostic() {
    // Two modules export `pub fn foo()` — importing both should produce a collision diagnostic.
    let output = check_project_from_files(&[
        ("main.ky", "import a\nimport b\nfn main() -> Int { foo() }"),
        ("a.ky", "pub fn foo() -> Int { 1 }"),
        ("b.ky", "pub fn foo() -> Int { 2 }"),
    ]);
    let collisions: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("foo") && d.message.contains("import"))
        .collect();
    assert!(
        !collisions.is_empty(),
        "expected import collision diagnostic for `foo`, got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert!(
        collisions[0].span.end > collisions[0].span.start,
        "expected conflicting import span to target source range, got: {:?}",
        collisions[0].span
    );
}

#[test]
fn project_import_collision_does_not_misattribute_call_edge_to_specific_module() {
    let output = check_project_from_files(&[
        ("main.ky", "import a\nimport b\nfn main() -> Int { foo() }"),
        ("a.ky", "pub fn foo() -> Int { 1 }"),
        ("b.ky", "pub fn foo() -> Int { 2 }"),
    ]);

    let collisions: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("foo") && d.message.contains("import"))
        .collect();
    assert!(
        !collisions.is_empty(),
        "expected conflicting import diagnostic for `foo`, got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    let main_fn = output
        .symbol_graph
        .functions
        .iter()
        .find(|f| f.name == "main")
        .expect("should contain main function");

    assert!(
        !main_fn.calls.contains(&"fn::a::foo".to_string())
            && !main_fn.calls.contains(&"fn::b::foo".to_string()),
        "ambiguous collision call should not be attributed to a specific module, got calls: {:?}",
        main_fn.calls
    );
}

#[test]
fn effect_declaration_with_body_produces_label_only_diagnostic() {
    let src = "effect C {\n  fn foo() -> Int\n}\nfn main() -> Int { 1 }";
    let output = check(src, "test.ky");
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("effect declarations are labels only")),
        "expected label-only effect diagnostic, got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn cyclic_type_alias_does_not_crash() {
    // `type A = A` is a direct cycle — should produce an error, not a stack overflow.
    let src = "type A = A\nfn main() -> A { _ }";
    let _output = check(src, "test.ky");
    // Just verifying we don't crash. The output will have type errors.
}

#[test]
fn extra_type_args_produce_diagnostic() {
    // `List<Int, Int>` has 2 type args but List expects 1.
    let src = "fn f(x: List<Int, Int>) -> Int { 1 }\nfn main() -> Int { 1 }";
    let output = check(src, "test.ky");
    let arity_errs: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("type argument"))
        .collect();
    assert!(
        !arity_errs.is_empty(),
        "expected type arity diagnostic for List<Int, Int>, got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn duplicate_effect_names_produce_diagnostic() {
    let src = "effect C\neffect C\nfn main() -> Int { 1 }";
    let output = check(src, "test.ky");
    let dups: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("duplicate") && d.message.contains("C"))
        .collect();
    assert!(
        !dups.is_empty(),
        "expected duplicate effect diagnostic, got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn effect_nodes_have_no_member_fn_refs() {
    let src = "effect C\nfn main() -> Int { 1 }";
    let output = check(src, "test.ky");
    for cap in &output.symbol_graph.capabilities {
        assert!(
            cap.functions.is_empty(),
            "label-only effect should not emit function refs, got: {:?}",
            cap.functions
        );
    }
}

#[test]
fn malformed_numeric_underscore_trailing() {
    let src = "fn main() -> Int { 1_ }";
    let output = check(src, "test.ky");
    let errs: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("underscore"))
        .collect();
    assert!(
        !errs.is_empty(),
        "expected malformed underscore diagnostic for `1_`, got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn malformed_numeric_underscore_consecutive() {
    let src = "fn main() -> Int { 1__2 }";
    let output = check(src, "test.ky");
    let errs: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("underscore"))
        .collect();
    assert!(
        !errs.is_empty(),
        "expected malformed underscore diagnostic for `1__2`, got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn parse_error_has_nonzero_span() {
    // `fn main( -> Int { 1 }` is missing `)` — the parse error should have a non-zero span.
    let src = "fn main( -> Int { 1 }";
    let output = check(src, "test.ky");
    let parse_errs: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|d| d.code == "E0100")
        .collect();
    assert!(!parse_errs.is_empty(), "expected at least one parse error");
    assert!(
        parse_errs
            .iter()
            .any(|d| d.span.start != 0 || d.span.end != 0),
        "expected parse error with non-zero span, got: {:?}",
        parse_errs
            .iter()
            .map(|d| (d.span.start, d.span.end))
            .collect::<Vec<_>>()
    );
}

#[test]
fn symbol_graph_partial_on_parse_error() {
    // When a file has parse errors, the symbol graph should be marked partial.
    let output = check("fn main( -> Int { 1 }", "test.ky");
    assert!(
        !output.diagnostics.is_empty(),
        "should have parse error diagnostics"
    );
    assert!(
        output.symbol_graph.partial,
        "symbol graph should be marked partial when parse errors exist"
    );
}

#[test]
fn symbol_graph_constructor_not_in_function_calls() {
    // Constructor expressions like Some(1) should NOT appear as fn::Some in calls.
    let output = check("fn main() -> Option<Int> { Some(1) }", "test.ky");
    let main_fn = output
        .symbol_graph
        .functions
        .iter()
        .find(|f| f.name == "main")
        .expect("should have main function");
    assert!(
        !main_fn.calls.iter().any(|c| c.contains("Some")),
        "constructor Some should not appear in function calls, got: {:?}",
        main_fn.calls
    );
}

#[test]
fn symbol_graph_local_closure_not_attributed_to_function() {
    // A local closure `foo` shadowing top-level `fn foo()` should not
    // appear as a call to fn::foo in the symbol graph.
    let src = "fn foo() -> Int { 1 }\nfn main() -> Int {\n  let foo = fn() => 2\n  foo()\n}";
    let output = check(src, "test.ky");
    let main_fn = output
        .symbol_graph
        .functions
        .iter()
        .find(|f| f.name == "main")
        .expect("should have main function");
    assert!(
        !main_fn.calls.iter().any(|c| c.contains("foo")),
        "local closure call should not appear as fn::foo, got: {:?}",
        main_fn.calls
    );
}

#[test]
fn symbol_graph_not_partial_on_clean_file() {
    let output = check("fn main() -> Int { 1 }", "test.ky");
    assert!(
        output.diagnostics.is_empty(),
        "should have no diagnostics, got: {:?}",
        output.diagnostics
    );
    assert!(
        !output.symbol_graph.partial,
        "symbol graph should NOT be partial for clean file"
    );
}

#[test]
fn rename_function_does_not_rename_shadowing_local() {
    // Bug test: local `foo` declared BEFORE usage shadows the function.
    // Renaming function `foo` → `bar` should NOT touch the local binding or its usages.
    let src = "fn foo() -> Int {\n  let foo = 1\n  foo\n}\n\nfn main() -> Int { foo() }";
    let action = kyokara_refactor::RefactorAction::RenameSymbol {
        old_name: "foo".into(),
        new_name: "bar".into(),
        kind: kyokara_refactor::SymbolKind::Function,
        target_file: None,
    };
    let output = refactor(src, "test.ky", action, false);
    let patched = &output
        .patched_sources
        .as_ref()
        .expect("should have patched sources")[0]
        .source;
    assert!(
        patched.contains("fn bar()"),
        "function definition should be renamed, got: {patched}"
    );
    assert!(
        patched.contains("bar()"),
        "function call should be renamed, got: {patched}"
    );
    assert!(
        patched.contains("let foo = 1"),
        "local binding should NOT be renamed, got: {patched}"
    );
    let lines: Vec<&str> = patched.lines().collect();
    assert_eq!(
        lines[2].trim(),
        "foo",
        "local variable usage should NOT be renamed, got: {patched}"
    );
}

#[test]
fn rename_function_renames_call_before_same_named_local() {
    // Guard test (#158): local `foo` declared AFTER call site should NOT
    // suppress renaming of the call. The call resolves to the function,
    // not the later local.
    let src = "fn foo() -> Int { 1 }\n\nfn main() -> Int {\n  foo()\n  let foo = 2\n  foo\n}";
    let action = kyokara_refactor::RefactorAction::RenameSymbol {
        old_name: "foo".into(),
        new_name: "bar".into(),
        kind: kyokara_refactor::SymbolKind::Function,
        target_file: None,
    };
    let output = refactor(src, "test.ky", action, false);
    let patched = &output
        .patched_sources
        .as_ref()
        .expect("should have patched sources")[0]
        .source;
    assert!(
        patched.contains("fn bar()"),
        "function definition should be renamed, got: {patched}"
    );
    // Check specifically inside main's body for the renamed call.
    // (Don't just check for "bar()" which also matches "fn bar() -> ...")
    let main_body = patched.split("fn main()").nth(1).expect("should have main");
    assert!(
        main_body.contains("bar()"),
        "call site BEFORE local should be renamed in main body, got: {patched}"
    );
    assert!(
        patched.contains("let foo = 2"),
        "local binding should NOT be renamed, got: {patched}"
    );
}

#[test]
fn rename_function_mixed_shadow_and_call() {
    // Edge case: same function has a call BEFORE the local, a local binding,
    // a local usage AFTER the binding, and another call AFTER the local usage.
    // The call before should be renamed; local binding + usage should not;
    // the call after the local should NOT be renamed (local shadows it).
    let src = "fn foo() -> Int { 1 }\n\nfn main() -> Int {\n  let a = foo()\n  let foo = 2\n  let b = foo\n  a + b\n}";
    let action = kyokara_refactor::RefactorAction::RenameSymbol {
        old_name: "foo".into(),
        new_name: "bar".into(),
        kind: kyokara_refactor::SymbolKind::Function,
        target_file: None,
    };
    let output = refactor(src, "test.ky", action, false);
    let patched = &output
        .patched_sources
        .as_ref()
        .expect("should have patched sources")[0]
        .source;
    assert!(
        patched.contains("fn bar()"),
        "function definition should be renamed, got: {patched}"
    );
    assert!(
        patched.contains("let a = bar()"),
        "call BEFORE local binding should be renamed, got: {patched}"
    );
    assert!(
        patched.contains("let foo = 2"),
        "local binding should NOT be renamed, got: {patched}"
    );
    assert!(
        patched.contains("let b = foo"),
        "local usage after binding should NOT be renamed, got: {patched}"
    );
}

#[test]
fn rename_function_param_shadows_entire_body() {
    // Edge case: a parameter named `foo` shadows the function for the
    // entire body — all usages inside should be skipped.
    let src = "fn foo() -> Int { 1 }\n\nfn main(foo: Int) -> Int { foo + foo }";
    let action = kyokara_refactor::RefactorAction::RenameSymbol {
        old_name: "foo".into(),
        new_name: "bar".into(),
        kind: kyokara_refactor::SymbolKind::Function,
        target_file: None,
    };
    let output = refactor(src, "test.ky", action, false);
    let patched = &output
        .patched_sources
        .as_ref()
        .expect("should have patched sources")[0]
        .source;
    assert!(
        patched.contains("fn bar()"),
        "function definition should be renamed, got: {patched}"
    );
    assert!(
        patched.contains("main(foo: Int)"),
        "param name should NOT be renamed, got: {patched}"
    );
    assert!(
        patched.contains("foo + foo"),
        "param usages should NOT be renamed, got: {patched}"
    );
}

#[test]
fn if_condition_rejects_non_bool() {
    // `if 1 {}` — the condition should require Bool, not accept Int silently.
    let output = check("fn f() -> Unit { if (1) { } }", "test.ky");
    assert!(
        !output.diagnostics.is_empty(),
        "expected a type mismatch diagnostic for `if 1`, got none"
    );
    assert!(
        output.diagnostics.iter().any(|d| d.code == "E0001"),
        "expected E0001 type mismatch, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn fallback_unification_catches_literal_type_mismatch() {
    // Returning Int where (String is expected — the fallback unification)
    // should catch this even for literal expressions.
    let output = check("fn f() -> String { 42 }", "test.ky");
    assert!(
        !output.diagnostics.is_empty(),
        "expected a type mismatch diagnostic, got none"
    );
    assert!(
        output.diagnostics.iter().any(|d| d.code == "E0001"),
        "expected E0001 type mismatch, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn symbol_graph_local_lambda_not_in_function_calls() {
    // A local lambda `f` should not produce a dangling fn::f call edge.
    let src = "fn main() -> Int {\n  let f = fn(x: Int, y: Int) => x\n  f(1, 2)\n}";
    let output = check(src, "test.ky");
    let main_fn = output
        .symbol_graph
        .functions
        .iter()
        .find(|f| f.name == "main")
        .expect("should have main function");
    assert!(
        !main_fn.calls.iter().any(|c| c.contains("f")),
        "local lambda call should not appear as fn::f, got: {:?}",
        main_fn.calls
    );
}

#[test]
fn unresolved_return_type_emits_diagnostic() {
    let output = check("fn main() -> Foo { 1 }", "test.ky");
    assert!(
        output.diagnostics.iter().any(|d| d.code == "E0012"
            && d.message.contains("unresolved type")
            && d.message.contains("Foo")),
        "expected E0012 unresolved type for `Foo`, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn unresolved_param_type_emits_diagnostic() {
    let output = check("fn main(x: Bar) -> Int { 1 }", "test.ky");
    assert!(
        output.diagnostics.iter().any(|d| d.code == "E0012"
            && d.message.contains("unresolved type")
            && d.message.contains("Bar")),
        "expected E0012 unresolved type for `Bar`, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn overflowing_int_literal_emits_diagnostic() {
    // i64::MAX + 1 = 9223372036854775808 should produce a diagnostic.
    let output = check("fn main() -> Int { 9223372036854775808 }", "test.ky");
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("overflow")
                || d.message.contains("out of range")
                || d.message.contains("invalid")),
        "expected a diagnostic for overflowing int literal, got: {:?}",
        output.diagnostics
    );
}

// ── Symbol graph pre-shadow call edges (#162) ──────────────────────

#[test]
fn symbol_graph_pre_shadow_call_edge_preserved() {
    // Bug test: call to top-level `foo()` before a same-named local should
    // still appear as a call edge.
    let src = r#"
fn foo() -> Int { 1 }
fn main() -> Int {
  foo()
  let foo = fn() => 2
  foo()
}
"#;
    let output = check(src, "test.ky");
    let main_fn = output
        .symbol_graph
        .functions
        .iter()
        .find(|f| f.name == "main")
        .expect("should have main function");
    assert!(
        main_fn.calls.iter().any(|c| c.contains("foo")),
        "pre-shadow call to top-level foo should appear in call edges, got: {:?}",
        main_fn.calls
    );
}

#[test]
fn symbol_graph_post_shadow_call_not_in_edges() {
    // Guard test: call to local `foo` after shadowing should NOT appear
    // as a top-level call edge.
    let src = r#"
fn foo() -> Int { 1 }
fn main() -> Int {
  let foo = fn() => 2
  foo()
}
"#;
    let output = check(src, "test.ky");
    let main_fn = output
        .symbol_graph
        .functions
        .iter()
        .find(|f| f.name == "main")
        .expect("should have main function");
    assert!(
        !main_fn.calls.iter().any(|c| c.contains("foo")),
        "post-shadow local call should not appear as top-level call edge, got: {:?}",
        main_fn.calls
    );
}

#[test]
fn symbol_graph_param_shadow_no_call_edge() {
    // Edge case: param with same name as function always shadows.
    let src = r#"
fn foo() -> Int { 1 }
fn main(foo: Int) -> Int {
  foo
}
"#;
    let output = check(src, "test.ky");
    let main_fn = output
        .symbol_graph
        .functions
        .iter()
        .find(|f| f.name == "main")
        .expect("should have main function");
    assert!(
        !main_fn.calls.iter().any(|c| c.contains("foo")),
        "param-shadowed name should not appear as call edge, got: {:?}",
        main_fn.calls
    );
}

#[test]
fn symbol_graph_lambda_param_shadow_no_call_edge() {
    // Bug test (#163): lambda param `foo` should shadow top-level `fn foo`,
    // so `foo()` inside the lambda should not produce a call edge.
    let src = r#"
fn foo() -> Int { 1 }
fn main() -> Int {
  let g = fn(foo) => foo()
  g(fn() => 2)
}
"#;
    let output = check(src, "test.ky");
    let main_fn = output
        .symbol_graph
        .functions
        .iter()
        .find(|f| f.name == "main")
        .expect("should have main function");
    assert!(
        !main_fn.calls.iter().any(|c| c.contains("foo")),
        "lambda-param-shadowed call should not appear as call edge, got: {:?}",
        main_fn.calls
    );
}

#[test]
fn symbol_graph_direct_call_still_recorded() {
    // Guard test: a direct (non-lambda) call to a top-level fn should
    // still produce a call edge.
    let src = r#"
fn foo() -> Int { 1 }
fn main() -> Int {
  foo()
}
"#;
    let output = check(src, "test.ky");
    let main_fn = output
        .symbol_graph
        .functions
        .iter()
        .find(|f| f.name == "main")
        .expect("should have main function");
    assert!(
        main_fn.calls.iter().any(|c| c.contains("foo")),
        "direct call to top-level fn should produce a call edge, got: {:?}",
        main_fn.calls
    );
}

#[test]
fn symbol_graph_lambda_shadow_does_not_hide_outer_direct_call() {
    // Mixed case: lambda param shadows `foo` locally, but the outer direct
    // `foo()` call should still be recorded exactly once.
    let src = r#"
fn foo() -> Int { 1 }
fn main() -> Int {
  foo()
  let g = fn(foo) => foo()
  g(fn() => 2)
}
"#;
    let output = check(src, "test.ky");
    let main_fn = output
        .symbol_graph
        .functions
        .iter()
        .find(|f| f.name == "main")
        .expect("should have main function");
    let foo_edges = main_fn
        .calls
        .iter()
        .filter(|c| c.as_str() == "fn::foo")
        .count();
    assert_eq!(
        foo_edges, 1,
        "expected exactly one top-level fn::foo edge (outer direct call only), got: {:?}",
        main_fn.calls
    );
}

#[test]
fn symbol_graph_nested_block_shadow_respects_lexical_scope() {
    // Nested block local `foo` should shadow only within the block.
    // We expect one top-level edge from the final outer `foo()` call.
    let src = r#"
fn foo() -> Int { 1 }
fn main() -> Int {
  {
    let foo = fn() => 2
    foo()
  }
  foo()
}
"#;
    let output = check(src, "test.ky");
    let main_fn = output
        .symbol_graph
        .functions
        .iter()
        .find(|f| f.name == "main")
        .expect("should have main function");
    let foo_edges = main_fn
        .calls
        .iter()
        .filter(|c| c.as_str() == "fn::foo")
        .count();
    assert_eq!(
        foo_edges, 1,
        "expected exactly one top-level fn::foo edge (outer lexical scope), got: {:?}",
        main_fn.calls
    );
}

#[test]
fn project_symbol_graph_pre_post_shadow_with_imported_function() {
    // Project mode: imported `math::add` should be recorded before local shadowing,
    // and local post-shadow call should not create an extra call edge.
    let output = check_project_from_files(&[
        (
            "main.ky",
            "import math\nfn caller() -> Int {\n  add(1, 2)\n  let add = fn(x: Int, y: Int) => x\n  add(1, 2)\n}\n",
        ),
        ("math.ky", "pub fn add(x: Int, y: Int) -> Int { x + y }\n"),
    ]);
    let caller = output
        .symbol_graph
        .functions
        .iter()
        .find(|f| f.name == "caller")
        .expect("should have caller function");
    let imported_edges = caller
        .calls
        .iter()
        .filter(|c| c.as_str() == "fn::math::add")
        .count();
    assert_eq!(
        imported_edges, 1,
        "expected exactly one fn::math::add edge (pre-shadow imported call), got: {:?}",
        caller.calls
    );
}

// ── Symbol graph call-edge invariants harness (#171) ───────────────

#[test]
fn symbol_graph_call_edge_invariants_single_file_shadowing_matrix() {
    let src = r#"
fn foo() -> Int { 1 }
fn pre_shadow() -> Int {
  foo()
  let foo = fn() => 2
  foo()
}
fn lambda_param_shadow() -> Int {
  let g = fn(foo) => foo()
  g(fn() => 2)
}
fn nested_block_shadow() -> Int {
  {
    let foo = fn() => 2
    foo()
  }
  foo()
}
"#;
    let output = check(src, "test.ky");
    assert_call_edges_target_existing_functions(&output);
    assert_no_duplicate_call_edges_per_caller(&output);

    assert_eq!(
        sorted_calls(find_function_by_id(&output, "fn::pre_shadow")),
        vec!["fn::foo".to_string()],
        "pre-shadow function should keep exactly one top-level foo edge"
    );
    assert_eq!(
        sorted_calls(find_function_by_id(&output, "fn::lambda_param_shadow")),
        Vec::<String>::new(),
        "lambda param shadow should not produce top-level foo edge"
    );
    assert_eq!(
        sorted_calls(find_function_by_id(&output, "fn::nested_block_shadow")),
        vec!["fn::foo".to_string()],
        "nested block local shadow should not affect outer foo attribution"
    );
}

#[test]
fn symbol_graph_call_edge_invariants_project_import_shadow_matrix() {
    let output = check_project_from_files(&[
        (
            "main.ky",
            "import math\n\
             fn imported_shadow() -> Int {\n\
               add(1, 2)\n\
               let add = fn(x, y) => x\n\
               add(1, 2)\n\
             }\n\
             fn imported_lambda_param_shadow() -> Int {\n\
               let g = fn(add) => add(1, 2)\n\
               g(fn(x, y) => x)\n\
             }\n\
             fn imported_nested_block_shadow() -> Int {\n\
               {\n\
                 let add = fn(x, y) => x\n\
                 add(1, 2)\n\
               }\n\
               add(1, 2)\n\
             }\n",
        ),
        ("math.ky", "pub fn add(x: Int, y: Int) -> Int { x + y }\n"),
    ]);
    assert_call_edges_target_existing_functions(&output);
    assert_no_duplicate_call_edges_per_caller(&output);

    assert_eq!(
        sorted_calls(find_function_by_id(&output, "fn::imported_shadow")),
        vec!["fn::math::add".to_string()],
        "imported call before local shadow should be attributed once"
    );
    assert_eq!(
        sorted_calls(find_function_by_id(
            &output,
            "fn::imported_lambda_param_shadow"
        )),
        Vec::<String>::new(),
        "lambda param shadow should not emit imported add edge"
    );
    assert_eq!(
        sorted_calls(find_function_by_id(
            &output,
            "fn::imported_nested_block_shadow"
        )),
        vec!["fn::math::add".to_string()],
        "nested block local shadow should preserve outer imported edge attribution"
    );
}

#[test]
fn symbol_graph_call_edges_stable_under_local_only_edits_single_file() {
    let original = r#"
fn foo() -> Int { 1 }
fn main() -> Int {
  let local = fn() => 2
  local()
  foo()
}
"#;
    let transformed = r#"
fn foo() -> Int { 1 }
fn main() -> Int {
  let renamed_local = fn() => 2
  renamed_local()
  foo()
}
"#;
    let original_output = check(original, "test.ky");
    let transformed_output = check(transformed, "test.ky");
    assert_call_edges_target_existing_functions(&original_output);
    assert_call_edges_target_existing_functions(&transformed_output);
    assert_no_duplicate_call_edges_per_caller(&original_output);
    assert_no_duplicate_call_edges_per_caller(&transformed_output);

    assert_eq!(
        call_edges_by_caller_id(&original_output),
        call_edges_by_caller_id(&transformed_output),
        "local-only rename should not change call-edge attribution"
    );
}

#[test]
fn symbol_graph_call_edges_stable_under_local_only_edits_project_mode() {
    let (dir_a, main_a) = write_project(&[
        (
            "main.ky",
            "import math\n\
             fn main() -> Int {\n\
               let local = fn() => 0\n\
               local()\n\
               add(1, 2)\n\
             }\n",
        ),
        ("math.ky", "pub fn add(x: Int, y: Int) -> Int { x + y }\n"),
    ]);
    let output_a = check_project(&main_a);

    let (dir_b, main_b) = write_project(&[
        (
            "main.ky",
            "import math\n\
             fn main() -> Int {\n\
               let renamed_local = fn() => 0\n\
               renamed_local()\n\
               add(1, 2)\n\
             }\n",
        ),
        ("math.ky", "pub fn add(x: Int, y: Int) -> Int { x + y }\n"),
    ]);
    let output_b = check_project(&main_b);

    assert_call_edges_target_existing_functions(&output_a);
    assert_call_edges_target_existing_functions(&output_b);
    assert_no_duplicate_call_edges_per_caller(&output_a);
    assert_no_duplicate_call_edges_per_caller(&output_b);
    assert_eq!(
        call_edges_by_caller_id(&output_a),
        call_edges_by_caller_id(&output_b),
        "project local-only rename should not change call-edge attribution"
    );

    drop(dir_a);
    drop(dir_b);
}

// ── Metamorphic shadowing tests (#172) ─────────────────────────────

#[test]
fn metamorphic_alpha_rename_local_binder_preserves_edges_and_diagnostics() {
    let original = r#"
fn foo() -> Int { 1 }
fn main() -> Int {
  foo()
  let local = fn() => 2
  local()
}
"#;
    let transformed = r#"
fn foo() -> Int { 1 }
fn main() -> Int {
  foo()
  let closure_local = fn() => 2
  closure_local()
}
"#;
    assert_metamorphic_equivalent(original, transformed);
}

#[test]
fn metamorphic_binding_order_change_toggles_shadow_call_attribution() {
    let pre_shadow = r#"
fn foo() -> Int { 1 }
fn main() -> Int {
  foo()
  let foo = fn() => 2
  foo()
}
"#;
    let post_shadow_only = r#"
fn foo() -> Int { 1 }
fn main() -> Int {
  let foo = fn() => 2
  foo()
}
"#;
    let pre_output = check(pre_shadow, "test.ky");
    let post_output = check(post_shadow_only, "test.ky");

    let pre_main = find_function_by_id(&pre_output, "fn::main");
    let post_main = find_function_by_id(&post_output, "fn::main");
    let pre_foo_edges = pre_main
        .calls
        .iter()
        .filter(|c| c.as_str() == "fn::foo")
        .count();
    let post_foo_edges = post_main
        .calls
        .iter()
        .filter(|c| c.as_str() == "fn::foo")
        .count();

    assert_eq!(
        pre_foo_edges, 1,
        "pre-shadow source should have one top-level foo edge\nsource:\n{}\nmain calls: {:?}",
        pre_shadow, pre_main.calls
    );
    assert_eq!(
        post_foo_edges, 0,
        "post-shadow-only source should have zero top-level foo edges\nsource:\n{}\nmain calls: {:?}",
        post_shadow_only, post_main.calls
    );
}

#[test]
fn metamorphic_nested_block_introduction_preserves_outer_call_edges() {
    let original = r#"
fn foo() -> Int { 1 }
fn main() -> Int {
  foo()
}
"#;
    let transformed = r#"
fn foo() -> Int { 1 }
fn main() -> Int {
  {
    let foo = fn() => 2
    foo()
  }
  foo()
}
"#;
    assert_metamorphic_equivalent(original, transformed);
}

#[test]
fn metamorphic_lambda_param_rename_preserves_edges_and_diagnostics() {
    let original = r#"
fn foo() -> Int { 1 }
fn main() -> Int {
  let g = fn(foo) => foo()
  g(fn() => 2)
}
"#;
    let transformed = r#"
fn foo() -> Int { 1 }
fn main() -> Int {
  let g = fn(callback) => callback()
  g(fn() => 2)
}
"#;
    assert_metamorphic_equivalent(original, transformed);
}

// ── Project-mode metamorphic tests (#174) ──────────────────────────

#[test]
fn project_metamorphic_local_alpha_rename_preserves_edges_and_diagnostics() {
    let original = [
        (
            "main.ky",
            "import math\n\
             fn main() -> Int {\n\
               let local = fn() => 0\n\
               local()\n\
               add(1, 2)\n\
             }\n",
        ),
        ("math.ky", "pub fn add(x: Int, y: Int) -> Int { x + y }\n"),
    ];
    let transformed = [
        (
            "main.ky",
            "import math\n\
             fn main() -> Int {\n\
               let renamed_local = fn() => 0\n\
               renamed_local()\n\
               add(1, 2)\n\
             }\n",
        ),
        ("math.ky", "pub fn add(x: Int, y: Int) -> Int { x + y }\n"),
    ];
    assert_project_metamorphic_equivalent(&original, &transformed);
}

#[test]
fn project_metamorphic_local_alpha_rename_preserves_edges_in_entry_and_imported_modules() {
    let original = [
        (
            "main.ky",
            "import math\n\
             fn local_add(x: Int) -> Int {\n\
               let tmp = x + 1\n\
               tmp\n\
             }\n\
             fn main() -> Int {\n\
               let n = 1\n\
               local_add(n) + inc(n)\n\
             }\n",
        ),
        (
            "math.ky",
            "pub fn inc(v: Int) -> Int {\n\
               let inner = v + 1\n\
               inner\n\
             }\n",
        ),
    ];
    let transformed = [
        (
            "main.ky",
            "import math\n\
             fn local_add(x: Int) -> Int {\n\
               let renamed_tmp = x + 1\n\
               renamed_tmp\n\
             }\n\
             fn main() -> Int {\n\
               let renamed_n = 1\n\
               local_add(renamed_n) + inc(renamed_n)\n\
             }\n",
        ),
        (
            "math.ky",
            "pub fn inc(v: Int) -> Int {\n\
               let renamed_inner = v + 1\n\
               renamed_inner\n\
             }\n",
        ),
    ];
    assert_project_metamorphic_equivalent(&original, &transformed);
}

#[test]
fn project_metamorphic_nested_block_shadow_preserves_outer_import_attribution() {
    let original = [
        ("main.ky", "import math\nfn main() -> Int { add(1, 2) }\n"),
        ("math.ky", "pub fn add(x: Int, y: Int) -> Int { x + y }\n"),
    ];
    let transformed = [
        (
            "main.ky",
            "import math\n\
             fn main() -> Int {\n\
               {\n\
                 let add = fn(x, y) => x\n\
                 add(1, 2)\n\
               }\n\
               add(1, 2)\n\
             }\n",
        ),
        ("math.ky", "pub fn add(x: Int, y: Int) -> Int { x + y }\n"),
    ];
    assert_project_metamorphic_equivalent(&original, &transformed);
}

#[test]
fn project_metamorphic_lambda_param_rename_preserves_edges_and_diagnostics() {
    let original = [
        (
            "main.ky",
            "import math\n\
             fn main() -> Int {\n\
               add(1, 2)\n\
               let g = fn(add) => add(1, 2)\n\
               g(fn(x, y) => x)\n\
             }\n",
        ),
        ("math.ky", "pub fn add(x: Int, y: Int) -> Int { x + y }\n"),
    ];
    let transformed = [
        (
            "main.ky",
            "import math\n\
             fn main() -> Int {\n\
               add(1, 2)\n\
               let g = fn(callback) => callback(1, 2)\n\
               g(fn(x, y) => x)\n\
             }\n",
        ),
        ("math.ky", "pub fn add(x: Int, y: Int) -> Int { x + y }\n"),
    ];
    assert_project_metamorphic_equivalent(&original, &transformed);
}

// ── Diagnostic-delta metamorphic tests (#175) ──────────────────────

#[test]
fn diagnostic_delta_duplicate_pattern_binding_adds_one_e0102() {
    let original = r#"
type Pair = Pair(Int, Int)
fn main() -> Int {
  let Pair(a, b) = Pair(1, 2)
  a + b
}
"#;
    let transformed = r#"
type Pair = Pair(Int, Int)
fn main() -> Int {
  let Pair(x, x) = Pair(1, 2)
  x
}
"#;
    assert_diagnostic_code_delta(original, transformed, &[("E0102", 1)]);
}

#[test]
fn diagnostic_delta_unresolved_return_type_adds_one_e0012() {
    let original = "fn main() -> Int { 1 }";
    let transformed = "fn main() -> MissingType { 1 }";
    assert_diagnostic_code_delta(original, transformed, &[("E0012", 1)]);
}

#[test]
fn diagnostic_delta_type_mismatch_adds_one_e0001() {
    let original = "fn main() -> Int { 1 }";
    let transformed = r#"fn main() -> Int { "x" }"#;
    assert_diagnostic_code_delta(original, transformed, &[("E0001", 1)]);
}

// ── Set element type diagnostics (E0026) ───────────────────────────

#[test]
fn check_set_invalid_element_type_reports_e0026() {
    let output = check(
        r#"fn main() -> Int {
            let s = Set.new().insert(3.14)
            s.len()
        }"#,
        "test.ky",
    );

    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code == "E0026" && d.message.contains("set element")),
        "expected E0026 set element diagnostic, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn check_set_valid_element_types_have_no_set_diagnostic() {
    let output = check(
        r#"fn helper() -> Bool {
            let i = Set.new().insert(1)
            let s = Set.new().insert("x")
            let c = Set.new().insert('z')
            let b = Set.new().insert(true)
            i.len() == 1 && s.len() == 1 && c.len() == 1 && b.len() == 1
        }"#,
        "test.ky",
    );

    assert!(
        output.diagnostics.iter().all(|d| d.code != "E0026"),
        "expected no E0026 diagnostics, got: {:?}",
        output.diagnostics
    );
}

// ── Map key type diagnostics (E0024) ────────────────────────────────

#[test]
fn check_map_invalid_key_type_reports_e0024() {
    let output = check(
        r#"fn main() -> Int {
            let m = Map.new().insert(3.14, 1)
            m.len()
        }"#,
        "test.ky",
    );

    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code == "E0024" && d.message.contains("map key")),
        "expected E0024 map key diagnostic, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn check_map_valid_key_types_have_no_map_key_diagnostic() {
    let output = check(
        r#"fn helper() -> Bool {
            let i = Map.new().insert(1, "x")
            let s = Map.new().insert("k", 1)
            let c = Map.new().insert('a', 1)
            let b = Map.new().insert(true, 1)
            i.len() == 1 && s.len() == 1 && c.len() == 1 && b.len() == 1
        }"#,
        "test.ky",
    );

    assert!(
        output.diagnostics.iter().all(|d| d.code != "E0024"),
        "expected no E0024 diagnostics, got: {:?}",
        output.diagnostics
    );
}

// ── List binary_search element type diagnostics (E0025) ─────────────

#[test]
fn check_list_binary_search_unsortable_element_reports_e0025() {
    let output = check(
        r#"fn main() -> Int {
            let needle = List.new().push(1)
            let xs = List.new().push(needle)
            xs.binary_search(needle)
        }"#,
        "test.ky",
    );

    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code == "E0025" && d.message.contains("cannot be sorted")),
        "expected E0025 unsortable element diagnostic, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn check_list_binary_search_sortable_elements_have_no_e0025() {
    let output = check(
        r#"fn main() -> Int {
            let xs = List.new().push(1).push(3).push(5)
            xs.binary_search(3)
        }"#,
        "test.ky",
    );

    assert!(
        output.diagnostics.iter().all(|d| d.code != "E0025"),
        "expected no E0025 diagnostics, got: {:?}",
        output.diagnostics
    );
}

// ── Iteration ergonomics API checks (#259) ─────────────────────────

#[test]
fn check_iteration_ergonomics_canonical_surface_has_no_diagnostics() {
    assert_check_no_diagnostics(
        r#"fn main() -> Bool {
            let xs = Seq.range(0, 5)
            let e = xs.enumerate().to_list()
            let z = xs.zip(List.new().push(10).push(20).seq()).to_list()
            let c = xs.chunks(2).to_list()
            let w = xs.windows(3).to_list()
            e[0].index == 0 && e[0].value == 0 && z.len() == 2 && c.len() == 3 && w.len() == 3
        }"#,
        "iteration canonical surface",
    );
}

#[test]
fn check_iteration_ergonomics_chains_from_map_set_string_have_no_diagnostics() {
    assert_check_no_diagnostics(
        r#"fn main() -> Bool {
            let m = Map.new().insert("x", 1).insert("y", 2)
            let km = m.keys().enumerate().to_list()
            let map_ok = km.len() == 2 && km[0].index == 0

            let s = Set.new().insert("a").insert("b").insert("c")
            let sc = s.values().chunks(2).to_list()
            let set_ok = sc.len() == 2 && sc[1].len() == 1

            let p = "abc".chars().zip(List.new().push(1).push(2).seq()).to_list()
            let str_ok = p.len() == 2 && p[0].left == 'a' && p[1].right == 2

            map_ok && set_ok && str_ok
        }"#,
        "iteration chaining from map/set/string",
    );
}

#[test]
fn check_seq_any_all_find_canonical_surface_has_no_diagnostics() {
    assert_check_no_diagnostics(
        r#"fn main() -> Int {
            let xs = Seq.range(0, 6)
            let a = xs.any(fn(n: Int) => n == 4)
            let b = xs.all(fn(n: Int) => n < 6)
            let c = xs.find(fn(n: Int) => n % 2 == 0).map_or(-1, fn(n: Int) => n)
            let d = Seq.range(0, 0).find(fn(_n: Int) => true).unwrap_or(-7)
            if (a && b && c == 0 && d == -7) { 1 } else { 0 }
        }"#,
        "seq any/all/find canonical surface",
    );
}

#[test]
fn check_seq_scan_unfold_int_pow_canonical_surface_has_no_diagnostics() {
    assert_check_no_diagnostics(
        r#"fn main() -> Int {
            let a = Seq.range(1, 4).scan(0, fn(acc: Int, n: Int) => acc + n).to_list()
            let b = Seq.unfold(0, fn(state: Int) =>
                if (state < 3) {
                    Some({ value: state + 1, state: state + 1 })
                } else {
                    None
                }
            ).to_list()
            a.len() + b.len() + 2.pow(10)
        }"#,
        "seq scan/unfold + int.pow canonical surface",
    );
}

#[test]
fn check_seq_unfold_accepts_named_record_alias_payload() {
    assert_check_no_diagnostics(
        r#"type PickStep = { value: Int, state: Int }

        fn main() -> Int {
            Seq.unfold(0, fn(state: Int) =>
                if (state < 3) {
                    Some(PickStep { value: state + 1, state: state + 1 })
                } else {
                    None
                }
            ).count()
        }"#,
        "seq unfold accepts named record payload alias",
    );
}

#[test]
fn check_non_canonical_free_scan_unfold_pow_int_report_unresolved_name() {
    let output = check(
        r#"fn main() -> Int {
            let a = scan(Seq.range(0, 3), 0, fn(acc: Int, n: Int) => acc + n)
            let b = unfold(0, fn(state: Int) => None)
            let c = pow_int(2, 10)
            a.count() + b.count() + c
        }"#,
        "test.ky",
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("unresolved name")),
        "expected unresolved-name diagnostics for free scan/unfold/pow_int, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn check_non_canonical_free_any_all_find_functions_report_unresolved_name() {
    let output = check(
        r#"fn main() -> Int {
            let a = any(Seq.range(0, 3), fn(n: Int) => n == 1)
            let b = all(Seq.range(0, 3), fn(n: Int) => n < 3)
            let c = find(Seq.range(0, 3), fn(n: Int) => n == 1)
            if (a && b) { c.unwrap_or(0) } else { 0 }
        }"#,
        "test.ky",
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("unresolved name")),
        "expected unresolved-name diagnostics for free any/all/find, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn check_seq_any_all_find_wrong_predicate_type_reports_type_mismatch() {
    let output = check(
        r#"fn main() -> Int {
            let a = Seq.range(0, 3).any(fn(n: Int) => n)
            let b = Seq.range(0, 3).all(fn(n: Int) => n + 1)
            let c = Seq.range(0, 3).find(fn(n: Int) => n * 2)
            if (a || b) { c.unwrap_or(0) } else { 0 }
        }"#,
        "test.ky",
    );

    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code == "E0001" && d.message.contains("type mismatch")),
        "expected E0001 type mismatch for seq any/all/find predicate type, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn check_result_ergonomics_canonical_surface_has_no_diagnostics() {
    assert_check_no_diagnostics(
        r#"fn main() -> Int {
            let a = "42".parse_int().unwrap_or(0)
            let b = "oops".parse_int().map_or(7, fn(n: Int) => n + 1)
            a + b
        }"#,
        "result ergonomics canonical surface",
    );
}

#[test]
fn check_option_result_combinator_parity_canonical_surface_has_no_diagnostics() {
    assert_check_no_diagnostics(
        r#"fn main() -> Int {
            let o0 = List.new().head().unwrap_or(1)
            let o1 = List.new().push(41).head().map_or(0, fn(n: Int) => n + 1)
            let o2 = List.new().push(41).head().map(fn(n: Int) => n + 1).unwrap_or(0)
            let o3 = List.new().push(41).head().and_then(fn(n: Int) => Some(n + 1)).unwrap_or(0)
            let r1 = "41".parse_int().map(fn(n: Int) => n + 1).unwrap_or(0)
            let r2 = "41".parse_int().and_then(fn(n: Int) => Ok(n + 1)).unwrap_or(0)
            let r3 = match ("oops".parse_int().map_err(fn(_e: ParseError) => 7)) {
                Ok(n) => n
                Err(e) => e
            }
            o0 + o1 + o2 + o3 + r1 + r2 + r3
        }"#,
        "option/result combinator parity canonical surface",
    );
}

#[test]
fn check_option_and_then_wrong_mapper_result_reports_type_mismatch() {
    let output = check(
        r#"fn main() -> Int {
            List.new().push(1).head().and_then(fn(n: Int) => n + 1).unwrap_or(0)
        }"#,
        "test.ky",
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code == "E0001" && d.message.contains("type mismatch")),
        "expected E0001 type mismatch for option and_then mapper return type, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn check_result_map_err_wrong_mapper_result_reports_type_mismatch() {
    let output = check(
        r#"fn main() -> Int {
            match ("oops".parse_int().map_err(fn(_e: ParseError) => "bad")) {
                Ok(n) => n
                Err(e) => e
            }
        }"#,
        "test.ky",
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code == "E0001" && d.message.contains("type mismatch")),
        "expected E0001 type mismatch for result map_err mapper result type, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn check_result_map_or_wrong_mapper_type_reports_type_mismatch() {
    let output = check(
        r#"fn main() -> Int {
            "42".parse_int().map_or(0, fn(n: Int) => "x")
        }"#,
        "test.ky",
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code == "E0001" && d.message.contains("type mismatch")),
        "expected E0001 type mismatch for result map_or mapper result type, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn check_non_canonical_free_range_function_reports_unresolved_name() {
    let output = check("fn main() -> Int { range(0, 3).len() }", "test.ky");
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("unresolved name")),
        "expected unresolved-name diagnostic for free `range`, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn check_seq_surface_canonical_has_no_diagnostics() {
    assert_check_no_diagnostics(
        r#"fn main() -> Int {
            let xs = Seq.range(0, 5)
                .map(fn(n: Int) => n + 1)
                .filter(fn(n: Int) => n > 2)
            let a = xs.count()
            let b = xs.to_list().len()
            let c = List.new().push(1).push(2).seq().count()
            let d = Map.new().insert("a", 1).insert("b", 2).keys().count()
            let e = Set.new().insert("x").insert("y").values().count()
            let f = "a,b,c".split(",").count()
            let g = "x\ny".lines().count()
            let h = "abc".chars().count()
            a + b + c + d + e + f + g + h
        }"#,
        "seq canonical surface",
    );
}

#[test]
fn check_removed_list_traversal_surface_reports_diagnostics() {
    let output = check(
        r#"fn main() -> Int {
            let a = List.range(0, 5).len()
            let b = List.new().push(1).map(fn(n: Int) => n + 1).len()
            a + b
        }"#,
        "test.ky",
    );

    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("no method") || d.message.contains("unresolved name")),
        "expected removed-surface diagnostics, got: {:?}",
        output.diagnostics
    );
}

// ── math.gcd/math.lcm type diagnostics (E0001) ──────────────────────

#[test]
fn check_math_gcd_invalid_argument_type_reports_e0001() {
    let output = check(
        r#"import math
fn main() -> Int {
    math.gcd("x", 1)
}"#,
        "test.ky",
    );

    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code == "E0001" && d.message.contains("expected `Int`")),
        "expected E0001 type mismatch diagnostic for math.gcd, got: {:?}",
        output.diagnostics
    );
}

#[test]
fn check_math_lcm_invalid_argument_type_reports_e0001() {
    let output = check(
        r#"import math
fn main() -> Int {
    math.lcm(1, "x")
}"#,
        "test.ky",
    );

    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.code == "E0001" && d.message.contains("expected `Int`")),
        "expected E0001 type mismatch diagnostic for math.lcm, got: {:?}",
        output.diagnostics
    );
}

// ── Modulo, logical AND, logical OR operator type-check tests ───────

#[test]
fn check_modulo_and_logical_ops_accept_valid_cases() {
    let cases = [
        ("Int % Int", "fn main() -> Int { 10 % 3 }"),
        ("Float % Float", "fn main() -> Float { 5.5 % 2.0 }"),
        ("Bool && Bool", "fn main() -> Bool { true && false }"),
        ("Bool || Bool", "fn main() -> Bool { true || false }"),
        (
            "comparison && comparison",
            "fn main() -> Bool { 1 > 0 && 2 > 1 }",
        ),
        (
            "modulo+equality+and",
            "fn main() -> Bool { 10 % 2 == 0 && 9 % 3 == 0 }",
        ),
    ];
    for (context, src) in cases {
        assert_check_no_diagnostics(src, context);
    }
}

#[test]
fn check_modulo_and_logical_ops_reject_invalid_cases() {
    let cases = [
        ("String % String", r#"fn main() -> String { "a" % "b" }"#),
        ("Int && Bool", "fn main() -> Bool { 1 && true }"),
        ("Bool || Int", "fn main() -> Bool { true || 1 }"),
    ];
    for (context, src) in cases {
        assert_check_has_diagnostics(src, context);
    }
}

// ── Bitwise operator type checking ─────────────────────────────────

#[test]
fn check_bitwise_ops_accept_valid_cases() {
    let cases = [
        ("Int & Int", "fn main() -> Int { 3 & 1 }"),
        ("Int | Int", "fn main() -> Int { 3 | 1 }"),
        ("Int ^ Int", "fn main() -> Int { 3 ^ 1 }"),
        ("Int << Int", "fn main() -> Int { 1 << 3 }"),
        ("Int >> Int", "fn main() -> Int { 8 >> 2 }"),
        ("~Int", "fn main() -> Int { ~42 }"),
        (
            "combined bitwise expression",
            "fn main() -> Bool { (255 & 15) == 15 && (1 << 3) == 8 }",
        ),
    ];
    for (context, src) in cases {
        assert_check_no_diagnostics(src, context);
    }
}

#[test]
fn check_bitwise_ops_reject_invalid_cases() {
    let cases = [
        ("Float & Float", "fn main() -> Float { 1.0 & 2.0 }"),
        ("Bool | Bool", "fn main() -> Bool { true | false }"),
        ("~Bool", "fn main() -> Bool { ~true }"),
        ("Float << Int", "fn main() -> Float { 1.0 << 2 }"),
    ];
    for (context, src) in cases {
        assert_check_has_diagnostics(src, context);
    }
}
