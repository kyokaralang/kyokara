#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

fn write_source(contents: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let file = dir.path().join("main.ky");
    std::fs::write(&file, contents).expect("failed to write source file");
    (dir, file)
}

#[test]
fn check_json_emit_typed_ast_includes_typed_ast_payload() {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_kyokara"));
    let (_dir, file) = write_source("fn main() -> Int { 1 }");

    let output = Command::new(&bin)
        .args(["check", "--format", "json", "--emit", "typed-ast"])
        .arg(&file)
        .output()
        .expect("failed to run kyokara check");

    assert!(
        output.status.success(),
        "expected success; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf-8");
    let json: Value = serde_json::from_str(&stdout).expect("check output must be valid json");
    assert!(
        json.get("typed_ast").is_some(),
        "typed_ast should be present when requested"
    );
}

#[test]
fn check_human_emit_typed_ast_is_rejected() {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_kyokara"));
    let (_dir, file) = write_source("fn main() -> Int { 1 }");

    let output = Command::new(&bin)
        .args(["check", "--format", "human", "--emit", "typed-ast"])
        .arg(&file)
        .output()
        .expect("failed to run kyokara check");

    assert!(
        !output.status.success(),
        "expected failure when --emit typed-ast is used with human format"
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf-8");
    assert!(
        stderr.contains("`--emit typed-ast` requires `--format json`"),
        "stderr should explain format requirement, got: {stderr}"
    );
}
