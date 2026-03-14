#![allow(clippy::unwrap_used)]

use kyokara_runtime::replay::{
    CapabilityCheckEvent, CapabilityOutcome, EffectCallEvent, EffectCallOutcome, EffectKind,
    EffectOutcomeKind, ProgramFingerprintEntry, ReplayHeader, ReplayLogLine, ReplayWriter,
    RequiredByKind,
};

#[test]
fn replay_jsonl_round_trip_preserves_header_and_event_sequence() {
    let mut out = Vec::new();
    let mut writer = ReplayWriter::new(&mut out);
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

    let lines: Vec<ReplayLogLine> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<ReplayLogLine>(line).unwrap())
        .collect();

    assert!(matches!(lines[0], ReplayLogLine::Header(_)));
    assert!(matches!(lines[1], ReplayLogLine::CapabilityCheck(_)));
    assert!(matches!(lines[2], ReplayLogLine::EffectCall(_)));
}
