use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_kyokara"))
}

fn write_project(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    for (rel, content) in files {
        let path = dir.path().join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dirs");
        }
        std::fs::write(&path, content).expect("write source file");
    }
    let main_path = dir.path().join("main.ky");
    (dir, main_path)
}

fn run_refactor(entry: &Path, args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .arg("refactor")
        .arg(entry)
        .args(args)
        .output()
        .expect("failed to run kyokara refactor")
}

fn parse_stdout_json(output: &std::process::Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("refactor stdout is not JSON: {e}\nstdout:\n{stdout}"))
}

#[test]
fn refactor_single_rename_typechecked() {
    let (_dir, main_path) = write_project(&[(
        "main.ky",
        "fn add(x: Int, y: Int) -> Int { x + y }\nfn main() -> Int { add(1, 2) }\n",
    )]);

    let output = run_refactor(
        &main_path,
        &["--action", "rename", "--symbol", "add", "--new-name", "sum"],
    );

    assert!(
        output.status.success(),
        "refactor should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert_eq!(json["status"], "typechecked");
    assert_eq!(json["verified"], true);
    let edits = json["edits"].as_array().expect("edits array");
    assert!(edits.len() >= 2, "expected >=2 edits, got: {edits:?}");

    let patched_sources = json["patched_sources"]
        .as_array()
        .expect("patched_sources array");
    let combined = patched_sources
        .iter()
        .filter_map(|p| p.get("source").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n---\n");
    assert!(combined.contains("fn sum("), "patched source: {combined}");
    assert!(combined.contains("sum(1, 2)"), "patched source: {combined}");
}

#[test]
fn refactor_project_rename_typechecked() {
    let (_dir, main_path) = write_project(&[
        (
            "main.ky",
            "import math\nfn main() -> Int {\n  let x = add(1, 2)\n  x\n}\n",
        ),
        ("math.ky", "pub fn add(x: Int, y: Int) -> Int { x + y }\n"),
    ]);

    let output = run_refactor(
        &main_path,
        &[
            "--project",
            "--action",
            "rename",
            "--symbol",
            "add",
            "--new-name",
            "sum",
        ],
    );

    assert!(
        output.status.success(),
        "project refactor should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert_eq!(json["status"], "typechecked");
    assert_eq!(json["verified"], true);

    let patched_sources = json["patched_sources"]
        .as_array()
        .expect("patched_sources array");
    let combined = patched_sources
        .iter()
        .filter_map(|p| p.get("source").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n---\n");

    assert!(combined.contains("fn sum("), "patched source: {combined}");
    assert!(combined.contains("sum(1, 2)"), "patched source: {combined}");
}

#[test]
fn refactor_symbol_not_found_returns_error_and_nonzero() {
    let (_dir, main_path) = write_project(&[("main.ky", "fn main() -> Int { 1 }\n")]);

    let output = run_refactor(
        &main_path,
        &[
            "--action",
            "rename",
            "--symbol",
            "missing_name",
            "--new-name",
            "sum",
        ],
    );

    assert!(
        !output.status.success(),
        "refactor should fail with nonzero exit\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert_eq!(json["status"], "error");
    let err = json["error"].as_str().unwrap_or_default();
    assert!(
        err.contains("not found"),
        "expected not-found error, got: {err}"
    );
}

#[test]
fn refactor_project_alias_shadow_case_renames_only_target_module() {
    let (_dir, main_path) = write_project(&[
        (
            "main.ky",
            "import util as math\nfn main() -> Int { add(1, 2) }\n",
        ),
        ("util.ky", "pub fn add(x: Int, y: Int) -> Int { x + y }\n"),
        ("math.ky", "pub fn add(x: Int, y: Int) -> Int { x - y }\n"),
    ]);
    let math_path = main_path
        .parent()
        .expect("main path has parent")
        .join("math.ky");

    let output = run_refactor(
        &main_path,
        &[
            "--project",
            "--action",
            "rename",
            "--symbol",
            "add",
            "--new-name",
            "sum",
            "--target-file",
            math_path.to_string_lossy().as_ref(),
        ],
    );

    assert!(
        output.status.success(),
        "refactor command should return JSON result\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let json = parse_stdout_json(&output);
    assert_eq!(
        json["status"],
        "typechecked",
        "alias-shadow case should now typecheck cleanly\nstdout:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(json["verified"], true);

    let edits = json["edits"].as_array().expect("edits array");
    assert!(!edits.is_empty(), "expected at least one edit");
    assert!(
        edits.iter().all(|e| {
            e.get("file")
                .and_then(Value::as_str)
                .is_some_and(|p| p.ends_with("math.ky"))
        }),
        "only math.ky should be edited in alias-shadow case, got: {edits:?}"
    );
}

#[test]
fn refactor_apply_must_refuse_failed_verification_without_force() {
    let (_dir, main_path) = write_project(&[
        (
            "main.ky",
            "import math\nfn main() -> Int { let z = missing\nadd(1, 2) }\n",
        ),
        ("math.ky", "pub fn add(x: Int, y: Int) -> Int { x - y }\n"),
    ]);
    let math_path = main_path
        .parent()
        .expect("main path has parent")
        .join("math.ky");

    let output = run_refactor(
        &main_path,
        &[
            "--project",
            "--action",
            "rename",
            "--symbol",
            "add",
            "--new-name",
            "sum",
            "--target-file",
            math_path.to_string_lossy().as_ref(),
            "--apply",
        ],
    );

    assert!(
        !output.status.success(),
        "--apply should refuse verification failures\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("refusing to apply edits that fail verification"),
        "expected refusal message, got stderr:\n{stderr}"
    );

    let math_after = std::fs::read_to_string(&math_path).expect("read math after failed apply");
    assert!(
        math_after.contains("fn add("),
        "failed --apply should not modify files; math.ky was:\n{math_after}"
    );
}
