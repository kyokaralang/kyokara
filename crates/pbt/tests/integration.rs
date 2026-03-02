#![allow(clippy::unwrap_used)]

use kyokara_pbt::{TestConfig, TestableKind, run_project_tests, run_tests};
use std::path::PathBuf;

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read fixture {name}: {e}"))
}

fn test_config() -> TestConfig {
    TestConfig {
        num_tests: 50,
        explore: true,
        seed: 42,
        format: "human".to_string(),
        corpus_base: tempfile::tempdir().unwrap().keep(),
    }
}

fn write_project(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    for (rel, src) in files {
        let path = dir.path().join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, src).unwrap();
    }
    let main_path = dir.path().join("main.ky");
    (dir, main_path)
}

#[test]
fn detects_buggy_abs() {
    let source = fixture("buggy_abs.ky");
    let config = test_config();
    let report = run_tests(&source, &config).unwrap();

    assert!(!report.all_passed(), "buggy_abs should fail");
    assert_eq!(report.failure_count(), 1);

    let result = &report.results[0];
    assert_eq!(result.name, "my_abs");
    let failure = result.failure.as_ref().unwrap();
    assert!(
        failure.error.contains("postcondition"),
        "expected postcondition failure, got: {}",
        failure.error
    );
}

#[test]
fn passing_contract_succeeds() {
    let source = fixture("passing.ky");
    let config = test_config();
    let report = run_tests(&source, &config).unwrap();

    assert!(
        report.all_passed(),
        "passing.ky should pass: {}",
        report.format_human()
    );
    assert_eq!(report.failure_count(), 0);
    assert_eq!(report.results.len(), 1);
    assert_eq!(report.results[0].name, "is_positive");
    assert!(report.results[0].passed > 0);
}

#[test]
fn no_contracts_skips_all() {
    let source = fixture("no_contracts.ky");
    let config = test_config();
    let report = run_tests(&source, &config).unwrap();

    assert!(report.results.is_empty(), "no functions should be tested");
    assert!(!report.skipped.is_empty(), "add should be in skipped list");
}

#[test]
fn shrinks_counterexample() {
    let source = fixture("buggy_abs.ky");
    let config = test_config();
    let report = run_tests(&source, &config).unwrap();

    let result = &report.results[0];
    let failure = result.failure.as_ref().unwrap();

    // The shrunk counterexample should be a small negative number.
    // my_abs(x) fails when x < 0, minimal is -1.
    assert!(
        !failure.args_display.is_empty(),
        "should have counterexample args"
    );
    let arg_str = &failure.args_display[0];
    let arg_val: i64 = arg_str
        .parse()
        .unwrap_or_else(|_| panic!("expected integer counterexample, got: {arg_str}"));
    assert!(arg_val < 0, "counterexample should be negative: {arg_val}");
    // After shrinking, should be close to -1.
    assert!(
        arg_val >= -10,
        "counterexample should be small after shrinking: {arg_val}"
    );
}

#[test]
fn json_format_is_valid() {
    let source = fixture("passing.ky");
    let config = test_config();
    let report = run_tests(&source, &config).unwrap();

    let json = report.format_json();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.get("results").is_some());
    assert!(parsed.get("skipped").is_some());
}

#[test]
fn corpus_replay_detects_regression() {
    let source = fixture("buggy_abs.ky");
    let corpus_dir = tempfile::tempdir().unwrap();

    // First run: explore to find and save a failure.
    let config = TestConfig {
        num_tests: 50,
        explore: true,
        seed: 42,
        format: "human".to_string(),
        corpus_base: corpus_dir.path().to_path_buf(),
    };
    let report1 = run_tests(&source, &config).unwrap();
    assert!(!report1.all_passed());

    // Second run: corpus-only (no explore) should replay the failure.
    let config2 = TestConfig {
        num_tests: 0,
        explore: false,
        seed: 0,
        format: "human".to_string(),
        corpus_base: corpus_dir.path().to_path_buf(),
    };
    let report2 = run_tests(&source, &config2).unwrap();
    assert!(
        !report2.all_passed(),
        "corpus replay should still detect the regression"
    );
}

#[test]
fn property_pass_succeeds() {
    let source = fixture("property_pass.ky");
    let config = test_config();
    let report = run_tests(&source, &config).unwrap();

    assert!(
        report.all_passed(),
        "property_pass.ky should pass: {}",
        report.format_human()
    );
    assert_eq!(report.results.len(), 1);
    assert_eq!(report.results[0].name, "bool_identity");
    assert_eq!(report.results[0].kind, TestableKind::Property);
    assert!(report.results[0].passed > 0);
}

#[test]
fn property_fail_detected() {
    let source = fixture("property_fail.ky");
    let config = test_config();
    let report = run_tests(&source, &config).unwrap();

    assert!(!report.all_passed(), "property_fail.ky should fail");
    assert_eq!(report.failure_count(), 1);

    let result = &report.results[0];
    assert_eq!(result.name, "bad_abs");
    assert_eq!(result.kind, TestableKind::Property);
    let failure = result.failure.as_ref().unwrap();
    assert!(
        failure.error.contains("property returned false"),
        "expected 'property returned false', got: {}",
        failure.error
    );

    // Counterexample should be a negative number (abs(x) != x when x < 0).
    assert!(!failure.args_display.is_empty());
    let arg_val: i64 = failure.args_display[0].parse().unwrap();
    assert!(arg_val < 0, "counterexample should be negative: {arg_val}");
}

#[test]
fn mixed_discovery() {
    let source = fixture("mixed.ky");
    let config = test_config();
    let report = run_tests(&source, &config).unwrap();

    assert!(
        report.all_passed(),
        "mixed.ky should pass: {}",
        report.format_human()
    );

    // Should have both a function and a property tested.
    assert_eq!(
        report.results.len(),
        2,
        "expected 2 results (fn + property)"
    );

    let fn_result = report
        .results
        .iter()
        .find(|r| r.kind == TestableKind::Function)
        .expect("should have a function result");
    assert_eq!(fn_result.name, "is_positive");
    assert!(fn_result.passed > 0);

    let prop_result = report
        .results
        .iter()
        .find(|r| r.kind == TestableKind::Property)
        .expect("should have a property result");
    assert_eq!(prop_result.name, "gt_antisymmetric");
    assert!(prop_result.passed > 0);
}

#[test]
fn property_type_check() {
    // Valid property with new canonical syntax: should have no diagnostics.
    let result = kyokara_hir::check_file(
        "property p(a: Int <- Gen.auto(), b: Int <- Gen.auto()) { a + b == b + a }",
    );
    let all_diags: Vec<_> = result
        .type_check
        .raw_diagnostics
        .iter()
        .map(|(d, _)| format!("{d:?}"))
        .collect();
    assert!(
        all_diags.is_empty(),
        "valid property should have no type errors: {all_diags:?}"
    );
}

// ── Refined-type tests ────────────────────────────────────────────

#[test]
fn refined_pass_succeeds() {
    let source = fixture("refined_pass.ky");
    let config = test_config();
    let report = run_tests(&source, &config).unwrap();

    assert!(
        report.all_passed(),
        "refined_pass.ky should pass: {}",
        report.format_human()
    );
    assert_eq!(report.results.len(), 1);
    assert_eq!(report.results[0].name, "positive_is_positive");
    assert_eq!(report.results[0].kind, TestableKind::Property);
    assert!(report.results[0].passed > 0);
}

#[test]
fn refined_fail_detected() {
    let source = fixture("refined_fail.ky");
    let config = test_config();
    let report = run_tests(&source, &config).unwrap();

    assert!(!report.all_passed(), "refined_fail.ky should fail");
    assert_eq!(report.failure_count(), 1);

    let result = &report.results[0];
    assert_eq!(result.name, "bad_bound");
    assert_eq!(result.kind, TestableKind::Property);
    let failure = result.failure.as_ref().unwrap();
    assert!(
        failure.error.contains("property returned false"),
        "expected 'property returned false', got: {}",
        failure.error
    );

    // Counterexample should still satisfy x > 0 (refinement respected).
    assert!(!failure.args_display.is_empty());
    let arg_val: i64 = failure.args_display[0].parse().unwrap();
    assert!(
        arg_val > 0,
        "counterexample should satisfy refinement x > 0, got: {arg_val}"
    );
}

#[test]
fn refined_shrink_respects_predicate() {
    let source = fixture("refined_fail.ky");
    let config = test_config();
    let report = run_tests(&source, &config).unwrap();

    let result = &report.results[0];
    let failure = result.failure.as_ref().unwrap();

    // After shrinking, the counterexample should still satisfy x > 0.
    let arg_val: i64 = failure.args_display[0].parse().unwrap();
    assert!(
        arg_val > 0,
        "shrunk counterexample should satisfy refinement: {arg_val}"
    );
    // Should be a small positive number (shrunk toward 1).
    assert!(
        arg_val <= 100,
        "shrunk counterexample should be <= 100: {arg_val}"
    );
}

#[test]
fn refined_mixed_discovery() {
    let source = fixture("refined_mixed.ky");
    let config = test_config();
    let report = run_tests(&source, &config).unwrap();

    assert!(
        report.all_passed(),
        "refined_mixed.ky should pass: {}",
        report.format_human()
    );

    // Should have both a contracted function and a refined property.
    assert_eq!(
        report.results.len(),
        2,
        "expected 2 results (fn + property)"
    );

    let fn_result = report
        .results
        .iter()
        .find(|r| r.kind == TestableKind::Function)
        .expect("should have a function result");
    assert_eq!(fn_result.name, "clamp");
    assert!(fn_result.passed > 0);

    let prop_result = report
        .results
        .iter()
        .find(|r| r.kind == TestableKind::Property)
        .expect("should have a property result");
    assert_eq!(prop_result.name, "pos_capped");
    assert!(prop_result.passed > 0);
}

#[test]
fn refined_unsatisfiable_reported() {
    let source = fixture("refined_unsatisfiable.ky");
    let config = test_config();
    let report = run_tests(&source, &config).unwrap();

    assert!(
        !report.all_passed(),
        "unsatisfiable refinement should report failure"
    );

    let result = &report.results[0];
    assert_eq!(result.name, "impossible");
    let failure = result.failure.as_ref().unwrap();
    assert!(
        failure.error.contains("unsatisfiable"),
        "expected 'unsatisfiable' error, got: {}",
        failure.error
    );
}

#[test]
fn where_constrained_type_check() {
    // Valid where-constrained property: should have no type errors.
    let result =
        kyokara_hir::check_file("property p(x: Int <- Gen.auto()) where (x > 0) { x > 0 }");
    let all_diags: Vec<_> = result
        .type_check
        .raw_diagnostics
        .iter()
        .map(|(d, _)| format!("{d:?}"))
        .collect();
    assert!(
        all_diags.is_empty(),
        "valid where-constrained property should have no type errors: {all_diags:?}"
    );
}

#[test]
fn refined_non_property_still_rejected() {
    // Refined type in a regular function param should still be rejected.
    let result = kyokara_hir::check_file("fn foo(x: { x: Int | x > 0 }) -> Int { x }");
    let has_refined_error = result
        .lowering_diagnostics
        .iter()
        .any(|d| d.message.contains("refined types are not yet supported"));
    assert!(
        has_refined_error,
        "refined type in regular fn should be rejected, diagnostics: {:?}",
        result.lowering_diagnostics
    );
}

#[test]
fn refined_corpus_replay() {
    let source = fixture("refined_fail.ky");
    let corpus_dir = tempfile::tempdir().unwrap();

    // First run: explore to find and save a failure.
    let config = TestConfig {
        num_tests: 50,
        explore: true,
        seed: 42,
        format: "human".to_string(),
        corpus_base: corpus_dir.path().to_path_buf(),
    };
    let report1 = run_tests(&source, &config).unwrap();
    assert!(!report1.all_passed());

    // Verify the counterexample satisfies the refinement.
    let failure1 = report1.results[0].failure.as_ref().unwrap();
    let arg_val: i64 = failure1.args_display[0].parse().unwrap();
    assert!(arg_val > 0, "corpus entry should satisfy refinement");

    // Second run: corpus-only replay.
    let config2 = TestConfig {
        num_tests: 0,
        explore: false,
        seed: 0,
        format: "human".to_string(),
        corpus_base: corpus_dir.path().to_path_buf(),
    };
    let report2 = run_tests(&source, &config2).unwrap();
    assert!(
        !report2.all_passed(),
        "corpus replay should still detect the failure"
    );

    // Replayed counterexample should also satisfy refinement.
    let failure2 = report2.results[0].failure.as_ref().unwrap();
    let arg_val2: i64 = failure2.args_display[0].parse().unwrap();
    assert!(
        arg_val2 > 0,
        "replayed corpus entry should satisfy refinement"
    );
}

// ── Canonical property syntax edge cases ────────────────────────────

#[test]
fn property_type_check_with_where_and_multiple_params() {
    let result = kyokara_hir::check_file(
        "property p(a: Int <- Gen.auto(), b: Int <- Gen.auto()) where (a > 0 && b > 0) { a + b > 0 }",
    );
    let all_diags: Vec<_> = result
        .type_check
        .raw_diagnostics
        .iter()
        .map(|(d, _)| format!("{d:?}"))
        .collect();
    assert!(
        all_diags.is_empty(),
        "multi-param property with where (should have no type errors:) {all_diags:?}"
    );
}

#[test]
fn property_type_check_gen_int() {
    let result = kyokara_hir::check_file("property p(x: Int <- Gen.int()) { x > 0 }");
    let all_diags: Vec<_> = result
        .type_check
        .raw_diagnostics
        .iter()
        .map(|(d, _)| format!("{d:?}"))
        .collect();
    assert!(
        all_diags.is_empty(),
        "Gen.int() property should have no type errors: {all_diags:?}"
    );
}

#[test]
fn property_type_check_gen_bool() {
    let result = kyokara_hir::check_file("property p(b: Bool <- Gen.bool()) { b == b }");
    let all_diags: Vec<_> = result
        .type_check
        .raw_diagnostics
        .iter()
        .map(|(d, _)| format!("{d:?}"))
        .collect();
    assert!(
        all_diags.is_empty(),
        "Gen.bool() property should have no type errors: {all_diags:?}"
    );
}

#[test]
fn property_gen_type_match_has_no_mismatch_diagnostic() {
    let result = kyokara_hir::check_file("property p(x: Int <- Gen.int()) { x > 0 }");
    let has_mismatch = result
        .lowering_diagnostics
        .iter()
        .any(|d| d.message.contains("generator") && d.message.contains("incompatible"));
    assert!(
        !has_mismatch,
        "matching Gen.int() with Int should not produce mismatch diagnostic: {:?}",
        result.lowering_diagnostics
    );
}

#[test]
fn property_gen_type_mismatch_produces_diagnostic() {
    let result = kyokara_hir::check_file("property p(x: Int <- Gen.bool()) { x > 0 }");
    let has_mismatch = result
        .lowering_diagnostics
        .iter()
        .any(|d| d.message.contains("generator") && d.message.contains("incompatible"));
    assert!(
        has_mismatch,
        "Gen.bool() for Int parameter should produce mismatch diagnostic: {:?}",
        result.lowering_diagnostics
    );
}

#[test]
fn run_tests_rejects_gen_spec_type_mismatch_before_execution() {
    let config = test_config();
    let source = "property p(x: Int <- Gen.bool()) { x > 0 }";
    let err = run_tests(source, &config).expect_err("gen/type mismatch must be rejected");
    assert!(
        err.contains("generator") && err.contains("incompatible"),
        "error should include generator/type mismatch, got: {err}"
    );
}

#[test]
fn run_project_tests_rejects_gen_spec_type_mismatch_before_execution() {
    let config = test_config();
    let (_dir, main_path) =
        write_project(&[("main.ky", "property p(x: Int <- Gen.bool()) { x > 0 }\n")]);
    let err =
        run_project_tests(&main_path, &config).expect_err("project mismatch must be rejected");
    assert!(
        err.contains("generator") && err.contains("incompatible"),
        "error should include generator/type mismatch, got: {err}"
    );
}

#[test]
fn property_type_check_gen_string() {
    let result = kyokara_hir::check_file("property p(s: String <- Gen.string()) { s.len() >= 0 }");
    let all_diags: Vec<_> = result
        .type_check
        .raw_diagnostics
        .iter()
        .map(|(d, _)| format!("{d:?}"))
        .collect();
    assert!(
        all_diags.is_empty(),
        "Gen.string() property should have no type errors: {all_diags:?}"
    );
}

#[test]
fn property_bare_param_parse_error() {
    // Old syntax: bare property params should produce parse errors.
    let result = kyokara_hir::check_file("property p(x: Int) { x > 0 }");
    assert!(
        !result.parse_errors.is_empty(),
        "bare property param should produce parse error"
    );
    assert!(
        result.parse_errors.iter().any(|e| e.message.contains("<-")),
        "error should mention `<-`: {:?}",
        result.parse_errors
    );
}

#[test]
fn property_where_unresolved_name() {
    // Where clause referencing a nonexistent variable.
    let result = kyokara_hir::check_file(
        "property p(x: Int <- Gen.auto()) where (nonexistent > 0) { x > 0 }",
    );
    // Unresolved names surface in body_lowering_diagnostics or diagnostics.
    let all_diags: Vec<String> = result
        .type_check
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .chain(
            result
                .type_check
                .body_lowering_diagnostics
                .iter()
                .map(|d| d.message.clone()),
        )
        .collect();
    let has_unresolved = all_diags.iter().any(|msg| msg.contains("unresolved"));
    assert!(
        has_unresolved,
        "where clause with unresolved name should produce diagnostic: {all_diags:?}"
    );
}

#[test]
fn property_invalid_gen_method_check() {
    // Invalid Gen method produces lowering diagnostic.
    let result = kyokara_hir::check_file("property p(x: Int <- Gen.unknown()) { x > 0 }");
    assert!(
        result
            .lowering_diagnostics
            .iter()
            .any(|d| d.message.contains("invalid generator expression")),
        "Gen.unknown() should produce 'invalid generator expression' diagnostic: {:?}",
        result.lowering_diagnostics
    );
}

#[test]
fn run_tests_rejects_compile_invalid_property_before_execution() {
    let config = test_config();
    let source = "property p(x: Int <- Gen.unknown()) { x > 0 }";
    let err = run_tests(source, &config).expect_err("compile-invalid source must be rejected");
    assert!(
        err.contains("invalid generator expression"),
        "error should include compile diagnostic, got: {err}"
    );
}

#[test]
fn run_tests_still_executes_compile_valid_property() {
    let config = test_config();
    let source = "property p(b: Bool <- Gen.bool()) { b == b }";
    let report = run_tests(source, &config).expect("compile-valid source should run");
    assert!(report.all_passed(), "expected passing report");
    assert_eq!(report.failure_count(), 0);
}

#[test]
fn run_project_tests_rejects_compile_invalid_property_before_execution() {
    let config = test_config();
    let (_dir, main_path) =
        write_project(&[("main.ky", "property p(x: Int <- Gen.unknown()) { x > 0 }\n")]);
    let err = run_project_tests(&main_path, &config)
        .expect_err("compile-invalid project must be rejected");
    assert!(
        err.contains("invalid generator expression"),
        "error should include compile diagnostic, got: {err}"
    );
}

#[test]
fn run_project_tests_still_executes_compile_valid_property() {
    let config = test_config();
    let (_dir, main_path) =
        write_project(&[("main.ky", "property p(b: Bool <- Gen.bool()) { b == b }\n")]);
    let report = run_project_tests(&main_path, &config).expect("compile-valid project should run");
    assert!(report.all_passed(), "expected passing report");
    assert_eq!(report.failure_count(), 0);
}

#[test]
fn property_refined_type_in_param_check() {
    // Refined type in property param should produce specific diagnostic.
    let result =
        kyokara_hir::check_file("property p(x: { x: Int | x > 0 } <- Gen.auto()) { x > 0 }");
    assert!(
        result.lowering_diagnostics.iter().any(|d| d
            .message
            .contains("refinement types are not allowed in property params")),
        "refined type in property param should be rejected: {:?}",
        result.lowering_diagnostics
    );
}

#[test]
fn property_gen_non_call_check() {
    // Plain identifier instead of Gen.method() call.
    let result = kyokara_hir::check_file("property p(x: Int <- something) { x > 0 }");
    assert!(
        result
            .lowering_diagnostics
            .iter()
            .any(|d| d.message.contains("invalid generator expression")),
        "non-call gen should produce diagnostic: {:?}",
        result.lowering_diagnostics
    );
}

#[test]
fn property_gen_wrong_base_check() {
    // Not Gen.*, using Other.auto().
    let result = kyokara_hir::check_file("property p(x: Int <- Other.auto()) { x > 0 }");
    assert!(
        result
            .lowering_diagnostics
            .iter()
            .any(|d| d.message.contains("invalid generator expression")),
        "Other.auto() should produce diagnostic: {:?}",
        result.lowering_diagnostics
    );
}

#[test]
fn property_gen_int_range_one_arg_check() {
    // Gen.int_range with only one argument.
    let result = kyokara_hir::check_file("property p(x: Int <- Gen.int_range(1)) { x > 0 }");
    assert!(
        result
            .lowering_diagnostics
            .iter()
            .any(|d| d.message.contains("invalid generator expression")),
        "Gen.int_range(1) should produce diagnostic: {:?}",
        result.lowering_diagnostics
    );
}

#[test]
fn property_gen_bool_with_args_check() {
    // Gen.bool doesn't take arguments.
    let result = kyokara_hir::check_file("property p(x: Bool <- Gen.bool(true)) { x }");
    assert!(
        result
            .lowering_diagnostics
            .iter()
            .any(|d| d.message.contains("invalid generator expression")),
        "Gen.bool(true) should produce diagnostic: {:?}",
        result.lowering_diagnostics
    );
}

#[test]
fn property_gen_list_no_inner_check() {
    // Gen.list() needs an inner generator.
    let result = kyokara_hir::check_file("property p(xs: List<Int> <- Gen.list()) { true }");
    assert!(
        result
            .lowering_diagnostics
            .iter()
            .any(|d| d.message.contains("invalid generator expression")),
        "Gen.list() should produce diagnostic: {:?}",
        result.lowering_diagnostics
    );
}

#[test]
fn property_gen_nested_invalid_check() {
    // Gen.list(Gen.bad()) — inner spec is invalid.
    let result =
        kyokara_hir::check_file("property p(xs: List<Int> <- Gen.list(Gen.bad())) { true }");
    assert!(
        result
            .lowering_diagnostics
            .iter()
            .any(|d| d.message.contains("invalid generator expression")),
        "Gen.list(Gen.bad()) should produce diagnostic: {:?}",
        result.lowering_diagnostics
    );
}

#[test]
fn property_int_range_pass() {
    // Gen.int_range should actually generate values in the given range.
    let source = "property in_range(x: Int <- Gen.int_range(1, 10)) { x >= 1 && x <= 10 }";
    let config = test_config();
    let report = run_tests(source, &config).unwrap();

    assert!(
        report.all_passed(),
        "int_range(1, 10) should always satisfy 1 <= x <= 10: {}",
        report.format_human()
    );
    assert!(report.results[0].passed > 0);
}

#[test]
fn property_where_complex_conjunction() {
    // Where clause with a complex boolean conjunction.
    let source = r#"
property bounded(x: Int <- Gen.auto())
where (x > 0 && x < 100)
{
  x + x > 0
}
"#;
    let config = test_config();
    let report = run_tests(source, &config).unwrap();

    assert!(
        report.all_passed(),
        "bounded property should pass: {}",
        report.format_human()
    );
}

#[test]
fn property_discard_rate_logged() {
    // Where clause that filters ~50% — should still succeed with budget.
    let source = r#"
property half_positive(x: Int <- Gen.auto())
where (x > 0)
{
  x > 0
}
"#;
    let config = test_config();
    let report = run_tests(source, &config).unwrap();

    assert!(
        report.all_passed(),
        "half_positive should pass: {}",
        report.format_human()
    );
    let result = &report.results[0];
    assert!(result.passed > 0, "should have passing tests");
    assert!(result.discarded > 0, "should have some discards");
}

#[test]
fn property_invalid_range_reports_generator_error_not_where_unsat() {
    let source = "property p(x: Int <- Gen.int_range(10, 1)) { x > 0 }";
    let config = test_config();
    let report = run_tests(source, &config).unwrap();
    assert!(!report.all_passed(), "invalid range should fail");

    let failure = report.results[0]
        .failure
        .as_ref()
        .expect("must have failure");
    assert!(
        failure
            .error
            .contains("invalid or unsupported generator configuration"),
        "expected generator-specific failure, got: {}",
        failure.error
    );
    assert!(
        !failure.error.contains("where"),
        "invalid generator should not be reported as where-unsat: {}",
        failure.error
    );
}

#[test]
fn project_invalid_range_reports_generator_error_not_where_unsat() {
    let config = test_config();
    let (_dir, main_path) = write_project(&[(
        "main.ky",
        "property p(x: Int <- Gen.int_range(10, 1)) { x > 0 }\n",
    )]);
    let report = run_project_tests(&main_path, &config).unwrap();
    assert!(
        !report.all_passed(),
        "invalid range should fail in project mode"
    );

    let failure = report.results[0]
        .failure
        .as_ref()
        .expect("must have failure");
    assert!(
        failure
            .error
            .contains("invalid or unsupported generator configuration"),
        "expected generator-specific failure, got: {}",
        failure.error
    );
    assert!(
        !failure.error.contains("where"),
        "invalid generator should not be reported as where-unsat: {}",
        failure.error
    );
}

#[test]
fn where_unsat_still_reports_where_unsatisfiable() {
    let source = "property impossible(x: Int <- Gen.int()) where (x > 0 && x < 0) { true }";
    let config = test_config();
    let report = run_tests(source, &config).unwrap();
    assert!(!report.all_passed(), "unsatisfiable where should fail");

    let failure = report.results[0]
        .failure
        .as_ref()
        .expect("must have failure");
    assert!(
        failure.error.contains("unsatisfiable"),
        "expected unsatisfiable failure, got: {}",
        failure.error
    );
    assert!(
        failure.error.contains("where"),
        "expected where-unsat message, got: {}",
        failure.error
    );
}
