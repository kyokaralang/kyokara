//! Quickfix refactors that promote diagnostic data into text edits.

use kyokara_hir::{CheckResult, ProjectCheckResult};
use kyokara_hir_ty::diagnostics::TyDiagnosticData;
use kyokara_span::FileId;

use crate::{RefactorError, RefactorResult, TextEdit};

/// Add missing match cases from an exhaustiveness diagnostic.
pub fn add_missing_match_cases(
    result: &CheckResult,
    file_id: FileId,
    offset: u32,
) -> Result<RefactorResult, RefactorError> {
    for (data, span) in &result.type_check.raw_diagnostics {
        if let TyDiagnosticData::MissingMatchArms { missing } = data {
            let start: u32 = span.range.start().into();
            let end: u32 = span.range.end().into();
            if offset >= start && offset <= end {
                let replacement = missing
                    .iter()
                    .map(|v| format!("| {v} -> _"))
                    .collect::<Vec<_>>()
                    .join("\n");

                return Ok(RefactorResult {
                    description: format!("add missing match arms: {}", missing.join(", ")),
                    edits: vec![TextEdit {
                        file_id,
                        range: span.range,
                        new_text: replacement,
                    }],
                    verified: false,
                });
            }
        }
    }

    Err(RefactorError::NoDiagnosticAtOffset { offset })
}

/// Add missing capability annotation from an effect violation diagnostic.
pub fn add_missing_capability(
    result: &CheckResult,
    file_id: FileId,
    offset: u32,
) -> Result<RefactorResult, RefactorError> {
    for (data, span) in &result.type_check.raw_diagnostics {
        if let TyDiagnosticData::EffectViolation { missing } = data {
            let start: u32 = span.range.start().into();
            let end: u32 = span.range.end().into();
            if offset >= start && offset <= end {
                let replacement = format!("with {}", missing.join(", "));

                return Ok(RefactorResult {
                    description: format!("add missing capabilities: {}", missing.join(", ")),
                    edits: vec![TextEdit {
                        file_id,
                        range: span.range,
                        new_text: replacement,
                    }],
                    verified: false,
                });
            }
        }
    }

    Err(RefactorError::NoDiagnosticAtOffset { offset })
}

/// Add missing match cases in a multi-file project.
pub fn add_missing_match_cases_project(
    result: &ProjectCheckResult,
    offset: u32,
) -> Result<RefactorResult, RefactorError> {
    for (mod_path, tc) in &result.type_checks {
        let file_id = result
            .module_graph
            .get(mod_path)
            .map(|i| i.file_id)
            .unwrap_or(FileId(0));

        for (data, span) in &tc.raw_diagnostics {
            if let TyDiagnosticData::MissingMatchArms { missing } = data {
                let start: u32 = span.range.start().into();
                let end: u32 = span.range.end().into();
                if offset >= start && offset <= end {
                    let replacement = missing
                        .iter()
                        .map(|v| format!("| {v} -> _"))
                        .collect::<Vec<_>>()
                        .join("\n");

                    return Ok(RefactorResult {
                        description: format!("add missing match arms: {}", missing.join(", ")),
                        edits: vec![TextEdit {
                            file_id,
                            range: span.range,
                            new_text: replacement,
                        }],
                        verified: false,
                    });
                }
            }
        }
    }

    Err(RefactorError::NoDiagnosticAtOffset { offset })
}

/// Add missing capability in a multi-file project.
pub fn add_missing_capability_project(
    result: &ProjectCheckResult,
    offset: u32,
) -> Result<RefactorResult, RefactorError> {
    for (mod_path, tc) in &result.type_checks {
        let file_id = result
            .module_graph
            .get(mod_path)
            .map(|i| i.file_id)
            .unwrap_or(FileId(0));

        for (data, span) in &tc.raw_diagnostics {
            if let TyDiagnosticData::EffectViolation { missing } = data {
                let start: u32 = span.range.start().into();
                let end: u32 = span.range.end().into();
                if offset >= start && offset <= end {
                    let replacement = format!("with {}", missing.join(", "));

                    return Ok(RefactorResult {
                        description: format!("add missing capabilities: {}", missing.join(", ")),
                        edits: vec![TextEdit {
                            file_id,
                            range: span.range,
                            new_text: replacement,
                        }],
                        verified: false,
                    });
                }
            }
        }
    }

    Err(RefactorError::NoDiagnosticAtOffset { offset })
}
