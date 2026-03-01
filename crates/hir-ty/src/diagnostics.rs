//! Type-checker diagnostics.

use kyokara_diagnostics::Diagnostic;
use kyokara_hir_def::item_tree::ItemTree;
use kyokara_intern::Interner;
use kyokara_span::Span;

use crate::ty::{Ty, display_ty_with_tree};

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
    /// Unknown named argument in function call.
    UnknownNamedArg { name: String },
    /// Duplicate named argument in function call.
    DuplicateNamedArg { name: String },
    /// Required parameter not provided by call arguments.
    MissingNamedArg { name: String },
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
    /// Unresolved constructor name in pattern.
    UnresolvedConstructor { name: String },
    /// Refutable pattern used in a `let` binding.
    RefutableLetPattern,
    /// Non-value symbol used in expression position.
    NonValueNameInExpr { kind: String, name: String },
    /// Multi-segment expression path used where only value names are supported.
    MultiSegmentValuePath { path: String },
    /// Record pattern with an explicit type path is not supported yet.
    UnsupportedRecordPatternPath { path: String },
    /// Path-qualified record literal where path is not a record type.
    NotARecordType { name: String },
    /// Index access on a type that does not support indexing.
    InvalidIndexTarget { ty: Ty },
    /// Method call on a type that has no such method.
    NoSuchMethod { method: String, ty: Ty },
    /// Non-hashable type used as a Map key.
    InvalidMapKey { ty: Ty },
    /// Unsortable element type used with `List.sort()`.
    UnsortableElement { ty: Ty },
}

impl TyDiagnosticData {
    /// Stable error code for this diagnostic variant.
    pub fn code(&self) -> &'static str {
        match self {
            TyDiagnosticData::TypeMismatch { .. } => "E0001",
            TyDiagnosticData::InvalidArithmeticOperand { .. } => "E0002",
            TyDiagnosticData::InvalidComparisonOperand { .. } => "E0003",
            TyDiagnosticData::InvalidNegationOperand { .. } => "E0004",
            TyDiagnosticData::InvalidNotOperand { .. } => "E0005",
            TyDiagnosticData::NotAFunction { .. } => "E0006",
            TyDiagnosticData::ArgCountMismatch { .. } => "E0007",
            TyDiagnosticData::UnknownNamedArg { .. } => "E0018",
            TyDiagnosticData::DuplicateNamedArg { .. } => "E0019",
            TyDiagnosticData::MissingNamedArg { .. } => "E0020",
            TyDiagnosticData::NoSuchField { .. } => "E0008",
            TyDiagnosticData::MissingMatchArms { .. } => "E0009",
            TyDiagnosticData::RedundantMatchArm => "E0010",
            TyDiagnosticData::EffectViolation { .. } => "E0011",
            TyDiagnosticData::UnresolvedType { .. } => "E0012",
            TyDiagnosticData::UnresolvedConstructor { .. } => "E0013",
            TyDiagnosticData::RefutableLetPattern => "E0014",
            TyDiagnosticData::NonValueNameInExpr { .. } => "E0015",
            TyDiagnosticData::MultiSegmentValuePath { .. } => "E0016",
            TyDiagnosticData::UnsupportedRecordPatternPath { .. } => "E0017",
            TyDiagnosticData::NotARecordType { .. } => "E0021",
            TyDiagnosticData::InvalidIndexTarget { .. } => "E0022",
            TyDiagnosticData::NoSuchMethod { .. } => "E0023",
            TyDiagnosticData::InvalidMapKey { .. } => "E0024",
            TyDiagnosticData::UnsortableElement { .. } => "E0025",
        }
    }

    /// The expected type, if this diagnostic carries one.
    pub fn expected_ty(&self) -> Option<&Ty> {
        match self {
            TyDiagnosticData::TypeMismatch { expected, .. } => Some(expected),
            _ => None,
        }
    }

    /// The actual type, if this diagnostic carries one.
    pub fn actual_ty(&self) -> Option<&Ty> {
        match self {
            TyDiagnosticData::TypeMismatch { actual, .. } => Some(actual),
            _ => None,
        }
    }

    /// Convert to a [`Diagnostic`] at the given span.
    pub fn into_diagnostic(
        self,
        span: Span,
        interner: &Interner,
        item_tree: &ItemTree,
    ) -> Diagnostic {
        let dt = |ty: &Ty| display_ty_with_tree(ty, interner, item_tree);
        let message = match &self {
            TyDiagnosticData::TypeMismatch { expected, actual } => {
                format!(
                    "type mismatch: expected `{}`, found `{}`",
                    dt(expected),
                    dt(actual),
                )
            }
            TyDiagnosticData::InvalidArithmeticOperand { ty } => {
                format!(
                    "arithmetic operator requires `Int` or `Float`, found `{}`",
                    dt(ty),
                )
            }
            TyDiagnosticData::InvalidComparisonOperand { ty } => {
                format!(
                    "comparison operator requires `Int` or `Float`, found `{}`",
                    dt(ty),
                )
            }
            TyDiagnosticData::InvalidNegationOperand { ty } => {
                format!("negation requires `Int` or `Float`, found `{}`", dt(ty),)
            }
            TyDiagnosticData::InvalidNotOperand { ty } => {
                format!("logical not requires `Bool`, found `{}`", dt(ty),)
            }
            TyDiagnosticData::NotAFunction { ty } => {
                format!("called expression is not a function: `{}`", dt(ty),)
            }
            TyDiagnosticData::ArgCountMismatch { expected, actual } => {
                format!("expected {expected} argument(s), found {actual}")
            }
            TyDiagnosticData::UnknownNamedArg { name } => {
                format!("unknown named argument `{name}`")
            }
            TyDiagnosticData::DuplicateNamedArg { name } => {
                format!("duplicate named argument `{name}`")
            }
            TyDiagnosticData::MissingNamedArg { name } => {
                format!("missing argument for parameter `{name}`")
            }
            TyDiagnosticData::NoSuchField { field, ty } => {
                format!("no field `{field}` on type `{}`", dt(ty),)
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
            TyDiagnosticData::UnresolvedConstructor { name } => {
                format!("unresolved constructor `{name}`")
            }
            TyDiagnosticData::RefutableLetPattern => {
                "refutable let pattern: use an irrefutable pattern or a match".into()
            }
            TyDiagnosticData::NonValueNameInExpr { kind, name } => {
                format!("{kind} name `{name}` used as value")
            }
            TyDiagnosticData::MultiSegmentValuePath { path } => {
                format!("multi-segment value path `{path}` is not supported")
            }
            TyDiagnosticData::UnsupportedRecordPatternPath { path } => {
                format!("record pattern type paths are not supported yet: `{path}`")
            }
            TyDiagnosticData::NotARecordType { name } => {
                format!("`{name}` is not a record type")
            }
            TyDiagnosticData::InvalidIndexTarget { ty } => {
                format!("cannot index into `{}`", dt(ty))
            }
            TyDiagnosticData::NoSuchMethod { method, ty } => {
                format!("no method `{method}` on type `{}`", dt(ty))
            }
            TyDiagnosticData::InvalidMapKey { ty } => {
                format!(
                    "`{}` cannot be used as a map key (only Int, String, Char, Bool, Unit)",
                    dt(ty),
                )
            }
            TyDiagnosticData::UnsortableElement { ty } => {
                format!(
                    "`{}` cannot be sorted (only Int, Float, String, Char, Bool)",
                    dt(ty),
                )
            }
        };
        Diagnostic::error(message, span)
    }
}
