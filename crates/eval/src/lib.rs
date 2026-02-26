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
    FnItem, FnParam, ModulePath, Name, Path, TypeRef, check_module, check_project,
    collect_item_tree, register_builtin_types,
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
    register_intrinsics(
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

    // Check raw_diagnostics for real type errors.
    // (type_check.diagnostics also includes body-lowering false positives
    // for constructor pattern bindings, so we skip those.)
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
    register_intrinsics(
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
    let entry_info = project.module_graph.get(&entry_path).unwrap();
    let entry_tree = &entry_info.item_tree;
    let entry_scope = &entry_info.scope;

    for (mod_path, tc) in &project.type_checks {
        if *mod_path == entry_path {
            continue;
        }
        let Some(mod_info) = project.module_graph.get(mod_path) else {
            continue;
        };

        // For each body in this module, check if the entry module imported it.
        for (src_fn_idx, body) in &tc.fn_bodies {
            let src_fn_item = &mod_info.item_tree.functions[*src_fn_idx];
            // Find matching function in entry module's tree by name
            // that doesn't already have a body (from the entry module's own check).
            for (entry_fn_idx, entry_fn_item) in entry_tree.functions.iter() {
                if entry_fn_item.name == src_fn_item.name
                    && !fn_bodies.contains_key(&entry_fn_idx)
                    && entry_scope.functions.get(&entry_fn_item.name) == Some(&entry_fn_idx)
                {
                    fn_bodies.insert(entry_fn_idx, body.clone());
                }
            }
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

/// Register intrinsic functions as bodyless items in the item tree and module scope.
fn register_intrinsics(
    tree: &mut kyokara_hir::ItemTree,
    scope: &mut kyokara_hir::ModuleScope,
    interner: &mut Interner,
) {
    let intrinsic_sigs = intrinsic_signatures(interner);

    for (name, fn_item) in intrinsic_sigs {
        let idx = tree.functions.alloc(fn_item);
        scope.functions.insert(name, idx);
    }
}

/// Helper to build a simple intrinsic FnItem.
fn mk_intrinsic(
    interner: &mut Interner,
    name_str: &str,
    type_params: Vec<Name>,
    params: Vec<(&str, TypeRef)>,
    ret: TypeRef,
) -> (Name, FnItem) {
    let name = Name::new(interner, name_str);
    let fn_params = params
        .into_iter()
        .map(|(pname, ty)| FnParam {
            name: Name::new(interner, pname),
            ty,
        })
        .collect();
    (
        name,
        FnItem {
            name,
            is_pub: false,
            type_params,
            params: fn_params,
            ret_type: Some(ret),
            with_caps: Vec::new(),
            pipe_caps: Vec::new(),
            has_body: false,
        },
    )
}

/// Build FnItem signatures for each intrinsic.
fn intrinsic_signatures(interner: &mut Interner) -> Vec<(Name, FnItem)> {
    // ── Shared type refs ──────────────────────────────────────────
    let string_ty = TypeRef::Path {
        path: Path::single(Name::new(interner, "String")),
        args: Vec::new(),
    };
    let int_ty = TypeRef::Path {
        path: Path::single(Name::new(interner, "Int")),
        args: Vec::new(),
    };
    let float_ty = TypeRef::Path {
        path: Path::single(Name::new(interner, "Float")),
        args: Vec::new(),
    };
    let bool_ty = TypeRef::Path {
        path: Path::single(Name::new(interner, "Bool")),
        args: Vec::new(),
    };
    let char_ty = TypeRef::Path {
        path: Path::single(Name::new(interner, "Char")),
        args: Vec::new(),
    };
    let unit_ty = TypeRef::Path {
        path: Path::single(Name::new(interner, "Unit")),
        args: Vec::new(),
    };

    // Type parameter names.
    let t_name = Name::new(interner, "T");
    let u_name = Name::new(interner, "U");
    let k_name = Name::new(interner, "K");
    let v_name = Name::new(interner, "V");

    // Generic type refs.
    let t_ref = TypeRef::Path {
        path: Path::single(t_name),
        args: Vec::new(),
    };
    let u_ref = TypeRef::Path {
        path: Path::single(u_name),
        args: Vec::new(),
    };
    let k_ref = TypeRef::Path {
        path: Path::single(k_name),
        args: Vec::new(),
    };
    let v_ref = TypeRef::Path {
        path: Path::single(v_name),
        args: Vec::new(),
    };

    // Composite type refs.
    let list_t = TypeRef::Path {
        path: Path::single(Name::new(interner, "List")),
        args: vec![t_ref.clone()],
    };
    let list_u = TypeRef::Path {
        path: Path::single(Name::new(interner, "List")),
        args: vec![u_ref.clone()],
    };
    let list_k = TypeRef::Path {
        path: Path::single(Name::new(interner, "List")),
        args: vec![k_ref.clone()],
    };
    let list_v = TypeRef::Path {
        path: Path::single(Name::new(interner, "List")),
        args: vec![v_ref.clone()],
    };
    let list_string = TypeRef::Path {
        path: Path::single(Name::new(interner, "List")),
        args: vec![string_ty.clone()],
    };
    let map_kv = TypeRef::Path {
        path: Path::single(Name::new(interner, "Map")),
        args: vec![k_ref.clone(), v_ref.clone()],
    };
    let option_t = TypeRef::Path {
        path: Path::single(Name::new(interner, "Option")),
        args: vec![t_ref.clone()],
    };
    let option_v = TypeRef::Path {
        path: Path::single(Name::new(interner, "Option")),
        args: vec![v_ref.clone()],
    };

    // Function type refs for higher-order intrinsics.
    let fn_t_to_u = TypeRef::Fn {
        params: vec![t_ref.clone()],
        ret: Box::new(u_ref.clone()),
    };
    let fn_t_to_bool = TypeRef::Fn {
        params: vec![t_ref.clone()],
        ret: Box::new(bool_ty.clone()),
    };
    let fn_ut_to_u = TypeRef::Fn {
        params: vec![u_ref.clone(), t_ref.clone()],
        ret: Box::new(u_ref.clone()),
    };

    vec![
        // ── I/O ──────────────────────────────────────────────────
        mk_intrinsic(
            interner,
            "print",
            vec![],
            vec![("s", string_ty.clone())],
            unit_ty.clone(),
        ),
        mk_intrinsic(
            interner,
            "println",
            vec![],
            vec![("s", string_ty.clone())],
            unit_ty.clone(),
        ),
        mk_intrinsic(
            interner,
            "int_to_string",
            vec![],
            vec![("n", int_ty.clone())],
            string_ty.clone(),
        ),
        mk_intrinsic(
            interner,
            "string_concat",
            vec![],
            vec![("a", string_ty.clone()), ("b", string_ty.clone())],
            string_ty.clone(),
        ),
        // ── List<T> ─────────────────────────────────────────────
        // list_new<T>() -> List<T>
        mk_intrinsic(interner, "list_new", vec![t_name], vec![], list_t.clone()),
        // list_push<T>(xs: List<T>, x: T) -> List<T>
        mk_intrinsic(
            interner,
            "list_push",
            vec![t_name],
            vec![("xs", list_t.clone()), ("x", t_ref.clone())],
            list_t.clone(),
        ),
        // list_len<T>(xs: List<T>) -> Int
        mk_intrinsic(
            interner,
            "list_len",
            vec![t_name],
            vec![("xs", list_t.clone())],
            int_ty.clone(),
        ),
        // list_get<T>(xs: List<T>, i: Int) -> Option<T>
        mk_intrinsic(
            interner,
            "list_get",
            vec![t_name],
            vec![("xs", list_t.clone()), ("i", int_ty.clone())],
            option_t.clone(),
        ),
        // list_head<T>(xs: List<T>) -> Option<T>
        mk_intrinsic(
            interner,
            "list_head",
            vec![t_name],
            vec![("xs", list_t.clone())],
            option_t.clone(),
        ),
        // list_tail<T>(xs: List<T>) -> List<T>
        mk_intrinsic(
            interner,
            "list_tail",
            vec![t_name],
            vec![("xs", list_t.clone())],
            list_t.clone(),
        ),
        // list_is_empty<T>(xs: List<T>) -> Bool
        mk_intrinsic(
            interner,
            "list_is_empty",
            vec![t_name],
            vec![("xs", list_t.clone())],
            bool_ty.clone(),
        ),
        // list_reverse<T>(xs: List<T>) -> List<T>
        mk_intrinsic(
            interner,
            "list_reverse",
            vec![t_name],
            vec![("xs", list_t.clone())],
            list_t.clone(),
        ),
        // list_concat<T>(a: List<T>, b: List<T>) -> List<T>
        mk_intrinsic(
            interner,
            "list_concat",
            vec![t_name],
            vec![("a", list_t.clone()), ("b", list_t.clone())],
            list_t.clone(),
        ),
        // list_map<T, U>(xs: List<T>, f: fn(T) -> U) -> List<U>
        mk_intrinsic(
            interner,
            "list_map",
            vec![t_name, u_name],
            vec![("xs", list_t.clone()), ("f", fn_t_to_u.clone())],
            list_u.clone(),
        ),
        // list_filter<T>(xs: List<T>, f: fn(T) -> Bool) -> List<T>
        mk_intrinsic(
            interner,
            "list_filter",
            vec![t_name],
            vec![("xs", list_t.clone()), ("f", fn_t_to_bool.clone())],
            list_t.clone(),
        ),
        // list_fold<T, U>(xs: List<T>, init: U, f: fn(U, T) -> U) -> U
        mk_intrinsic(
            interner,
            "list_fold",
            vec![t_name, u_name],
            vec![
                ("xs", list_t.clone()),
                ("init", u_ref.clone()),
                ("f", fn_ut_to_u.clone()),
            ],
            u_ref.clone(),
        ),
        // ── Map<K, V> ───────────────────────────────────────────
        // map_new<K, V>() -> Map<K, V>
        mk_intrinsic(
            interner,
            "map_new",
            vec![k_name, v_name],
            vec![],
            map_kv.clone(),
        ),
        // map_insert<K, V>(m: Map<K,V>, k: K, v: V) -> Map<K,V>
        mk_intrinsic(
            interner,
            "map_insert",
            vec![k_name, v_name],
            vec![
                ("m", map_kv.clone()),
                ("k", k_ref.clone()),
                ("v", v_ref.clone()),
            ],
            map_kv.clone(),
        ),
        // map_get<K, V>(m: Map<K,V>, k: K) -> Option<V>
        mk_intrinsic(
            interner,
            "map_get",
            vec![k_name, v_name],
            vec![("m", map_kv.clone()), ("k", k_ref.clone())],
            option_v.clone(),
        ),
        // map_contains<K, V>(m: Map<K,V>, k: K) -> Bool
        mk_intrinsic(
            interner,
            "map_contains",
            vec![k_name, v_name],
            vec![("m", map_kv.clone()), ("k", k_ref.clone())],
            bool_ty.clone(),
        ),
        // map_remove<K, V>(m: Map<K,V>, k: K) -> Map<K,V>
        mk_intrinsic(
            interner,
            "map_remove",
            vec![k_name, v_name],
            vec![("m", map_kv.clone()), ("k", k_ref.clone())],
            map_kv.clone(),
        ),
        // map_len<K, V>(m: Map<K,V>) -> Int
        mk_intrinsic(
            interner,
            "map_len",
            vec![k_name, v_name],
            vec![("m", map_kv.clone())],
            int_ty.clone(),
        ),
        // map_keys<K, V>(m: Map<K,V>) -> List<K>
        mk_intrinsic(
            interner,
            "map_keys",
            vec![k_name, v_name],
            vec![("m", map_kv.clone())],
            list_k.clone(),
        ),
        // map_values<K, V>(m: Map<K,V>) -> List<V>
        mk_intrinsic(
            interner,
            "map_values",
            vec![k_name, v_name],
            vec![("m", map_kv.clone())],
            list_v.clone(),
        ),
        // map_is_empty<K, V>(m: Map<K,V>) -> Bool
        mk_intrinsic(
            interner,
            "map_is_empty",
            vec![k_name, v_name],
            vec![("m", map_kv.clone())],
            bool_ty.clone(),
        ),
        // ── String ops ──────────────────────────────────────────
        // string_len(s: String) -> Int
        mk_intrinsic(
            interner,
            "string_len",
            vec![],
            vec![("s", string_ty.clone())],
            int_ty.clone(),
        ),
        // string_contains(s: String, sub: String) -> Bool
        mk_intrinsic(
            interner,
            "string_contains",
            vec![],
            vec![("s", string_ty.clone()), ("sub", string_ty.clone())],
            bool_ty.clone(),
        ),
        // string_starts_with(s: String, prefix: String) -> Bool
        mk_intrinsic(
            interner,
            "string_starts_with",
            vec![],
            vec![("s", string_ty.clone()), ("prefix", string_ty.clone())],
            bool_ty.clone(),
        ),
        // string_ends_with(s: String, suffix: String) -> Bool
        mk_intrinsic(
            interner,
            "string_ends_with",
            vec![],
            vec![("s", string_ty.clone()), ("suffix", string_ty.clone())],
            bool_ty.clone(),
        ),
        // string_trim(s: String) -> String
        mk_intrinsic(
            interner,
            "string_trim",
            vec![],
            vec![("s", string_ty.clone())],
            string_ty.clone(),
        ),
        // string_split(s: String, delim: String) -> List<String>
        mk_intrinsic(
            interner,
            "string_split",
            vec![],
            vec![("s", string_ty.clone()), ("delim", string_ty.clone())],
            list_string.clone(),
        ),
        // string_substring(s: String, start: Int, end: Int) -> String
        mk_intrinsic(
            interner,
            "string_substring",
            vec![],
            vec![
                ("s", string_ty.clone()),
                ("start", int_ty.clone()),
                ("end", int_ty.clone()),
            ],
            string_ty.clone(),
        ),
        // string_to_upper(s: String) -> String
        mk_intrinsic(
            interner,
            "string_to_upper",
            vec![],
            vec![("s", string_ty.clone())],
            string_ty.clone(),
        ),
        // string_to_lower(s: String) -> String
        mk_intrinsic(
            interner,
            "string_to_lower",
            vec![],
            vec![("s", string_ty.clone())],
            string_ty.clone(),
        ),
        // char_to_string(c: Char) -> String
        mk_intrinsic(
            interner,
            "char_to_string",
            vec![],
            vec![("c", char_ty.clone())],
            string_ty.clone(),
        ),
        // ── Int/Float math ──────────────────────────────────────
        // abs(n: Int) -> Int
        mk_intrinsic(
            interner,
            "abs",
            vec![],
            vec![("n", int_ty.clone())],
            int_ty.clone(),
        ),
        // min(a: Int, b: Int) -> Int
        mk_intrinsic(
            interner,
            "min",
            vec![],
            vec![("a", int_ty.clone()), ("b", int_ty.clone())],
            int_ty.clone(),
        ),
        // max(a: Int, b: Int) -> Int
        mk_intrinsic(
            interner,
            "max",
            vec![],
            vec![("a", int_ty.clone()), ("b", int_ty.clone())],
            int_ty.clone(),
        ),
        // float_abs(f: Float) -> Float
        mk_intrinsic(
            interner,
            "float_abs",
            vec![],
            vec![("f", float_ty.clone())],
            float_ty.clone(),
        ),
        // float_min(a: Float, b: Float) -> Float
        mk_intrinsic(
            interner,
            "float_min",
            vec![],
            vec![("a", float_ty.clone()), ("b", float_ty.clone())],
            float_ty.clone(),
        ),
        // float_max(a: Float, b: Float) -> Float
        mk_intrinsic(
            interner,
            "float_max",
            vec![],
            vec![("a", float_ty.clone()), ("b", float_ty.clone())],
            float_ty.clone(),
        ),
        // int_to_float(n: Int) -> Float
        mk_intrinsic(
            interner,
            "int_to_float",
            vec![],
            vec![("n", int_ty.clone())],
            float_ty.clone(),
        ),
        // float_to_int(f: Float) -> Int
        mk_intrinsic(
            interner,
            "float_to_int",
            vec![],
            vec![("f", float_ty.clone())],
            int_ty.clone(),
        ),
    ]
}
