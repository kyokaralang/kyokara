//! End-to-end API tests: source → `check()` → verify structured output.

use kyokara_api::{check, check_project};

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
    // 5 types: Color (user-defined) + Option + Result + List + Map (builtins)
    assert_eq!(output.symbol_graph.types.len(), 5);
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
        cap.functions.contains(&"cap::IO::read".to_string()),
        "missing 'cap::IO::read' in {:?}",
        cap.functions
    );
    assert!(
        cap.functions.contains(&"cap::IO::write".to_string()),
        "missing 'cap::IO::write' in {:?}",
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
    let src = "type Color = | Red | Green\nfn id(x: Int) -> Int { x }";
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
    let src = "type Color = | Red | Green | Blue\nfn id(x: Int) -> Int { x }";
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
        cap IO {
            fn read() -> String
        }
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
        cap IO {
            fn read() -> String
            fn write(s: String) -> Unit
        }
        fn noop() -> Unit { () }
    "#;
    let output = check(src, "test.ky");
    let cap = &output.symbol_graph.capabilities[0];
    for f in &cap.functions {
        assert!(
            f.starts_with("cap::IO::"),
            "cap function ref should start with 'cap::IO::', got: {f}"
        );
    }
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
        type Color = | Red | Green | Blue
        cap IO {
            fn read() -> String
        }
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
    let src = "type Color = | Red | Green | Blue\nfn id(x: Int) -> Int { x }";
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

#[test]
fn project_symbol_ids_are_module_qualified() {
    let (_dir, main_path) = write_project(&[
        ("main.ky", "fn helper() -> Int { 1 }"),
        ("math.ky", "pub fn helper() -> Int { 2 }"),
    ]);
    let output = check_project(&main_path);
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
    let (_dir, main_path) = write_project(&[
        ("main.ky", "fn foo() -> Int { 1 }"),
        ("math.ky", "pub fn bar() -> Int { 2 }"),
    ]);
    let output = check_project(&main_path);
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
fn project_symbol_id_uniqueness() {
    let (_dir, main_path) = write_project(&[
        (
            "main.ky",
            "type Color = | Red | Green\ncap IO { fn read() -> String }\nfn foo() -> Int { 1 }",
        ),
        (
            "math.ky",
            "pub fn add(x: Int, y: Int) -> Int { x + y }\npub type Point = { x: Int, y: Int }",
        ),
    ]);
    let output = check_project(&main_path);

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
fn project_call_edges_use_qualified_ids() {
    let (_dir, main_path) = write_project(&[
        ("main.ky", "import math\nfn caller() -> Int { add(1, 2) }"),
        ("math.ky", "pub fn add(x: Int, y: Int) -> Int { x + y }"),
    ]);
    let output = check_project(&main_path);
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
fn project_root_module_uses_bare_ids() {
    let (_dir, main_path) = write_project(&[
        ("main.ky", "fn foo() -> Int { 1 }"),
        ("math.ky", "pub fn bar() -> Int { 2 }"),
    ]);
    let output = check_project(&main_path);
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
    let (_dir, main_path) = write_project(&[
        ("main.ky", "fn foo() -> Int { 1 }"),
        ("math.ky", "pub type Color = | Red | Green"),
    ]);
    let output = check_project(&main_path);
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
    let (_dir, main_path) = write_project(&[
        ("main.ky", "fn foo() -> Int { 1 }"),
        ("math.ky", "pub cap IO { fn read() -> String }"),
    ]);
    let output = check_project(&main_path);
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
        io.functions.contains(&"cap::math::IO::read".to_string()),
        "cap function ref should be module-qualified, got: {:?}",
        io.functions
    );
}

#[test]
fn project_builtin_type_ids_are_unqualified() {
    let (_dir, main_path) = write_project(&[
        ("main.ky", "fn foo() -> Int { 1 }"),
        ("math.ky", "pub fn bar() -> Int { 2 }"),
    ]);
    let output = check_project(&main_path);
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
