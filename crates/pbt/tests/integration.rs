#![allow(clippy::unwrap_used)]

use kyokara_pbt::{TestConfig, TestableKind, run_tests};
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
    // Valid property: should have no diagnostics.
    let result = kyokara_hir::check_file("property p(a: Int, b: Int) { a + b == b + a }");
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
