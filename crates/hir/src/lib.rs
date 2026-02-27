//! `kyokara-hir` — High-level facade over the semantic model.
//!
//! This crate is the **public API** for semantic queries. It ties
//! together `hir-def` (data) and `hir-ty` (checking) behind a simple
//! interface that `api` and `cli` consume.
//!
//! When salsa lands (v0.3), the incremental database will live here.

pub use kyokara_hir_def::body::Body;
pub use kyokara_hir_def::builtins::register_builtin_intrinsics;
pub use kyokara_hir_def::builtins::register_builtin_types;
pub use kyokara_hir_def::item_tree::lower::collect_item_tree;
pub use kyokara_hir_def::item_tree::{
    CapItem, FnItem, FnParam, ItemTree, TypeDefKind, TypeItem, VariantDef,
};
pub use kyokara_hir_def::module_graph::{ModuleGraph, ModuleInfo, ModulePath, discover_modules};
pub use kyokara_hir_def::name::Name;
pub use kyokara_hir_def::path::Path;
pub use kyokara_hir_def::resolver::ModuleScope;
pub use kyokara_hir_def::type_ref::TypeRef;
pub use kyokara_hir_ty::diagnostics::TyDiagnosticData;
pub use kyokara_hir_ty::holes::HoleInfo;
pub use kyokara_hir_ty::infer::InferenceResult;
pub use kyokara_hir_ty::ty::{Ty, display_ty, display_ty_with_tree};
pub use kyokara_hir_ty::{TypeCheckResult, check_module};

use kyokara_intern::Interner;
use kyokara_parser::ParseError;
use kyokara_span::{FileId, FileMap};
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
    let mut item_result = collect_item_tree(&sf, file_id, &mut interner);

    // 4. Register builtin types (Option, Result).
    register_builtin_types(
        &mut item_result.tree,
        &mut item_result.module_scope,
        &mut interner,
    );
    register_builtin_intrinsics(
        &mut item_result.tree,
        &mut item_result.module_scope,
        &mut interner,
    );

    // 5. Type-check all functions (Pass 2 + 3).
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
        lowering_diagnostics: item_result.diagnostics,
    }
}

/// Result of checking a multi-file project.
pub struct ProjectCheckResult {
    pub module_graph: ModuleGraph,
    pub type_checks: Vec<(ModulePath, TypeCheckResult)>,
    pub interner: Interner,
    pub file_map: FileMap,
    pub parse_errors: Vec<(ModulePath, Vec<ParseError>)>,
    pub lowering_diagnostics: Vec<kyokara_diagnostics::Diagnostic>,
}

/// Parse, lower, and type-check a multi-file project.
///
/// `entry_file` is the main `.ky` file (e.g., `main.ky`). Other `.ky`
/// files in the same directory (and subdirectories) are discovered and
/// treated as importable modules.
pub fn check_project(entry_file: &std::path::Path) -> ProjectCheckResult {
    let mut interner = Interner::new();
    let mut file_map = FileMap::new();
    let mut module_graph = ModuleGraph::new();
    let mut all_parse_errors = Vec::new();
    let mut all_lowering_diagnostics = Vec::new();
    let mut cst_roots: Vec<(ModulePath, SyntaxNode)> = Vec::new();

    let root = entry_file.parent().unwrap_or(std::path::Path::new("."));

    // 1. Discover modules.
    let discovered = discover_modules(root, entry_file, &mut interner);

    // 2. Parse each file and build item trees.
    for (mod_path, file_path) in &discovered {
        let file_id = file_map.insert(file_path.clone());
        let source = match std::fs::read_to_string(file_path) {
            Ok(s) => s,
            Err(err) => {
                all_lowering_diagnostics.push(kyokara_diagnostics::Diagnostic::error(
                    format!("failed to read module `{}`: {}", file_path.display(), err),
                    kyokara_span::Span {
                        file: file_id,
                        range: kyokara_span::TextRange::default(),
                    },
                ));
                continue;
            }
        };
        let parse = kyokara_syntax::parse(&source);

        if !parse.errors.is_empty() {
            all_parse_errors.push((mod_path.clone(), parse.errors.clone()));
        }

        let cst_root = SyntaxNode::new_root(parse.green);
        let sf = SourceFile::cast(cst_root.clone()).expect("parsed root should cast to SourceFile");

        let mut item_result = collect_item_tree(&sf, file_id, &mut interner);

        register_builtin_types(
            &mut item_result.tree,
            &mut item_result.module_scope,
            &mut interner,
        );
        register_builtin_intrinsics(
            &mut item_result.tree,
            &mut item_result.module_scope,
            &mut interner,
        );

        all_lowering_diagnostics.extend(item_result.diagnostics);

        cst_roots.push((mod_path.clone(), cst_root));

        module_graph.insert(
            mod_path.clone(),
            ModuleInfo {
                file_id,
                path: file_path.clone(),
                item_tree: item_result.tree,
                scope: item_result.module_scope,
                source,
            },
        );
    }

    // Mark entry module.
    module_graph.entry = Some(ModulePath::root());

    // 3. Resolve cross-module imports.
    let import_diags = resolve_project_imports(&mut module_graph, &interner);
    all_lowering_diagnostics.extend(import_diags);

    // 4. Type-check each module.
    let mut type_checks = Vec::new();
    for (mod_path, cst_root) in &cst_roots {
        let info = module_graph.get(mod_path).unwrap();
        let tc = check_module(
            cst_root,
            &info.item_tree,
            &info.scope,
            info.file_id,
            &mut interner,
        );
        type_checks.push((mod_path.clone(), tc));
    }

    ProjectCheckResult {
        module_graph,
        type_checks,
        interner,
        file_map,
        parse_errors: all_parse_errors,
        lowering_diagnostics: all_lowering_diagnostics,
    }
}

/// Resolve imports across all modules in the graph.
///
/// For each module's imports, look up the target module and **copy** its
/// pub items into the importing module's item tree and scope. This way
/// the indices are valid within the importing module's arena.
///
/// Returns diagnostics for unresolved imports.
fn resolve_project_imports(
    graph: &mut ModuleGraph,
    interner: &Interner,
) -> Vec<kyokara_diagnostics::Diagnostic> {
    let mut diagnostics = Vec::new();

    // Collect what needs to be resolved (to avoid borrow conflicts).
    struct PendingImport {
        importing_mod: ModulePath,
        resolve_name: Name,
        file_id: FileId,
    }

    let mut to_resolve: Vec<PendingImport> = Vec::new();

    for (mod_path, info) in graph.iter() {
        for imp in &info.item_tree.imports {
            // Resolve by the actual import path, not alias.
            let Some(resolve_name) = imp.path.last() else {
                continue;
            };
            to_resolve.push(PendingImport {
                importing_mod: mod_path.clone(),
                resolve_name,
                file_id: info.file_id,
            });
        }
    }

    // Collect the pub items from target modules (clone them to avoid borrow issues).
    for pending in to_resolve {
        let PendingImport {
            importing_mod,
            resolve_name,
            file_id,
        } = pending;

        // Collect pub items from the target module.
        let pub_data = {
            let candidates: Vec<ModulePath> = graph
                .iter()
                .filter_map(|(mod_path, _)| {
                    if mod_path.last() == Some(resolve_name) {
                        Some(mod_path.clone())
                    } else {
                        None
                    }
                })
                .collect();

            if candidates.is_empty() {
                let name_str = resolve_name.resolve(interner);
                diagnostics.push(kyokara_diagnostics::Diagnostic::error(
                    format!("unresolved import `{name_str}`"),
                    kyokara_span::Span {
                        file: file_id,
                        range: kyokara_span::TextRange::default(),
                    },
                ));
                continue;
            }

            if candidates.len() > 1 {
                let name_str = resolve_name.resolve(interner);
                let mut labels: Vec<String> = candidates
                    .iter()
                    .map(|path| {
                        if path.0.is_empty() {
                            "<root>".to_string()
                        } else {
                            path.0
                                .iter()
                                .map(|seg| seg.resolve(interner).to_owned())
                                .collect::<Vec<_>>()
                                .join(".")
                        }
                    })
                    .collect();
                labels.sort();
                diagnostics.push(kyokara_diagnostics::Diagnostic::error(
                    format!(
                        "ambiguous import `{name_str}`: matches {}",
                        labels.join(", ")
                    ),
                    kyokara_span::Span {
                        file: file_id,
                        range: kyokara_span::TextRange::default(),
                    },
                ));
                continue;
            }

            let target_path = &candidates[0];
            let target_info = graph
                .get(target_path)
                .expect("candidate path must exist in module graph");
            collect_pub_data(&target_info.item_tree)
        };

        // Re-allocate pub items in the importing module's item tree.
        let Some(importing_info) = graph.get_mut(&importing_mod) else {
            continue;
        };

        let import_file_id = importing_info.file_id;
        for item in pub_data {
            match item {
                PubData::Fn(mut fn_item) => {
                    let name = fn_item.name;
                    if importing_info.scope.functions.contains_key(&name) {
                        let name_str = name.resolve(interner);
                        diagnostics.push(kyokara_diagnostics::Diagnostic::error(
                            format!("conflicting import: `{name_str}` is already defined"),
                            kyokara_span::Span {
                                file: import_file_id,
                                range: kyokara_span::TextRange::default(),
                            },
                        ));
                    } else {
                        // Imported functions are not source-owned by this module.
                        // Keep them resolvable in scope but prevent duplicate
                        // symbol-graph function nodes in the importing module.
                        fn_item.source_range = None;
                        let idx = importing_info.item_tree.functions.alloc(fn_item);
                        importing_info.scope.functions.insert(name, idx);
                    }
                }
                PubData::Type(type_item) => {
                    let name = type_item.name;
                    if importing_info.scope.types.contains_key(&name) {
                        let name_str = name.resolve(interner);
                        diagnostics.push(kyokara_diagnostics::Diagnostic::error(
                            format!("conflicting import: `{name_str}` is already defined"),
                            kyokara_span::Span {
                                file: import_file_id,
                                range: kyokara_span::TextRange::default(),
                            },
                        ));
                    } else {
                        let variants_info: Vec<(Name, usize)> =
                            if let TypeDefKind::Adt { ref variants } = type_item.kind {
                                variants
                                    .iter()
                                    .enumerate()
                                    .map(|(vi, v)| (v.name, vi))
                                    .collect()
                            } else {
                                Vec::new()
                            };
                        let idx = importing_info.item_tree.types.alloc(type_item);
                        importing_info.scope.types.insert(name, idx);
                        // Register constructors.
                        for (vname, vi) in variants_info {
                            importing_info
                                .scope
                                .constructors
                                .entry(vname)
                                .or_insert((idx, vi));
                        }
                    }
                }
                PubData::Cap(cap_item) => {
                    let name = cap_item.name;
                    if importing_info.scope.caps.contains_key(&name) {
                        let name_str = name.resolve(interner);
                        diagnostics.push(kyokara_diagnostics::Diagnostic::error(
                            format!("conflicting import: `{name_str}` is already defined"),
                            kyokara_span::Span {
                                file: import_file_id,
                                range: kyokara_span::TextRange::default(),
                            },
                        ));
                    } else {
                        let idx = importing_info.item_tree.caps.alloc(cap_item);
                        importing_info.scope.caps.insert(name, idx);
                    }
                }
            }
        }
    }
    diagnostics
}

enum PubData {
    Fn(FnItem),
    Type(TypeItem),
    Cap(CapItem),
}

/// Collect clones of all pub items from a module's item tree.
fn collect_pub_data(item_tree: &ItemTree) -> Vec<PubData> {
    let mut items = Vec::new();

    for (_, fn_item) in item_tree.functions.iter() {
        if fn_item.is_pub {
            items.push(PubData::Fn(fn_item.clone()));
        }
    }

    for (_, type_item) in item_tree.types.iter() {
        if type_item.is_pub {
            items.push(PubData::Type(type_item.clone()));
        }
    }

    for (_, cap_item) in item_tree.caps.iter() {
        if cap_item.is_pub {
            items.push(PubData::Cap(cap_item.clone()));
        }
    }

    items
}
