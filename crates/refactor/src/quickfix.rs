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
    if let Some(target) = target_file {
        let target_exists = result
            .module_graph
            .iter()
            .any(|(_, info)| info.path.display().to_string() == target);
        if !target_exists {
            return Err(RefactorError::IoError {
                message: format!("target_file `{target}` not found in project module graph"),
            });
        }
    }

    for (mod_path, tc) in &result.type_checks {
        let Some(info) = result.module_graph.get(mod_path) else {
            return Err(RefactorError::InternalError {
                message: format!(
                    "module graph missing entry for module in quickfix lookup: {:?}",
                    mod_path
                ),
            });
        };

        // Filter by target_file if provided.
        if let Some(target) = target_file
            && info.path.display().to_string() != target
        {
            continue;
        }

        let file_id = info.file_id;

        for (data, span) in &tc.raw_diagnostics {
            if let TyDiagnosticData::MissingMatchArms { missing } = data {
                let start: u32 = span.range.start().into();
                let end: u32 = span.range.end().into();
                if offset >= start && offset <= end {
                    let parse = kyokara_syntax::parse(info.source.as_str());
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
        if let Some(fb) = fallback {
            return build_match_edit_from_node(file_id, &fb, missing);
        }
        return Err(RefactorError::NoDiagnosticAtOffset {
            offset: diag_range.start().into(),
        });
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

        // Prefer an existing arm's indentation, but derive the fallback from
        // the surrounding match context instead of guessing a fixed width.
        let indent = last_arm
            .as_ref()
            .and_then(|arm| extract_indent(arm.syntax()))
            .unwrap_or_else(|| infer_child_indent(match_expr.syntax()));
        let closing_indent = line_indent(match_expr.syntax());

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

        let (insert_range, trailing_closing_indent) =
            if needs_closing_brace_newline(arm_list.syntax(), insert_offset) {
                let close_brace_start = arm_list.syntax().text_range().end() - TextSize::from(1);
                (
                    TextRange::new(insert_offset, close_brace_start),
                    format!("\n{closing_indent}"),
                )
            } else {
                (TextRange::new(insert_offset, insert_offset), String::new())
            };

        return Ok(RefactorResult {
            description: format!("add missing match arms: {}", missing.join(", ")),
            edits: vec![TextEdit {
                file_id,
                range: insert_range,
                new_text: format!("\n{new_arms}{trailing_closing_indent}"),
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
    if let Some(target) = target_file {
        let target_exists = result
            .module_graph
            .iter()
            .any(|(_, info)| info.path.display().to_string() == target);
        if !target_exists {
            return Err(RefactorError::IoError {
                message: format!("target_file `{target}` not found in project module graph"),
            });
        }
    }

    for (mod_path, tc) in &result.type_checks {
        let Some(info) = result.module_graph.get(mod_path) else {
            return Err(RefactorError::InternalError {
                message: format!(
                    "module graph missing entry for module in quickfix lookup: {:?}",
                    mod_path
                ),
            });
        };

        // Filter by target_file if provided.
        if let Some(target) = target_file
            && info.path.display().to_string() != target
        {
            continue;
        }

        let file_id = info.file_id;

        for (data, span) in &tc.raw_diagnostics {
            if let TyDiagnosticData::EffectViolation { missing } = data {
                let start: u32 = span.range.start().into();
                let end: u32 = span.range.end().into();
                if offset >= start && offset <= end {
                    let parse = kyokara_syntax::parse(info.source.as_str());
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

/// Extract exact leading whitespace for a node when it already starts a line.
fn extract_indent(node: &SyntaxNode) -> Option<String> {
    let root = node.ancestors().last()?;
    let full_text = root.text().to_string();
    let pos: usize = node.text_range().start().into();
    let line_start = full_text[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let indent = &full_text[line_start..pos];
    indent
        .chars()
        .all(is_indent_char)
        .then(|| indent.to_string())
}

/// Derive child indentation from the node's line indentation and the nearest
/// enclosing indentation step found in surrounding source text.
fn infer_child_indent(node: &SyntaxNode) -> String {
    let Some(root) = node.ancestors().last() else {
        return "    ".to_string();
    };

    let full_text = root.text().to_string();
    let base_indent = line_indent_at(&full_text, node.text_range().start().into());
    let indent_unit = infer_indent_unit(node, &full_text, &base_indent)
        .or_else(|| infer_indent_unit_from_source(&full_text))
        .unwrap_or_else(|| "    ".to_string());

    format!("{base_indent}{indent_unit}")
}

fn infer_indent_unit(node: &SyntaxNode, full_text: &str, base_indent: &str) -> Option<String> {
    node.ancestors().skip(1).find_map(|ancestor| {
        let ancestor_indent = line_indent_at(full_text, ancestor.text_range().start().into());
        if ancestor_indent.len() >= base_indent.len() || !base_indent.starts_with(&ancestor_indent)
        {
            return None;
        }

        let unit = &base_indent[ancestor_indent.len()..];
        (!unit.is_empty() && unit.chars().all(is_indent_char)).then(|| unit.to_string())
    })
}

fn infer_indent_unit_from_source(full_text: &str) -> Option<String> {
    full_text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            line.chars()
                .take_while(|c| is_indent_char(*c))
                .collect::<String>()
        })
        .filter(|indent| !indent.is_empty())
        .min_by_key(|indent| indent.len())
}

fn line_indent(node: &SyntaxNode) -> String {
    let Some(root) = node.ancestors().last() else {
        return String::new();
    };
    line_indent_at(&root.text().to_string(), node.text_range().start().into())
}

fn line_indent_at(full_text: &str, pos: usize) -> String {
    let line_start = full_text[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
    full_text[line_start..]
        .chars()
        .take_while(|c| is_indent_char(*c))
        .collect()
}

fn needs_closing_brace_newline(arm_list: &SyntaxNode, insert_offset: TextSize) -> bool {
    let Some(root) = arm_list.ancestors().last() else {
        return false;
    };
    let full_text = root.text().to_string();
    let start: usize = insert_offset.into();
    let end: usize = arm_list.text_range().end().into();
    !full_text[start..end].contains('\n')
}

fn is_indent_char(c: char) -> bool {
    c == ' ' || c == '\t'
}
