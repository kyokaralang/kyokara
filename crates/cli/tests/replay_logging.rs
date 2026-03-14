#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use kyokara_runtime::replay::{ReplayLogLine, RequiredByKind};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_kyokara"))
}

fn write_source(contents: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(&file, contents).expect("write source");
    (dir, file)
}

fn write_project(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    for (rel, src) in files {
        let path = dir.path().join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create dir");
        }
        fs::write(&path, src).expect("write file");
    }
    let main_path = dir.path().join("main.ky");
    (dir, main_path)
}

fn run_cli(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .output()
        .expect("run kyokara")
}

#[test]
fn run_replay_log_flag_writes_header_and_effect_events_for_single_file() {
    let (_dir, file) = write_source("import io\nfn main() -> Unit { io.println(\"hello\") }");
    let log_path = file.parent().unwrap().join("run.jsonl");

    let output = run_cli(&[
        "run",
        file.to_string_lossy().as_ref(),
        "--replay-log",
        log_path.to_string_lossy().as_ref(),
    ]);

    assert!(
        output.status.success(),
        "run should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let lines: Vec<ReplayLogLine> = fs::read_to_string(&log_path)
        .expect("read replay log")
        .lines()
        .map(|line| serde_json::from_str::<ReplayLogLine>(line).expect("valid replay line"))
        .collect();
    assert!(matches!(lines[0], ReplayLogLine::Header(_)));
    assert!(matches!(lines[1], ReplayLogLine::CapabilityCheck(_)));
    assert!(matches!(lines[2], ReplayLogLine::EffectCall(_)));
}

#[test]
fn run_replay_log_flag_writes_user_effect_capability_checks_for_project_mode() {
    let (_dir, main_path) = write_project(&[
        (
            "main.ky",
            "from worker import do_work\neffect Console\nfn main() -> Int with Console { do_work() }\n",
        ),
        (
            "worker.ky",
            "effect Console\npub fn do_work() -> Int with Console { 7 }\n",
        ),
    ]);
    let log_path = main_path.parent().unwrap().join("project.jsonl");

    let output = run_cli(&[
        "run",
        "--project",
        main_path.to_string_lossy().as_ref(),
        "--replay-log",
        log_path.to_string_lossy().as_ref(),
    ]);

    assert!(
        output.status.success(),
        "project run should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let lines: Vec<ReplayLogLine> = fs::read_to_string(&log_path)
        .expect("read replay log")
        .lines()
        .map(|line| serde_json::from_str::<ReplayLogLine>(line).expect("valid replay line"))
        .collect();

    assert!(
        lines.iter().any(|line| matches!(
            line,
            ReplayLogLine::CapabilityCheck(event)
                if event.required_by_kind == RequiredByKind::UserFn
                    && event.required_by_name == "do_work"
        )),
        "expected user function capability check in log: {lines:?}"
    );
}
