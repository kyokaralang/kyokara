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
    pub message: String,
    pub span: Span,
    pub fixes: Vec<Fix>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>, span: Span) -> Self {
        Self {
            severity: Severity::Error,
            message: message.into(),
            span,
            fixes: Vec::new(),
        }
    }

    pub fn warning(message: impl Into<String>, span: Span) -> Self {
        Self {
            severity: Severity::Warning,
            message: message.into(),
            span,
            fixes: Vec::new(),
        }
    }

    pub fn with_fix(mut self, fix: Fix) -> Self {
        self.fixes.push(fix);
        self
    }
}
