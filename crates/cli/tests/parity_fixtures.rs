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

#[test]
fn parity_fixtures_check_and_run() {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_kyokara"));
    let fixtures = load_fixtures();
    assert!(
        !fixtures.is_empty(),
        "expected at least one parity fixture in {}",
        fixtures_root().display()
    );

    for fixture in fixtures {
        run_fixture(&bin, &fixture);
    }
}

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/parity")
}

fn load_fixtures() -> Vec<Fixture> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(fixtures_root())
        .expect("failed to read parity fixture directory")
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

    run_check(bin, fixture, &entry_abs, project, check_expect);
    run_run(bin, fixture, &entry_abs, project, run_expect);
}

fn run_check(bin: &Path, fixture: &Fixture, entry_abs: &Path, project: bool, expect: &Value) {
    let mut args = vec!["check", "--format", "json"];
    if project {
        args.push("--project");
    }

    let entry_arg = entry_abs.to_string_lossy().into_owned();
    let output = Command::new(bin)
        .args(&args)
        .arg(&entry_arg)
        .output()
        .unwrap_or_else(|e| panic!("fixture `{}` failed to run check: {e}", fixture.name));

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        !stdout.trim().is_empty(),
        "fixture `{}` check emitted empty stdout; stderr: {}",
        fixture.name,
        stderr
    );

    let json: Value = serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!(
            "fixture `{}` check output is not valid JSON: {e}\nstdout:\n{}\nstderr:\n{}",
            fixture.name, stdout, stderr
        )
    });

    let expected_ok = expect
        .get("ok")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| panic!("fixture `{}` check.expect.ok missing/bad", fixture.name));
    assert_eq!(
        output.status.success(),
        expected_ok,
        "fixture `{}` check exit mismatch\nstdout:\n{}\nstderr:\n{}",
        fixture.name,
        stdout,
        stderr
    );

    let diagnostics = json
        .get("diagnostics")
        .and_then(Value::as_array)
        .unwrap_or_else(|| {
            panic!(
                "fixture `{}` check JSON missing diagnostics array",
                fixture.name
            )
        });

    if let Some(codes) = expect.get("codes_all").and_then(Value::as_array) {
        let actual_codes: Vec<&str> = diagnostics
            .iter()
            .filter_map(|d| d.get("code").and_then(Value::as_str))
            .collect();
        for code in codes {
            let code_str = code.as_str().unwrap_or_else(|| {
                panic!(
                    "fixture `{}` check.codes_all entries must be strings",
                    fixture.name
                )
            });
            assert!(
                actual_codes.contains(&code_str),
                "fixture `{}` missing diagnostic code `{}` in {:?}",
                fixture.name,
                code_str,
                actual_codes
            );
        }
    }

    if let Some(messages) = expect.get("messages_all").and_then(Value::as_array) {
        let actual_messages: Vec<&str> = diagnostics
            .iter()
            .filter_map(|d| d.get("message").and_then(Value::as_str))
            .collect();
        for msg in messages {
            let msg_str = msg.as_str().unwrap_or_else(|| {
                panic!(
                    "fixture `{}` check.messages_all entries must be strings",
                    fixture.name
                )
            });
            assert!(
                actual_messages.iter().any(|m| m.contains(msg_str)),
                "fixture `{}` missing diagnostic message containing `{}` in {:?}",
                fixture.name,
                msg_str,
                actual_messages
            );
        }
    }

    if let Some(edge_expectations) = expect.get("call_edges").and_then(Value::as_array) {
        for edge in edge_expectations {
            assert_call_edges(&json, fixture, edge);
        }
    }
}

fn assert_call_edges(json: &Value, fixture: &Fixture, edge: &Value) {
    let caller = edge
        .get("caller")
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            panic!(
                "fixture `{}` call_edges entry missing `caller`",
                fixture.name
            )
        });

    let functions = json
        .get("symbol_graph")
        .and_then(|sg| sg.get("functions"))
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("fixture `{}` missing symbol_graph.functions", fixture.name));

    let caller_fn = functions
        .iter()
        .find(|f| f.get("name").and_then(Value::as_str) == Some(caller))
        .unwrap_or_else(|| panic!("fixture `{}` missing function `{}`", fixture.name, caller));

    let calls: Vec<String> = caller_fn
        .get("calls")
        .and_then(Value::as_array)
        .unwrap_or_else(|| {
            panic!(
                "fixture `{}` function `{}` missing calls",
                fixture.name, caller
            )
        })
        .iter()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect();

    if let Some(contains) = edge.get("contains").and_then(Value::as_array) {
        for expected in contains {
            let expected = expected.as_str().unwrap_or_else(|| {
                panic!(
                    "fixture `{}` call_edges.contains entries must be strings",
                    fixture.name
                )
            });
            assert!(
                calls.iter().any(|c| c == expected),
                "fixture `{}` caller `{}` missing expected edge `{}` in {:?}",
                fixture.name,
                caller,
                expected,
                calls
            );
        }
    }

    if let Some(not_contains) = edge.get("not_contains").and_then(Value::as_array) {
        for unexpected in not_contains {
            let unexpected = unexpected.as_str().unwrap_or_else(|| {
                panic!(
                    "fixture `{}` call_edges.not_contains entries must be strings",
                    fixture.name
                )
            });
            assert!(
                calls.iter().all(|c| c != unexpected),
                "fixture `{}` caller `{}` unexpectedly contains edge `{}` in {:?}",
                fixture.name,
                caller,
                unexpected,
                calls
            );
        }
    }

    if let Some(counts) = edge.get("counts").and_then(Value::as_object) {
        for (edge_id, count) in counts {
            let expected_count = count.as_u64().unwrap_or_else(|| {
                panic!(
                    "fixture `{}` call_edges.counts values must be integers",
                    fixture.name
                )
            }) as usize;
            let actual_count = calls.iter().filter(|c| c.as_str() == edge_id).count();
            assert_eq!(
                actual_count, expected_count,
                "fixture `{}` caller `{}` expected edge `{}` count {} but got {} in {:?}",
                fixture.name, caller, edge_id, expected_count, actual_count, calls
            );
        }
    }
}

fn run_run(bin: &Path, fixture: &Fixture, entry_abs: &Path, project: bool, expect: &Value) {
    let mut args = vec!["run"];
    if project {
        args.push("--project");
    }

    let entry_arg = entry_abs.to_string_lossy().into_owned();
    let output = Command::new(bin)
        .args(&args)
        .arg(&entry_arg)
        .output()
        .unwrap_or_else(|e| panic!("fixture `{}` failed to run run: {e}", fixture.name));

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    let expected_ok = expect
        .get("ok")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| panic!("fixture `{}` run.expect.ok missing/bad", fixture.name));
    assert_eq!(
        output.status.success(),
        expected_ok,
        "fixture `{}` run exit mismatch\nstdout:\n{}\nstderr:\n{}",
        fixture.name,
        stdout,
        stderr
    );

    if let Some(expected_stdout) = expect.get("stdout_contains").and_then(Value::as_array) {
        for fragment in expected_stdout {
            let fragment = fragment.as_str().unwrap_or_else(|| {
                panic!(
                    "fixture `{}` run.stdout_contains entries must be strings",
                    fixture.name
                )
            });
            assert!(
                stdout.contains(fragment),
                "fixture `{}` run stdout missing fragment `{}`\nstdout:\n{}",
                fixture.name,
                fragment,
                stdout
            );
        }
    }

    if let Some(expected_stderr) = expect.get("stderr_contains").and_then(Value::as_array) {
        for fragment in expected_stderr {
            let fragment = fragment.as_str().unwrap_or_else(|| {
                panic!(
                    "fixture `{}` run.stderr_contains entries must be strings",
                    fixture.name
                )
            });
            assert!(
                stderr.contains(fragment),
                "fixture `{}` run stderr missing fragment `{}`\nstderr:\n{}",
                fixture.name,
                fragment,
                stderr
            );
        }
    }
}
