//! Quickfix refactors that promote diagnostic data into text edits.

use kyokara_hir::{CheckResult, ProjectCheckResult};
use kyokara_hir_ty::diagnostics::TyDiagnosticData;
use kyokara_span::{FileId, TextRange};
use kyokara_syntax::SyntaxNode;
use kyokara_syntax::ast::AstNode;
use kyokara_syntax::ast::nodes::{FnDef, MatchExpr};
use text_size::TextSize;

use crate::{RefactorError, RefactorResult, TextEdit};

// ── Match cases ──────────────────────────────────────────────────────

/// Add missing match cases from an exhaustiveness diagnostic (single-file).
pub fn add_missing_match_cases(
    result: &CheckResult,
    file_id: FileId,
    offset: u32,
) -> Result<RefactorResult, RefactorError> {
    let root = SyntaxNode::new_root(result.green.clone());
    for (data, span) in &result.type_check.raw_diagnostics {
        if let TyDiagnosticData::MissingMatchArms { missing } = data {
            let start: u32 = span.range.start().into();
            let end: u32 = span.range.end().into();
            if offset >= start && offset <= end {
                return build_match_cases_edit(file_id, &root, span.range, missing);
            }
        }
    }
    Err(RefactorError::NoDiagnosticAtOffset { offset })
}

/// Add missing match cases in a multi-file project.
///
/// When `target_file` is `Some`, only diagnostics in the matching module are
/// considered. This prevents offset collisions across files.
pub fn add_missing_match_cases_project(
    result: &ProjectCheckResult,
    offset: u32,
    target_file: Option<&str>,
) -> Result<RefactorResult, RefactorError> {
    for (mod_path, tc) in &result.type_checks {
        let info = result.module_graph.get(mod_path);

        // Filter by target_file if provided.
        if let Some(target) = target_file {
            let matches = info.is_some_and(|i| i.path.display().to_string() == target);
            if !matches {
                continue;
            }
        }

        let file_id = info.map(|i| i.file_id).unwrap_or(FileId(0));

        for (data, span) in &tc.raw_diagnostics {
            if let TyDiagnosticData::MissingMatchArms { missing } = data {
                let start: u32 = span.range.start().into();
                let end: u32 = span.range.end().into();
                if offset >= start && offset <= end {
                    let source = info.map(|i| i.source.as_str()).unwrap_or("");
                    let parse = kyokara_syntax::parse(source);
                    let root = SyntaxNode::new_root(parse.green);
                    return build_match_cases_edit(file_id, &root, span.range, missing);
                }
            }
        }
    }
    Err(RefactorError::NoDiagnosticAtOffset { offset })
}

/// Build the text edit for adding missing match arms.
///
/// Finds the MatchExpr in the CST at the diagnostic range, locates the
/// last arm, and inserts new arms after it.
fn build_match_cases_edit(
    file_id: FileId,
    root: &SyntaxNode,
    diag_range: TextRange,
    missing: &[String],
) -> Result<RefactorResult, RefactorError> {
    // Find the MatchExpr node at the diagnostic range.
    let match_node = root
        .descendants()
        .find_map(|n| MatchExpr::cast(n.clone()).filter(|_| n.text_range() == diag_range));

    let Some(match_expr) = match_node else {
        // Fallback: try to find any MatchExpr containing the range.
        let fallback = root.descendants().find_map(|n| {
            MatchExpr::cast(n.clone()).filter(|_| {
                let r = n.text_range();
                r.start() <= diag_range.start() && r.end() >= diag_range.end()
            })
        });
        if fallback.is_none() {
            return Err(RefactorError::NoDiagnosticAtOffset {
                offset: diag_range.start().into(),
            });
        }
        return build_match_edit_from_node(file_id, &fallback.unwrap(), missing);
    };

    build_match_edit_from_node(file_id, &match_expr, missing)
}

fn build_match_edit_from_node(
    file_id: FileId,
    match_expr: &MatchExpr,
    missing: &[String],
) -> Result<RefactorResult, RefactorError> {
    if let Some(arm_list) = match_expr.arm_list() {
        let last_arm = arm_list.arms().last();

        // Determine indentation from existing arms.
        let indent = if let Some(ref arm) = last_arm {
            extract_indent(arm.syntax())
        } else {
            "        ".to_string()
        };

        // Build new arms text.
        let new_arms: String = missing
            .iter()
            .map(|v| format!("{indent}{v} => _"))
            .collect::<Vec<_>>()
            .join("\n");

        // Insert point: after the last arm.
        let insert_offset = if let Some(arm) = last_arm {
            arm.syntax().text_range().end()
        } else {
            // No arms exist — insert after the opening of the arm list.
            arm_list.syntax().text_range().start() + TextSize::from(1)
        };

        let insert_range = TextRange::new(insert_offset, insert_offset);

        return Ok(RefactorResult {
            description: format!("add missing match arms: {}", missing.join(", ")),
            edits: vec![TextEdit {
                file_id,
                range: insert_range,
                new_text: format!("\n{new_arms}"),
            }],
            verified: false,
        });
    }

    Err(RefactorError::NoDiagnosticAtOffset {
        offset: match_expr.syntax().text_range().start().into(),
    })
}

// ── Capability ───────────────────────────────────────────────────────

/// Add missing capability annotation from an effect violation diagnostic (single-file).
pub fn add_missing_capability(
    result: &CheckResult,
    file_id: FileId,
    offset: u32,
) -> Result<RefactorResult, RefactorError> {
    let root = SyntaxNode::new_root(result.green.clone());
    for (data, span) in &result.type_check.raw_diagnostics {
        if let TyDiagnosticData::EffectViolation { missing } = data {
            let start: u32 = span.range.start().into();
            let end: u32 = span.range.end().into();
            if offset >= start && offset <= end {
                return build_capability_edit(file_id, &root, span.range, missing);
            }
        }
    }
    Err(RefactorError::NoDiagnosticAtOffset { offset })
}

/// Add missing capability in a multi-file project.
///
/// When `target_file` is `Some`, only diagnostics in the matching module are
/// considered. This prevents offset collisions across files.
pub fn add_missing_capability_project(
    result: &ProjectCheckResult,
    offset: u32,
    target_file: Option<&str>,
) -> Result<RefactorResult, RefactorError> {
    for (mod_path, tc) in &result.type_checks {
        let info = result.module_graph.get(mod_path);

        // Filter by target_file if provided.
        if let Some(target) = target_file {
            let matches = info.is_some_and(|i| i.path.display().to_string() == target);
            if !matches {
                continue;
            }
        }

        let file_id = info.map(|i| i.file_id).unwrap_or(FileId(0));

        for (data, span) in &tc.raw_diagnostics {
            if let TyDiagnosticData::EffectViolation { missing } = data {
                let start: u32 = span.range.start().into();
                let end: u32 = span.range.end().into();
                if offset >= start && offset <= end {
                    let source = info.map(|i| i.source.as_str()).unwrap_or("");
                    let parse = kyokara_syntax::parse(source);
                    let root = SyntaxNode::new_root(parse.green);
                    return build_capability_edit(file_id, &root, span.range, missing);
                }
            }
        }
    }
    Err(RefactorError::NoDiagnosticAtOffset { offset })
}

/// Build the text edit for adding a missing capability annotation.
///
/// Finds the FnDef ancestor of the diagnostic location and inserts
/// `with CapName` after the return type (or param list).
fn build_capability_edit(
    file_id: FileId,
    root: &SyntaxNode,
    diag_range: TextRange,
    missing: &[String],
) -> Result<RefactorResult, RefactorError> {
    // Find the FnDef that contains the diagnostic range.
    let token = root.token_at_offset(diag_range.start()).left_biased();
    let fn_def = token.and_then(|t| t.parent_ancestors().find_map(FnDef::cast));

    let Some(fn_def) = fn_def else {
        return Err(RefactorError::NoDiagnosticAtOffset {
            offset: diag_range.start().into(),
        });
    };

    let caps_text = missing.join(", ");

    // Find insertion point: after with_clause, return_type, or param_list.
    let insert_offset = if let Some(with_clause) = fn_def.with_clause() {
        // Extend existing with clause: insert after its end.
        // The edit will append to the existing "with X" -> "with X, Y".
        let end = with_clause.syntax().text_range().end();
        let range = TextRange::new(end, end);
        return Ok(RefactorResult {
            description: format!("add missing capabilities: {caps_text}"),
            edits: vec![TextEdit {
                file_id,
                range,
                new_text: format!(", {caps_text}"),
            }],
            verified: false,
        });
    } else if let Some(ret_type) = fn_def.return_type() {
        ret_type.syntax().text_range().end()
    } else if let Some(param_list) = fn_def.param_list() {
        param_list.syntax().text_range().end()
    } else {
        // Fallback: insert before body.
        if let Some(body) = fn_def.body() {
            body.syntax().text_range().start()
        } else {
            return Err(RefactorError::NoDiagnosticAtOffset {
                offset: diag_range.start().into(),
            });
        }
    };

    let insert_range = TextRange::new(insert_offset, insert_offset);

    Ok(RefactorResult {
        description: format!("add missing capabilities: {caps_text}"),
        edits: vec![TextEdit {
            file_id,
            range: insert_range,
            new_text: format!(" with {caps_text}"),
        }],
        verified: false,
    })
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Extract leading whitespace (indentation) for a syntax node.
fn extract_indent(node: &SyntaxNode) -> String {
    // Walk backwards from node start to find the line's leading whitespace.
    let offset = node.text_range().start();
    if let Some(root) = node.ancestors().last() {
        let full_text = root.text().to_string();
        let pos: usize = offset.into();
        // Find the start of the line.
        let line_start = full_text[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let indent = &full_text[line_start..pos];
        if indent.chars().all(|c| c == ' ' || c == '\t') {
            return indent.to_string();
        }
    }
    "        ".to_string()
}
