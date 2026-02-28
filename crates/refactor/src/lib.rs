//! `kyokara-refactor` — Semantic refactor engine.
//!
//! Produces structured text edits for semantically correct code
//! transformations. All edits are CST-based (source-preserving).
//!
//! Supported refactors:
//! - **Rename symbol** — functions, types, capabilities, variants
//! - **Add missing match cases** — from exhaustiveness diagnostics
//! - **Add missing capability** — from effect violation diagnostics

pub mod quickfix;
pub mod rename;
pub mod transaction;

pub use transaction::{TransactionResult, VerificationDiagnostic, VerificationStatus};

use kyokara_span::{FileId, TextRange};

// ── Core types ──────────────────────────────────────────────────────

/// A single text replacement in a source file.
#[derive(Debug, Clone)]
pub struct TextEdit {
    pub file_id: FileId,
    pub range: TextRange,
    pub new_text: String,
}

/// The result of a successful refactor operation.
#[derive(Debug)]
pub struct RefactorResult {
    pub description: String,
    pub edits: Vec<TextEdit>,
    /// Set to `true` after post-refactor re-check passes with zero diagnostics.
    pub verified: bool,
}

/// A refactor action to perform.
#[derive(Debug, Clone)]
pub enum RefactorAction {
    RenameSymbol {
        old_name: String,
        new_name: String,
        kind: SymbolKind,
        /// In project mode, the file path containing the target symbol's
        /// definition. Required when multiple modules define the same name.
        target_file: Option<String>,
    },
    AddMissingMatchCases {
        offset: u32,
        /// In project mode, the file path that contains the target diagnostic.
        /// `None` for single-file mode (selects the only file).
        target_file: Option<String>,
    },
    AddMissingCapability {
        offset: u32,
        /// In project mode, the file path that contains the target diagnostic.
        /// `None` for single-file mode (selects the only file).
        target_file: Option<String>,
    },
}

/// The kind of symbol being refactored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Type,
    Capability,
    Variant,
}

/// Errors that can occur during a refactor operation.
#[derive(Debug)]
pub enum RefactorError {
    SymbolNotFound {
        name: String,
        kind: SymbolKind,
    },
    NameConflict {
        name: String,
        existing_kind: SymbolKind,
    },
    NewNameIsKeyword {
        name: String,
    },
    NoDiagnosticAtOffset {
        offset: u32,
    },
    AmbiguousRename {
        name: String,
        kind: SymbolKind,
        files: Vec<String>,
    },
    IoError {
        message: String,
    },
    InternalError {
        message: String,
    },
}

impl std::fmt::Display for RefactorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RefactorError::SymbolNotFound { name, kind } => {
                write!(f, "{kind:?} `{name}` not found in module scope")
            }
            RefactorError::NameConflict {
                name,
                existing_kind,
            } => {
                write!(f, "name `{name}` already exists as {existing_kind:?}")
            }
            RefactorError::NewNameIsKeyword { name } => {
                write!(f, "`{name}` is a keyword and cannot be used as a name")
            }
            RefactorError::NoDiagnosticAtOffset { offset } => {
                write!(f, "no applicable diagnostic at offset {offset}")
            }
            RefactorError::AmbiguousRename { name, kind, files } => {
                write!(
                    f,
                    "{kind:?} `{name}` is defined in multiple modules ({}); specify target_file to disambiguate",
                    files.join(", ")
                )
            }
            RefactorError::IoError { message } => {
                write!(f, "I/O error: {message}")
            }
            RefactorError::InternalError { message } => {
                write!(f, "internal error: {message}")
            }
        }
    }
}

// ── Entry points ────────────────────────────────────────────────────

/// Run a refactor on a single-file check result.
pub fn refactor(
    result: &kyokara_hir::CheckResult,
    file_id: FileId,
    action: RefactorAction,
) -> Result<RefactorResult, RefactorError> {
    match action {
        RefactorAction::RenameSymbol {
            old_name,
            new_name,
            kind,
            ..
        } => rename::rename_symbol(result, file_id, &old_name, &new_name, kind),
        RefactorAction::AddMissingMatchCases { offset, .. } => {
            quickfix::add_missing_match_cases(result, file_id, offset)
        }
        RefactorAction::AddMissingCapability { offset, .. } => {
            quickfix::add_missing_capability(result, file_id, offset)
        }
    }
}

/// Run a refactor across a multi-file project.
pub fn refactor_project(
    result: &kyokara_hir::ProjectCheckResult,
    action: RefactorAction,
) -> Result<RefactorResult, RefactorError> {
    match action {
        RefactorAction::RenameSymbol {
            old_name,
            new_name,
            kind,
            target_file,
        } => rename::rename_symbol_project(
            result,
            &old_name,
            &new_name,
            kind,
            target_file.as_deref(),
        ),
        RefactorAction::AddMissingMatchCases {
            offset,
            target_file,
        } => quickfix::add_missing_match_cases_project(result, offset, target_file.as_deref()),
        RefactorAction::AddMissingCapability {
            offset,
            target_file,
        } => quickfix::add_missing_capability_project(result, offset, target_file.as_deref()),
    }
}

// ── Verification ────────────────────────────────────────────────────

/// Apply edits to source text (edits must be for a single file, sorted
/// by range start descending).
pub fn apply_edits(source: &str, edits: &[TextEdit]) -> String {
    let mut result = source.to_string();
    // Apply in reverse order (largest offset first) to preserve positions.
    let mut sorted: Vec<&TextEdit> = edits.iter().collect();
    sorted.sort_by(|a, b| b.range.start().cmp(&a.range.start()));
    for edit in sorted {
        let start: usize = edit.range.start().into();
        let end: usize = edit.range.end().into();
        result.replace_range(start..end, &edit.new_text);
    }
    result
}

/// Verify that applying the edits to the source produces zero diagnostics.
pub fn verify_single(source: &str, edits: &[TextEdit]) -> bool {
    let new_source = apply_edits(source, edits);
    let result = kyokara_hir::check_file(&new_source);
    result.type_check.raw_diagnostics.is_empty()
        && result.type_check.body_lowering_diagnostics.is_empty()
        && result.parse_errors.is_empty()
        && result.lowering_diagnostics.is_empty()
}
