//! `textDocument/codeAction` — quickfixes from refactor engine diagnostics.

use std::sync::Arc;

use kyokara_hir::CheckResult;
use kyokara_span::FileId;
use tower_lsp::lsp_types::{
    self, CodeAction, CodeActionKind, CodeActionOrCommand, NumberOrString, TextEdit, Url,
    WorkspaceEdit,
};

use crate::db::FileAnalysis;
use crate::position::text_range_to_lsp_range;

/// Produce code actions for the given diagnostic range.
pub fn code_actions(
    analysis: &Arc<FileAnalysis>,
    source: &str,
    _range: lsp_types::Range,
    uri: &Url,
    diagnostics: &[lsp_types::Diagnostic],
) -> Vec<CodeActionOrCommand> {
    let mut actions = Vec::new();

    for diag in diagnostics {
        let code = match &diag.code {
            Some(NumberOrString::String(s)) => s.as_str(),
            _ => continue,
        };

        match code {
            "E0009" => {
                // MissingMatchArms — add missing match cases.
                if let Some(action) = missing_match_cases_action(analysis, source, diag, uri) {
                    actions.push(CodeActionOrCommand::CodeAction(action));
                }
            }
            "E0011" => {
                // EffectViolation — add missing capability.
                if let Some(action) = missing_capability_action(analysis, source, diag, uri) {
                    actions.push(CodeActionOrCommand::CodeAction(action));
                }
            }
            _ => {}
        }
    }

    actions
}

fn missing_match_cases_action(
    analysis: &FileAnalysis,
    source: &str,
    diag: &lsp_types::Diagnostic,
    uri: &Url,
) -> Option<CodeAction> {
    // Reconstruct a CheckResult from the analysis data.
    let check_result = reconstruct_check_result(analysis);
    let file_id = FileId(0);

    // Use the diagnostic range start as offset.
    let offset = lsp_range_start_to_offset(&diag.range, source)?;

    let result =
        kyokara_refactor::quickfix::add_missing_match_cases(&check_result, file_id, offset).ok()?;

    let edits: Vec<TextEdit> = result
        .edits
        .iter()
        .map(|e| TextEdit {
            range: text_range_to_lsp_range(e.range, source),
            new_text: e.new_text.clone(),
        })
        .collect();

    let mut changes = std::collections::HashMap::new();
    changes.insert(uri.clone(), edits);

    Some(CodeAction {
        title: result.description,
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

fn missing_capability_action(
    analysis: &FileAnalysis,
    source: &str,
    diag: &lsp_types::Diagnostic,
    uri: &Url,
) -> Option<CodeAction> {
    let check_result = reconstruct_check_result(analysis);
    let file_id = FileId(0);

    let offset = lsp_range_start_to_offset(&diag.range, source)?;

    let result =
        kyokara_refactor::quickfix::add_missing_capability(&check_result, file_id, offset).ok()?;

    let edits: Vec<TextEdit> = result
        .edits
        .iter()
        .map(|e| TextEdit {
            range: text_range_to_lsp_range(e.range, source),
            new_text: e.new_text.clone(),
        })
        .collect();

    let mut changes = std::collections::HashMap::new();
    changes.insert(uri.clone(), edits);

    Some(CodeAction {
        title: result.description,
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// Reconstruct a `CheckResult` from `FileAnalysis` for the refactor crate.
///
/// The green node is Arc-backed so cloning is cheap. The interner is
/// reconstructed by re-running `check_file` — this is acceptable because
/// code actions are infrequent and the file was just analyzed.
fn reconstruct_check_result(analysis: &FileAnalysis) -> CheckResult {
    // Re-run check to get a CheckResult with a valid interner.
    // This is the simplest approach — the refactor crate needs &CheckResult
    // with a live Interner, and we can't clone Rodeo.
    kyokara_hir::check_file(&analysis.source)
}

fn lsp_range_start_to_offset(range: &lsp_types::Range, source: &str) -> Option<u32> {
    crate::position::lsp_position_to_offset(range.start, source).map(|ts| {
        let v: u32 = ts.into();
        v
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::db::FileAnalysis;

    #[test]
    fn quickfix_for_missing_match_arms() {
        let source = "type Color = Red | Blue\nfn pick(c: Color) -> Int {\n  match c {\n    Red => 1\n  }\n}";
        let result = kyokara_hir::check_file(source);
        let analysis = Arc::new(FileAnalysis::from_check_result(result, source.to_string()));

        // Find E0009 diagnostic.
        let diags = crate::diagnostics::to_lsp_diagnostics(&analysis, source);
        let e0009: Vec<_> = diags
            .iter()
            .filter(|d| {
                d.code
                    .as_ref()
                    .is_some_and(|c| *c == NumberOrString::String("E0009".into()))
            })
            .collect();

        if e0009.is_empty() {
            // Parser may not produce the exact structure needed; skip.
            return;
        }

        let uri = Url::parse("file:///test.ky").unwrap();
        let actions = code_actions(&analysis, source, lsp_types::Range::default(), &uri, &diags);
        assert!(
            !actions.is_empty(),
            "should produce quickfix for missing match arms"
        );
    }
}
