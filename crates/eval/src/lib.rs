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
    let file_id = FileId(0);

    // 1. Parse.
    let parse = kyokara_syntax::parse(source);
    if !parse.errors.is_empty() {
        let msgs: Vec<String> = parse.errors.iter().map(|e| format!("{e:?}")).collect();
        return Err(RuntimeError::TypeError(format!(
            "parse errors: {}",
            msgs.join("; ")
        )));
    }

    // 2. Build CST.
    let root = SyntaxNode::new_root(parse.green);
    let sf = SourceFile::cast(root.clone()).expect("parsed root should cast to SourceFile");

    // 3. Collect item tree.
    let mut interner = Interner::new();
    let mut item_result = collect_item_tree(&sf, file_id, &mut interner);

    if item_result
        .diagnostics
        .iter()
        .any(|d| d.severity == kyokara_diagnostics::Severity::Error)
    {
        let msgs: Vec<String> = item_result
            .diagnostics
            .iter()
            .map(|d| d.message.clone())
            .collect();
        return Err(RuntimeError::TypeError(format!(
            "lowering errors: {}",
            msgs.join("; ")
        )));
    }

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

    // Check body_lowering_diagnostics for errors (e.g. unresolved names,
    // duplicate bindings).
    let body_errors: Vec<String> = type_check
        .body_lowering_diagnostics
        .iter()
        .filter(|d| d.severity == kyokara_diagnostics::Severity::Error)
        .map(|d| d.message.clone())
        .collect();
    if !body_errors.is_empty() {
        return Err(RuntimeError::TypeError(format!(
            "lowering errors: {}",
            body_errors.join("; ")
        )));
    }

    // Check raw_diagnostics for real type errors.
    if !type_check.raw_diagnostics.is_empty() {
        let msgs: Vec<String> = type_check
            .raw_diagnostics
            .iter()
            .map(|(data, _span)| {
                data.clone()
                    .into_diagnostic(
                        kyokara_span::Span {
                            file: file_id,
                            range: Default::default(),
                        },
                        &interner,
                        &item_result.tree,
                    )
                    .message
            })
            .collect();
        return Err(RuntimeError::TypeError(msgs.join("; ")));
    }

    // 7. Interpret.
    let mut interp = Interpreter::new(
        item_result.tree,
        item_result.module_scope,
        type_check.fn_bodies,
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
    let mut project = check_project(entry_file);

    // Check for parse errors across all modules.
    if !project.parse_errors.is_empty() {
        let msgs: Vec<String> = project
            .parse_errors
            .iter()
            .flat_map(|(_mod_path, errs)| errs.iter().map(|e| format!("{e:?}")))
            .collect();
        return Err(RuntimeError::TypeError(format!(
            "parse errors: {}",
            msgs.join("; ")
        )));
    }

    // Check for lowering diagnostics (e.g. duplicate definitions).
    let lowering_errors: Vec<&kyokara_diagnostics::Diagnostic> = project
        .lowering_diagnostics
        .iter()
        .filter(|d| d.severity == kyokara_diagnostics::Severity::Error)
        .collect();
    if !lowering_errors.is_empty() {
        let msgs: Vec<String> = lowering_errors.iter().map(|d| d.message.clone()).collect();
        return Err(RuntimeError::TypeError(format!(
            "lowering errors: {}",
            msgs.join("; ")
        )));
    }

    // Check for body lowering errors (e.g. unresolved names) across all modules.
    let mut body_lowering_errors = Vec::new();
    for (_mod_path, tc) in &project.type_checks {
        for diag in &tc.body_lowering_diagnostics {
            if diag.severity == kyokara_diagnostics::Severity::Error {
                body_lowering_errors.push(diag.message.clone());
            }
        }
    }
    if !body_lowering_errors.is_empty() {
        return Err(RuntimeError::TypeError(format!(
            "lowering errors: {}",
            body_lowering_errors.join("; ")
        )));
    }

    // Check for type errors across all modules.
    let mut type_errors = Vec::new();
    for (_mod_path, tc) in &project.type_checks {
        for (data, _span) in &tc.raw_diagnostics {
            let msg = format!("{data:?}");
            type_errors.push(msg);
        }
    }
    if !type_errors.is_empty() {
        return Err(RuntimeError::TypeError(format!(
            "type error at compile time: {}",
            type_errors.join("; ")
        )));
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
    let entry_info = project.module_graph.get(&entry_path).unwrap();

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

    // Collect private function items + bodies from imported modules so that
    // public functions can call private helpers in their own module (#69).
    // We accumulate (FnItem, Body) pairs to splice into the entry module later.
    let mut private_fns_to_splice: Vec<(
        kyokara_hir_def::item_tree::FnItem,
        kyokara_hir_def::body::Body,
    )> = Vec::new();

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

        let entry_info = project.module_graph.get(&entry_path).unwrap();
        let entry_tree = &entry_info.item_tree;
        let entry_scope = &entry_info.scope;

        // For each body in this module, check if the entry module imported it (pub fn).
        for (src_fn_idx, body) in &tc.fn_bodies {
            let src_fn_item = &mod_info.item_tree.functions[*src_fn_idx];

            // Check if this function was cloned into the entry module's tree (pub).
            let mut matched_entry = false;
            for (entry_fn_idx, entry_fn_item) in entry_tree.functions.iter() {
                if entry_fn_item.name == src_fn_item.name
                    && !fn_bodies.contains_key(&entry_fn_idx)
                    && entry_scope.functions.get(&entry_fn_item.name) == Some(&entry_fn_idx)
                {
                    fn_bodies.insert(entry_fn_idx, body.clone());
                    matched_entry = true;
                }
            }

            // If it wasn't matched (private fn), collect it for splicing.
            if !matched_entry && !src_fn_item.is_pub {
                private_fns_to_splice.push((src_fn_item.clone(), body.clone()));
            }
        }
    }

    // Splice private helper functions into the entry module's item tree and scope
    // so that imported public functions can resolve their private callees at
    // runtime. The type checker already prevents direct access from main (#69).
    let entry_info = project.module_graph.get_mut(&entry_path).unwrap();
    for (fn_item, body) in private_fns_to_splice {
        let name = fn_item.name;
        // Only add if there's no name conflict with the entry module's own functions.
        if !entry_info.scope.functions.contains_key(&name) {
            let idx = entry_info.item_tree.functions.alloc(fn_item);
            entry_info.scope.functions.insert(name, idx);
            fn_bodies.insert(idx, body);
        }
    }

    let entry_info = project.module_graph.get(&entry_path).unwrap();
    let mut interp = Interpreter::new(
        entry_info.item_tree.clone(),
        entry_info.scope.clone(),
        fn_bodies,
        project.interner,
        manifest,
    );

    let value = interp.run_main()?;
    let interner = interp.into_interner();
    Ok(RunResult { value, interner })
}
