#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_kyokara"))
}

fn run_cli(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .output()
        .expect("run kyokara")
}

fn write_file(path: &std::path::Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create dir");
    }
    fs::write(path, contents).expect("write file");
}

fn write_project(root: &std::path::Path, files: &[(&str, &str)]) -> std::path::PathBuf {
    for (rel, contents) in files {
        write_file(&root.join(rel), contents);
    }
    root.join("main.ky")
}

#[test]
fn replay_single_file_uses_logged_read_value_without_touching_live_host() {
    let dir = tempfile::tempdir().unwrap();
    let main_path = dir.path().join("main.ky");
    let input_path = dir.path().join("input.txt");
    let log_path = dir.path().join("run.jsonl");
    write_file(
        &main_path,
        &format!(
            "import fs\nfn main() -> String {{ fs.read_file(\"{}\") }}\n",
            input_path.display()
        ),
    );
    write_file(&input_path, "old");
    write_file(&dir.path().join("caps.json"), r#"{"caps":{"fs":{}}}"#);

    let run = run_cli(&[
        "run",
        main_path.to_string_lossy().as_ref(),
        "--caps",
        dir.path().join("caps.json").to_string_lossy().as_ref(),
        "--replay-log",
        log_path.to_string_lossy().as_ref(),
    ]);
    assert!(
        run.status.success(),
        "initial run failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );

    write_file(&input_path, "new");
    let replay = run_cli(&["replay", log_path.to_string_lossy().as_ref()]);
    assert!(
        replay.status.success(),
        "replay failed: {}",
        String::from_utf8_lossy(&replay.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&replay.stdout).trim(), "old");
}

#[test]
fn replay_verify_detects_mutated_write_payload() {
    let dir = tempfile::tempdir().unwrap();
    let main_path = dir.path().join("main.ky");
    let log_path = dir.path().join("run.jsonl");
    write_file(
        &main_path,
        "import io\nfn main() -> Unit { io.println(\"hello\") }\n",
    );

    let run = run_cli(&[
        "run",
        main_path.to_string_lossy().as_ref(),
        "--replay-log",
        log_path.to_string_lossy().as_ref(),
    ]);
    assert!(
        run.status.success(),
        "initial run failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );

    let log = fs::read_to_string(&log_path)
        .unwrap()
        .replace("\"hello\"", "\"goodbye\"");
    fs::write(&log_path, log).unwrap();

    let replay = run_cli(&[
        "replay",
        log_path.to_string_lossy().as_ref(),
        "--mode",
        "verify",
    ]);
    assert!(!replay.status.success(), "verify should fail");
    assert!(
        String::from_utf8_lossy(&replay.stderr).contains("mismatch"),
        "stderr: {}",
        String::from_utf8_lossy(&replay.stderr)
    );
}

#[test]
fn replay_mode_tolerates_mutated_write_payloads() {
    let dir = tempfile::tempdir().unwrap();
    let main_path = dir.path().join("main.ky");
    let log_path = dir.path().join("run.jsonl");
    write_file(
        &main_path,
        "import io\nfn main() -> Unit { io.println(\"hello\") }\n",
    );

    let run = run_cli(&[
        "run",
        main_path.to_string_lossy().as_ref(),
        "--replay-log",
        log_path.to_string_lossy().as_ref(),
    ]);
    assert!(
        run.status.success(),
        "initial run failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );

    let log = fs::read_to_string(&log_path)
        .unwrap()
        .replace("\"hello\"", "\"goodbye\"");
    fs::write(&log_path, log).unwrap();

    let replay = run_cli(&["replay", log_path.to_string_lossy().as_ref()]);
    assert!(
        replay.status.success(),
        "plain replay should trust recorded write outcomes: {}",
        String::from_utf8_lossy(&replay.stderr)
    );
}

#[test]
fn replay_fails_on_source_fingerprint_drift() {
    let dir = tempfile::tempdir().unwrap();
    let main_path = dir.path().join("main.ky");
    let log_path = dir.path().join("run.jsonl");
    write_file(&main_path, "fn main() -> Int { 1 }\n");

    let run = run_cli(&[
        "run",
        main_path.to_string_lossy().as_ref(),
        "--replay-log",
        log_path.to_string_lossy().as_ref(),
    ]);
    assert!(
        run.status.success(),
        "initial run failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );

    write_file(&main_path, "fn main() -> Int { 2 }\n");
    let replay = run_cli(&["replay", log_path.to_string_lossy().as_ref()]);
    assert!(
        !replay.status.success(),
        "replay should fail on source drift"
    );
    assert!(
        String::from_utf8_lossy(&replay.stderr).contains("fingerprint"),
        "stderr: {}",
        String::from_utf8_lossy(&replay.stderr)
    );
}

#[test]
fn replay_rejects_unknown_schema_version() {
    let dir = tempfile::tempdir().unwrap();
    let main_path = dir.path().join("main.ky");
    let log_path = dir.path().join("run.jsonl");
    write_file(&main_path, "fn main() -> Int { 1 }\n");

    let run = run_cli(&[
        "run",
        main_path.to_string_lossy().as_ref(),
        "--replay-log",
        log_path.to_string_lossy().as_ref(),
    ]);
    assert!(
        run.status.success(),
        "initial run failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );

    let log = fs::read_to_string(&log_path)
        .unwrap()
        .replace("\"schema_version\":1", "\"schema_version\":99");
    fs::write(&log_path, log).unwrap();

    let replay = run_cli(&["replay", log_path.to_string_lossy().as_ref()]);
    assert!(
        !replay.status.success(),
        "replay should fail on unsupported schema version"
    );
    assert!(
        String::from_utf8_lossy(&replay.stderr).contains("schema"),
        "stderr: {}",
        String::from_utf8_lossy(&replay.stderr)
    );
}

#[test]
fn replay_rejects_non_interpreter_runtime_header() {
    let dir = tempfile::tempdir().unwrap();
    let main_path = dir.path().join("main.ky");
    let log_path = dir.path().join("run.jsonl");
    write_file(&main_path, "fn main() -> Int { 1 }\n");

    let run = run_cli(&[
        "run",
        main_path.to_string_lossy().as_ref(),
        "--replay-log",
        log_path.to_string_lossy().as_ref(),
    ]);
    assert!(
        run.status.success(),
        "initial run failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );

    let log = fs::read_to_string(&log_path)
        .unwrap()
        .replace("\"runtime\":\"interpreter\"", "\"runtime\":\"jit\"");
    fs::write(&log_path, log).unwrap();

    let replay = run_cli(&["replay", log_path.to_string_lossy().as_ref()]);
    assert!(
        !replay.status.success(),
        "replay should fail on unsupported runtime header"
    );
    assert!(
        String::from_utf8_lossy(&replay.stderr).contains("runtime"),
        "stderr: {}",
        String::from_utf8_lossy(&replay.stderr)
    );
}

#[test]
fn replay_reproduces_capability_denied_path() {
    let dir = tempfile::tempdir().unwrap();
    let main_path = dir.path().join("main.ky");
    let caps_path = dir.path().join("caps.json");
    let log_path = dir.path().join("deny.jsonl");
    write_file(
        &main_path,
        "import io\nfn main() -> Unit { io.println(\"blocked\") }\n",
    );
    write_file(&caps_path, r#"{"caps": {}}"#);

    let run = run_cli(&[
        "run",
        main_path.to_string_lossy().as_ref(),
        "--caps",
        caps_path.to_string_lossy().as_ref(),
        "--replay-log",
        log_path.to_string_lossy().as_ref(),
    ]);
    assert!(!run.status.success(), "initial run should fail");

    let replay = run_cli(&["replay", log_path.to_string_lossy().as_ref()]);
    assert!(!replay.status.success(), "replay should reproduce denial");
    assert!(
        String::from_utf8_lossy(&replay.stderr).contains("capability denied"),
        "stderr: {}",
        String::from_utf8_lossy(&replay.stderr)
    );
}

#[test]
fn replay_project_mode_reruns_original_project_entry() {
    let dir = tempfile::tempdir().unwrap();
    let main_path = write_project(
        dir.path(),
        &[
            (
                "main.ky",
                "from helper import answer\nfn main() -> Int { answer() }\n",
            ),
            ("helper.ky", "pub fn answer() -> Int { 7 }\n"),
        ],
    );
    let log_path = dir.path().join("project.jsonl");

    let run = run_cli(&[
        "run",
        "--project",
        main_path.to_string_lossy().as_ref(),
        "--replay-log",
        log_path.to_string_lossy().as_ref(),
    ]);
    assert!(
        run.status.success(),
        "project run failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );

    let replay = run_cli(&["replay", log_path.to_string_lossy().as_ref()]);
    assert!(
        replay.status.success(),
        "project replay failed: {}",
        String::from_utf8_lossy(&replay.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&replay.stdout).trim(), "7");
}

#[test]
fn replay_fails_on_wrong_sequence_number() {
    let dir = tempfile::tempdir().unwrap();
    let main_path = dir.path().join("main.ky");
    let log_path = dir.path().join("run.jsonl");
    write_file(
        &main_path,
        "import io\nfn main() -> Unit { io.println(\"hello\") }\n",
    );

    let run = run_cli(&[
        "run",
        main_path.to_string_lossy().as_ref(),
        "--replay-log",
        log_path.to_string_lossy().as_ref(),
    ]);
    assert!(
        run.status.success(),
        "initial run failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );

    let log = fs::read_to_string(&log_path)
        .unwrap()
        .replace("\"seq\":1", "\"seq\":9");
    fs::write(&log_path, log).unwrap();

    let replay = run_cli(&["replay", log_path.to_string_lossy().as_ref()]);
    assert!(!replay.status.success(), "replay should fail on wrong seq");
    assert!(
        String::from_utf8_lossy(&replay.stderr).contains("mismatch"),
        "stderr: {}",
        String::from_utf8_lossy(&replay.stderr)
    );
}
