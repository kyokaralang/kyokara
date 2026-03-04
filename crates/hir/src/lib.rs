//! `kyokara-hir` — High-level facade over the semantic model.
//!
//! This crate is the **public API** for semantic queries. It ties
//! together `hir-def` (data) and `hir-ty` (checking) behind a simple
//! interface that `api` and `cli` consume.
//!
//! When salsa lands (v0.3), the incremental database will live here.

pub use kyokara_hir_def::body::Body;
pub use kyokara_hir_def::builtins::activate_synthetic_imports;
pub use kyokara_hir_def::builtins::register_builtin_intrinsics;
pub use kyokara_hir_def::builtins::register_builtin_methods;
pub use kyokara_hir_def::builtins::register_builtin_types;
pub use kyokara_hir_def::builtins::register_static_methods;
pub use kyokara_hir_def::builtins::register_synthetic_modules;
pub use kyokara_hir_def::item_tree::lower::collect_item_tree;
pub use kyokara_hir_def::item_tree::{
    EffectItem, FnItem, FnParam, ItemTree, PropertyItem, PropertyItemIdx, TypeDefKind, TypeItem,
    VariantDef,
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

use std::collections::HashMap;

use kyokara_diagnostics::{Diagnostic, DiagnosticKind};
use kyokara_intern::Interner;
use kyokara_parser::ParseError;
use kyokara_span::{FileId, FileMap, TextRange, TextSize};
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

/// Map lowering/body-lowering diagnostics to stable public diagnostic codes.
///
/// Duplicate-definition diagnostics map to `E0102`; unresolved-name and
/// all other lowering diagnostics map to `E0101`.
pub fn lowering_diagnostic_code(diag: &Diagnostic) -> &'static str {
    match diag.kind {
        DiagnosticKind::DuplicateDefinition => "E0102",
        DiagnosticKind::UnresolvedName | DiagnosticKind::General => "E0101",
    }
}

/// Render a surface-level [`TypeRef`] as a human-readable string.
pub fn display_type_ref(tr: &TypeRef, interner: &Interner) -> String {
    fn surface_type_segment(raw: &str) -> String {
        match raw {
            "$core_Seq" => "<traversal>".to_string(),
            _ => raw
                .strip_prefix("$core_")
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| raw.to_string()),
        }
    }

    match tr {
        TypeRef::Path { path, args } => {
            let base: String = path
                .segments
                .iter()
                .map(|s| surface_type_segment(s.resolve(interner)))
                .collect::<Vec<_>>()
                .join(".");
            if args.is_empty() {
                base
            } else {
                let arg_strs: Vec<String> =
                    args.iter().map(|a| display_type_ref(a, interner)).collect();
                format!("{base}<{}>", arg_strs.join(", "))
            }
        }
        TypeRef::Fn { params, ret } => {
            let ps: Vec<String> = params
                .iter()
                .map(|p| display_type_ref(p, interner))
                .collect();
            format!(
                "fn({}) -> {}",
                ps.join(", "),
                display_type_ref(ret, interner)
            )
        }
        TypeRef::Record { fields } => {
            let fs: Vec<String> = fields
                .iter()
                .map(|(n, t)| format!("{}: {}", n.resolve(interner), display_type_ref(t, interner)))
                .collect();
            format!("{{ {} }}", fs.join(", "))
        }
        TypeRef::Refined { name, base, .. } => {
            format!(
                "{{ {}: {} | ... }}",
                name.resolve(interner),
                display_type_ref(base, interner)
            )
        }
        TypeRef::Error => "<error>".into(),
    }
}

/// Parse, lower, and type-check a single source file.
pub fn check_file(source: &str) -> CheckResult {
    let file_id = FileId(0);

    // 1. Parse.
    let parse = kyokara_syntax::parse(source);
    let green = parse.green.clone();
    let parse_errors = parse.errors;
    let parse_error_ranges = normalized_parse_error_ranges(&parse_errors, source.len() as u32);

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
    register_builtin_methods(&mut item_result.module_scope, &mut interner);
    register_synthetic_modules(
        &mut item_result.tree,
        &mut item_result.module_scope,
        &mut interner,
    );
    activate_synthetic_imports(
        &item_result.tree,
        &mut item_result.module_scope,
        &mut interner,
    );
    register_static_methods(&mut item_result.module_scope, &mut interner);

    // 5. Type-check all functions (Pass 2 + 3).
    let type_check = check_module(
        &root,
        &item_result.tree,
        &item_result.module_scope,
        &parse_error_ranges,
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
    let mut parse_error_ranges_by_module: HashMap<ModulePath, Vec<TextRange>> = HashMap::new();
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
        let parse_error_ranges = normalized_parse_error_ranges(&parse.errors, source.len() as u32);
        parse_error_ranges_by_module.insert(mod_path.clone(), parse_error_ranges);

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
        register_builtin_methods(&mut item_result.module_scope, &mut interner);
        register_synthetic_modules(
            &mut item_result.tree,
            &mut item_result.module_scope,
            &mut interner,
        );
        register_static_methods(&mut item_result.module_scope, &mut interner);

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
        #[allow(clippy::unwrap_used)] // key comes from cst_roots, always in module_graph
        let info = module_graph.get(mod_path).unwrap();
        let parse_error_ranges = parse_error_ranges_by_module
            .get(mod_path)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let tc = check_module(
            cst_root,
            &info.item_tree,
            &info.scope,
            parse_error_ranges,
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

fn normalized_parse_error_ranges(parse_errors: &[ParseError], source_len: u32) -> Vec<TextRange> {
    parse_errors
        .iter()
        .map(|err| normalize_parse_error_range(err, source_len))
        .collect()
}

fn normalize_parse_error_range(err: &ParseError, source_len: u32) -> TextRange {
    let start = err.range_start.min(source_len);
    let end = err.range_end.min(source_len);
    if start < end {
        return TextRange::new(TextSize::from(start), TextSize::from(end));
    }

    if source_len == 0 {
        return TextRange::new(TextSize::from(0), TextSize::from(0));
    }

    if start < source_len {
        let right = (start + 1).min(source_len);
        return TextRange::new(TextSize::from(start), TextSize::from(right));
    }

    let left = start.saturating_sub(1);
    TextRange::new(TextSize::from(left), TextSize::from(start))
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
        import_path: Path,
        file_id: FileId,
        import_range: TextRange,
    }

    let mut to_resolve: Vec<PendingImport> = Vec::new();
    let mut single_segment_index: HashMap<Name, Vec<ModulePath>> = HashMap::new();

    for (mod_path, _) in graph.iter() {
        if let Some(last) = mod_path.last() {
            single_segment_index
                .entry(last)
                .or_default()
                .push(mod_path.clone());
        }
    }

    for (mod_path, info) in graph.iter() {
        for imp in &info.item_tree.imports {
            // Resolve by the actual import path, not alias.
            if imp.path.segments.is_empty() {
                continue;
            }
            to_resolve.push(PendingImport {
                importing_mod: mod_path.clone(),
                import_path: imp.path.clone(),
                file_id: info.file_id,
                import_range: imp.source_range.unwrap_or_default(),
            });
        }
    }

    // Collect the pub items from target modules (clone them to avoid borrow issues).
    for pending in to_resolve {
        let PendingImport {
            importing_mod,
            import_path,
            file_id,
            import_range,
        } = pending;

        // Collect pub items from the target module.
        let pub_data = {
            let import_name = import_path
                .segments
                .iter()
                .map(|seg| seg.resolve(interner).to_owned())
                .collect::<Vec<_>>()
                .join(".");
            let candidates: Vec<ModulePath> = if import_path.segments.len() > 1 {
                let target_path = ModulePath(import_path.segments.clone());
                if graph.get(&target_path).is_some() {
                    vec![target_path]
                } else {
                    Vec::new()
                }
            } else {
                let resolve_name = import_path.segments[0];
                single_segment_index
                    .get(&resolve_name)
                    .cloned()
                    .unwrap_or_default()
            };

            if candidates.is_empty() {
                // Check if it's a synthetic module import (io, math, fs).
                // If so, the modules are already registered — this import just
                // makes the name available in scope. Nothing else to do.
                if import_path.segments.len() == 1 {
                    let seg_name = import_path.segments[0];
                    let is_synthetic = graph
                        .get(&importing_mod)
                        .is_some_and(|info| info.scope.synthetic_modules.contains_key(&seg_name));
                    if is_synthetic {
                        if let Some(info) = graph.get_mut(&importing_mod) {
                            info.scope.imported_modules.insert(seg_name);
                        }
                        continue;
                    }
                }
                diagnostics.push(kyokara_diagnostics::Diagnostic::error(
                    format!("unresolved import `{import_name}`"),
                    kyokara_span::Span {
                        file: file_id,
                        range: import_range,
                    },
                ));
                continue;
            }

            if candidates.len() > 1 {
                let mut labels: Vec<String> = candidates
                    .iter()
                    .map(|path| module_path_label(path, interner))
                    .collect();
                labels.sort();
                diagnostics.push(kyokara_diagnostics::Diagnostic::error(
                    format!(
                        "ambiguous import `{import_name}`: matches {}",
                        labels.join(", ")
                    ),
                    kyokara_span::Span {
                        file: file_id,
                        range: import_range,
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
                                range: import_range,
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
                                range: import_range,
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
                PubData::Effect(cap_item) => {
                    let name = cap_item.name;
                    if importing_info.scope.effects.contains_key(&name) {
                        let name_str = name.resolve(interner);
                        diagnostics.push(kyokara_diagnostics::Diagnostic::error(
                            format!("conflicting import: `{name_str}` is already defined"),
                            kyokara_span::Span {
                                file: import_file_id,
                                range: import_range,
                            },
                        ));
                    } else {
                        let idx = importing_info.item_tree.effects.alloc(cap_item);
                        importing_info.scope.effects.insert(name, idx);
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
    Effect(EffectItem),
}

fn module_path_label(path: &ModulePath, interner: &Interner) -> String {
    if path.0.is_empty() {
        "<root>".to_string()
    } else {
        path.0
            .iter()
            .map(|seg| seg.resolve(interner).to_owned())
            .collect::<Vec<_>>()
            .join(".")
    }
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

    for (_, cap_item) in item_tree.effects.iter() {
        if cap_item.is_pub {
            items.push(PubData::Effect(cap_item.clone()));
        }
    }

    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowering_diagnostic_code_maps_duplicate() {
        let diag = kyokara_diagnostics::Diagnostic::error_with_kind(
            "duplicate function `foo`",
            kyokara_span::Span {
                file: FileId(0),
                range: kyokara_span::TextRange::default(),
            },
            DiagnosticKind::DuplicateDefinition,
        );
        assert_eq!(lowering_diagnostic_code(&diag), "E0102");
    }

    #[test]
    fn lowering_diagnostic_code_maps_non_duplicate() {
        let unresolved = kyokara_diagnostics::Diagnostic::error_with_kind(
            "unresolved name `foo`",
            kyokara_span::Span {
                file: FileId(0),
                range: kyokara_span::TextRange::default(),
            },
            DiagnosticKind::UnresolvedName,
        );
        assert_eq!(lowering_diagnostic_code(&unresolved), "E0101");

        let fallback = kyokara_diagnostics::Diagnostic::error(
            "some other lowering diagnostic",
            kyokara_span::Span {
                file: FileId(0),
                range: kyokara_span::TextRange::default(),
            },
        );
        assert_eq!(lowering_diagnostic_code(&fallback), "E0101");
    }

    #[test]
    fn display_type_ref_keeps_full_paths() {
        let mut interner = Interner::new();
        let tr = TypeRef::Path {
            path: Path {
                segments: vec![
                    Name::new(&mut interner, "foo"),
                    Name::new(&mut interner, "bar"),
                    Name::new(&mut interner, "Baz"),
                ],
            },
            args: Vec::new(),
        };
        assert_eq!(display_type_ref(&tr, &interner), "foo.bar.Baz");
    }

    #[test]
    fn display_type_ref_renders_nested_shapes() {
        let mut interner = Interner::new();
        let t_name = Name::new(&mut interner, "T");
        let result = TypeRef::Path {
            path: Path {
                segments: vec![Name::new(&mut interner, "Result")],
            },
            args: vec![
                TypeRef::Path {
                    path: Path {
                        segments: vec![
                            Name::new(&mut interner, "foo"),
                            Name::new(&mut interner, "A"),
                        ],
                    },
                    args: Vec::new(),
                },
                TypeRef::Record {
                    fields: vec![(
                        t_name,
                        TypeRef::Path {
                            path: Path {
                                segments: vec![
                                    Name::new(&mut interner, "foo"),
                                    Name::new(&mut interner, "bar"),
                                    Name::new(&mut interner, "B"),
                                ],
                            },
                            args: Vec::new(),
                        },
                    )],
                },
            ],
        };

        assert_eq!(
            display_type_ref(&result, &interner),
            "Result<foo.A, { T: foo.bar.B }>"
        );
    }
}
