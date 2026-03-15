#![allow(clippy::unwrap_used)]

use std::fs;

use kyokara_runtime::replay::{
    CapabilityCheckEvent, CapabilityOutcome, EffectCallEvent, EffectCallOutcome, EffectKind,
    EffectOutcomeKind, ProgramFingerprintEntry, ReplayHeader, ReplayLogLine, ReplayReader,
    ReplayWriter, RequiredByKind, WASM_RUNTIME, verify_program_fingerprint,
};

#[test]
fn replay_reader_round_trips_header_and_events_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("run.jsonl");
    let mut writer = ReplayWriter::new(fs::File::create(&log_path).unwrap());
    writer
        .write_header(&ReplayHeader {
            schema_version: 1,
            runtime: "interpreter".to_string(),
            entry_file: "/tmp/main.ky".to_string(),
            project_mode: false,
            program_fingerprint: vec![ProgramFingerprintEntry {
                path: "/tmp/main.ky".to_string(),
                sha256: "abc123".to_string(),
            }],
        })
        .unwrap();
    writer
        .write_capability_check(&CapabilityCheckEvent {
            seq: 0,
            capability: "io".to_string(),
            required_by_kind: RequiredByKind::Builtin,
            required_by_name: "io.println".to_string(),
            outcome: CapabilityOutcome::Allowed,
        })
        .unwrap();
    writer
        .write_effect_call(&EffectCallEvent {
            seq: 1,
            capability: "io".to_string(),
            operation: "println".to_string(),
            effect_kind: EffectKind::Write,
            required_by_name: "io.println".to_string(),
            input: serde_json::json!({ "text": "hello" }),
            outcome: EffectCallOutcome {
                kind: EffectOutcomeKind::Ok,
                value: serde_json::Value::Null,
                message: None,
            },
        })
        .unwrap();

    let mut reader = ReplayReader::from_path(&log_path).unwrap();
    assert_eq!(reader.header().schema_version, 1);
    assert!(matches!(
        reader.next_event().unwrap(),
        Some(ReplayLogLine::CapabilityCheck(_))
    ));
    assert!(matches!(
        reader.next_event().unwrap(),
        Some(ReplayLogLine::EffectCall(_))
    ));
    assert!(reader.next_event().unwrap().is_none());
}

#[test]
fn verify_program_fingerprint_rejects_source_drift() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("main.ky");
    fs::write(&file_path, "fn main() -> Int { 1 }\n").unwrap();

    let entries = vec![ProgramFingerprintEntry {
        path: file_path.canonicalize().unwrap().display().to_string(),
        sha256: "deadbeef".to_string(),
    }];
    let err = verify_program_fingerprint(&entries).expect_err("drift should fail fingerprint");
    assert!(err.to_string().contains("fingerprint"));
}

#[test]
fn replay_reader_rejects_unknown_schema_version() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("run.jsonl");
    fs::write(
        &log_path,
        r#"{"kind":"header","schema_version":99,"runtime":"interpreter","entry_file":"/tmp/main.ky","project_mode":false,"program_fingerprint":[]}"#,
    )
    .unwrap();

    let err = match ReplayReader::from_path(&log_path) {
        Ok(_) => panic!("schema version should be rejected"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("schema"));
}

#[test]
fn replay_reader_accepts_wasm_runtime_header() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("run.jsonl");
    fs::write(
        &log_path,
        format!(
            r#"{{"kind":"header","schema_version":1,"runtime":"{WASM_RUNTIME}","entry_file":"/tmp/main.ky","project_mode":false,"program_fingerprint":[]}}"#
        ),
    )
    .unwrap();

    let reader = ReplayReader::from_path(&log_path).expect("wasm runtime should be accepted");
    assert_eq!(reader.header().runtime, WASM_RUNTIME);
}
