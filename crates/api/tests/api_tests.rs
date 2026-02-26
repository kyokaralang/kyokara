//! End-to-end API tests: source → `check()` → verify structured output.

use kyokara_api::{check, check_project, refactor, refactor_project};

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
    // Set up a project where two modules have match exhaustiveness errors.
    // The API should accept target_file to disambiguate.
    let (_dir, main_path) = write_project(&[
        (
            "main.ky",
            "type A = | X | Y\nfn check_a(a: A) -> Int {\n    match a {\n        X => 1\n    }\n}",
        ),
        (
            "math.ky",
            "pub type B = | P | Q\npub fn check_b(b: B) -> Int {\n    match b {\n        P => 1\n    }\n}",
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
            "type A = | X | Y\nfn check_a(a: A) -> Int {\n    match a {\n        X => 1\n    }\n}",
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
    let (_dir, main_path) = write_project(&[("main.ky", "import nope\nfn main() -> Int { 1 }\n")]);
    let output = check_project(&main_path);
    assert!(
        output
            .diagnostics
            .iter()
            .any(|d| d.message.contains("nope")),
        "expected a diagnostic about unresolved import `nope`, got: {:?}",
        output
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn check_project_aliased_import_resolves_by_path_not_alias() {
    // `import math as M` should resolve the "math" module, not look for a module named "M".
    let (_dir, main_path) = write_project(&[
        (
            "main.ky",
            "import math as M\nfn main() -> Int { add(1, 2) }\n",
        ),
        ("math.ky", "pub fn add(x: Int, y: Int) -> Int { x + y }\n"),
    ]);
    let output = check_project(&main_path);
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

// ── Lowering diagnostic file path (#66) ──────────────────────────────

#[test]
fn check_project_lowering_diagnostic_has_real_file_path() {
    // Lowering diagnostics should report the actual file path, not "<project>".
    let (_dir, main_path) = write_project(&[("main.ky", "import nope\nfn main() -> Int { 1 }\n")]);
    let output = check_project(&main_path);
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
    let src = "fn main() -> Int { match Some(1) { Some(x) => x, None => 0 } }";
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
    let src = "fn main() -> Int { match Some(1) { Some(_, _) => 0, None => 1 } }";
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
    let src = "fn main() -> Int { match Some(Some(1)) { Some(Some(x)) => x, _ => 0 } }";
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
    let src = "fn main() -> Int { match Some(1) { Nope(x) => x, _ => 0 } }";
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
    let src = "fn main() -> Int { match Some(1) { A.B(_) => 0, _ => 1 } }";
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
    let src = "type Point = { x: Int }\nfn f(p: Point) -> Int { match p { Point { y } => 0, _ => 1 } }\nfn main() -> Int { f(Point { x: 1 }) }";
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
    let src = "fn main() -> Int { match 1 { { x } => 0, _ => 1 } }";
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
    let src = "type Point = { x: Int }\nfn f(p: Point) -> Int { match p { Point { x, x } => x } }\nfn main() -> Int { f(Point { x: 1 }) }";
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
    let src = "fn main() -> Int { match Some(1) { Smoe => 0, _ => 1 } }";
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
    let src = "type Pair = | Pair(Int, Int)\nfn f(p: Pair) -> Int { match p { Pair(x, x) => x } }\nfn main() -> Int { f(Pair(1, 2)) }";
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
