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
fn run_backend_wasm_displays_structural_main_output() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "type Point = { x: Int, y: Int }\nfn main() -> Point { { x: 3, y: 4 } }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "{ x: 3, y: 4 }",
        "run --backend wasm structural main output",
    );
}

#[test]
fn run_backend_wasm_displays_collection_main_output() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableMap\nfn main() -> MutableMap<String, Int> { MutableMap.new().insert(\"k\", 1).insert(\"z\", 2) }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "MutableMap({k: 1, z: 2})",
        "run --backend wasm collection main output",
    );
}

#[test]
fn run_backend_wasm_supports_io_println_effects() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "import io\nfn main() -> Unit { io.println(\"hello from wasm io\") }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "hello from wasm io",
        "run --backend wasm with io.println",
    );
}

#[test]
fn run_backend_wasm_supports_direct_imported_hash_md5() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from hash import md5\nfn main() -> String { md5(\"abc\") }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "900150983cd24fb0d6963f7d28e17f72",
        "run --backend wasm with direct-imported hash.md5",
    );
}

#[test]
fn run_backend_wasm_supports_fs_read_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    let input = dir.path().join("input.txt");
    fs::write(&input, "hello from file").expect("write input");
    fs::write(
        &file,
        format!(
            "import fs\nfn main() -> String {{ fs.read_file(\"{}\") }}",
            input.display()
        ),
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "hello from file",
        "run --backend wasm with fs.read_file",
    );
}

#[test]
fn run_backend_wasm_supports_direct_imported_fs_read_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    let input = dir.path().join("input.txt");
    fs::write(&input, "hello from direct import").expect("write input");
    fs::write(
        &file,
        format!(
            "from fs import read_file\nfn main() -> String {{ read_file(\"{}\") }}",
            input.display()
        ),
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "hello from direct import",
        "run --backend wasm with direct-imported fs.read_file",
    );
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
fn replay_dispatches_wasm_logs_for_structural_output() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    let log = dir.path().join("run.jsonl");
    fs::write(
        &file,
        "type Point = { x: Int, y: Int }\nfn main() -> Point { { x: 5, y: 8 } }",
    )
    .expect("write source");

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
    assert_stdout_trimmed(
        &run,
        "{ x: 5, y: 8 }",
        "run --backend wasm --replay-log structural output",
    );

    let replay = run_cli(
        dir.path(),
        &["replay", log.to_str().expect("utf-8 log path")],
    );
    assert_stdout_trimmed(&replay, "{ x: 5, y: 8 }", "replay wasm structural log");
}

#[test]
fn run_backend_wasm_executes_project_mode_program() {
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
    assert_stdout_trimmed(&output, "7", "wasm project-mode run");
}

#[test]
fn run_backend_wasm_displays_structural_project_output() {
    let dir = tempfile::tempdir().expect("tempdir");
    let app = dir.path().join("main.ky");
    let helper = dir.path().join("helper.ky");
    fs::write(
        &app,
        "from helper import make\nfn main() -> { x: Int, y: Int } { make() }",
    )
    .expect("write app");
    fs::write(
        &helper,
        "pub fn make() -> { x: Int, y: Int } { { x: 7, y: 9 } }",
    )
    .expect("write helper");

    let output = run_cli(
        dir.path(),
        &["run", "main.ky", "--backend", "wasm", "--project"],
    );
    assert_stdout_trimmed(
        &output,
        "{ x: 7, y: 9 }",
        "wasm project-mode structural output",
    );
}

#[test]
fn build_backend_wasm_supports_project_mode() {
    let dir = tempfile::tempdir().expect("tempdir");
    let app = dir.path().join("main.ky");
    let helper = dir.path().join("helper.ky");
    let out = dir.path().join("out").join("app.wasm");
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
            out.to_str().expect("utf-8 out path"),
        ],
    );
    assert_success(&output, "wasm project-mode build");
    assert!(out.is_file(), "expected wasm artifact at {}", out.display());
}

#[test]
fn replay_dispatches_wasm_project_logs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let app = dir.path().join("main.ky");
    let helper = dir.path().join("helper.ky");
    let log = dir.path().join("run.jsonl");
    fs::write(
        &app,
        "from helper import value\nfn main() -> Int { value() }",
    )
    .expect("write app");
    fs::write(&helper, "pub fn value() -> Int { 7 }").expect("write helper");

    let run = run_cli(
        dir.path(),
        &[
            "run",
            "main.ky",
            "--project",
            "--backend",
            "wasm",
            "--replay-log",
            log.to_str().expect("utf-8 log path"),
        ],
    );
    assert_stdout_trimmed(&run, "7", "wasm project-mode run with replay log");

    let replay = run_cli(
        dir.path(),
        &["replay", log.to_str().expect("utf-8 log path")],
    );
    assert_stdout_trimmed(&replay, "7", "replay wasm project log");
}
