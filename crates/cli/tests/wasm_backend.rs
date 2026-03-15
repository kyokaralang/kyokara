#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_kyokara"))
}

fn run_cli(cwd: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("run kyokara")
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_stdout_trimmed(output: &Output, expected: &str, context: &str) {
    assert_success(output, context);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        expected,
        "{context} stdout mismatch\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn run_backend_wasm_executes_single_file_program() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "fn main() -> String { \"he\".concat(\"llo\").concat(\" wasm\") }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(&output, "hello wasm", "run --backend wasm");
}

#[test]
fn replay_dispatches_wasm_logs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    let log = dir.path().join("run.jsonl");
    fs::write(&file, "fn main() -> Int { 42 }").expect("write source");

    let run = run_cli(
        dir.path(),
        &[
            "run",
            "main.ky",
            "--backend",
            "wasm",
            "--replay-log",
            log.to_str().expect("utf-8 log path"),
        ],
    );
    assert_stdout_trimmed(&run, "42", "run --backend wasm --replay-log");

    let replay = run_cli(
        dir.path(),
        &["replay", log.to_str().expect("utf-8 log path")],
    );
    assert_stdout_trimmed(&replay, "42", "replay wasm log");
}

#[test]
fn run_backend_wasm_rejects_project_mode_for_now() {
    let dir = tempfile::tempdir().expect("tempdir");
    let app = dir.path().join("main.ky");
    let helper = dir.path().join("helper.ky");
    fs::write(
        &app,
        "from helper import value\nfn main() -> Int { value() }",
    )
    .expect("write app");
    fs::write(&helper, "pub fn value() -> Int { 7 }").expect("write helper");

    let output = run_cli(
        dir.path(),
        &["run", "main.ky", "--backend", "wasm", "--project"],
    );
    assert!(
        !output.status.success(),
        "wasm project-mode run should fail until project lowering exists\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("project mode"),
        "stderr should explain current wasm project-mode limitation\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
