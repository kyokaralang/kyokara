//! Integration tests for the refactor engine.

use kyokara_refactor::{RefactorAction, RefactorError, SymbolKind, apply_edits, verify_single};
use kyokara_span::FileId;

fn file_id() -> FileId {
    FileId(0)
}

// ── Rename function ─────────────────────────────────────────────────

#[test]
fn rename_fn_definition_and_calls() {
    let src = "fn add(x: Int, y: Int) -> Int { x + y }\nfn main() -> Int { add(1, 2) }";
    let result = kyokara_hir::check_file(src);
    let action = RefactorAction::RenameSymbol {
        old_name: "add".into(),
        new_name: "sum".into(),
        kind: SymbolKind::Function,
    };
    let refactor = kyokara_refactor::refactor(&result, file_id(), action).unwrap();

    assert!(!refactor.edits.is_empty(), "expected edits");
    let new_source = apply_edits(src, &refactor.edits);

    assert!(
        new_source.contains("fn sum("),
        "definition should be renamed: {new_source}"
    );
    assert!(
        new_source.contains("sum(1, 2)"),
        "call site should be renamed: {new_source}"
    );
    assert!(
        !new_source.contains("add"),
        "old name should not remain: {new_source}"
    );
}

// ── Rename type ─────────────────────────────────────────────────────

#[test]
fn rename_type() {
    let src = "type Color = | Red | Green | Blue\nfn pick(c: Color) -> Color { c }";
    let result = kyokara_hir::check_file(src);
    let action = RefactorAction::RenameSymbol {
        old_name: "Color".into(),
        new_name: "Hue".into(),
        kind: SymbolKind::Type,
    };
    let refactor = kyokara_refactor::refactor(&result, file_id(), action).unwrap();
    let new_source = apply_edits(src, &refactor.edits);

    assert!(
        new_source.contains("type Hue"),
        "type def should be renamed: {new_source}"
    );
    assert!(
        new_source.contains("c: Hue"),
        "param type should be renamed: {new_source}"
    );
    assert!(
        new_source.contains("-> Hue"),
        "return type should be renamed: {new_source}"
    );
}

// ── Rename variant ──────────────────────────────────────────────────

#[test]
fn rename_variant() {
    let src = r#"type Color = | Red | Green | Blue
fn name(c: Color) -> String {
    match c {
        Red => "red"
        Green => "green"
        Blue => "blue"
    }
}"#;
    let result = kyokara_hir::check_file(src);
    let action = RefactorAction::RenameSymbol {
        old_name: "Red".into(),
        new_name: "Crimson".into(),
        kind: SymbolKind::Variant,
    };
    let refactor = kyokara_refactor::refactor(&result, file_id(), action).unwrap();
    let new_source = apply_edits(src, &refactor.edits);

    assert!(
        new_source.contains("| Crimson"),
        "variant def should be renamed: {new_source}"
    );
    assert!(
        new_source.contains("Crimson =>"),
        "pattern should be renamed: {new_source}"
    );
    assert!(
        !new_source.contains("Red"),
        "old variant name should not remain: {new_source}"
    );
}

// ── Rename capability ───────────────────────────────────────────────

#[test]
fn rename_cap() {
    let src = r#"cap Console {
    fn print_line(s: String) -> Unit
}
fn log(msg: String) -> Unit with Console {
    print_line(msg)
}"#;
    let result = kyokara_hir::check_file(src);
    let action = RefactorAction::RenameSymbol {
        old_name: "Console".into(),
        new_name: "Output".into(),
        kind: SymbolKind::Capability,
    };
    let refactor = kyokara_refactor::refactor(&result, file_id(), action).unwrap();
    let new_source = apply_edits(src, &refactor.edits);

    assert!(
        new_source.contains("cap Output"),
        "cap def should be renamed: {new_source}"
    );
    assert!(
        new_source.contains("with Output"),
        "with clause should be renamed: {new_source}"
    );
    assert!(
        !new_source.contains("Console"),
        "old cap name should not remain: {new_source}"
    );
}

// ── Local shadowing ─────────────────────────────────────────────────

#[test]
fn rename_skips_local_shadowing() {
    let src = r#"fn add(x: Int, y: Int) -> Int { x + y }
fn main() -> Int {
    let add = 1
    add
}"#;
    let result = kyokara_hir::check_file(src);
    let action = RefactorAction::RenameSymbol {
        old_name: "add".into(),
        new_name: "sum".into(),
        kind: SymbolKind::Function,
    };
    let refactor = kyokara_refactor::refactor(&result, file_id(), action).unwrap();
    let new_source = apply_edits(src, &refactor.edits);

    // The fn definition should be renamed.
    assert!(
        new_source.contains("fn sum("),
        "fn def should be renamed: {new_source}"
    );
    // The local `let add = 1` should NOT be renamed.
    assert!(
        new_source.contains("let add = 1"),
        "local binding should NOT be renamed: {new_source}"
    );
}

// ── Error cases ─────────────────────────────────────────────────────

#[test]
fn rename_conflict_error() {
    let src = "fn add(x: Int) -> Int { x }\nfn sum(x: Int) -> Int { x }";
    let result = kyokara_hir::check_file(src);
    let action = RefactorAction::RenameSymbol {
        old_name: "add".into(),
        new_name: "sum".into(),
        kind: SymbolKind::Function,
    };
    let err = kyokara_refactor::refactor(&result, file_id(), action).unwrap_err();
    assert!(
        matches!(err, RefactorError::NameConflict { .. }),
        "expected NameConflict, got: {err:?}"
    );
}

#[test]
fn rename_keyword_error() {
    let src = "fn add(x: Int) -> Int { x }";
    let result = kyokara_hir::check_file(src);
    let action = RefactorAction::RenameSymbol {
        old_name: "add".into(),
        new_name: "fn".into(),
        kind: SymbolKind::Function,
    };
    let err = kyokara_refactor::refactor(&result, file_id(), action).unwrap_err();
    assert!(
        matches!(err, RefactorError::NewNameIsKeyword { .. }),
        "expected NewNameIsKeyword, got: {err:?}"
    );
}

#[test]
fn rename_symbol_not_found() {
    let src = "fn add(x: Int) -> Int { x }";
    let result = kyokara_hir::check_file(src);
    let action = RefactorAction::RenameSymbol {
        old_name: "nonexistent".into(),
        new_name: "something".into(),
        kind: SymbolKind::Function,
    };
    let err = kyokara_refactor::refactor(&result, file_id(), action).unwrap_err();
    assert!(
        matches!(err, RefactorError::SymbolNotFound { .. }),
        "expected SymbolNotFound, got: {err:?}"
    );
}

// ── Quickfix: add missing match cases ───────────────────────────────

#[test]
fn add_missing_match_cases() {
    let src = r#"type Color = | Red | Green | Blue
fn pick(c: Color) -> Int {
    match c {
        Red => 1
    }
}"#;
    let result = kyokara_hir::check_file(src);

    // Find the offset of the match diagnostic.
    let (_, span) = result
        .type_check
        .raw_diagnostics
        .iter()
        .find(|(d, _)| matches!(d, kyokara_hir::TyDiagnosticData::MissingMatchArms { .. }))
        .expect("expected MissingMatchArms diagnostic");

    let offset: u32 = span.range.start().into();
    let action = RefactorAction::AddMissingMatchCases { offset };
    let refactor = kyokara_refactor::refactor(&result, file_id(), action).unwrap();

    assert!(!refactor.edits.is_empty(), "expected edits");
    let edit = &refactor.edits[0];
    assert!(
        edit.new_text.contains("Green"),
        "should contain Green: {}",
        edit.new_text
    );
    assert!(
        edit.new_text.contains("Blue"),
        "should contain Blue: {}",
        edit.new_text
    );
}

// ── Quickfix: add missing capability ────────────────────────────────

#[test]
fn add_missing_capability() {
    let src = r#"cap Console {
    fn print(s: String) -> Unit
}
fn effectful() -> Unit with Console { print("hi") }
fn pure_caller() -> Unit { effectful() }"#;
    let result = kyokara_hir::check_file(src);

    let (_, span) = result
        .type_check
        .raw_diagnostics
        .iter()
        .find(|(d, _)| matches!(d, kyokara_hir::TyDiagnosticData::EffectViolation { .. }))
        .expect("expected EffectViolation diagnostic");

    let offset: u32 = span.range.start().into();
    let action = RefactorAction::AddMissingCapability { offset };
    let refactor = kyokara_refactor::refactor(&result, file_id(), action).unwrap();

    assert!(!refactor.edits.is_empty(), "expected edits");
    let edit = &refactor.edits[0];
    assert!(
        edit.new_text.contains("Console"),
        "should contain Console: {}",
        edit.new_text
    );
}

// ── Multi-file rename ───────────────────────────────────────────────

#[test]
fn rename_multifile() {
    // Set up temp files for a multi-file project.
    let dir = std::env::temp_dir().join("kyokara_refactor_test_multifile");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let main_path = dir.join("main.ky");
    let math_path = dir.join("math.ky");

    std::fs::write(
        &main_path,
        "import math\nfn main() -> Int {\n    let x = add(10, 20)\n    x\n}\n",
    )
    .unwrap();
    std::fs::write(
        &math_path,
        "pub fn add(x: Int, y: Int) -> Int {\n    x + y\n}\n",
    )
    .unwrap();

    let result = kyokara_hir::check_project(&main_path);
    let action = RefactorAction::RenameSymbol {
        old_name: "add".into(),
        new_name: "sum".into(),
        kind: SymbolKind::Function,
    };
    let refactor = kyokara_refactor::refactor_project(&result, action).unwrap();

    // Should have edits in at least 2 files (definition in math.ky + usage in main.ky).
    let files: std::collections::HashSet<_> = refactor.edits.iter().map(|e| e.file_id).collect();
    assert!(
        files.len() >= 2,
        "expected edits in at least 2 files, got {:?}",
        files
    );

    // Apply edits and verify the content.
    let math_src = std::fs::read_to_string(&math_path).unwrap();
    let math_edits: Vec<_> = refactor
        .edits
        .iter()
        .filter(|e| {
            result
                .file_map
                .path(e.file_id)
                .is_some_and(|p| p == &math_path)
        })
        .cloned()
        .collect();
    let new_math = apply_edits(&math_src, &math_edits);
    assert!(
        new_math.contains("fn sum("),
        "math.ky should have sum: {new_math}"
    );

    let main_src = std::fs::read_to_string(&main_path).unwrap();
    let main_edits: Vec<_> = refactor
        .edits
        .iter()
        .filter(|e| {
            result
                .file_map
                .path(e.file_id)
                .is_some_and(|p| p == &main_path)
        })
        .cloned()
        .collect();
    let new_main = apply_edits(&main_src, &main_edits);
    assert!(
        new_main.contains("sum(10, 20)"),
        "main.ky should have sum call: {new_main}"
    );

    // Clean up.
    let _ = std::fs::remove_dir_all(&dir);
}

// ── Verification ────────────────────────────────────────────────────

#[test]
fn verify_rename_passes() {
    let src = "fn add(x: Int, y: Int) -> Int { x + y }\nfn main() -> Int { add(1, 2) }";
    let result = kyokara_hir::check_file(src);
    let action = RefactorAction::RenameSymbol {
        old_name: "add".into(),
        new_name: "sum".into(),
        kind: SymbolKind::Function,
    };
    let refactor = kyokara_refactor::refactor(&result, file_id(), action).unwrap();
    assert!(
        verify_single(src, &refactor.edits),
        "renamed source should pass verification"
    );
}
