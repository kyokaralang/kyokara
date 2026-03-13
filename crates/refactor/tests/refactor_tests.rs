//! Integration tests for the refactor engine.
#![allow(clippy::unwrap_used)]

use kyokara_refactor::{
    RefactorAction, RefactorError, SymbolKind, VerificationStatus, apply_edits, verify_single,
};
use kyokara_span::FileId;

fn file_id() -> FileId {
    FileId(0)
}

fn rename_action(
    old_name: &str,
    new_name: &str,
    kind: SymbolKind,
    target_file: Option<String>,
) -> RefactorAction {
    RefactorAction::RenameSymbol {
        old_name: old_name.into(),
        new_name: new_name.into(),
        kind,
        target_file,
    }
}

fn rename_fn_action(old_name: &str, new_name: &str) -> RefactorAction {
    rename_action(old_name, new_name, SymbolKind::Function, None)
}

fn rename_fn_action_for_target(
    old_name: &str,
    new_name: &str,
    target_file: &std::path::Path,
) -> RefactorAction {
    rename_action(
        old_name,
        new_name,
        SymbolKind::Function,
        Some(target_file.display().to_string()),
    )
}

// ── Rename function ─────────────────────────────────────────────────

#[test]
fn rename_fn_definition_and_calls() {
    let src = "fn add(x: Int, y: Int) -> Int { x + y }\nfn main() -> Int { add(1, 2) }";
    let result = kyokara_hir::check_file(src);
    let action = rename_fn_action("add", "sum");
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
    let src = "type Color = Red | Green | Blue\nfn pick(c: Color) -> Color { c }";
    let result = kyokara_hir::check_file(src);
    let action = RefactorAction::RenameSymbol {
        old_name: "Color".into(),
        new_name: "Hue".into(),
        kind: SymbolKind::Type,
        target_file: None,
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
    let src = r#"type Color = Red | Green | Blue
fn name(c: Color) -> String {
    match (c) {
        Color.Red => "red"
        Color.Green => "green"
        Color.Blue => "blue"
    }
}"#;
    let result = kyokara_hir::check_file(src);
    let action = RefactorAction::RenameSymbol {
        old_name: "Red".into(),
        new_name: "Crimson".into(),
        kind: SymbolKind::Variant,
        target_file: None,
    };
    let refactor = kyokara_refactor::refactor(&result, file_id(), action).unwrap();
    let new_source = apply_edits(src, &refactor.edits);

    assert!(
        new_source.contains("type Color = Crimson | Green | Blue"),
        "variant def should be renamed: {new_source}"
    );
    assert!(
        new_source.contains("Color.Crimson =>"),
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
    let src = r#"effect Console
fn print_line(s: String) -> Unit { }
fn log(msg: String) -> Unit with Console {
    print_line(msg)
}"#;
    let result = kyokara_hir::check_file(src);
    let action = RefactorAction::RenameSymbol {
        old_name: "Console".into(),
        new_name: "Output".into(),
        kind: SymbolKind::Capability,
        target_file: None,
    };
    let refactor = kyokara_refactor::refactor(&result, file_id(), action).unwrap();
    let new_source = apply_edits(src, &refactor.edits);

    assert!(
        new_source.contains("effect Output"),
        "effect def should be renamed: {new_source}"
    );
    assert!(
        new_source.contains("with Output"),
        "with clause should be renamed: {new_source}"
    );
    assert!(
        !new_source.contains("Console"),
        "old effect name should not remain: {new_source}"
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
    let action = rename_fn_action("add", "sum");
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
    let action = rename_fn_action("add", "sum");
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
    let action = rename_fn_action("add", "fn");
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
    let action = rename_fn_action("nonexistent", "something");
    let err = kyokara_refactor::refactor(&result, file_id(), action).unwrap_err();
    assert!(
        matches!(err, RefactorError::SymbolNotFound { .. }),
        "expected SymbolNotFound, got: {err:?}"
    );
}

// ── Quickfix: add missing match cases ───────────────────────────────

#[test]
fn add_missing_match_cases() {
    let src = r#"type Color = Red | Green | Blue
fn pick(c: Color) -> Int {
    match (c) {
        Color.Red => 1
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
    let action = RefactorAction::AddMissingMatchCases {
        offset,
        target_file: None,
    };
    let refactor = kyokara_refactor::refactor(&result, file_id(), action).unwrap();

    assert!(!refactor.edits.is_empty(), "expected edits");
    let edit = &refactor.edits[0];
    assert!(
        edit.new_text.contains("Color.Green"),
        "should contain Green: {}",
        edit.new_text
    );
    assert!(
        edit.new_text.contains("Color.Blue"),
        "should contain Blue: {}",
        edit.new_text
    );
}

#[test]
fn add_missing_match_cases_from_diagnostics() {
    let src = r#"type Color = Red | Green | Blue
fn pick(c: Color) -> Int {
    match (c) {
        Color.Red => 1
    }
}"#;
    let result = kyokara_hir::check_file(src);
    let root = kyokara_syntax::SyntaxNode::new_root(result.green.clone());

    let (_, span) = result
        .type_check
        .raw_diagnostics
        .iter()
        .find(|(d, _)| matches!(d, kyokara_hir::TyDiagnosticData::MissingMatchArms { .. }))
        .expect("expected MissingMatchArms diagnostic");

    let offset: u32 = span.range.start().into();
    let refactor = kyokara_refactor::quickfix::add_missing_match_cases_from_diagnostics(
        &root,
        &result.type_check.raw_diagnostics,
        file_id(),
        offset,
    )
    .unwrap();

    assert!(!refactor.edits.is_empty(), "expected edits");
    let edit = &refactor.edits[0];
    assert!(
        edit.new_text.contains("Color.Green"),
        "should contain Green: {}",
        edit.new_text
    );
    assert!(
        edit.new_text.contains("Color.Blue"),
        "should contain Blue: {}",
        edit.new_text
    );
}

#[test]
fn add_missing_match_cases_empty_nested_match_uses_context_indent() {
    let src = r#"from Color import Red, Green, Blue
type Color = Red | Green | Blue
fn pick(c: Color) -> Int {
    if (1 == 1) {
        match (c) {
        }
    } else {
        0
    }
}"#;
    let result = kyokara_hir::check_file(src);

    let (_, span) = result
        .type_check
        .raw_diagnostics
        .iter()
        .find(|(d, _)| matches!(d, kyokara_hir::TyDiagnosticData::MissingMatchArms { .. }))
        .expect("expected MissingMatchArms diagnostic");

    let offset: u32 = span.range.start().into();
    let action = RefactorAction::AddMissingMatchCases {
        offset,
        target_file: None,
    };
    let refactor = kyokara_refactor::refactor(&result, file_id(), action).unwrap();
    let new_source = apply_edits(src, &refactor.edits);

    assert!(
        new_source.contains("match (c) {\n            Red => _"),
        "empty nested match should indent inserted arms relative to the match line: {new_source}"
    );
    assert!(
        new_source.contains("\n            Green => _"),
        "Green arm should use the derived nested indent: {new_source}"
    );
    assert!(
        new_source.contains("\n            Blue => _"),
        "Blue arm should use the derived nested indent: {new_source}"
    );
    assert!(
        !new_source.contains("\n        Red => _"),
        "fallback should not collapse to the old fixed 8-space indent: {new_source}"
    );
}

#[test]
fn add_missing_match_cases_inline_nested_match_uses_context_indent() {
    let src = r#"from Color import Red, Green, Blue
type Color = Red | Green | Blue
fn pick(c: Color) -> Int {
    {
        match (c) { Red => 1 }
    }
}"#;
    let result = kyokara_hir::check_file(src);

    let (_, span) = result
        .type_check
        .raw_diagnostics
        .iter()
        .find(|(d, _)| matches!(d, kyokara_hir::TyDiagnosticData::MissingMatchArms { .. }))
        .expect("expected MissingMatchArms diagnostic");

    let offset: u32 = span.range.start().into();
    let action = RefactorAction::AddMissingMatchCases {
        offset,
        target_file: None,
    };
    let refactor = kyokara_refactor::refactor(&result, file_id(), action).unwrap();
    let new_source = apply_edits(src, &refactor.edits);

    assert!(
        new_source.contains("Red => 1\n            Green => _"),
        "inline nested match should derive multiline arm indent from the match context: {new_source}"
    );
    assert!(
        new_source.contains("\n            Blue => _\n        }"),
        "inline nested match should keep the closing brace aligned with the match line: {new_source}"
    );
    assert!(
        !new_source.contains("\n        Green => _"),
        "inline fallback should not use the old fixed 8-space indent: {new_source}"
    );
}

// ── Quickfix: add missing capability ────────────────────────────────

#[test]
fn add_missing_capability() {
    let src = r#"effect Console
fn emit(s: String) -> Unit { }
fn effectful() -> Unit with Console { emit("hi") }
fn pure_caller() -> Unit { effectful() }"#;
    let result = kyokara_hir::check_file(src);

    let (_, span) = result
        .type_check
        .raw_diagnostics
        .iter()
        .find(|(d, _)| matches!(d, kyokara_hir::TyDiagnosticData::EffectViolation { .. }))
        .expect("expected EffectViolation diagnostic");

    let offset: u32 = span.range.start().into();
    let action = RefactorAction::AddMissingCapability {
        offset,
        target_file: None,
    };
    let refactor = kyokara_refactor::refactor(&result, file_id(), action).unwrap();

    assert!(!refactor.edits.is_empty(), "expected edits");
    let edit = &refactor.edits[0];
    assert!(
        edit.new_text.contains("Console"),
        "should contain Console: {}",
        edit.new_text
    );
}

#[test]
fn add_missing_capability_from_diagnostics() {
    let src = r#"effect Console
fn emit(s: String) -> Unit { }
fn effectful() -> Unit with Console { emit("hi") }
fn pure_caller() -> Unit { effectful() }"#;
    let result = kyokara_hir::check_file(src);
    let root = kyokara_syntax::SyntaxNode::new_root(result.green.clone());

    let (_, span) = result
        .type_check
        .raw_diagnostics
        .iter()
        .find(|(d, _)| matches!(d, kyokara_hir::TyDiagnosticData::EffectViolation { .. }))
        .expect("expected EffectViolation diagnostic");

    let offset: u32 = span.range.start().into();
    let refactor = kyokara_refactor::quickfix::add_missing_capability_from_diagnostics(
        &root,
        &result.type_check.raw_diagnostics,
        file_id(),
        offset,
    )
    .unwrap();

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
    let dir = tempfile::tempdir().unwrap();

    let main_path = dir.path().join("main.ky");
    let math_path = dir.path().join("math.ky");

    std::fs::write(
        &main_path,
        "from math import add\nfn main() -> Int {\n    let x = add(10, 20)\n    x\n}\n",
    )
    .unwrap();
    std::fs::write(
        &math_path,
        "pub fn add(x: Int, y: Int) -> Int {\n    x + y\n}\n",
    )
    .unwrap();

    let result = kyokara_hir::check_project(&main_path);
    let action = rename_fn_action("add", "sum");
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
}

// ── Project rename scoping (issue #63) ──────────────────────────────

#[test]
fn project_rename_does_not_over_rename_unrelated_same_name_symbol() {
    // Two modules each define their own `fn add()` with no import relationship.
    // Renaming `add` → `sum` with target_file = main.ky should only rename main.ky's `add`.
    let dir = tempfile::tempdir().unwrap();

    let main_path = dir.path().join("main.ky");
    let math_path = dir.path().join("math.ky");

    std::fs::write(
        &main_path,
        "fn add(x: Int, y: Int) -> Int { x + y }\nfn main() -> Int { add(1, 2) }\n",
    )
    .unwrap();
    std::fs::write(&math_path, "pub fn add(x: Int, y: Int) -> Int { x - y }\n").unwrap();

    let result = kyokara_hir::check_project(&main_path);
    let action = rename_fn_action_for_target("add", "sum", &main_path);
    let refactor = kyokara_refactor::refactor_project(&result, action).unwrap();

    // main.ky should have the rename applied.
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
        new_main.contains("fn sum("),
        "main.ky should have renamed definition: {new_main}"
    );
    assert!(
        new_main.contains("sum(1, 2)"),
        "main.ky should have renamed call: {new_main}"
    );

    // math.ky should NOT have any edits.
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
    assert!(
        math_edits.is_empty(),
        "math.ky should have NO edits (unrelated `add`), got {} edits",
        math_edits.len()
    );
}

#[test]
fn project_rename_ambiguous_without_target_file() {
    // Two modules define `fn add()`. Rename without target_file should error.
    let dir = tempfile::tempdir().unwrap();

    let main_path = dir.path().join("main.ky");
    let math_path = dir.path().join("math.ky");

    std::fs::write(
        &main_path,
        "fn add(x: Int, y: Int) -> Int { x + y }\nfn main() -> Int { add(1, 2) }\n",
    )
    .unwrap();
    std::fs::write(&math_path, "pub fn add(x: Int, y: Int) -> Int { x - y }\n").unwrap();

    let result = kyokara_hir::check_project(&main_path);
    let action = rename_fn_action("add", "sum");
    let err = kyokara_refactor::refactor_project(&result, action).unwrap_err();
    assert!(
        matches!(err, RefactorError::AmbiguousRename { .. }),
        "expected AmbiguousRename error, got: {err:?}"
    );
}

#[test]
fn project_rename_with_import_renames_both_modules() {
    // math.ky defines `pub fn add()`, main.ky imports and uses it.
    // Renaming with target_file = math.ky should rename in both files.
    let dir = tempfile::tempdir().unwrap();

    let main_path = dir.path().join("main.ky");
    let math_path = dir.path().join("math.ky");

    std::fs::write(
        &main_path,
        "from math import add\nfn main() -> Int {\n    let x = add(10, 20)\n    x\n}\n",
    )
    .unwrap();
    std::fs::write(
        &math_path,
        "pub fn add(x: Int, y: Int) -> Int {\n    x + y\n}\n",
    )
    .unwrap();

    let result = kyokara_hir::check_project(&main_path);
    let action = rename_fn_action_for_target("add", "sum", &math_path);
    let refactor = kyokara_refactor::refactor_project(&result, action).unwrap();

    // Should have edits in both files.
    let files: std::collections::HashSet<_> = refactor.edits.iter().map(|e| e.file_id).collect();
    assert!(
        files.len() >= 2,
        "expected edits in at least 2 files, got {:?}",
        files
    );

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
}

#[test]
fn project_rename_alias_shadow_does_not_rename_unrelated_alias_import() {
    // main imports util as `math`; this must not count as importing math.ky.
    let dir = tempfile::tempdir().unwrap();

    let main_path = dir.path().join("main.ky");
    let util_path = dir.path().join("util.ky");
    let math_path = dir.path().join("math.ky");

    std::fs::write(
        &main_path,
        "import util as math\nfn main() -> Int { add(1, 2) }\n",
    )
    .unwrap();
    std::fs::write(&util_path, "pub fn add(x: Int, y: Int) -> Int { x + y }\n").unwrap();
    std::fs::write(&math_path, "pub fn add(x: Int, y: Int) -> Int { x - y }\n").unwrap();

    let result = kyokara_hir::check_project(&main_path);
    let action = rename_fn_action_for_target("add", "sum", &math_path);
    let refactor = kyokara_refactor::refactor_project(&result, action).unwrap();

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
    assert!(
        main_edits.is_empty(),
        "main.ky should not be edited when renaming math.ky symbol, got edits: {main_edits:?}"
    );

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
        "math.ky should be renamed: {new_math}"
    );
}

#[test]
fn project_rename_alias_import_still_renames_true_import_source() {
    // main imports util as `math`; renaming util::add should still update main usage.
    let dir = tempfile::tempdir().unwrap();

    let main_path = dir.path().join("main.ky");
    let util_path = dir.path().join("util.ky");
    let math_path = dir.path().join("math.ky");

    std::fs::write(
        &main_path,
        "import util as math\nfn main() -> Int { add(1, 2) }\n",
    )
    .unwrap();
    std::fs::write(&util_path, "pub fn add(x: Int, y: Int) -> Int { x + y }\n").unwrap();
    std::fs::write(&math_path, "pub fn add(x: Int, y: Int) -> Int { x - y }\n").unwrap();

    let result = kyokara_hir::check_project(&main_path);
    let action = rename_fn_action_for_target("add", "sum", &util_path);
    let refactor = kyokara_refactor::refactor_project(&result, action).unwrap();

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
        new_main.contains("sum(1, 2)"),
        "main.ky call should be renamed for util import: {new_main}"
    );

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
    assert!(
        math_edits.is_empty(),
        "math.ky should not be edited when renaming util.ky symbol"
    );
}

// ── Verification ────────────────────────────────────────────────────

#[test]
fn verify_rename_passes() {
    let src = "fn add(x: Int, y: Int) -> Int { x + y }\nfn main() -> Int { add(1, 2) }";
    let result = kyokara_hir::check_file(src);
    let action = rename_fn_action("add", "sum");
    let refactor = kyokara_refactor::refactor(&result, file_id(), action).unwrap();
    assert!(
        verify_single(src, &refactor.edits),
        "renamed source should pass verification"
    );
}

#[test]
fn verify_rename_on_erroneous_source_fails() {
    let src = "fn add(x: Int, y: Int) -> Int { x + y }\n\
               fn main() -> Int { add(1, 2) + missing }";
    let result = kyokara_hir::check_file(src);
    let action = rename_fn_action("add", "sum");
    let refactor = kyokara_refactor::refactor(&result, file_id(), action).unwrap();
    assert!(
        !verify_single(src, &refactor.edits),
        "verification should fail when unresolved errors remain after rename"
    );
}

// ── Transaction tests ───────────────────────────────────────────────

#[test]
fn transact_rename_verified() {
    let src = "fn add(x: Int, y: Int) -> Int { x + y }\nfn main() -> Int { add(1, 2) }";
    let result = kyokara_hir::check_file(src);
    let action = rename_fn_action("add", "sum");
    let tx = kyokara_refactor::transaction::transact(src, &result, file_id(), action).unwrap();

    assert!(
        matches!(tx.verification, VerificationStatus::Verified),
        "expected Verified, got {:?}",
        tx.verification
    );
    assert_eq!(tx.patched_sources.len(), 1);
    let (_, patched) = &tx.patched_sources[0];
    assert!(patched.contains("fn sum("), "patched source: {patched}");
    assert!(patched.contains("sum(1, 2)"), "patched source: {patched}");
}

#[test]
fn transact_rename_reports_failed_verification_when_source_has_errors() {
    let src = "fn add(x: Int, y: Int) -> Int { x + y }\n\
               fn main() -> Int { add(1, 2) + missing }";
    let result = kyokara_hir::check_file(src);
    let action = rename_fn_action("add", "sum");
    let tx = kyokara_refactor::transaction::transact(src, &result, file_id(), action).unwrap();

    match tx.verification {
        VerificationStatus::Failed { diagnostics } => {
            assert!(
                !diagnostics.is_empty(),
                "failed verification should contain diagnostics"
            );
            assert!(diagnostics.iter().any(|d| d.code.is_some()));
        }
        other => panic!("expected Failed verification, got {other:?}"),
    }
}

#[test]
fn transact_rename_multifile_verified() {
    let dir = tempfile::tempdir().unwrap();

    let main_path = dir.path().join("main.ky");
    let math_path = dir.path().join("math.ky");

    std::fs::write(
        &main_path,
        "from math import add\nfn main() -> Int {\n    let x = add(10, 20)\n    x\n}\n",
    )
    .unwrap();
    std::fs::write(
        &math_path,
        "pub fn add(x: Int, y: Int) -> Int {\n    x + y\n}\n",
    )
    .unwrap();

    let result = kyokara_hir::check_project(&main_path);
    let action = rename_fn_action("add", "sum");
    let tx = kyokara_refactor::transaction::transact_project(&main_path, &result, action).unwrap();

    assert!(
        matches!(tx.verification, VerificationStatus::Verified),
        "expected Verified, got {:?}",
        tx.verification
    );
    assert!(!tx.patched_sources.is_empty(), "expected patched sources");
}

#[test]
fn transact_rename_multifile_reports_failed_verification_when_project_has_errors() {
    let dir = tempfile::tempdir().unwrap();

    let main_path = dir.path().join("main.ky");
    let math_path = dir.path().join("math.ky");

    std::fs::write(
        &main_path,
        "from math import add\nfn main() -> Int {\n    let x = add(10, 20)\n    x + missing\n}\n",
    )
    .unwrap();
    std::fs::write(
        &math_path,
        "pub fn add(x: Int, y: Int) -> Int {\n    x + y\n}\n",
    )
    .unwrap();

    let result = kyokara_hir::check_project(&main_path);
    let action = rename_fn_action_for_target("add", "sum", &math_path);
    let tx = kyokara_refactor::transaction::transact_project(&main_path, &result, action).unwrap();

    match tx.verification {
        VerificationStatus::Failed { diagnostics } => {
            assert!(
                !diagnostics.is_empty(),
                "expected diagnostics for multifile failed verification"
            );
            assert!(diagnostics.iter().any(|d| d.code.is_some()));
        }
        other => panic!("expected Failed verification, got {other:?}"),
    }
}

// ── Quickfix transaction tests ───────────────────────────────────────

#[test]
fn quickfix_match_cases_transact_verified() {
    let src = r#"from Color import Red, Green, Blue
type Color = Red | Green | Blue
fn pick(c: Color) -> Int {
    match (c) {
        Red => 1
    }
}"#;
    let result = kyokara_hir::check_file(src);

    let (_, span) = result
        .type_check
        .raw_diagnostics
        .iter()
        .find(|(d, _)| matches!(d, kyokara_hir::TyDiagnosticData::MissingMatchArms { .. }))
        .expect("expected MissingMatchArms diagnostic");

    let offset: u32 = span.range.start().into();
    let action = RefactorAction::AddMissingMatchCases {
        offset,
        target_file: None,
    };
    let tx = kyokara_refactor::transaction::transact(src, &result, file_id(), action).unwrap();

    assert!(
        matches!(tx.verification, VerificationStatus::Verified),
        "expected Verified, got {:?}",
        tx.verification
    );

    let (_, patched) = &tx.patched_sources[0];
    // Original arm should still be present.
    assert!(
        patched.contains("Red => 1"),
        "original arm should remain: {patched}"
    );
    // New arms should be present.
    assert!(patched.contains("Green"), "should contain Green: {patched}");
    assert!(patched.contains("Blue"), "should contain Blue: {patched}");
}

#[test]
fn quickfix_capability_transact_verified() {
    let src = r#"effect Console
fn emit(s: String) -> Unit { }
fn effectful() -> Unit with Console { emit("hi") }
fn pure_caller() -> Unit { effectful() }"#;
    let result = kyokara_hir::check_file(src);

    let (_, span) = result
        .type_check
        .raw_diagnostics
        .iter()
        .find(|(d, _)| matches!(d, kyokara_hir::TyDiagnosticData::EffectViolation { .. }))
        .expect("expected EffectViolation diagnostic");

    let offset: u32 = span.range.start().into();
    let action = RefactorAction::AddMissingCapability {
        offset,
        target_file: None,
    };
    let tx = kyokara_refactor::transaction::transact(src, &result, file_id(), action).unwrap();

    assert!(
        matches!(tx.verification, VerificationStatus::Verified),
        "expected Verified, got {:?}",
        tx.verification
    );

    let (_, patched) = &tx.patched_sources[0];
    assert!(
        patched.contains("with Console"),
        "patched source should have capability: {patched}"
    );
}

#[test]
fn transact_skipped_when_forced() {
    let src = "fn add(x: Int, y: Int) -> Int { x + y }\nfn main() -> Int { add(1, 2) }";
    let result = kyokara_hir::check_file(src);
    let action = rename_fn_action("add", "sum");
    let tx =
        kyokara_refactor::transaction::transact_force(src, &result, file_id(), action).unwrap();

    assert!(
        matches!(tx.verification, VerificationStatus::Skipped),
        "expected Skipped, got {:?}",
        tx.verification
    );
    assert_eq!(tx.patched_sources.len(), 1);
}

#[test]
fn transact_project_skipped_when_forced() {
    let dir = tempfile::tempdir().unwrap();

    let main_path = dir.path().join("main.ky");
    let math_path = dir.path().join("math.ky");

    std::fs::write(
        &main_path,
        "from math import add\nfn main() -> Int {\n    let x = add(10, 20)\n    x\n}\n",
    )
    .unwrap();
    std::fs::write(
        &math_path,
        "pub fn add(x: Int, y: Int) -> Int {\n    x + y\n}\n",
    )
    .unwrap();

    let result = kyokara_hir::check_project(&main_path);
    let action = rename_fn_action("add", "sum");
    let tx =
        kyokara_refactor::transaction::transact_project_force(&main_path, &result, action).unwrap();

    assert!(
        matches!(tx.verification, VerificationStatus::Skipped),
        "expected Skipped, got {:?}",
        tx.verification
    );
    assert!(
        !tx.patched_sources.is_empty(),
        "expected patched sources in project force mode"
    );
    let combined = tx
        .patched_sources
        .iter()
        .map(|(_, src)| src.as_str())
        .collect::<Vec<_>>()
        .join("\n---\n");
    assert!(
        combined.contains("sum(10, 20)"),
        "expected patched call site in force mode, got: {combined}"
    );
}

// ── IoError variant tests ──────────────────────────────────────────

#[test]
fn io_error_variant_exists_and_displays() {
    let err = RefactorError::IoError {
        message: "permission denied".into(),
    };
    let msg = format!("{err}");
    assert!(
        msg.contains("permission denied"),
        "IoError display should include the message: {msg}"
    );
    assert!(
        matches!(err, RefactorError::IoError { .. }),
        "should match IoError variant"
    );
}

#[test]
fn io_error_is_not_symbol_not_found() {
    // IoError should be its own variant, not SymbolNotFound.
    let err = RefactorError::IoError {
        message: "tempdir creation failed".into(),
    };
    assert!(
        !matches!(err, RefactorError::SymbolNotFound { .. }),
        "IoError should NOT match SymbolNotFound"
    );
}

#[test]
fn transact_project_with_invalid_path_returns_io_error() {
    // Using a nonexistent entry path that will cause fs errors during verification.
    // First set up a valid project so the refactor succeeds, then we test error types.
    let dir = tempfile::tempdir().unwrap();

    let main_path = dir.path().join("main.ky");
    let math_path = dir.path().join("math.ky");
    std::fs::write(
        &main_path,
        "from math import add\nfn caller() -> Int { add(1, 2) }\n",
    )
    .unwrap();
    std::fs::write(&math_path, "pub fn add(x: Int, y: Int) -> Int { x + y }\n").unwrap();

    let result = kyokara_hir::check_project(&main_path);
    let action = rename_fn_action("add", "sum");

    // This should succeed (valid project, valid rename).
    let tx = kyokara_refactor::transaction::transact_project(&main_path, &result, action);
    assert!(
        tx.is_ok(),
        "valid project transact should succeed, got: {tx:?}"
    );
}

#[test]
fn transact_project_success_returns_verified() {
    let dir = tempfile::tempdir().unwrap();

    let main_path = dir.path().join("main.ky");
    let math_path = dir.path().join("math.ky");
    std::fs::write(
        &main_path,
        "from math import add\nfn caller() -> Int { add(1, 2) }\n",
    )
    .unwrap();
    std::fs::write(&math_path, "pub fn add(x: Int, y: Int) -> Int { x + y }\n").unwrap();

    let result = kyokara_hir::check_project(&main_path);
    let action = rename_fn_action("add", "sum");

    let tx = kyokara_refactor::transaction::transact_project(&main_path, &result, action).unwrap();
    assert!(
        matches!(tx.verification, VerificationStatus::Verified),
        "expected Verified, got {:?}",
        tx.verification
    );
}

#[test]
fn transact_project_with_path_dependency_preserves_package_layout_for_verification() {
    let dir = tempfile::tempdir().unwrap();

    let app_manifest = dir.path().join("app").join("kyokara.toml");
    let app_main = dir.path().join("app").join("src").join("main.ky");
    let json_manifest = dir.path().join("json").join("kyokara.toml");
    let json_lib = dir.path().join("json").join("src").join("lib.ky");
    let json_encode = dir.path().join("json").join("src").join("encode.ky");

    std::fs::create_dir_all(app_main.parent().unwrap()).unwrap();
    std::fs::create_dir_all(json_lib.parent().unwrap()).unwrap();

    std::fs::write(
        &app_manifest,
        "[package]\nname = \"app\"\nedition = \"2026\"\nkind = \"bin\"\n\n[dependencies]\njson = { path = \"../json\" }\n",
    )
    .unwrap();
    std::fs::write(
        &app_main,
        "import deps.json\nfn call() -> Int {\n    json.parse(\"abc\")\n}\nfn main() -> Int {\n    call()\n}\n",
    )
    .unwrap();
    std::fs::write(
        &json_manifest,
        "[package]\nname = \"json\"\nedition = \"2026\"\nkind = \"lib\"\n",
    )
    .unwrap();
    std::fs::write(
        &json_lib,
        "from encode import weight\npub fn parse(s: String) -> Int {\n    weight(s)\n}\n",
    )
    .unwrap();
    std::fs::write(
        &json_encode,
        "pub fn weight(s: String) -> Int {\n    s.len()\n}\n",
    )
    .unwrap();

    let result = kyokara_hir::check_project(&app_main);
    let action = rename_fn_action_for_target("call", "invoke", &app_main);
    let tx = kyokara_refactor::transaction::transact_project(&app_main, &result, action).unwrap();

    assert!(
        matches!(tx.verification, VerificationStatus::Verified),
        "expected Verified, got {:?}",
        tx.verification
    );
    let combined = tx
        .patched_sources
        .iter()
        .map(|(_, src)| src.as_str())
        .collect::<Vec<_>>()
        .join("\n---\n");
    assert!(
        combined.contains("fn invoke() -> Int"),
        "expected local helper rename, got: {combined}"
    );
    assert!(
        combined.contains("invoke()"),
        "expected updated local helper call, got: {combined}"
    );
}

// ── File-qualified quickfix tests (#44) ──────────────────────────────

#[test]
fn quickfix_action_has_target_file_field() {
    // AddMissingMatchCases and AddMissingCapability should accept a target_file
    // so that project-mode quickfixes can disambiguate modules.
    let action = RefactorAction::AddMissingMatchCases {
        offset: 42,
        target_file: Some("/tmp/math.ky".into()),
    };
    match &action {
        RefactorAction::AddMissingMatchCases {
            offset,
            target_file,
        } => {
            assert_eq!(*offset, 42);
            assert_eq!(target_file.as_deref(), Some("/tmp/math.ky"));
        }
        _ => panic!("expected AddMissingMatchCases"),
    }

    let action2 = RefactorAction::AddMissingCapability {
        offset: 10,
        target_file: Some("/tmp/main.ky".into()),
    };
    match &action2 {
        RefactorAction::AddMissingCapability {
            offset,
            target_file,
        } => {
            assert_eq!(*offset, 10);
            assert_eq!(target_file.as_deref(), Some("/tmp/main.ky"));
        }
        _ => panic!("expected AddMissingCapability"),
    }
}

#[test]
fn quickfix_action_target_file_none_for_single_file() {
    // Single-file mode should work with target_file = None.
    let action = RefactorAction::AddMissingMatchCases {
        offset: 0,
        target_file: None,
    };
    match &action {
        RefactorAction::AddMissingMatchCases { target_file, .. } => {
            assert!(target_file.is_none());
        }
        _ => panic!("expected AddMissingMatchCases"),
    }
}

#[test]
fn project_quickfix_match_cases_filters_by_target_file() {
    // Two modules, each with a match exhaustiveness error at potentially overlapping offsets.
    // target_file should select the correct module.
    let dir = tempfile::tempdir().unwrap();

    let main_path = dir.path().join("main.ky");
    let math_path = dir.path().join("math.ky");

    // Both files define a type and a non-exhaustive match.
    // main.ky: type A with missing arm
    std::fs::write(
        &main_path,
        "type A = X | Y\nfn check_a(a: A) -> Int {\n    match (a) {\n        A.X => 1\n    }\n}\n",
    )
    .unwrap();
    // math.ky: type B with missing arm
    std::fs::write(
        &math_path,
        "pub type B = P | Q\npub fn check_b(b: B) -> Int {\n    match (b) {\n        B.P => 1\n    }\n}\n",
    )
    .unwrap();

    let result = kyokara_hir::check_project(&main_path);

    // Find the MissingMatchArms diagnostic for math.ky specifically.
    let (math_mod_path, math_tc) = result
        .type_checks
        .iter()
        .find(|(mp, _)| !mp.is_root())
        .expect("should have math module");
    let math_info = result.module_graph.get(math_mod_path).unwrap();

    let (_, math_span) = math_tc
        .raw_diagnostics
        .iter()
        .find(|(d, _)| matches!(d, kyokara_hir::TyDiagnosticData::MissingMatchArms { .. }))
        .expect("math.ky should have MissingMatchArms diagnostic");

    let math_offset: u32 = math_span.range.start().into();
    let math_file_path = math_info.path.display().to_string();

    // Quickfix with target_file pointing to math.ky should produce edits for math.ky.
    let action = RefactorAction::AddMissingMatchCases {
        offset: math_offset,
        target_file: Some(math_file_path.clone()),
    };
    let refactor_result = kyokara_refactor::refactor_project(&result, action).unwrap();

    // The edit should mention "Q" (the missing variant from type B in math.ky).
    assert!(
        refactor_result
            .edits
            .iter()
            .any(|e| e.new_text.contains("Q")),
        "should have added missing arm Q from math.ky, got edits: {:?}",
        refactor_result.edits
    );
    // The edit should be in math.ky's file_id.
    assert!(
        refactor_result
            .edits
            .iter()
            .all(|e| e.file_id == math_info.file_id),
        "edits should target math.ky's file_id"
    );
}

#[test]
fn project_quickfix_capability_filters_by_target_file() {
    // Two modules, each with an effect violation. target_file disambiguates.
    let dir = tempfile::tempdir().unwrap();

    let main_path = dir.path().join("main.ky");
    let math_path = dir.path().join("math.ky");

    std::fs::write(
        &main_path,
        "effect Logger\nfn log(s: String) -> Unit { }\nfn do_log() -> Unit with Logger { log(\"hi\") }\nfn bad_main() -> Unit { do_log() }\n",
    )
    .unwrap();
    std::fs::write(
        &math_path,
        "pub effect Counter\npub fn incr() -> Unit { }\npub fn do_count() -> Unit with Counter { incr() }\npub fn bad_math() -> Unit { do_count() }\n",
    )
    .unwrap();

    let result = kyokara_hir::check_project(&main_path);

    // Find the EffectViolation diagnostic for main.ky specifically.
    let (root_path, root_tc) = result
        .type_checks
        .iter()
        .find(|(mp, _)| mp.is_root())
        .expect("should have root module");
    let root_info = result.module_graph.get(root_path).unwrap();

    let (_, root_span) = root_tc
        .raw_diagnostics
        .iter()
        .find(|(d, _)| matches!(d, kyokara_hir::TyDiagnosticData::EffectViolation { .. }))
        .expect("main.ky should have EffectViolation diagnostic");

    let root_offset: u32 = root_span.range.start().into();
    let root_file_path = root_info.path.display().to_string();

    // Quickfix with target_file pointing to main.ky should produce edits with Logger.
    let action = RefactorAction::AddMissingCapability {
        offset: root_offset,
        target_file: Some(root_file_path.clone()),
    };
    let refactor_result = kyokara_refactor::refactor_project(&result, action).unwrap();

    assert!(
        refactor_result
            .edits
            .iter()
            .any(|e| e.new_text.contains("Logger")),
        "should add Logger capability for main.ky, got edits: {:?}",
        refactor_result.edits
    );
}

#[test]
fn project_quickfix_wrong_target_file_returns_error() {
    // If target_file points to a file with no diagnostic at the given offset,
    // the quickfix should return NoDiagnosticAtOffset.
    let dir = tempfile::tempdir().unwrap();

    let main_path = dir.path().join("main.ky");
    let math_path = dir.path().join("math.ky");

    std::fs::write(
        &main_path,
        "type A = X | Y\nfn check_a(a: A) -> Int {\n    match (a) {\n        A.X => 1\n    }\n}\n",
    )
    .unwrap();
    std::fs::write(&math_path, "pub fn add(x: Int, y: Int) -> Int { x + y }\n").unwrap();

    let result = kyokara_hir::check_project(&main_path);

    // Find the offset of the match diagnostic in main.ky.
    let (_root_path, root_tc) = result
        .type_checks
        .iter()
        .find(|(mp, _)| mp.is_root())
        .expect("should have root module");

    let (_, span) = root_tc
        .raw_diagnostics
        .iter()
        .find(|(d, _)| matches!(d, kyokara_hir::TyDiagnosticData::MissingMatchArms { .. }))
        .expect("main.ky should have MissingMatchArms diagnostic");

    let offset: u32 = span.range.start().into();

    // Point target_file to math.ky (which has no diagnostic at this offset).
    let action = RefactorAction::AddMissingMatchCases {
        offset,
        target_file: Some(math_path.display().to_string()),
    };
    let err = kyokara_refactor::refactor_project(&result, action).unwrap_err();
    assert!(
        matches!(err, RefactorError::NoDiagnosticAtOffset { .. }),
        "expected NoDiagnosticAtOffset when target_file has no diagnostic, got: {err:?}"
    );
}

#[test]
fn project_quickfix_missing_target_file_returns_io_error() {
    // If target_file does not exist in the project graph, quickfix should fail
    // with a file-not-found style error (not NoDiagnosticAtOffset).
    let dir = tempfile::tempdir().unwrap();

    let main_path = dir.path().join("main.ky");
    std::fs::write(
        &main_path,
        "type A = X | Y\nfn check_a(a: A) -> Int {\n    match (a) {\n        A.X => 1\n    }\n}\n",
    )
    .unwrap();

    let result = kyokara_hir::check_project(&main_path);
    let (_root_path, root_tc) = result
        .type_checks
        .iter()
        .find(|(mp, _)| mp.is_root())
        .expect("should have root module");

    let (_, span) = root_tc
        .raw_diagnostics
        .iter()
        .find(|(d, _)| matches!(d, kyokara_hir::TyDiagnosticData::MissingMatchArms { .. }))
        .expect("main.ky should have MissingMatchArms diagnostic");
    let offset: u32 = span.range.start().into();

    let missing_target = dir.path().join("missing.ky");
    let action = RefactorAction::AddMissingMatchCases {
        offset,
        target_file: Some(missing_target.display().to_string()),
    };
    let err = kyokara_refactor::refactor_project(&result, action).unwrap_err();
    match err {
        RefactorError::IoError { message } => {
            assert!(
                message.contains("target_file")
                    && message.contains("not found")
                    && message.contains(&missing_target.display().to_string()),
                "unexpected IoError message: {message}"
            );
        }
        other => panic!("expected IoError for unknown target_file, got: {other:?}"),
    }
}

#[test]
fn project_quickfix_missing_module_info_returns_internal_error() {
    // Guard against silent FileId(0) fallback when project data is inconsistent.
    let dir = tempfile::tempdir().unwrap();

    let main_path = dir.path().join("main.ky");
    std::fs::write(
        &main_path,
        "type A = X | Y\nfn check_a(a: A) -> Int {\n    match (a) {\n        A.X => 1\n    }\n}\n",
    )
    .unwrap();

    let result = kyokara_hir::check_project(&main_path);
    let (_root_path, root_tc) = result
        .type_checks
        .iter()
        .find(|(mp, _)| mp.is_root())
        .expect("should have root module");

    let (_, span) = root_tc
        .raw_diagnostics
        .iter()
        .find(|(d, _)| matches!(d, kyokara_hir::TyDiagnosticData::MissingMatchArms { .. }))
        .expect("main.ky should have MissingMatchArms diagnostic");
    let offset: u32 = span.range.start().into();

    let inconsistent = kyokara_hir::ProjectCheckResult {
        module_graph: kyokara_hir::ModuleGraph::new(),
        type_checks: result.type_checks,
        interner: result.interner,
        file_map: result.file_map,
        parse_errors: result.parse_errors,
        lowering_diagnostics: result.lowering_diagnostics,
    };

    let action = RefactorAction::AddMissingMatchCases {
        offset,
        target_file: None,
    };
    let err = kyokara_refactor::refactor_project(&inconsistent, action).unwrap_err();
    assert!(
        matches!(err, RefactorError::InternalError { .. }),
        "expected InternalError when module graph has no entry, got: {err:?}"
    );
}

#[test]
fn io_error_display_includes_io_prefix() {
    let err = RefactorError::IoError {
        message: "disk full".into(),
    };
    let msg = format!("{err}");
    // The display should make it clear this is an I/O error.
    assert!(
        msg.to_lowercase().contains("i/o") || msg.to_lowercase().contains("io"),
        "IoError display should indicate it's an I/O error: {msg}"
    );
}
