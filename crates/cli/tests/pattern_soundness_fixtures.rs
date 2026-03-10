#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

#[derive(Debug)]
struct Fixture {
    name: String,
    dir: PathBuf,
    expect: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SoundnessMode {
    Strict,
    KnownViolation,
}

impl SoundnessMode {
    fn from_expect(expect: &Value) -> Self {
        match expect
            .get("soundness")
            .and_then(|s| s.get("mode"))
            .and_then(Value::as_str)
        {
            Some("known_violation") => SoundnessMode::KnownViolation,
            Some("strict") | None => SoundnessMode::Strict,
            Some(other) => panic!("unknown soundness.mode `{other}`"),
        }
    }
}

#[derive(Debug)]
struct CheckInvocation {
    ok: bool,
    stdout: String,
    stderr: String,
    json: Value,
}

#[derive(Debug)]
struct RunInvocation {
    ok: bool,
    stdout: String,
    stderr: String,
}

#[derive(Debug)]
struct Observation {
    check: CheckInvocation,
    run: RunInvocation,
}

#[derive(Debug)]
struct MatrixCase {
    name: &'static str,
    prelude: &'static str,
    match_ty: &'static str,
    arms: &'static str,
    scrutinees: &'static [&'static str],
    expected_runtime_exhaustive: bool,
    mode: SoundnessMode,
    issue: Option<u32>,
}

#[test]
fn pattern_soundness_fixtures_check_and_run() {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_kyokara"));
    let fixtures = load_fixtures();
    assert!(
        !fixtures.is_empty(),
        "expected at least one pattern soundness fixture in {}",
        fixtures_root().display()
    );

    for fixture in fixtures {
        run_fixture(&bin, &fixture);
    }
}

#[test]
fn pattern_exhaustiveness_matrix() {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_kyokara"));

    let cases = [
        MatrixCase {
            name: "bit_all_variants_exhaustive",
            prelude: "type Bit = Zero | One",
            match_ty: "Bit",
            arms: "    Bit.Zero => 0\n    Bit.One => 1",
            scrutinees: &["Bit.Zero", "Bit.One"],
            expected_runtime_exhaustive: true,
            mode: SoundnessMode::Strict,
            issue: None,
        },
        MatrixCase {
            name: "bit_missing_variant_non_exhaustive",
            prelude: "type Bit = Zero | One",
            match_ty: "Bit",
            arms: "    Bit.Zero => 0",
            scrutinees: &["Bit.Zero", "Bit.One"],
            expected_runtime_exhaustive: false,
            mode: SoundnessMode::Strict,
            issue: None,
        },
        MatrixCase {
            name: "opt_nested_missing_leaf_non_exhaustive",
            prelude: "type AB = A | B\ntype Opt = Some(AB) | None",
            match_ty: "Opt",
            arms: "    Opt.Some(AB.A) => 1\n    Opt.None => 0",
            scrutinees: &["Opt.Some(AB.A)", "Opt.Some(AB.B)", "Opt.None"],
            expected_runtime_exhaustive: false,
            mode: SoundnessMode::Strict,
            issue: Some(137),
        },
        MatrixCase {
            name: "tri_all_variants_exhaustive",
            prelude: "type Tri = A | B | C",
            match_ty: "Tri",
            arms: "    Tri.A => 1\n    Tri.B => 2\n    Tri.C => 3",
            scrutinees: &["Tri.A", "Tri.B", "Tri.C"],
            expected_runtime_exhaustive: true,
            mode: SoundnessMode::Strict,
            issue: None,
        },
        MatrixCase {
            name: "tri_missing_variant_non_exhaustive",
            prelude: "type Tri = A | B | C",
            match_ty: "Tri",
            arms: "    Tri.A => 1\n    Tri.B => 2",
            scrutinees: &["Tri.A", "Tri.B", "Tri.C"],
            expected_runtime_exhaustive: false,
            mode: SoundnessMode::Strict,
            issue: None,
        },
        MatrixCase {
            name: "wrap3_nested_missing_leaf_non_exhaustive",
            prelude: "type ABC = A | B | C\ntype Wrap3 = Wrap(ABC) | Empty",
            match_ty: "Wrap3",
            arms: "    Wrap3.Wrap(ABC.A) => 1\n    Wrap3.Empty => 0",
            scrutinees: &["Wrap3.Wrap(ABC.A)", "Wrap3.Wrap(ABC.B)", "Wrap3.Wrap(ABC.C)", "Wrap3.Empty"],
            expected_runtime_exhaustive: false,
            mode: SoundnessMode::Strict,
            issue: Some(137),
        },
    ];

    for case in cases {
        let check_src = matrix_source(case.prelude, case.match_ty, case.arms, case.scrutinees[0]);
        let check_obs = run_source_case(&bin, case.name, &check_src);
        let checker_exhaustive = !diagnostic_codes(&check_obs.check)
            .iter()
            .any(|code| code == "E0009");

        let mut runtime_failures = Vec::new();
        for scrutinee in case.scrutinees {
            let src = matrix_source(case.prelude, case.match_ty, case.arms, scrutinee);
            let obs = run_source_case(&bin, case.name, &src);
            if !obs.run.ok || is_runtime_match_failure(&obs.run) {
                runtime_failures.push((
                    (*scrutinee).to_string(),
                    obs.run.ok,
                    obs.run.stderr.clone(),
                    obs.check.ok,
                    diagnostic_codes(&obs.check),
                ));
            }
        }

        let runtime_exhaustive = runtime_failures.is_empty();
        assert_eq!(
            runtime_exhaustive,
            case.expected_runtime_exhaustive,
            "matrix runtime exhaustiveness mismatch for `{}`\nissue={:?}\nchecker_exhaustive={}\nexpected_runtime_exhaustive={}\nruntime_failures={:?}\nsource:\n{}",
            case.name,
            case.issue,
            checker_exhaustive,
            case.expected_runtime_exhaustive,
            runtime_failures,
            check_src
        );

        let equivalence_holds = checker_exhaustive == runtime_exhaustive;
        match case.mode {
            SoundnessMode::Strict => assert!(
                equivalence_holds,
                "matrix soundness mismatch for `{}` (strict)\nissue={:?}\nchecker_exhaustive={}\nruntime_exhaustive={}\nruntime_failures={:?}\nchecker_codes={:?}\nsource:\n{}",
                case.name,
                case.issue,
                checker_exhaustive,
                runtime_exhaustive,
                runtime_failures,
                diagnostic_codes(&check_obs.check),
                check_src
            ),
            SoundnessMode::KnownViolation => assert!(
                !equivalence_holds,
                "matrix expected known violation no longer reproduces for `{}`\nissue={:?}\nchecker_exhaustive={}\nruntime_exhaustive={}\nruntime_failures={:?}\nchecker_codes={:?}\nsource:\n{}",
                case.name,
                case.issue,
                checker_exhaustive,
                runtime_exhaustive,
                runtime_failures,
                diagnostic_codes(&check_obs.check),
                check_src
            ),
        }
    }
}

#[test]
fn metamorphic_catch_all_addition_does_not_increase_errors() {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_kyokara"));
    let original = r#"
type Color = Red | Green | Blue
fn main() -> Int {
  match (Color.Red) {
    Color.Red => 1
  }
}
"#;
    let transformed = r#"
type Color = Red | Green | Blue
fn main() -> Int {
  match (Color.Red) {
    Color.Red => 1
    _ => 0
  }
}
"#;

    let orig = run_source_case(&bin, "metamorphic_catch_all_original", original);
    let tx = run_source_case(&bin, "metamorphic_catch_all_transformed", transformed);

    let orig_codes = diagnostic_codes(&orig.check);
    let tx_codes = diagnostic_codes(&tx.check);
    let orig_errors = orig_codes.len();
    let tx_errors = tx_codes.len();

    assert!(
        tx_errors <= orig_errors,
        "catch-all addition should not increase diagnostics\noriginal_codes={:?}\ntransformed_codes={:?}\n--- original source ---\n{}\n--- transformed source ---\n{}",
        orig_codes,
        tx_codes,
        original,
        transformed
    );

    if orig_codes.iter().any(|code| code == "E0009") {
        assert!(
            !tx_codes.iter().any(|code| code == "E0009"),
            "catch-all addition should remove E0009\noriginal_codes={:?}\ntransformed_codes={:?}\n--- original source ---\n{}\n--- transformed source ---\n{}",
            orig_codes,
            tx_codes,
            original,
            transformed
        );
    }

    assert!(
        tx.run.ok,
        "transformed program should run successfully\nstdout:\n{}\nstderr:\n{}\nsource:\n{}",
        tx.run.stdout, tx.run.stderr, transformed
    );
    assert!(
        !is_runtime_match_failure(&tx.run),
        "transformed program should not hit runtime match failure\nstderr:\n{}\nsource:\n{}",
        tx.run.stderr,
        transformed
    );
}

#[test]
fn metamorphic_arm_reorder_preserves_exhaustiveness_and_behavior() {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_kyokara"));
    let original = r#"
type Color = Red | Green | Blue
fn main() -> Int {
  match (Color.Green) {
    Color.Red => 1
    Color.Green => 1
    Color.Blue => 1
  }
}
"#;
    let transformed = r#"
type Color = Red | Green | Blue
fn main() -> Int {
  match (Color.Green) {
    Color.Blue => 1
    Color.Red => 1
    Color.Green => 1
  }
}
"#;

    assert_metamorphic_equivalent(
        &bin,
        original,
        transformed,
        SoundnessMode::Strict,
        None,
        "arm_reorder",
    );
}

#[test]
fn metamorphic_nested_flat_equivalence() {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_kyokara"));
    let original = r#"
type AB = A | B
type Opt = Some(AB) | None
fn main() -> Int {
  match (Opt.Some(AB.B)) {
    Opt.Some(AB.A) => 1
    Opt.Some(AB.B) => 2
    Opt.None => 0
  }
}
"#;
    let transformed = r#"
type AB = A | B
type Opt = Some(AB) | None
fn main() -> Int {
  match (Opt.Some(AB.B)) {
    Opt.Some(x) => match (x) {
      AB.A => 1
      AB.B => 2
    }
    Opt.None => 0
  }
}
"#;

    assert_metamorphic_equivalent(
        &bin,
        original,
        transformed,
        SoundnessMode::KnownViolation,
        Some(137),
        "nested_flat_equivalence",
    );
}

#[test]
fn metamorphic_alpha_rename_binder_preserves_diagnostics_and_behavior() {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_kyokara"));
    let original = r#"
from Option import Some, None

fn main() -> Int {
  match (Some(1)) {
    Some(x) => x
    None => 0
  }
}
"#;
    let transformed = r#"
from Option import Some, None

fn main() -> Int {
  match (Some(1)) {
    Some(y) => y
    None => 0
  }
}
"#;

    assert_metamorphic_equivalent(
        &bin,
        original,
        transformed,
        SoundnessMode::Strict,
        None,
        "alpha_rename_binder",
    );
}

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pattern_soundness")
}

fn load_fixtures() -> Vec<Fixture> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(fixtures_root())
        .expect("failed to read pattern soundness fixture directory")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();

    dirs.into_iter()
        .map(|dir| {
            let name = dir
                .file_name()
                .expect("fixture dir has no name")
                .to_string_lossy()
                .into_owned();
            let expect_path = dir.join("expect.json");
            let expect_str = fs::read_to_string(&expect_path)
                .unwrap_or_else(|e| panic!("failed to read {}: {e}", expect_path.display()));
            let expect: Value = serde_json::from_str(&expect_str)
                .unwrap_or_else(|e| panic!("invalid JSON in {}: {e}", expect_path.display()));

            Fixture { name, dir, expect }
        })
        .collect()
}

fn run_fixture(bin: &Path, fixture: &Fixture) {
    let project = fixture
        .expect
        .get("project")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let entry_rel = fixture
        .expect
        .get("entry")
        .and_then(Value::as_str)
        .unwrap_or("main.ky");
    let entry_abs = fixture.dir.join(entry_rel);

    assert!(
        entry_abs.exists(),
        "fixture `{}` missing entry file {}",
        fixture.name,
        entry_abs.display()
    );

    let check_expect = fixture
        .expect
        .get("check")
        .unwrap_or_else(|| panic!("fixture `{}` missing `check` in expect.json", fixture.name));
    let run_expect = fixture
        .expect
        .get("run")
        .unwrap_or_else(|| panic!("fixture `{}` missing `run` in expect.json", fixture.name));

    let check = run_check(bin, project, &entry_abs, &fixture.name);
    let run = run_run(bin, project, &entry_abs, &fixture.name);
    assert_check_expectations(&check, check_expect, &fixture.name);
    assert_run_expectations(&run, run_expect, &fixture.name);

    let source = fs::read_to_string(&entry_abs)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", entry_abs.display()));
    assert_soundness_gate(fixture, &source, &check, &run);
}

fn run_check(bin: &Path, project: bool, entry_abs: &Path, fixture_name: &str) -> CheckInvocation {
    let mut args = vec!["check", "--format", "json"];
    if project {
        args.push("--project");
    }

    let entry_arg = entry_abs.to_string_lossy().into_owned();
    let output = Command::new(bin)
        .args(&args)
        .arg(&entry_arg)
        .output()
        .unwrap_or_else(|e| panic!("fixture `{fixture_name}` failed to run check: {e}"));

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        !stdout.trim().is_empty(),
        "fixture `{}` check emitted empty stdout; stderr:\n{}",
        fixture_name,
        stderr
    );

    let json: Value = serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!(
            "fixture `{}` check output is not valid JSON: {e}\nstdout:\n{}\nstderr:\n{}",
            fixture_name, stdout, stderr
        )
    });

    CheckInvocation {
        ok: output.status.success(),
        stdout,
        stderr,
        json,
    }
}

fn run_run(bin: &Path, project: bool, entry_abs: &Path, fixture_name: &str) -> RunInvocation {
    let mut args = vec!["run"];
    if project {
        args.push("--project");
    }

    let entry_arg = entry_abs.to_string_lossy().into_owned();
    let output = Command::new(bin)
        .args(&args)
        .arg(&entry_arg)
        .output()
        .unwrap_or_else(|e| panic!("fixture `{}` failed to run run: {e}", fixture_name));

    RunInvocation {
        ok: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn diagnostic_codes(check: &CheckInvocation) -> Vec<String> {
    check
        .json
        .get("diagnostics")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("check JSON missing diagnostics array"))
        .iter()
        .filter_map(|d| d.get("code").and_then(Value::as_str).map(ToOwned::to_owned))
        .collect()
}

fn diagnostic_messages(check: &CheckInvocation) -> Vec<String> {
    check
        .json
        .get("diagnostics")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("check JSON missing diagnostics array"))
        .iter()
        .filter_map(|d| {
            d.get("message")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .collect()
}

fn diagnostic_signatures(check: &CheckInvocation) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = check
        .json
        .get("diagnostics")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("check JSON missing diagnostics array"))
        .iter()
        .filter_map(|d| {
            let code = d.get("code").and_then(Value::as_str)?;
            let message = d.get("message").and_then(Value::as_str)?;
            Some((code.to_string(), message.to_string()))
        })
        .collect();
    out.sort();
    out
}

fn assert_check_expectations(check: &CheckInvocation, expect: &Value, fixture_name: &str) {
    let expected_ok = expect
        .get("ok")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| panic!("fixture `{}` check.expect.ok missing/bad", fixture_name));
    assert_eq!(
        check.ok, expected_ok,
        "fixture `{}` check exit mismatch\nstdout:\n{}\nstderr:\n{}",
        fixture_name, check.stdout, check.stderr
    );

    if let Some(codes) = expect.get("codes_all").and_then(Value::as_array) {
        let actual_codes = diagnostic_codes(check);
        for code in codes {
            let code_str = code.as_str().unwrap_or_else(|| {
                panic!(
                    "fixture `{}` check.codes_all entries must be strings",
                    fixture_name
                )
            });
            assert!(
                actual_codes.iter().any(|c| c == code_str),
                "fixture `{}` missing diagnostic code `{}` in {:?}",
                fixture_name,
                code_str,
                actual_codes
            );
        }
    }

    if let Some(messages) = expect.get("messages_all").and_then(Value::as_array) {
        let actual_messages = diagnostic_messages(check);
        for msg in messages {
            let msg_str = msg.as_str().unwrap_or_else(|| {
                panic!(
                    "fixture `{}` check.messages_all entries must be strings",
                    fixture_name
                )
            });
            assert!(
                actual_messages.iter().any(|m| m.contains(msg_str)),
                "fixture `{}` missing diagnostic message containing `{}` in {:?}",
                fixture_name,
                msg_str,
                actual_messages
            );
        }
    }
}

fn assert_run_expectations(run: &RunInvocation, expect: &Value, fixture_name: &str) {
    let expected_ok = expect
        .get("ok")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| panic!("fixture `{}` run.expect.ok missing/bad", fixture_name));
    assert_eq!(
        run.ok, expected_ok,
        "fixture `{}` run exit mismatch\nstdout:\n{}\nstderr:\n{}",
        fixture_name, run.stdout, run.stderr
    );

    if let Some(expected_stdout) = expect.get("stdout_contains").and_then(Value::as_array) {
        for fragment in expected_stdout {
            let fragment = fragment.as_str().unwrap_or_else(|| {
                panic!(
                    "fixture `{}` run.stdout_contains entries must be strings",
                    fixture_name
                )
            });
            assert!(
                run.stdout.contains(fragment),
                "fixture `{}` run stdout missing fragment `{}`\nstdout:\n{}",
                fixture_name,
                fragment,
                run.stdout
            );
        }
    }

    if let Some(expected_stderr) = expect.get("stderr_contains").and_then(Value::as_array) {
        for fragment in expected_stderr {
            let fragment = fragment.as_str().unwrap_or_else(|| {
                panic!(
                    "fixture `{}` run.stderr_contains entries must be strings",
                    fixture_name
                )
            });
            assert!(
                run.stderr.contains(fragment),
                "fixture `{}` run stderr missing fragment `{}`\nstderr:\n{}",
                fixture_name,
                fragment,
                run.stderr
            );
        }
    }
}

fn assert_soundness_gate(
    fixture: &Fixture,
    source: &str,
    check: &CheckInvocation,
    run: &RunInvocation,
) {
    let mode = SoundnessMode::from_expect(&fixture.expect);
    let issue = fixture
        .expect
        .get("soundness")
        .and_then(|s| s.get("issue"))
        .and_then(Value::as_u64);
    let require_check_rejection = fixture
        .expect
        .get("soundness")
        .and_then(|s| s.get("require_check_rejection"))
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let runtime_violation = check.ok && (!run.ok || is_runtime_match_failure(run));
    let rejection_violation = require_check_rejection && check.ok;
    let any_violation = runtime_violation || rejection_violation;

    match mode {
        SoundnessMode::Strict => {
            assert!(
                !any_violation,
                "pattern soundness gate violated in fixture `{}` (strict)\nissue={:?}\nruntime_violation={}\nrejection_violation={}\ncheck_ok={}\nrun_ok={}\ncheck_codes={:?}\nrun_stderr=\n{}\nsource:\n{}",
                fixture.name,
                issue,
                runtime_violation,
                rejection_violation,
                check.ok,
                run.ok,
                diagnostic_codes(check),
                run.stderr,
                source
            );
        }
        SoundnessMode::KnownViolation => {
            assert!(
                any_violation,
                "expected known soundness violation no longer reproduces in fixture `{}`\nissue={:?}\ncheck_ok={}\nrun_ok={}\ncheck_codes={:?}\nrun_stderr=\n{}\nsource:\n{}",
                fixture.name,
                issue,
                check.ok,
                run.ok,
                diagnostic_codes(check),
                run.stderr,
                source
            );
        }
    }
}

fn is_runtime_match_failure(run: &RunInvocation) -> bool {
    run.stderr.contains("pattern match failure")
}

fn matrix_source(prelude: &str, match_ty: &str, arms: &str, scrutinee: &str) -> String {
    format!(
        "{prelude}\n\nfn probe(v: {match_ty}) -> Int {{\n  match (v) {{\n{arms}\n  }}\n}}\n\nfn main() -> Int {{\n  probe({scrutinee})\n}}\n"
    )
}

fn run_source_case(bin: &Path, name: &str, source: &str) -> Observation {
    let mut prefix = String::from("kyokara_pattern_soundness_");
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            prefix.push(ch);
        } else {
            prefix.push('_');
        }
    }

    let temp_dir = tempfile::Builder::new()
        .prefix(&prefix)
        .tempdir()
        .unwrap_or_else(|e| panic!("failed to create temporary directory for `{name}`: {e}"));

    let file = temp_dir.path().join("main.ky");
    fs::write(&file, source).unwrap_or_else(|e| {
        panic!(
            "failed to write temporary source for `{}` at {}: {e}",
            name,
            file.display()
        )
    });

    let check = run_check(bin, false, &file, name);
    let run = run_run(bin, false, &file, name);

    Observation { check, run }
}

fn assert_metamorphic_equivalent(
    bin: &Path,
    original: &str,
    transformed: &str,
    mode: SoundnessMode,
    issue: Option<u32>,
    label: &str,
) {
    let original_obs = run_source_case(bin, &format!("{}_original", label), original);
    let transformed_obs = run_source_case(bin, &format!("{}_transformed", label), transformed);

    let equivalent = diagnostic_signatures(&original_obs.check)
        == diagnostic_signatures(&transformed_obs.check)
        && original_obs.run.ok == transformed_obs.run.ok
        && original_obs.run.stdout.trim() == transformed_obs.run.stdout.trim()
        && is_runtime_match_failure(&original_obs.run)
            == is_runtime_match_failure(&transformed_obs.run);

    match mode {
        SoundnessMode::Strict => assert!(
            equivalent,
            "metamorphic mismatch for `{}` (strict)\nissue={:?}\n--- original diagnostics ---\n{:?}\n--- transformed diagnostics ---\n{:?}\n--- original run ---\nok={}\nstdout=\n{}\nstderr=\n{}\n--- transformed run ---\nok={}\nstdout=\n{}\nstderr=\n{}\n--- original source ---\n{}\n--- transformed source ---\n{}",
            label,
            issue,
            diagnostic_signatures(&original_obs.check),
            diagnostic_signatures(&transformed_obs.check),
            original_obs.run.ok,
            original_obs.run.stdout,
            original_obs.run.stderr,
            transformed_obs.run.ok,
            transformed_obs.run.stdout,
            transformed_obs.run.stderr,
            original,
            transformed
        ),
        SoundnessMode::KnownViolation => assert!(
            !equivalent,
            "expected known metamorphic violation no longer reproduces for `{}`\nissue={:?}\n--- original diagnostics ---\n{:?}\n--- transformed diagnostics ---\n{:?}\n--- original run ---\nok={}\nstdout=\n{}\nstderr=\n{}\n--- transformed run ---\nok={}\nstdout=\n{}\nstderr=\n{}\n--- original source ---\n{}\n--- transformed source ---\n{}",
            label,
            issue,
            diagnostic_signatures(&original_obs.check),
            diagnostic_signatures(&transformed_obs.check),
            original_obs.run.ok,
            original_obs.run.stdout,
            original_obs.run.stderr,
            transformed_obs.run.ok,
            transformed_obs.run.stdout,
            transformed_obs.run.stderr,
            original,
            transformed
        ),
    }
}
