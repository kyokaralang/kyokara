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

#[test]
fn build_target_wasm_writes_single_file_module() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    let out = dir.path().join("dist").join("app.wasm");
    fs::write(&file, "fn main() -> Int { 42 }").expect("write source");

    let output = run_cli(
        dir.path(),
        &[
            "build",
            "main.ky",
            "--target",
            "wasm",
            "--out",
            out.to_str().expect("utf-8 output path"),
        ],
    );
    assert_success(&output, "build --target wasm");

    let bytes = fs::read(&out).expect("read wasm artifact");
    assert!(
        bytes.starts_with(b"\0asm"),
        "built file should be a wasm module\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn build_target_wasm_supports_project_mode() {
    let dir = tempfile::tempdir().expect("tempdir");
    let app = dir.path().join("main.ky");
    let helper = dir.path().join("helper.ky");
    let out = dir.path().join("out.wasm");
    fs::write(
        &app,
        "from helper import value\nfn main() -> Int { value() }",
    )
    .expect("write app");
    fs::write(&helper, "pub fn value() -> Int { 7 }").expect("write helper");

    let output = run_cli(
        dir.path(),
        &[
            "build",
            "main.ky",
            "--project",
            "--target",
            "wasm",
            "--out",
            out.to_str().expect("utf-8 output path"),
        ],
    );
    assert_success(&output, "build --project --target wasm");

    let bytes = fs::read(&out).expect("read wasm artifact");
    assert!(
        bytes.starts_with(b"\0asm"),
        "project-mode build should produce a wasm module\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
