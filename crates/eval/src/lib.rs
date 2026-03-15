//! `kyokara-eval` — Tree-walking interpreter for Kyokara.
//!
//! Walks the HIR expression trees produced by the compiler frontend
//! and evaluates them directly. Used by `kyokara run <file>`.

pub mod env;
pub mod error;
pub mod interpreter;
pub mod intrinsics;
pub mod manifest;
mod runtime;
pub mod value;

use kyokara_hir::{
    ModulePath, activate_synthetic_imports, activate_type_member_imports, check_module,
    check_project, collect_item_tree, register_builtin_intrinsics, register_builtin_methods,
    register_builtin_traits, register_builtin_types, register_static_methods,
    register_synthetic_modules,
};
use kyokara_intern::Interner;
use kyokara_span::{FileId, TextRange, TextSize};
use kyokara_stdx::FxHashMap;
use kyokara_syntax::SyntaxNode;
use kyokara_syntax::ast::AstNode;
use kyokara_syntax::ast::nodes::SourceFile;

use crate::error::RuntimeError;
use crate::interpreter::Interpreter;
use crate::manifest::CapabilityManifest;
use crate::runtime::{
    SharedRuntimeService, build_replay_header, map_runtime_effect_error, new_live_runtime,
    new_replay_runtime,
};
use crate::value::Value;
pub use kyokara_runtime::service::ReplayMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompileErrorClass {
    Parse,
    Lowering,
    Type,
}

impl CompileErrorClass {
    fn label(self) -> &'static str {
        match self {
            CompileErrorClass::Parse => "compile parse errors",
            CompileErrorClass::Lowering => "compile lowering errors",
            CompileErrorClass::Type => "compile type errors",
        }
    }
}

#[derive(Default)]
struct CompileGateErrors {
    parse: Vec<String>,
    lowering: Vec<String>,
    type_errors: Vec<String>,
}

impl CompileGateErrors {
    fn add(&mut self, class: CompileErrorClass, msg: String) {
        match class {
            CompileErrorClass::Parse => self.parse.push(msg),
            CompileErrorClass::Lowering => self.lowering.push(msg),
            CompileErrorClass::Type => self.type_errors.push(msg),
        }
    }

    fn into_runtime_error(self) -> Option<RuntimeError> {
        let (class, msgs) = if !self.parse.is_empty() {
            (CompileErrorClass::Parse, self.parse)
        } else if !self.lowering.is_empty() {
            (CompileErrorClass::Lowering, self.lowering)
        } else if !self.type_errors.is_empty() {
            (CompileErrorClass::Type, self.type_errors)
        } else {
            return None;
        };
        Some(RuntimeError::TypeError(format!(
            "{}: {}",
            class.label(),
            msgs.join("; ")
        )))
    }
}

fn validate_manifest_constraints(manifest: &CapabilityManifest) -> Result<(), RuntimeError> {
    if let Some((capability, field)) = manifest.first_unsupported_constraint() {
        return Err(RuntimeError::UnsupportedManifestConstraint {
            capability,
            field: field.to_string(),
        });
    }
    Ok(())
}

/// Result of running a Kyokara program.
pub struct RunResult {
    pub value: Value,
    pub interner: Interner,
}

pub struct RunOptions<'a> {
    pub manifest: Option<CapabilityManifest>,
    pub replay_log: Option<&'a std::path::Path>,
}

impl Default for RunOptions<'_> {
    fn default() -> Self {
        Self {
            manifest: None,
            replay_log: None,
        }
    }
}

/// Parse, type-check, and evaluate a Kyokara source file.
///
/// Injects builtin types (`Option`, `Result`) and intrinsic function
/// signatures before type-checking so that constructors and calls to
/// `println`, `int_to_string`, etc. resolve correctly.
///
/// All capabilities are allowed (no manifest enforcement).
pub fn run(source: &str) -> Result<RunResult, RuntimeError> {
    run_with_manifest(source, None)
}

/// Like [`run`], but with an optional capability manifest for deny-by-default enforcement.
///
/// When `manifest` is `Some`, only capabilities listed in the manifest are allowed.
/// When `manifest` is `None`, all capabilities are permitted (backward compatible).
pub fn run_with_manifest(
    source: &str,
    manifest: Option<CapabilityManifest>,
) -> Result<RunResult, RuntimeError> {
    if let Some(ref m) = manifest {
        validate_manifest_constraints(m)?;
    }

    let runtime = new_live_runtime(manifest.clone(), None)?;
    run_single_source_with_runtime(source, runtime)
}

pub fn run_file_with_options(
    entry_file: &std::path::Path,
    options: &RunOptions<'_>,
) -> Result<RunResult, RuntimeError> {
    if let Some(ref m) = options.manifest {
        validate_manifest_constraints(m)?;
    }

    let replay = if let Some(path) = options.replay_log {
        Some(kyokara_runtime::replay::ReplayLogConfig {
            path: path.to_path_buf(),
            header: build_replay_header(entry_file, false, [entry_file.to_path_buf()])?,
        })
    } else {
        None
    };

    let runtime = new_live_runtime(options.manifest.clone(), replay)?;
    run_file_with_runtime(entry_file, runtime)
}

pub fn replay_from_log(
    log_path: &std::path::Path,
    mode: ReplayMode,
) -> Result<RunResult, RuntimeError> {
    let (runtime, header) = new_replay_runtime(log_path, mode)?;
    let entry_file = std::path::PathBuf::from(&header.entry_file);
    if header.project_mode {
        run_project_with_runtime(&entry_file, runtime)
    } else {
        run_file_with_runtime(&entry_file, runtime)
    }
}

fn run_file_with_runtime(
    entry_file: &std::path::Path,
    runtime: SharedRuntimeService,
) -> Result<RunResult, RuntimeError> {
    let source = std::fs::read_to_string(entry_file).map_err(|err| {
        RuntimeError::TypeError(format!("cannot read `{}`: {err}", entry_file.display()))
    })?;
    run_single_source_with_runtime(&source, runtime)
}

fn run_single_source_with_runtime(
    source: &str,
    runtime: SharedRuntimeService,
) -> Result<RunResult, RuntimeError> {
    let file_id = FileId(0);

    // 1. Parse.
    let parse = kyokara_syntax::parse(source);
    let parse_error_ranges: Vec<TextRange> = parse
        .errors
        .iter()
        .map(|err| normalize_parse_error_range(err.range_start, err.range_end, source.len() as u32))
        .collect();

    // 2. Build CST.
    let root = SyntaxNode::new_root(parse.green);
    let sf = SourceFile::cast(root.clone()).expect("parsed root should cast to SourceFile");

    // 3. Collect item tree.
    let mut interner = Interner::new();
    let mut item_result = collect_item_tree(&sf, file_id, &mut interner);

    // 4. Register builtin types (Option, Result) before intrinsics and type-checking.
    register_builtin_types(
        &mut item_result.tree,
        &mut item_result.module_scope,
        &mut interner,
    );
    register_builtin_traits(
        &mut item_result.tree,
        &mut item_result.module_scope,
        &mut interner,
    );

    // 5. Register intrinsic function signatures and canonical API surface.
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
    activate_synthetic_imports(
        &item_result.tree,
        &mut item_result.module_scope,
        &mut interner,
    );
    activate_type_member_imports(&item_result.tree, &mut item_result.module_scope);

    // 6. Type-check.
    let type_check = check_module(
        &root,
        &item_result.tree,
        &item_result.module_scope,
        &parse_error_ranges,
        file_id,
        &mut interner,
    );

    if let Some(err) = collect_single_file_compile_errors(
        &parse.errors,
        &item_result.diagnostics,
        &type_check.body_lowering_diagnostics,
        &type_check.raw_diagnostics,
        &interner,
        &item_result.tree,
    )
    .into_runtime_error()
    {
        return Err(err);
    }

    // 7. Interpret.
    let mut interp = Interpreter::new_with_runtime(
        item_result.tree,
        item_result.module_scope,
        type_check.fn_bodies,
        type_check.let_bodies,
        FxHashMap::default(),
        FxHashMap::default(),
        FxHashMap::default(),
        interner,
        runtime.clone(),
    );

    let result = interp.run_main();
    let interner = interp.into_interner();
    finish_run_result(result, interner, &runtime)
}

/// Parse, type-check, and evaluate a multi-file Kyokara project.
///
/// `entry_file` is the main `.ky` file. Sibling `.ky` files are
/// discovered as importable modules.
///
/// All capabilities are allowed (no manifest enforcement).
pub fn run_project(entry_file: &std::path::Path) -> Result<RunResult, RuntimeError> {
    run_project_with_manifest(entry_file, None)
}

/// Like [`run_project`], but with an optional capability manifest.
pub fn run_project_with_manifest(
    entry_file: &std::path::Path,
    manifest: Option<CapabilityManifest>,
) -> Result<RunResult, RuntimeError> {
    if let Some(ref m) = manifest {
        validate_manifest_constraints(m)?;
    }

    run_project_internal(entry_file, manifest, None)
}

pub fn run_project_with_options(
    entry_file: &std::path::Path,
    options: &RunOptions<'_>,
) -> Result<RunResult, RuntimeError> {
    if let Some(ref m) = options.manifest {
        validate_manifest_constraints(m)?;
    }

    run_project_internal(entry_file, options.manifest.clone(), options.replay_log)
}

fn run_project_internal(
    entry_file: &std::path::Path,
    manifest: Option<CapabilityManifest>,
    replay_log: Option<&std::path::Path>,
) -> Result<RunResult, RuntimeError> {
    let replay = if let Some(path) = replay_log {
        Some(kyokara_runtime::replay::ReplayLogConfig {
            path: path.to_path_buf(),
            header: {
                let project = check_project(entry_file);
                if let Some(err) = collect_project_compile_errors(&project).into_runtime_error() {
                    return Err(err);
                }
                let files = project
                    .module_graph
                    .iter()
                    .map(|(_, info)| info.path.clone())
                    .collect::<Vec<_>>();
                build_replay_header(entry_file, true, files)?
            },
        })
    } else {
        None
    };
    let runtime = new_live_runtime(manifest.clone(), replay)?;
    run_project_with_runtime(entry_file, runtime)
}

fn run_project_with_runtime(
    entry_file: &std::path::Path,
    runtime: SharedRuntimeService,
) -> Result<RunResult, RuntimeError> {
    let mut project = check_project(entry_file);

    if let Some(err) = collect_project_compile_errors(&project).into_runtime_error() {
        return Err(err);
    }

    // Find the entry module.
    let entry_path = ModulePath::root();
    let entry_info = project
        .module_graph
        .get_mut(&entry_path)
        .ok_or(RuntimeError::NoMainFunction)?;

    // Register intrinsics and canonical API surface in the entry module.
    register_builtin_intrinsics(
        &mut entry_info.item_tree,
        &mut entry_info.scope,
        &mut project.interner,
    );
    register_builtin_methods(&mut entry_info.scope, &mut project.interner);
    register_synthetic_modules(
        &mut entry_info.item_tree,
        &mut entry_info.scope,
        &mut project.interner,
    );
    register_static_methods(&mut entry_info.scope, &mut project.interner);
    activate_synthetic_imports(
        &entry_info.item_tree,
        &mut entry_info.scope,
        &mut project.interner,
    );
    activate_type_member_imports(&entry_info.item_tree, &mut entry_info.scope);

    // Collect fn bodies: start with the entry module's type check.
    let entry_tc = project
        .type_checks
        .iter()
        .find(|(p, _)| *p == entry_path)
        .map(|(_, tc)| tc)
        .ok_or(RuntimeError::NoMainFunction)?;

    let mut fn_bodies: FxHashMap<
        kyokara_hir_def::item_tree::FnItemIdx,
        kyokara_hir_def::body::Body,
    > = FxHashMap::default();
    for (k, v) in &entry_tc.fn_bodies {
        fn_bodies.insert(*k, v.clone());
    }

    // Also collect bodies from imported modules and map them to entry module indices.
    // Only consider modules that the entry module actually imports (#68).
    let entry_info = project
        .module_graph
        .get(&entry_path)
        .ok_or(RuntimeError::TypeError("entry module not found".into()))?;

    // Build the set of module paths that the entry module imports and the
    // visible namespace names for namespace imports.
    let mut imported_namespace_names: FxHashMap<ModulePath, Vec<kyokara_hir_def::name::Name>> =
        FxHashMap::default();
    let mut imported_member_names: FxHashMap<
        ModulePath,
        Vec<(kyokara_hir_def::name::Name, kyokara_hir_def::name::Name)>,
    > = FxHashMap::default();
    for imp in &entry_info.item_tree.imports {
        let direct_path = ModulePath(imp.path.segments.clone());
        let resolved_path = if project.module_graph.get(&direct_path).is_some() {
            Some(direct_path)
        } else {
            let target_name = imp.path.last();
            target_name.and_then(|name| {
                project.module_graph.iter().find_map(|(mod_path, _)| {
                    (mod_path.last() == Some(name)).then(|| mod_path.clone())
                })
            })
        };

        let Some(resolved_path) = resolved_path else {
            continue;
        };

        match &imp.kind {
            kyokara_hir_def::item_tree::ImportKind::Namespace { alias } => {
                let visible_name = alias.unwrap_or_else(|| {
                    imp.path
                        .last()
                        .expect("namespace import path should not be empty")
                });
                imported_namespace_names
                    .entry(resolved_path)
                    .or_default()
                    .push(visible_name);
            }
            kyokara_hir_def::item_tree::ImportKind::Members { members } => {
                let imported = imported_member_names.entry(resolved_path).or_default();
                for member in members {
                    imported.push((member.alias.unwrap_or(member.name), member.name));
                }
            }
        }
    }

    // Per-function module override maps used by the interpreter so imported
    // functions can resolve private helpers in their source module without
    // leaking names into entry scope.
    let mut fn_scope_overrides: FxHashMap<
        kyokara_hir_def::item_tree::FnItemIdx,
        FxHashMap<kyokara_hir_def::name::Name, Vec<kyokara_hir_def::item_tree::FnItemIdx>>,
    > = FxHashMap::default();
    let mut module_scope_overrides: FxHashMap<
        kyokara_hir_def::item_tree::FnItemIdx,
        kyokara_hir_def::resolver::ModuleScope,
    > = FxHashMap::default();
    let mut let_scope_overrides: FxHashMap<
        kyokara_hir_def::item_tree::FnItemIdx,
        FxHashMap<kyokara_hir_def::name::Name, Value>,
    > = FxHashMap::default();

    let mut runtime_functions = Vec::new();
    for (mod_path, tc) in &project.type_checks {
        if *mod_path == entry_path {
            continue;
        }
        let Some(mod_info) = project.module_graph.get(mod_path) else {
            continue;
        };
        for (src_fn_idx, body) in &tc.fn_bodies {
            runtime_functions.push(ProjectRuntimeFunction {
                module_path: mod_path.clone(),
                fn_item: mod_info.item_tree.functions[*src_fn_idx].clone(),
                body: body.clone(),
                runtime_indices: Vec::new(),
            });
        }
    }

    {
        let entry_info = project
            .module_graph
            .get_mut(&entry_path)
            .ok_or(RuntimeError::TypeError("entry module not found".into()))?;

        for runtime_fn in &mut runtime_functions {
            let matching_indices = matching_entry_runtime_fn_indices(
                &entry_info.item_tree,
                &entry_info.scope,
                imported_member_names.get(&runtime_fn.module_path),
                imported_namespace_names.get(&runtime_fn.module_path),
                &runtime_fn.fn_item,
            );
            for runtime_idx in matching_indices {
                fn_bodies
                    .entry(runtime_idx)
                    .or_insert_with(|| runtime_fn.body.clone());
                if !runtime_fn.runtime_indices.contains(&runtime_idx) {
                    runtime_fn.runtime_indices.push(runtime_idx);
                }
            }

            if runtime_fn.runtime_indices.is_empty() {
                let runtime_idx = entry_info
                    .item_tree
                    .functions
                    .alloc(runtime_fn.fn_item.clone());
                fn_bodies.insert(runtime_idx, runtime_fn.body.clone());
                runtime_fn.runtime_indices.push(runtime_idx);
            }
        }
    }

    for (mod_path, tc) in &project.type_checks {
        if *mod_path == entry_path {
            continue;
        }
        let Some(mod_info) = project.module_graph.get(mod_path) else {
            continue;
        };
        let mod_item_tree = mod_info.item_tree.clone();

        let mut module_fn_map: FxHashMap<
            kyokara_hir_def::name::Name,
            Vec<kyokara_hir_def::item_tree::FnItemIdx>,
        > = FxHashMap::default();
        for runtime_fn in runtime_functions
            .iter()
            .filter(|runtime_fn| runtime_fn.module_path == *mod_path)
        {
            let Some(&runtime_idx) = runtime_fn.runtime_indices.first() else {
                continue;
            };
            let runtime_family = module_fn_map.entry(runtime_fn.fn_item.name).or_default();
            if !runtime_family.contains(&runtime_idx) {
                runtime_family.push(runtime_idx);
            }
        }

        let mut module_scope_override = mod_info.scope.clone();
        for import in &mod_info.item_tree.imports {
            let Some(target_path) = resolve_runtime_import_target(
                &project.module_graph,
                mod_path,
                &import.path,
                &project.interner,
            ) else {
                continue;
            };

            match &import.kind {
                kyokara_hir_def::item_tree::ImportKind::Members { members } => {
                    for member in members {
                        let visible_name = member.alias.unwrap_or(member.name);
                        let family = module_fn_map.entry(visible_name).or_default();
                        for runtime_fn in runtime_functions.iter().filter(|runtime_fn| {
                            runtime_fn.module_path == target_path
                                && runtime_fn.fn_item.is_pub
                                && runtime_fn.fn_item.name == member.name
                        }) {
                            let Some(&runtime_idx) = runtime_fn.runtime_indices.first() else {
                                continue;
                            };
                            if !family.contains(&runtime_idx) {
                                family.push(runtime_idx);
                            }
                        }
                    }
                }
                kyokara_hir_def::item_tree::ImportKind::Namespace { alias } => {
                    let visible_name = alias.unwrap_or_else(|| {
                        import
                            .path
                            .last()
                            .expect("namespace import path should not be empty")
                    });
                    let Some(namespace) = module_scope_override.namespaces.get_mut(&visible_name)
                    else {
                        continue;
                    };

                    let mut rewritten_functions = FxHashMap::default();
                    for runtime_fn in runtime_functions.iter().filter(|runtime_fn| {
                        runtime_fn.module_path == target_path && runtime_fn.fn_item.is_pub
                    }) {
                        let Some(&runtime_idx) = runtime_fn.runtime_indices.first() else {
                            continue;
                        };
                        let family = rewritten_functions
                            .entry(runtime_fn.fn_item.name)
                            .or_insert_with(Vec::new);
                        if !family.contains(&runtime_idx) {
                            family.push(runtime_idx);
                        }
                    }
                    namespace.functions = rewritten_functions;
                }
            }
        }

        let module_runtime_indices: Vec<_> = runtime_functions
            .iter()
            .filter(|runtime_fn| runtime_fn.module_path == *mod_path)
            .flat_map(|runtime_fn| runtime_fn.runtime_indices.iter().copied())
            .collect();
        for fn_idx in module_runtime_indices {
            fn_scope_overrides.insert(fn_idx, module_fn_map.clone());
            module_scope_overrides.insert(fn_idx, module_scope_override.clone());
        }

        if !tc.let_bodies.is_empty() {
            let module_interner =
                std::mem::replace(&mut project.interner, kyokara_intern::Interner::new());
            let mut let_interp = Interpreter::new_with_runtime(
                mod_item_tree.clone(),
                module_scope_override.clone(),
                augmented_module_fn_bodies(
                    mod_path,
                    mod_info,
                    tc,
                    &project.module_graph,
                    &project.interner,
                    &runtime_functions,
                ),
                tc.let_bodies.clone(),
                FxHashMap::default(),
                FxHashMap::default(),
                FxHashMap::default(),
                module_interner,
                runtime.clone(),
            );
            let module_let_values = let_interp.materialize_top_level_let_values()?;
            project.interner = let_interp.into_interner();
            for fn_idx in runtime_functions
                .iter()
                .filter(|runtime_fn| runtime_fn.module_path == *mod_path)
                .flat_map(|runtime_fn| runtime_fn.runtime_indices.iter().copied())
            {
                let_scope_overrides.insert(fn_idx, module_let_values.clone());
            }
        }
    }

    let entry_info = project
        .module_graph
        .get(&entry_path)
        .ok_or(RuntimeError::TypeError("entry module not found".into()))?;
    let mut interp = Interpreter::new_with_runtime(
        entry_info.item_tree.clone(),
        entry_info.scope.clone(),
        fn_bodies,
        entry_tc.let_bodies.clone(),
        let_scope_overrides,
        fn_scope_overrides,
        module_scope_overrides,
        project.interner,
        runtime.clone(),
    );

    let result = interp.run_main();
    let interner = interp.into_interner();
    finish_run_result(result, interner, &runtime)
}

fn finalize_runtime(runtime: &SharedRuntimeService) -> Result<(), RuntimeError> {
    runtime
        .borrow_mut()
        .finalize()
        .map_err(map_runtime_effect_error)
}

fn finish_run_result(
    result: Result<Value, RuntimeError>,
    interner: Interner,
    runtime: &SharedRuntimeService,
) -> Result<RunResult, RuntimeError> {
    let finalize = finalize_runtime(runtime);
    match (result, finalize) {
        (Ok(value), Ok(())) => Ok(RunResult { value, interner }),
        (Err(err), Ok(())) => Err(err),
        (_, Err(err)) => Err(err),
    }
}

struct ProjectRuntimeFunction {
    module_path: ModulePath,
    fn_item: kyokara_hir_def::item_tree::FnItem,
    body: kyokara_hir_def::body::Body,
    runtime_indices: Vec<kyokara_hir_def::item_tree::FnItemIdx>,
}

fn project_runtime_fn_matches(
    lhs: &kyokara_hir_def::item_tree::FnItem,
    rhs: &kyokara_hir_def::item_tree::FnItem,
) -> bool {
    let compatible_source_range = match (lhs.source_range, rhs.source_range) {
        (Some(lhs_range), Some(rhs_range)) => lhs_range == rhs_range,
        _ => true,
    };

    lhs.name == rhs.name
        && lhs.params == rhs.params
        && lhs.ret_type == rhs.ret_type
        && lhs.type_params == rhs.type_params
        && lhs.receiver_type == rhs.receiver_type
        && compatible_source_range
}

fn matching_entry_runtime_fn_indices(
    entry_item_tree: &kyokara_hir::ItemTree,
    entry_scope: &kyokara_hir::ModuleScope,
    member_imports: Option<&Vec<(kyokara_hir_def::name::Name, kyokara_hir_def::name::Name)>>,
    namespace_names: Option<&Vec<kyokara_hir_def::name::Name>>,
    src_fn_item: &kyokara_hir_def::item_tree::FnItem,
) -> Vec<kyokara_hir_def::item_tree::FnItemIdx> {
    let mut runtime_indices = Vec::new();

    if src_fn_item.is_pub {
        if let Some(member_imports) = member_imports {
            for (visible_name, source_name) in member_imports {
                if *source_name != src_fn_item.name {
                    continue;
                }
                let Some(entry_candidates) = entry_scope.functions.get(visible_name) else {
                    continue;
                };
                for &entry_fn_idx in entry_candidates {
                    let candidate = &entry_item_tree.functions[entry_fn_idx];
                    if project_runtime_fn_matches(candidate, src_fn_item)
                        && !runtime_indices.contains(&entry_fn_idx)
                    {
                        runtime_indices.push(entry_fn_idx);
                    }
                }
            }
        }
    }

    if let Some(namespace_names) = namespace_names {
        for namespace_name in namespace_names {
            let Some(namespace) = entry_scope.namespaces.get(namespace_name) else {
                continue;
            };
            let Some(namespace_candidates) = namespace.functions.get(&src_fn_item.name) else {
                continue;
            };
            for &entry_fn_idx in namespace_candidates {
                let candidate = &entry_item_tree.functions[entry_fn_idx];
                if project_runtime_fn_matches(candidate, src_fn_item)
                    && !runtime_indices.contains(&entry_fn_idx)
                {
                    runtime_indices.push(entry_fn_idx);
                }
            }
        }
    }

    runtime_indices
}

fn runtime_package_prefix<'a>(
    module_path: &'a ModulePath,
    interner: &Interner,
) -> &'a [kyokara_hir_def::name::Name] {
    let mut prefix_len = 0;
    while prefix_len + 1 < module_path.0.len()
        && module_path.0[prefix_len].resolve(interner) == "deps"
    {
        prefix_len += 2;
    }
    &module_path.0[..prefix_len]
}

fn resolve_runtime_import_target(
    graph: &kyokara_hir::ModuleGraph,
    importing_mod: &ModulePath,
    import_path: &kyokara_hir_def::path::Path,
    interner: &Interner,
) -> Option<ModulePath> {
    if import_path.segments.is_empty() {
        return None;
    }

    let is_dependency_import = import_path
        .segments
        .first()
        .is_some_and(|seg| seg.resolve(interner) == "deps");
    let importing_prefix = runtime_package_prefix(importing_mod, interner);
    if is_dependency_import {
        if import_path.segments.len() < 2 {
            return None;
        }
        let mut target_segments = importing_prefix.to_vec();
        target_segments.extend(import_path.segments.iter().copied());
        let target_path = ModulePath(target_segments);
        return graph.get(&target_path).map(|_| target_path);
    }

    if import_path.segments.len() > 1 {
        let mut target_segments = importing_prefix.to_vec();
        target_segments.extend(import_path.segments.iter().copied());
        let target_path = ModulePath(target_segments);
        return graph.get(&target_path).map(|_| target_path);
    }

    let resolve_name = import_path.segments[0];
    let candidates: Vec<_> = graph
        .iter()
        .filter_map(|(candidate_path, _)| {
            (runtime_package_prefix(candidate_path, interner) == importing_prefix
                && candidate_path.last() == Some(resolve_name))
            .then(|| candidate_path.clone())
        })
        .collect();
    (candidates.len() == 1).then(|| candidates[0].clone())
}

fn augmented_module_fn_bodies(
    module_path: &ModulePath,
    mod_info: &kyokara_hir::ModuleInfo,
    type_check: &kyokara_hir::TypeCheckResult,
    graph: &kyokara_hir::ModuleGraph,
    interner: &Interner,
    runtime_functions: &[ProjectRuntimeFunction],
) -> FxHashMap<kyokara_hir_def::item_tree::FnItemIdx, kyokara_hir_def::body::Body> {
    let mut fn_bodies = type_check.fn_bodies.clone();
    for import in &mod_info.item_tree.imports {
        let Some(target_path) =
            resolve_runtime_import_target(graph, module_path, &import.path, interner)
        else {
            continue;
        };

        match &import.kind {
            kyokara_hir_def::item_tree::ImportKind::Members { members } => {
                for member in members {
                    let visible_name = member.alias.unwrap_or(member.name);
                    let Some(candidates) = mod_info.scope.functions.get(&visible_name) else {
                        continue;
                    };
                    for &candidate_idx in candidates {
                        if fn_bodies.contains_key(&candidate_idx) {
                            continue;
                        }
                        let candidate_item = &mod_info.item_tree.functions[candidate_idx];
                        let Some(runtime_fn) = runtime_functions.iter().find(|runtime_fn| {
                            runtime_fn.module_path == target_path
                                && runtime_fn.fn_item.is_pub
                                && runtime_fn.fn_item.name == member.name
                                && project_runtime_fn_matches(&runtime_fn.fn_item, candidate_item)
                        }) else {
                            continue;
                        };
                        fn_bodies.insert(candidate_idx, runtime_fn.body.clone());
                    }
                }
            }
            kyokara_hir_def::item_tree::ImportKind::Namespace { alias } => {
                let visible_name = alias.unwrap_or_else(|| {
                    import
                        .path
                        .last()
                        .expect("namespace import path should not be empty")
                });
                let Some(namespace) = mod_info.scope.namespaces.get(&visible_name) else {
                    continue;
                };
                for candidates in namespace.functions.values() {
                    for &candidate_idx in candidates {
                        if fn_bodies.contains_key(&candidate_idx) {
                            continue;
                        }
                        let candidate_item = &mod_info.item_tree.functions[candidate_idx];
                        let Some(runtime_fn) = runtime_functions.iter().find(|runtime_fn| {
                            runtime_fn.module_path == target_path
                                && runtime_fn.fn_item.is_pub
                                && project_runtime_fn_matches(&runtime_fn.fn_item, candidate_item)
                        }) else {
                            continue;
                        };
                        fn_bodies.insert(candidate_idx, runtime_fn.body.clone());
                    }
                }
            }
        }
    }
    fn_bodies
}

fn collect_single_file_compile_errors(
    parse_errors: &[impl std::fmt::Debug],
    lowering_diagnostics: &[kyokara_diagnostics::Diagnostic],
    body_lowering_diagnostics: &[kyokara_diagnostics::Diagnostic],
    type_diagnostics: &[(kyokara_hir::TyDiagnosticData, kyokara_span::Span)],
    interner: &Interner,
    item_tree: &kyokara_hir::ItemTree,
) -> CompileGateErrors {
    let mut errors = CompileGateErrors::default();

    for err in parse_errors {
        errors.add(CompileErrorClass::Parse, format!("{err:?}"));
    }

    for diag in lowering_diagnostics {
        if diag.severity == kyokara_diagnostics::Severity::Error {
            errors.add(CompileErrorClass::Lowering, diag.message.clone());
        }
    }

    for diag in body_lowering_diagnostics {
        if diag.severity == kyokara_diagnostics::Severity::Error {
            errors.add(CompileErrorClass::Lowering, diag.message.clone());
        }
    }

    for (data, span) in type_diagnostics {
        let msg = data
            .clone()
            .into_diagnostic(*span, interner, item_tree)
            .message;
        errors.add(CompileErrorClass::Type, msg);
    }

    errors
}

fn collect_project_compile_errors(project: &kyokara_hir::ProjectCheckResult) -> CompileGateErrors {
    let mut errors = CompileGateErrors::default();

    for (_mod_path, errs) in &project.parse_errors {
        for err in errs {
            errors.add(CompileErrorClass::Parse, format!("{err:?}"));
        }
    }

    for diag in &project.lowering_diagnostics {
        if diag.severity == kyokara_diagnostics::Severity::Error {
            errors.add(CompileErrorClass::Lowering, diag.message.clone());
        }
    }

    for (_mod_path, tc) in &project.type_checks {
        for diag in &tc.body_lowering_diagnostics {
            if diag.severity == kyokara_diagnostics::Severity::Error {
                errors.add(CompileErrorClass::Lowering, diag.message.clone());
            }
        }
    }

    for (mod_path, tc) in &project.type_checks {
        let Some(mod_info) = project.module_graph.get(mod_path) else {
            continue;
        };
        for (data, span) in &tc.raw_diagnostics {
            let msg = data
                .clone()
                .into_diagnostic(*span, &project.interner, &mod_info.item_tree)
                .message;
            errors.add(CompileErrorClass::Type, msg);
        }
    }

    errors
}

fn normalize_parse_error_range(start: u32, end: u32, source_len: u32) -> TextRange {
    let start = start.min(source_len);
    let end = end.min(source_len);
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
