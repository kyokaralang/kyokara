//! Type-checker diagnostics.

use kyokara_diagnostics::Diagnostic;
use kyokara_intern::Interner;
use kyokara_span::Span;

use crate::ty::{Ty, display_ty};

/// Diagnostic data produced by the type checker.
#[derive(Debug, Clone)]
pub enum TyDiagnosticData {
    /// Expected one type but found another.
    TypeMismatch { expected: Ty, actual: Ty },
    /// Arithmetic operator applied to non-numeric type.
    InvalidArithmeticOperand { ty: Ty },
    /// Comparison operator applied to non-comparable type.
    InvalidComparisonOperand { ty: Ty },
    /// Negation applied to non-numeric type.
    InvalidNegationOperand { ty: Ty },
    /// Logical not applied to non-Bool type.
    InvalidNotOperand { ty: Ty },
    /// Callee is not a function type.
    NotAFunction { ty: Ty },
    /// Wrong number of arguments in function call.
    ArgCountMismatch { expected: usize, actual: usize },
    /// Field access on non-record type.
    NoSuchField { field: String, ty: Ty },
    /// Missing match arms for an ADT.
    MissingMatchArms { missing: Vec<String> },
    /// Redundant match arm (already covered).
    RedundantMatchArm,
    /// Effect not in scope (missing capability).
    EffectViolation { missing: Vec<String> },
    /// Unresolved type name.
    UnresolvedType { name: String },
}

impl TyDiagnosticData {
    /// Convert to a [`Diagnostic`] at the given span.
    pub fn into_diagnostic(self, span: Span, interner: &Interner) -> Diagnostic {
        let message = match &self {
            TyDiagnosticData::TypeMismatch { expected, actual } => {
                format!(
                    "type mismatch: expected `{}`, found `{}`",
                    display_ty(expected, interner),
                    display_ty(actual, interner),
                )
            }
            TyDiagnosticData::InvalidArithmeticOperand { ty } => {
                format!(
                    "arithmetic operator requires `Int` or `Float`, found `{}`",
                    display_ty(ty, interner),
                )
            }
            TyDiagnosticData::InvalidComparisonOperand { ty } => {
                format!(
                    "comparison operator requires `Int` or `Float`, found `{}`",
                    display_ty(ty, interner),
                )
            }
            TyDiagnosticData::InvalidNegationOperand { ty } => {
                format!(
                    "negation requires `Int` or `Float`, found `{}`",
                    display_ty(ty, interner),
                )
            }
            TyDiagnosticData::InvalidNotOperand { ty } => {
                format!(
                    "logical not requires `Bool`, found `{}`",
                    display_ty(ty, interner),
                )
            }
            TyDiagnosticData::NotAFunction { ty } => {
                format!(
                    "called expression is not a function: `{}`",
                    display_ty(ty, interner),
                )
            }
            TyDiagnosticData::ArgCountMismatch { expected, actual } => {
                format!("expected {expected} argument(s), found {actual}")
            }
            TyDiagnosticData::NoSuchField { field, ty } => {
                format!("no field `{field}` on type `{}`", display_ty(ty, interner),)
            }
            TyDiagnosticData::MissingMatchArms { missing } => {
                format!("non-exhaustive match: missing {}", missing.join(", "))
            }
            TyDiagnosticData::RedundantMatchArm => "redundant match arm".into(),
            TyDiagnosticData::EffectViolation { missing } => {
                format!(
                    "effect violation: missing capabilities: {}",
                    missing.join(", ")
                )
            }
            TyDiagnosticData::UnresolvedType { name } => {
                format!("unresolved type `{name}`")
            }
        };
        Diagnostic::error(message, span)
    }
}
