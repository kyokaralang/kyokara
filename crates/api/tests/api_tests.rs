//! End-to-end API tests: source → `check()` → verify structured output.

use kyokara_api::check;

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
    // Return a String where Int is expected.
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
fn check_effect_violation_code() {
    let src = r#"
        cap Console {
            fn print(s: String) -> Unit
        }
        fn effectful() -> Unit with Console { print("hi") }
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
        cap IO {
            fn read() -> String
        }
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
    let src = "type Color = | Red | Green | Blue
        fn id(x: Int) -> Int { x }";
    let output = check(src, "test.ky");
    assert_eq!(output.symbol_graph.types.len(), 1);
    let ty = &output.symbol_graph.types[0];
    assert_eq!(ty.name, "Color");
    assert_eq!(ty.kind, "adt");
    let variant_names: Vec<&str> = ty.variants.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(variant_names, vec!["Red", "Green", "Blue"]);
}

#[test]
fn symbol_graph_contains_capabilities() {
    let src = r#"
        cap IO {
            fn read() -> String
            fn write(s: String) -> Unit
        }
        fn noop() -> Unit { () }
    "#;
    let output = check(src, "test.ky");
    assert_eq!(output.symbol_graph.capabilities.len(), 1);
    let cap = &output.symbol_graph.capabilities[0];
    assert_eq!(cap.name, "IO");
    assert!(
        cap.functions.contains(&"read".to_string()),
        "missing 'read' in {:?}",
        cap.functions
    );
    assert!(
        cap.functions.contains(&"write".to_string()),
        "missing 'write' in {:?}",
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
        caller_node.calls.contains(&"callee".to_string()),
        "expected caller to call callee, got: {:?}",
        caller_node.calls
    );
}

#[test]
fn symbol_graph_effect_annotations() {
    let src = r#"
        cap IO {
            fn read() -> String
        }
        fn effectful() -> String with IO { read() }
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
    let src = "type Color = | Red | Green | Blue
        fn describe(c: Color) -> Int {
            match c {
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
        cap Console {
            fn print(s: String) -> Unit
        }
        fn effectful() -> Unit with Console { print("hi") }
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
    let src = "type Color = | Red | Green | Blue
        fn describe(c: Color) -> Int {
            match c {
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
        cap Console {
            fn print(s: String) -> Unit
        }
        fn effectful() -> Unit with Console { print("hi") }
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
