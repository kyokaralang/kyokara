//! `kyokara-api` — Compiler-as-API with JSON serialization.
//!
//! This crate owns all serialization. Internal compiler types stay
//! `serde`-free; instead, `api` defines its own DTO types that mirror
//! the internal structures and derive `Serialize`.
//!
//! Outputs (v0.0):
//! - `CheckOutput` with structured diagnostics and typed hole specs

use kyokara_diagnostics::Severity;
use kyokara_hir::{CheckResult, HoleInfo, TyDiagnosticData, display_ty};
use kyokara_intern::Interner;
use serde::Serialize;

/// Top-level output from the check pipeline.
#[derive(Debug, Serialize)]
pub struct CheckOutput {
    pub diagnostics: Vec<DiagnosticDto>,
    pub holes: Vec<HoleSpecDto>,
}

/// A serialisable diagnostic for JSON output.
#[derive(Debug, Serialize)]
pub struct DiagnosticDto {
    pub code: String,
    pub severity: String,
    pub message: String,
    pub span: SpanDto,
    pub expected_type: Option<String>,
    pub actual_type: Option<String>,
    pub fixes: Vec<FixDto>,
}

/// A typed hole specification.
#[derive(Debug, Serialize)]
pub struct HoleSpecDto {
    pub id: usize,
    pub name: Option<String>,
    pub expected_type: Option<String>,
    pub effects: Vec<String>,
    pub inputs: Vec<InputVarDto>,
    pub span: SpanDto,
}

/// Source span for JSON output.
#[derive(Debug, Serialize)]
pub struct SpanDto {
    pub file: String,
    pub start: u32,
    pub end: u32,
}

/// A suggested fix.
#[derive(Debug, Serialize)]
pub struct FixDto {
    pub message: String,
    pub span: SpanDto,
    pub replacement: String,
}

/// An available local variable at a hole site.
#[derive(Debug, Serialize)]
pub struct InputVarDto {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
}

/// Run the full check pipeline on source text and return structured output.
pub fn check(source: &str, file_name: &str) -> CheckOutput {
    let result = kyokara_hir::check_file(source);
    convert_result(&result, file_name)
}

fn convert_result(result: &CheckResult, file_name: &str) -> CheckOutput {
    let interner = &result.interner;
    let mut diagnostics = Vec::new();

    // Parse errors → diagnostics with code E0100.
    for err in &result.parse_errors {
        diagnostics.push(DiagnosticDto {
            code: "E0100".into(),
            severity: "error".into(),
            message: err.message.clone(),
            span: SpanDto {
                file: file_name.into(),
                start: 0,
                end: 0,
            },
            expected_type: None,
            actual_type: None,
            fixes: Vec::new(),
        });
    }

    // Lowering diagnostics → E0101 (unresolved name) / E0102 (duplicate definition).
    for diag in &result.lowering_diagnostics {
        let code = if diag.message.contains("duplicate") {
            "E0102"
        } else {
            "E0101"
        };
        diagnostics.push(convert_lowering_diagnostic(diag, code, file_name));
    }

    // Type-checker raw diagnostics.
    for (data, span) in &result.type_check.raw_diagnostics {
        diagnostics.push(convert_ty_diagnostic(data, span, interner, file_name));
    }

    // Collect hole specs from all function results.
    let mut holes = Vec::new();
    for fn_result in result.type_check.fn_results.values() {
        for hole in &fn_result.holes {
            holes.push(convert_hole(holes.len(), hole, interner, file_name));
        }
    }

    CheckOutput { diagnostics, holes }
}

fn convert_ty_diagnostic(
    data: &TyDiagnosticData,
    span: &kyokara_span::Span,
    interner: &Interner,
    file_name: &str,
) -> DiagnosticDto {
    let expected_type = data.expected_ty().map(|t| display_ty(t, interner));
    let actual_type = data.actual_ty().map(|t| display_ty(t, interner));

    // Build the message by converting through Diagnostic.
    let diag = data.clone().into_diagnostic(*span, interner);

    DiagnosticDto {
        code: data.code().into(),
        severity: "error".into(),
        message: diag.message,
        span: span_dto(span, file_name),
        expected_type,
        actual_type,
        fixes: diag
            .fixes
            .iter()
            .map(|f| convert_fix(f, file_name))
            .collect(),
    }
}

fn convert_lowering_diagnostic(
    diag: &kyokara_diagnostics::Diagnostic,
    code: &str,
    file_name: &str,
) -> DiagnosticDto {
    DiagnosticDto {
        code: code.into(),
        severity: severity_str(diag.severity),
        message: diag.message.clone(),
        span: span_dto(&diag.span, file_name),
        expected_type: None,
        actual_type: None,
        fixes: diag
            .fixes
            .iter()
            .map(|f| convert_fix(f, file_name))
            .collect(),
    }
}

fn convert_fix(fix: &kyokara_diagnostics::Fix, file_name: &str) -> FixDto {
    FixDto {
        message: fix.message.clone(),
        span: span_dto(&fix.span, file_name),
        replacement: fix.replacement.clone(),
    }
}

fn convert_hole(id: usize, hole: &HoleInfo, interner: &Interner, file_name: &str) -> HoleSpecDto {
    let expected_type = hole.expected_type.as_ref().map(|t| display_ty(t, interner));

    let effects: Vec<String> = hole
        .effect_constraints
        .caps
        .iter()
        .map(|n| n.resolve(interner).to_owned())
        .collect();

    let inputs: Vec<InputVarDto> = hole
        .available_locals
        .iter()
        .map(|(name, ty)| InputVarDto {
            name: name.resolve(interner).to_owned(),
            ty: display_ty(ty, interner),
        })
        .collect();

    HoleSpecDto {
        id,
        name: hole.name.clone(),
        expected_type,
        effects,
        inputs,
        span: span_dto(&hole.span, file_name),
    }
}

fn span_dto(span: &kyokara_span::Span, file_name: &str) -> SpanDto {
    SpanDto {
        file: file_name.into(),
        start: span.range.start().into(),
        end: span.range.end().into(),
    }
}

fn severity_str(sev: Severity) -> String {
    match sev {
        Severity::Error => "error".into(),
        Severity::Warning => "warning".into(),
        Severity::Info => "info".into(),
        Severity::Hint => "hint".into(),
    }
}
