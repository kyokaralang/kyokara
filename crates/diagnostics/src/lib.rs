//! `kyokara-diagnostics` — Compiler diagnostic types.
//!
//! Defines [`Diagnostic`], [`Severity`], and [`Fix`] — the uniform
//! error/warning representation used by every compiler phase.

use kyokara_span::Span;

/// How severe a diagnostic is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    Error,
    Warning,
    Info,
    Hint,
}

/// Structured classification for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticKind {
    General,
    DuplicateDefinition,
    UnresolvedName,
}

/// A suggested fix for a diagnostic.
#[derive(Debug, Clone)]
pub struct Fix {
    pub message: String,
    pub span: Span,
    pub replacement: String,
}

/// A compiler diagnostic (error, warning, etc.).
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub kind: DiagnosticKind,
    pub message: String,
    pub span: Span,
    pub fixes: Vec<Fix>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>, span: Span) -> Self {
        Self::error_with_kind(message, span, DiagnosticKind::General)
    }

    pub fn error_with_kind(message: impl Into<String>, span: Span, kind: DiagnosticKind) -> Self {
        Self {
            severity: Severity::Error,
            kind,
            message: message.into(),
            span,
            fixes: Vec::new(),
        }
    }

    pub fn warning(message: impl Into<String>, span: Span) -> Self {
        Self {
            severity: Severity::Warning,
            kind: DiagnosticKind::General,
            message: message.into(),
            span,
            fixes: Vec::new(),
        }
    }

    pub fn with_kind(mut self, kind: DiagnosticKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn with_fix(mut self, fix: Fix) -> Self {
        self.fixes.push(fix);
        self
    }
}
