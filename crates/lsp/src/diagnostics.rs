//! Convert Kyokara diagnostics to LSP diagnostics.

use tower_lsp::lsp_types::{self, DiagnosticSeverity, NumberOrString};

use crate::db::FileAnalysis;
use crate::position::text_range_to_lsp_range;

/// Convert all diagnostics from a `FileAnalysis` into LSP diagnostics.
pub fn to_lsp_diagnostics(analysis: &FileAnalysis, source: &str) -> Vec<lsp_types::Diagnostic> {
    let mut out = Vec::new();

    // 1. Parse errors.
    for err in &analysis.parse_errors {
        out.push(lsp_types::Diagnostic {
            range: lsp_types::Range::default(),
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String("E0100".into())),
            source: Some("kyokara".into()),
            message: err.message.clone(),
            ..Default::default()
        });
    }

    // 2. Lowering diagnostics (from item tree collection / body lowering).
    for diag in &analysis.lowering_diagnostics {
        let severity = match diag.severity {
            kyokara_diagnostics::Severity::Error => DiagnosticSeverity::ERROR,
            kyokara_diagnostics::Severity::Warning => DiagnosticSeverity::WARNING,
            kyokara_diagnostics::Severity::Info => DiagnosticSeverity::INFORMATION,
            kyokara_diagnostics::Severity::Hint => DiagnosticSeverity::HINT,
        };
        out.push(lsp_types::Diagnostic {
            range: text_range_to_lsp_range(diag.span.range, source),
            severity: Some(severity),
            source: Some("kyokara".into()),
            message: diag.message.clone(),
            ..Default::default()
        });
    }

    // 3. Type-checker diagnostics.
    for (data, span) in &analysis.type_check.raw_diagnostics {
        let diag = data
            .clone()
            .into_diagnostic(*span, &analysis.interner, &analysis.item_tree);
        out.push(lsp_types::Diagnostic {
            range: text_range_to_lsp_range(span.range, source),
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String(data.code().into())),
            source: Some("kyokara".into()),
            message: diag.message,
            ..Default::default()
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::FileAnalysis;

    #[test]
    fn parse_error_produces_diagnostic() {
        let source = "fn { }";
        let result = kyokara_hir::check_file(source);
        let analysis = FileAnalysis::from_check_result(result, source.to_string());
        let diags = to_lsp_diagnostics(&analysis, source);
        assert!(!diags.is_empty(), "should have parse error diagnostics");
        assert!(diags.iter().any(|d| {
            d.code
                .as_ref()
                .is_some_and(|c| *c == NumberOrString::String("E0100".into()))
        }));
    }

    #[test]
    fn type_error_produces_diagnostic() {
        let source = "fn foo() -> Int { \"hello\" }";
        let result = kyokara_hir::check_file(source);
        let analysis = FileAnalysis::from_check_result(result, source.to_string());
        let diags = to_lsp_diagnostics(&analysis, source);
        assert!(
            diags.iter().any(|d| d
                .code
                .as_ref()
                .is_some_and(|c| *c == NumberOrString::String("E0001".into()))),
            "should have type mismatch E0001: {diags:?}"
        );
    }

    #[test]
    fn clean_file_no_diagnostics() {
        let source = "fn foo() -> Int { 42 }";
        let result = kyokara_hir::check_file(source);
        let analysis = FileAnalysis::from_check_result(result, source.to_string());
        let diags = to_lsp_diagnostics(&analysis, source);
        assert!(diags.is_empty(), "clean file should have no diagnostics");
    }
}
