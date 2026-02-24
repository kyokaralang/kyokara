//! `kyokara-hir` — High-level facade over the semantic model.
//!
//! This crate is the **public API** for semantic queries. It ties
//! together `hir-def` (data) and `hir-ty` (checking) behind a simple
//! interface that `api` and `cli` consume.
//!
//! When salsa lands (v0.3), the incremental database will live here.

pub use kyokara_hir_def::item_tree::ItemTree;
pub use kyokara_hir_def::resolver::ModuleScope;
pub use kyokara_hir_ty::diagnostics::TyDiagnosticData;
pub use kyokara_hir_ty::holes::HoleInfo;
pub use kyokara_hir_ty::infer::InferenceResult;
pub use kyokara_hir_ty::ty::{Ty, display_ty};
pub use kyokara_hir_ty::{TypeCheckResult, check_module};

use kyokara_hir_def::item_tree::lower::collect_item_tree;
use kyokara_intern::Interner;
use kyokara_parser::ParseError;
use kyokara_span::FileId;
use kyokara_syntax::SyntaxNode;
use kyokara_syntax::ast::AstNode;
use kyokara_syntax::ast::nodes::SourceFile;

/// Combined result of parsing + type-checking a single source file.
pub struct CheckResult {
    pub green: rowan::GreenNode,
    pub parse_errors: Vec<ParseError>,
    pub item_tree: ItemTree,
    pub module_scope: ModuleScope,
    pub type_check: TypeCheckResult,
    pub interner: Interner,
    /// Diagnostics from item tree collection and body lowering.
    pub lowering_diagnostics: Vec<kyokara_diagnostics::Diagnostic>,
}

/// Parse, lower, and type-check a single source file.
pub fn check_file(source: &str) -> CheckResult {
    let file_id = FileId(0);

    // 1. Parse.
    let parse = kyokara_syntax::parse(source);
    let green = parse.green.clone();
    let parse_errors = parse.errors;

    // 2. Build CST root and SourceFile.
    let root = SyntaxNode::new_root(parse.green);
    let sf = SourceFile::cast(root.clone()).expect("parsed root should cast to SourceFile");

    // 3. Collect item tree (Pass 1).
    let mut interner = Interner::new();
    let item_result = collect_item_tree(&sf, file_id, &mut interner);
    let lowering_diagnostics = item_result.diagnostics;

    // 4. Type-check all functions (Pass 2 + 3).
    let type_check = check_module(
        &root,
        &item_result.tree,
        &item_result.module_scope,
        file_id,
        &mut interner,
    );

    CheckResult {
        green,
        parse_errors,
        item_tree: item_result.tree,
        module_scope: item_result.module_scope,
        type_check,
        interner,
        lowering_diagnostics,
    }
}
