//! Salsa database for incremental file analysis.
//!
//! Each file is a `SourceFile` input. When its text changes, salsa
//! invalidates the cached result. The `FileAnalysis` bundles all
//! semantic data for a single file.

use kyokara_hir::{CheckResult, ItemTree, ModuleScope, TypeCheckResult};
use kyokara_intern::Interner;
use kyokara_parser::ParseError;
use kyokara_syntax::SyntaxNode;
use rowan::GreenNode;

/// Salsa input: a file's source text.
#[salsa::input]
pub struct SourceFile {
    pub text: String,
}

/// Run the full `check_file()` pipeline, returning the analysis hash.
/// Salsa memoizes: if `file.text(db)` is unchanged, the cached value is returned.
#[salsa::tracked]
pub fn check_file_query(db: &dyn salsa::Database, file: SourceFile) -> u64 {
    // Force a dependency on the file's text so salsa tracks changes.
    let _text = file.text(db);
    // Return a revision counter — the caller will do the actual analysis.
    // This is a sentinel; the real caching happens in the server layer.
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

/// Bundled analysis result for a single file.
///
/// Stores the green node (cheap to clone, Arc-backed) plus all semantic
/// data. The `Interner` is file-local — each `check_file()` call creates
/// its own, so there's no shared mutable interner across queries.
pub struct FileAnalysis {
    pub green: GreenNode,
    pub parse_errors: Vec<ParseError>,
    pub item_tree: ItemTree,
    pub module_scope: ModuleScope,
    pub type_check: TypeCheckResult,
    pub interner: Interner,
    pub lowering_diagnostics: Vec<kyokara_diagnostics::Diagnostic>,
    /// The source text at the time of analysis.
    pub source: String,
}

impl FileAnalysis {
    pub fn from_check_result(result: CheckResult, source: String) -> Self {
        Self {
            green: result.green,
            parse_errors: result.parse_errors,
            item_tree: result.item_tree,
            module_scope: result.module_scope,
            type_check: result.type_check,
            interner: result.interner,
            lowering_diagnostics: result.lowering_diagnostics,
            source,
        }
    }

    /// Reconstruct a typed `SyntaxNode` root from the green node.
    pub fn syntax_root(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }
}

/// The salsa database for the LSP server.
///
/// Used from a single thread (behind a `Mutex` in the server). Salsa's
/// `Storage` is not `Sync`, so we protect access with a `Mutex` rather
/// than an `RwLock`.
#[salsa::db]
#[derive(Clone, Default)]
pub struct LspDatabase {
    storage: salsa::Storage<Self>,
}

#[salsa::db]
impl salsa::Database for LspDatabase {}
