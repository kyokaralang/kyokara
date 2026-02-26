//! Atomic refactor transactions with type-safety verification.
//!
//! Wraps the refactor engine's edit production with an in-memory
//! re-check step, so callers can gate application on verification.

use std::collections::HashMap;

use kyokara_span::{FileId, Span};

use crate::{RefactorAction, RefactorError, RefactorResult, TextEdit, apply_edits};

// ── Types ──────────────────────────────────────────────────────────

/// Outcome of post-refactor type checking.
#[derive(Debug)]
pub enum VerificationStatus {
    /// Zero diagnostics after applying edits.
    Verified,
    /// Edits introduced errors.
    Failed {
        diagnostics: Vec<VerificationDiagnostic>,
    },
    /// Verification was not attempted (e.g. `--force`).
    Skipped,
}

/// A diagnostic produced during post-refactor verification.
#[derive(Debug)]
pub struct VerificationDiagnostic {
    pub message: String,
    pub span: Option<Span>,
    pub code: Option<String>,
}

/// Result of a transactional refactor: edits + verification + patched sources.
#[derive(Debug)]
pub struct TransactionResult {
    pub refactor: RefactorResult,
    pub verification: VerificationStatus,
    /// Source text after edits applied, one entry per affected file.
    pub patched_sources: Vec<(FileId, String)>,
}

// ── Single-file transaction ────────────────────────────────────────

/// Run a refactor on a single file, apply edits in-memory, and re-check.
pub fn transact(
    source: &str,
    result: &kyokara_hir::CheckResult,
    file_id: FileId,
    action: RefactorAction,
) -> Result<TransactionResult, RefactorError> {
    let refactor = crate::refactor(result, file_id, action)?;
    let patched = apply_edits(source, &refactor.edits);

    let check = kyokara_hir::check_file(&patched);
    let verification = collect_single_verification(&check);

    Ok(TransactionResult {
        refactor,
        verification,
        patched_sources: vec![(file_id, patched)],
    })
}

/// Run a single-file refactor but skip verification.
pub fn transact_force(
    source: &str,
    result: &kyokara_hir::CheckResult,
    file_id: FileId,
    action: RefactorAction,
) -> Result<TransactionResult, RefactorError> {
    let refactor = crate::refactor(result, file_id, action)?;
    let patched = apply_edits(source, &refactor.edits);

    Ok(TransactionResult {
        refactor,
        verification: VerificationStatus::Skipped,
        patched_sources: vec![(file_id, patched)],
    })
}

// ── Multi-file transaction ─────────────────────────────────────────

/// Run a refactor across a multi-file project, apply edits in-memory,
/// write patched sources to a temp directory, and re-check the project.
pub fn transact_project(
    entry_file: &std::path::Path,
    result: &kyokara_hir::ProjectCheckResult,
    action: RefactorAction,
) -> Result<TransactionResult, RefactorError> {
    let refactor = crate::refactor_project(result, action)?;

    // Group edits by FileId.
    let mut edits_by_file: HashMap<FileId, Vec<&TextEdit>> = HashMap::new();
    for edit in &refactor.edits {
        edits_by_file.entry(edit.file_id).or_default().push(edit);
    }

    // Apply edits to each file's source.
    let mut patched_sources: Vec<(FileId, String)> = Vec::new();
    let mut patched_map: HashMap<FileId, String> = HashMap::new();

    for (_mod_path, info) in result.module_graph.iter() {
        let fid = info.file_id;
        if let Some(file_edits) = edits_by_file.get(&fid) {
            let owned: Vec<TextEdit> = file_edits.iter().map(|e| (*e).clone()).collect();
            let patched = apply_edits(&info.source, &owned);
            patched_map.insert(fid, patched.clone());
            patched_sources.push((fid, patched));
        } else {
            // No edits for this file — keep original source.
            patched_map.insert(fid, info.source.clone());
        }
    }

    // Write patched sources to a temp directory preserving filenames, then re-check.
    let temp_dir = tempfile::tempdir().map_err(|e| RefactorError::IoError {
        message: format!("failed to create temp directory: {e}"),
    })?;

    let entry_name = entry_file.file_name().unwrap_or_default();
    let mut temp_entry = temp_dir.path().join(entry_name);

    for (_mod_path, info) in result.module_graph.iter() {
        let original_path = &info.path;
        let file_name = original_path.file_name().unwrap_or_default();

        // Preserve subdirectory structure relative to the project root.
        let relative = if let Some(parent) = entry_file.parent() {
            original_path
                .strip_prefix(parent)
                .unwrap_or(std::path::Path::new(file_name))
        } else {
            std::path::Path::new(file_name)
        };

        let dest = temp_dir.path().join(relative);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| RefactorError::IoError {
                message: format!("failed to create directory {}: {e}", parent.display()),
            })?;
        }

        let source = patched_map
            .get(&info.file_id)
            .map(|s| s.as_str())
            .unwrap_or(&info.source);
        std::fs::write(&dest, source).map_err(|e| RefactorError::IoError {
            message: format!("failed to write {}: {e}", dest.display()),
        })?;

        // Track the entry file in the temp dir.
        if original_path == entry_file {
            temp_entry = dest;
        }
    }

    let check = kyokara_hir::check_project(&temp_entry);
    let verification = collect_project_verification(&check);

    // temp_dir is dropped here, cleaning up automatically.

    Ok(TransactionResult {
        refactor,
        verification,
        patched_sources,
    })
}

/// Run a multi-file refactor but skip verification.
pub fn transact_project_force(
    entry_file: &std::path::Path,
    result: &kyokara_hir::ProjectCheckResult,
    action: RefactorAction,
) -> Result<TransactionResult, RefactorError> {
    let refactor = crate::refactor_project(result, action)?;

    // Group edits by FileId.
    let mut edits_by_file: HashMap<FileId, Vec<&TextEdit>> = HashMap::new();
    for edit in &refactor.edits {
        edits_by_file.entry(edit.file_id).or_default().push(edit);
    }

    let _ = entry_file; // used only for signature consistency

    let mut patched_sources: Vec<(FileId, String)> = Vec::new();
    for (_mod_path, info) in result.module_graph.iter() {
        let fid = info.file_id;
        if let Some(file_edits) = edits_by_file.get(&fid) {
            let owned: Vec<TextEdit> = file_edits.iter().map(|e| (*e).clone()).collect();
            let patched = apply_edits(&info.source, &owned);
            patched_sources.push((fid, patched));
        }
    }

    Ok(TransactionResult {
        refactor,
        verification: VerificationStatus::Skipped,
        patched_sources,
    })
}

// ── Helpers ────────────────────────────────────────────────────────

fn collect_single_verification(check: &kyokara_hir::CheckResult) -> VerificationStatus {
    let mut diags = Vec::new();

    for err in &check.parse_errors {
        diags.push(VerificationDiagnostic {
            message: err.message.clone(),
            span: None,
            code: Some("E0100".into()),
        });
    }
    for d in &check.lowering_diagnostics {
        let code = if d.message.contains("duplicate") {
            "E0102"
        } else {
            "E0101"
        };
        diags.push(VerificationDiagnostic {
            message: d.message.clone(),
            span: Some(d.span),
            code: Some(code.into()),
        });
    }
    for (data, span) in &check.type_check.raw_diagnostics {
        let diag = data
            .clone()
            .into_diagnostic(*span, &check.interner, &check.item_tree);
        diags.push(VerificationDiagnostic {
            message: diag.message,
            span: Some(*span),
            code: Some(data.code().into()),
        });
    }

    if diags.is_empty() {
        VerificationStatus::Verified
    } else {
        VerificationStatus::Failed { diagnostics: diags }
    }
}

fn collect_project_verification(check: &kyokara_hir::ProjectCheckResult) -> VerificationStatus {
    let mut diags = Vec::new();
    let interner = &check.interner;

    for (_mod_path, errors) in &check.parse_errors {
        for err in errors {
            diags.push(VerificationDiagnostic {
                message: err.message.clone(),
                span: None,
                code: Some("E0100".into()),
            });
        }
    }
    for d in &check.lowering_diagnostics {
        let code = if d.message.contains("duplicate") {
            "E0102"
        } else {
            "E0101"
        };
        diags.push(VerificationDiagnostic {
            message: d.message.clone(),
            span: Some(d.span),
            code: Some(code.into()),
        });
    }
    for (mod_path, tc) in &check.type_checks {
        let item_tree = check.module_graph.get(mod_path).map(|i| &i.item_tree);
        for (data, span) in &tc.raw_diagnostics {
            if let Some(tree) = item_tree {
                let diag = data.clone().into_diagnostic(*span, interner, tree);
                diags.push(VerificationDiagnostic {
                    message: diag.message,
                    span: Some(*span),
                    code: Some(data.code().into()),
                });
            } else {
                diags.push(VerificationDiagnostic {
                    message: format!("{data:?}"),
                    span: Some(*span),
                    code: Some(data.code().into()),
                });
            }
        }
    }

    if diags.is_empty() {
        VerificationStatus::Verified
    } else {
        VerificationStatus::Failed { diagnostics: diags }
    }
}
