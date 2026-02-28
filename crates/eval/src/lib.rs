//! `kyokara-eval` — Tree-walking interpreter for Kyokara.
//!
//! Walks the HIR expression trees produced by the compiler frontend
//! and evaluates them directly. Used by `kyokara run <file>`.

pub mod env;
pub mod error;
pub mod interpreter;
pub mod intrinsics;
pub mod manifest;
pub mod value;

use kyokara_hir::{
    ModulePath, check_module, check_project, collect_item_tree, register_builtin_intrinsics,
    register_builtin_types,
};
use kyokara_intern::Interner;
use kyokara_span::FileId;
use kyokara_stdx::FxHashMap;
use kyokara_syntax::SyntaxNode;
use kyokara_syntax::ast::AstNode;
use kyokara_syntax::ast::nodes::SourceFile;

use crate::error::RuntimeError;
use crate::interpreter::Interpreter;
use crate::manifest::CapabilityManifest;
use crate::value::Value;

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

    let file_id = FileId(0);

    // 1. Parse.
    let parse = kyokara_syntax::parse(source);

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

    // 5. Register intrinsic function signatures in the item tree + module scope.
    register_builtin_intrinsics(
        &mut item_result.tree,
        &mut item_result.module_scope,
        &mut interner,
    );

    // 6. Type-check.
    let type_check = check_module(
        &root,
        &item_result.tree,
        &item_result.module_scope,
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
    let mut interp = Interpreter::new(
        item_result.tree,
        item_result.module_scope,
        type_check.fn_bodies,
        FxHashMap::default(),
        interner,
        manifest,
    );

    let value = interp.run_main()?;
    let interner = interp.into_interner();
    Ok(RunResult { value, interner })
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

    // Register intrinsics in the entry module.
    register_builtin_intrinsics(
        &mut entry_info.item_tree,
        &mut entry_info.scope,
        &mut project.interner,
    );

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

    // Build the set of module paths that the entry module imports.
    let imported_mod_paths: Vec<ModulePath> = entry_info
        .item_tree
        .imports
        .iter()
        .filter_map(|imp| {
            let target_name = imp.path.last()?;
            // Resolve to the actual module path, same as resolve_project_imports does.
            for (mod_path, _) in project.module_graph.iter() {
                if mod_path.last() == Some(target_name) {
                    return Some(mod_path.clone());
                }
            }
            None
        })
        .collect();

    // Per-function module override maps used by the interpreter so imported
    // functions can resolve private helpers in their source module without
    // leaking names into entry scope.
    let mut fn_scope_overrides: FxHashMap<
        kyokara_hir_def::item_tree::FnItemIdx,
        FxHashMap<kyokara_hir_def::name::Name, kyokara_hir_def::item_tree::FnItemIdx>,
    > = FxHashMap::default();

    for (mod_path, tc) in &project.type_checks {
        if *mod_path == entry_path {
            continue;
        }
        // Only process modules that the entry module actually imported.
        if !imported_mod_paths.contains(mod_path) {
            continue;
        }
        let Some(mod_info) = project.module_graph.get(mod_path) else {
            continue;
        };

        // Build a module-local name -> runtime fn index map.
        let mut module_fn_map: FxHashMap<
            kyokara_hir_def::name::Name,
            kyokara_hir_def::item_tree::FnItemIdx,
        > = FxHashMap::default();

        // Collect private function items + bodies to splice after immutable borrows end.
        let mut private_fns_to_splice: Vec<(
            kyokara_hir_def::name::Name,
            kyokara_hir_def::item_tree::FnItem,
            kyokara_hir_def::body::Body,
        )> = Vec::new();

        // For each body in this module, check if the entry module imported it (pub fn).
        for (src_fn_idx, body) in &tc.fn_bodies {
            let src_fn_item = &mod_info.item_tree.functions[*src_fn_idx];

            if src_fn_item.is_pub {
                let entry_info = project
                    .module_graph
                    .get(&entry_path)
                    .ok_or(RuntimeError::TypeError("entry module not found".into()))?;
                if let Some(&entry_fn_idx) = entry_info.scope.functions.get(&src_fn_item.name) {
                    fn_bodies
                        .entry(entry_fn_idx)
                        .or_insert_with(|| body.clone());
                    module_fn_map.insert(src_fn_item.name, entry_fn_idx);
                }
                continue;
            }

            private_fns_to_splice.push((src_fn_item.name, src_fn_item.clone(), body.clone()));
        }

        // Splice private helpers into the entry item tree (not entry scope).
        // They stay inaccessible from `main` but callable from imported module
        // functions via module-local override maps.
        {
            let entry_info = project
                .module_graph
                .get_mut(&entry_path)
                .ok_or(RuntimeError::TypeError("entry module not found".into()))?;
            for (name, fn_item, body) in private_fns_to_splice {
                let idx = entry_info.item_tree.functions.alloc(fn_item);
                fn_bodies.insert(idx, body);
                module_fn_map.insert(name, idx);
            }
        }

        // Attach the same module-local map to every function from this module.
        for &fn_idx in module_fn_map.values() {
            fn_scope_overrides.insert(fn_idx, module_fn_map.clone());
        }
    }

    let entry_info = project
        .module_graph
        .get(&entry_path)
        .ok_or(RuntimeError::TypeError("entry module not found".into()))?;
    let mut interp = Interpreter::new(
        entry_info.item_tree.clone(),
        entry_info.scope.clone(),
        fn_bodies,
        fn_scope_overrides,
        project.interner,
        manifest,
    );

    let value = interp.run_main()?;
    let interner = interp.into_interner();
    Ok(RunResult { value, interner })
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
