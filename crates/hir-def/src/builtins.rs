//! Built-in type definitions injected before type-checking.
//!
//! Registers `Option<T>` and `Result<T, E>` as synthetic ADT types
//! in the item tree and module scope. Both the eval pipeline and the
//! hir `check_file` pipeline call [`register_builtin_types`] after
//! item tree collection but before type-checking.

use crate::item_tree::{FnItem, FnParam, ItemTree, TypeDefKind, TypeItem, TypeItemIdx, VariantDef};
use crate::name::Name;
use crate::path::Path;
use crate::resolver::{
    CoreType, CoreTypeInfo, ModuleScope, PrimitiveType, ReceiverKey, StaticOwnerKey, WellKnownNames,
};
use crate::type_ref::TypeRef;
use kyokara_intern::Interner;

/// Temporary reservation until qualified constructors are implemented.
pub const RESERVED_CORE_CONSTRUCTORS: [&str; 6] =
    ["Some", "None", "Ok", "Err", "InvalidInt", "InvalidFloat"];

pub fn is_reserved_core_constructor_name(name: Name, interner: &Interner) -> bool {
    RESERVED_CORE_CONSTRUCTORS
        .iter()
        .any(|reserved| name.resolve(interner) == *reserved)
}

fn core_hidden_type_name(interner: &mut Interner, core: CoreType) -> Name {
    let hidden = match core {
        CoreType::Option => "$core_Option",
        CoreType::Result => "$core_Result",
        CoreType::List => "$core_List",
        CoreType::Seq => "$core_Seq",
        CoreType::Map => "$core_Map",
        CoreType::Set => "$core_Set",
        CoreType::ParseError => "$core_ParseError",
    };
    Name::new(interner, hidden)
}

fn core_public_type_name(interner: &mut Interner, core: CoreType) -> Name {
    let public = match core {
        CoreType::Option => "Option",
        CoreType::Result => "Result",
        CoreType::List => "List",
        CoreType::Seq => "Seq",
        CoreType::Map => "Map",
        CoreType::Set => "Set",
        CoreType::ParseError => "ParseError",
    };
    Name::new(interner, public)
}

fn register_core_type_item(
    tree: &mut ItemTree,
    scope: &mut ModuleScope,
    interner: &mut Interner,
    core: CoreType,
    type_params: Vec<Name>,
    kind: TypeDefKind,
) -> (TypeItemIdx, Name) {
    let public_name = core_public_type_name(interner, core);
    let type_name = if scope.types.contains_key(&public_name) {
        core_hidden_type_name(interner, core)
    } else {
        public_name
    };

    let idx = tree.types.alloc(TypeItem {
        name: type_name,
        is_pub: false,
        type_params,
        kind,
    });

    scope.types.insert(type_name, idx);
    scope.core_types.set(
        core,
        CoreTypeInfo {
            type_idx: idx,
            type_name,
        },
    );

    (idx, type_name)
}

/// Inject `Option<T>`, `Result<T, E>`, `List<T>`, `Seq<T>`, `Map<K,V>`, `Set<T>`, and
/// `ParseError` into the item tree
/// and module scope.
///
/// Core types are always registered with stable identities. If a user type
/// shadows a public name (e.g. `type Result = ...`), the core type is
/// allocated under a hidden internal name.
pub fn register_builtin_types(
    tree: &mut ItemTree,
    scope: &mut ModuleScope,
    interner: &mut Interner,
) {
    register_option(tree, scope, interner);
    register_result(tree, scope, interner);
    register_list(tree, scope, interner);
    register_seq(tree, scope, interner);
    register_map(tree, scope, interner);
    register_set(tree, scope, interner);
    register_parse_error(tree, scope, interner);
}

/// `type Option<T> = Some(T) | None`
fn register_option(tree: &mut ItemTree, scope: &mut ModuleScope, interner: &mut Interner) {
    let t_name = Name::new(interner, "T");
    let some_name = Name::new(interner, "Some");
    let none_name = Name::new(interner, "None");

    let t_ref = TypeRef::Path {
        path: Path::single(t_name),
        args: Vec::new(),
    };

    let (idx, _) = register_core_type_item(
        tree,
        scope,
        interner,
        CoreType::Option,
        vec![t_name],
        TypeDefKind::Adt {
            variants: vec![
                VariantDef {
                    name: some_name,
                    fields: vec![t_ref],
                },
                VariantDef {
                    name: none_name,
                    fields: vec![],
                },
            ],
        },
    );

    scope.constructors.insert(some_name, (idx, 0));
    scope.constructors.insert(none_name, (idx, 1));
}

/// `List<T>` — opaque builtin type (no variants, no pattern matching).
fn register_list(tree: &mut ItemTree, scope: &mut ModuleScope, interner: &mut Interner) {
    let t_name = Name::new(interner, "T");
    let _ = register_core_type_item(
        tree,
        scope,
        interner,
        CoreType::List,
        vec![t_name],
        TypeDefKind::Adt { variants: vec![] },
    );
}

/// `Seq<T>` — opaque builtin type (no variants, no pattern matching).
fn register_seq(tree: &mut ItemTree, scope: &mut ModuleScope, interner: &mut Interner) {
    let t_name = Name::new(interner, "T");
    let _ = register_core_type_item(
        tree,
        scope,
        interner,
        CoreType::Seq,
        vec![t_name],
        TypeDefKind::Adt { variants: vec![] },
    );
}

/// `Map<K, V>` — opaque builtin type (no variants, no pattern matching).
fn register_map(tree: &mut ItemTree, scope: &mut ModuleScope, interner: &mut Interner) {
    let k_name = Name::new(interner, "K");
    let v_name = Name::new(interner, "V");
    let _ = register_core_type_item(
        tree,
        scope,
        interner,
        CoreType::Map,
        vec![k_name, v_name],
        TypeDefKind::Adt { variants: vec![] },
    );
}

/// `Set<T>` — opaque builtin type (no variants, no pattern matching).
fn register_set(tree: &mut ItemTree, scope: &mut ModuleScope, interner: &mut Interner) {
    let t_name = Name::new(interner, "T");
    let _ = register_core_type_item(
        tree,
        scope,
        interner,
        CoreType::Set,
        vec![t_name],
        TypeDefKind::Adt { variants: vec![] },
    );
}

/// `type Result<T, E> = Ok(T) | Err(E)`
fn register_result(tree: &mut ItemTree, scope: &mut ModuleScope, interner: &mut Interner) {
    let t_name = Name::new(interner, "T");
    let e_name = Name::new(interner, "E");
    let ok_name = Name::new(interner, "Ok");
    let err_name = Name::new(interner, "Err");

    let t_ref = TypeRef::Path {
        path: Path::single(t_name),
        args: Vec::new(),
    };
    let e_ref = TypeRef::Path {
        path: Path::single(e_name),
        args: Vec::new(),
    };

    let (idx, _) = register_core_type_item(
        tree,
        scope,
        interner,
        CoreType::Result,
        vec![t_name, e_name],
        TypeDefKind::Adt {
            variants: vec![
                VariantDef {
                    name: ok_name,
                    fields: vec![t_ref],
                },
                VariantDef {
                    name: err_name,
                    fields: vec![e_ref],
                },
            ],
        },
    );

    scope.constructors.insert(ok_name, (idx, 0));
    scope.constructors.insert(err_name, (idx, 1));
}
/// `type ParseError = InvalidInt(String) | InvalidFloat(String)`
fn register_parse_error(tree: &mut ItemTree, scope: &mut ModuleScope, interner: &mut Interner) {
    let invalid_int_name = Name::new(interner, "InvalidInt");
    let invalid_float_name = Name::new(interner, "InvalidFloat");

    let string_ref = TypeRef::Path {
        path: Path::single(Name::new(interner, "String")),
        args: Vec::new(),
    };

    let (idx, _) = register_core_type_item(
        tree,
        scope,
        interner,
        CoreType::ParseError,
        vec![],
        TypeDefKind::Adt {
            variants: vec![
                VariantDef {
                    name: invalid_int_name,
                    fields: vec![string_ref.clone()],
                },
                VariantDef {
                    name: invalid_float_name,
                    fields: vec![string_ref],
                },
            ],
        },
    );

    scope.constructors.insert(invalid_int_name, (idx, 0));
    scope.constructors.insert(invalid_float_name, (idx, 1));
}

/// Allocate all intrinsic FnItem signatures in the item tree and return
/// a lookup table `name → FnItemIdx`. Does NOT insert into `scope.functions`.
///
/// The returned lookup table is used by `register_builtin_methods`,
/// `register_synthetic_modules`, and `register_static_methods` to set up
/// the canonical API surface.
pub fn register_builtin_intrinsics(
    tree: &mut ItemTree,
    scope: &mut ModuleScope,
    interner: &mut Interner,
) {
    let intrinsic_sigs = intrinsic_signatures(scope, interner);
    let mut lookup = kyokara_stdx::FxHashMap::default();

    for (name, fn_item) in intrinsic_sigs {
        let idx = tree.functions.alloc(fn_item);
        lookup.insert(name, idx);
    }

    // Store the lookup table on the scope for use by downstream registration.
    scope.intrinsic_fn_lookup = lookup;
}

/// Register built-in methods that map existing intrinsics to method-call syntax.
///
/// For example, `string_len` becomes callable as `s.len()` by registering
/// `(ReceiverKey, "len") → FnItemIdx` in `scope.methods`.
///
/// Also populates `scope.well_known_names` with cached primitive type names.
pub fn register_builtin_methods(scope: &mut ModuleScope, interner: &mut Interner) {
    // Cache well-known type names for method resolution in type inference.
    scope.well_known_names = WellKnownNames {
        string: Some(Name::new(interner, "String")),
        int: Some(Name::new(interner, "Int")),
        float: Some(Name::new(interner, "Float")),
        bool_: Some(Name::new(interner, "Bool")),
        char_: Some(Name::new(interner, "Char")),
        list: scope.core_types.get(CoreType::List).map(|t| t.type_name),
        seq: scope.core_types.get(CoreType::Seq).map(|t| t.type_name),
        map: scope.core_types.get(CoreType::Map).map(|t| t.type_name),
        set: scope.core_types.get(CoreType::Set).map(|t| t.type_name),
    };

    // (intrinsic_fn_name, receiver_key, method_name)
    let mappings: &[(&str, ReceiverKey, &str)] = &[
        // String methods
        (
            "string_len",
            ReceiverKey::Primitive(PrimitiveType::String),
            "len",
        ),
        (
            "string_contains",
            ReceiverKey::Primitive(PrimitiveType::String),
            "contains",
        ),
        (
            "string_starts_with",
            ReceiverKey::Primitive(PrimitiveType::String),
            "starts_with",
        ),
        (
            "string_ends_with",
            ReceiverKey::Primitive(PrimitiveType::String),
            "ends_with",
        ),
        (
            "string_trim",
            ReceiverKey::Primitive(PrimitiveType::String),
            "trim",
        ),
        (
            "string_split",
            ReceiverKey::Primitive(PrimitiveType::String),
            "split",
        ),
        (
            "string_substring",
            ReceiverKey::Primitive(PrimitiveType::String),
            "substring",
        ),
        (
            "string_to_upper",
            ReceiverKey::Primitive(PrimitiveType::String),
            "to_upper",
        ),
        (
            "string_to_lower",
            ReceiverKey::Primitive(PrimitiveType::String),
            "to_lower",
        ),
        (
            "string_concat",
            ReceiverKey::Primitive(PrimitiveType::String),
            "concat",
        ),
        (
            "string_lines",
            ReceiverKey::Primitive(PrimitiveType::String),
            "lines",
        ),
        (
            "string_chars",
            ReceiverKey::Primitive(PrimitiveType::String),
            "chars",
        ),
        (
            "parse_int",
            ReceiverKey::Primitive(PrimitiveType::String),
            "parse_int",
        ),
        (
            "parse_float",
            ReceiverKey::Primitive(PrimitiveType::String),
            "parse_float",
        ),
        // List methods
        ("list_push", ReceiverKey::Core(CoreType::List), "push"),
        ("list_len", ReceiverKey::Core(CoreType::List), "len"),
        ("list_get", ReceiverKey::Core(CoreType::List), "get"),
        ("list_head", ReceiverKey::Core(CoreType::List), "head"),
        ("list_tail", ReceiverKey::Core(CoreType::List), "tail"),
        (
            "list_is_empty",
            ReceiverKey::Core(CoreType::List),
            "is_empty",
        ),
        ("list_reverse", ReceiverKey::Core(CoreType::List), "reverse"),
        ("list_concat", ReceiverKey::Core(CoreType::List), "concat"),
        ("list_seq", ReceiverKey::Core(CoreType::List), "seq"),
        ("list_sort", ReceiverKey::Core(CoreType::List), "sort"),
        ("list_sort_by", ReceiverKey::Core(CoreType::List), "sort_by"),
        (
            "list_binary_search",
            ReceiverKey::Core(CoreType::List),
            "binary_search",
        ),
        // Seq methods
        ("seq_map", ReceiverKey::Core(CoreType::Seq), "map"),
        ("seq_filter", ReceiverKey::Core(CoreType::Seq), "filter"),
        ("seq_fold", ReceiverKey::Core(CoreType::Seq), "fold"),
        ("seq_scan", ReceiverKey::Core(CoreType::Seq), "scan"),
        (
            "seq_enumerate",
            ReceiverKey::Core(CoreType::Seq),
            "enumerate",
        ),
        ("seq_zip", ReceiverKey::Core(CoreType::Seq), "zip"),
        ("seq_chunks", ReceiverKey::Core(CoreType::Seq), "chunks"),
        ("seq_windows", ReceiverKey::Core(CoreType::Seq), "windows"),
        ("seq_count", ReceiverKey::Core(CoreType::Seq), "count"),
        ("seq_any", ReceiverKey::Core(CoreType::Seq), "any"),
        ("seq_all", ReceiverKey::Core(CoreType::Seq), "all"),
        ("seq_find", ReceiverKey::Core(CoreType::Seq), "find"),
        ("seq_to_list", ReceiverKey::Core(CoreType::Seq), "to_list"),
        // Map methods
        ("map_insert", ReceiverKey::Core(CoreType::Map), "insert"),
        ("map_get", ReceiverKey::Core(CoreType::Map), "get"),
        ("map_contains", ReceiverKey::Core(CoreType::Map), "contains"),
        ("map_remove", ReceiverKey::Core(CoreType::Map), "remove"),
        ("map_len", ReceiverKey::Core(CoreType::Map), "len"),
        ("map_keys", ReceiverKey::Core(CoreType::Map), "keys"),
        ("map_values", ReceiverKey::Core(CoreType::Map), "values"),
        ("map_is_empty", ReceiverKey::Core(CoreType::Map), "is_empty"),
        // Set methods
        ("set_insert", ReceiverKey::Core(CoreType::Set), "insert"),
        ("set_contains", ReceiverKey::Core(CoreType::Set), "contains"),
        ("set_remove", ReceiverKey::Core(CoreType::Set), "remove"),
        ("set_len", ReceiverKey::Core(CoreType::Set), "len"),
        ("set_is_empty", ReceiverKey::Core(CoreType::Set), "is_empty"),
        ("set_values", ReceiverKey::Core(CoreType::Set), "values"),
        // Option methods
        (
            "option_unwrap_or",
            ReceiverKey::Core(CoreType::Option),
            "unwrap_or",
        ),
        (
            "option_map_or",
            ReceiverKey::Core(CoreType::Option),
            "map_or",
        ),
        ("option_map", ReceiverKey::Core(CoreType::Option), "map"),
        (
            "option_and_then",
            ReceiverKey::Core(CoreType::Option),
            "and_then",
        ),
        // Result methods
        (
            "result_unwrap_or",
            ReceiverKey::Core(CoreType::Result),
            "unwrap_or",
        ),
        ("result_map", ReceiverKey::Core(CoreType::Result), "map"),
        (
            "result_and_then",
            ReceiverKey::Core(CoreType::Result),
            "and_then",
        ),
        (
            "result_map_err",
            ReceiverKey::Core(CoreType::Result),
            "map_err",
        ),
        (
            "result_map_or",
            ReceiverKey::Core(CoreType::Result),
            "map_or",
        ),
        // Int methods
        (
            "int_to_string",
            ReceiverKey::Primitive(PrimitiveType::Int),
            "to_string",
        ),
        (
            "int_to_float",
            ReceiverKey::Primitive(PrimitiveType::Int),
            "to_float",
        ),
        ("int_pow", ReceiverKey::Primitive(PrimitiveType::Int), "pow"),
        ("abs", ReceiverKey::Primitive(PrimitiveType::Int), "abs"),
        // Float methods
        (
            "float_to_int",
            ReceiverKey::Primitive(PrimitiveType::Float),
            "to_int",
        ),
        (
            "float_abs",
            ReceiverKey::Primitive(PrimitiveType::Float),
            "abs",
        ),
        // Char methods
        (
            "char_to_string",
            ReceiverKey::Primitive(PrimitiveType::Char),
            "to_string",
        ),
    ];

    for &(intrinsic_name, receiver_key, method_name) in mappings {
        let intr_name = Name::new(interner, intrinsic_name);
        let meth_name = Name::new(interner, method_name);

        if let Some(&fn_idx) = scope.intrinsic_fn_lookup.get(&intr_name) {
            scope.methods.insert((receiver_key, meth_name), fn_idx);
        }
    }
}

/// Register synthetic modules (`io`, `math`, `fs`) that hold module-qualified intrinsics.
///
/// Module-qualified calls like `io.println(s)` resolve through `scope.synthetic_modules`.
/// Each module's FnItems are allocated in the item tree (via `register_builtin_intrinsics`).
///
/// **Important:** This function registers the module definitions, but they are NOT available
/// for name resolution until explicitly imported. Call [`activate_synthetic_imports`] after
/// processing the item tree's import list to mark which modules are actually imported.
pub fn register_synthetic_modules(
    tree: &mut ItemTree,
    scope: &mut ModuleScope,
    interner: &mut Interner,
) {
    // (module_name, intrinsic_fn_name, method_name, requires_capability)
    let module_fns: &[(&str, &[(&str, &str)])] = &[
        (
            "io",
            &[
                ("print", "print"),
                ("println", "println"),
                ("read_line", "read_line"),
                ("read_stdin", "read_stdin"),
            ],
        ),
        (
            "math",
            &[
                ("min", "min"),
                ("max", "max"),
                ("gcd", "gcd"),
                ("lcm", "lcm"),
                ("float_min", "fmin"),
                ("float_max", "fmax"),
            ],
        ),
        ("fs", &[("read_file", "read_file")]),
    ];

    for &(mod_name_str, fns) in module_fns {
        let mod_name = Name::new(interner, mod_name_str);
        let mut mod_fns = kyokara_stdx::FxHashMap::default();

        for &(intrinsic_name, pub_name) in fns {
            let intr_name = Name::new(interner, intrinsic_name);
            let pub_fn_name = Name::new(interner, pub_name);

            if let Some(&fn_idx) = scope.intrinsic_fn_lookup.get(&intr_name) {
                mod_fns.insert(pub_fn_name, fn_idx);
            } else {
                // Intrinsic not yet allocated (e.g., read_line, read_stdin are new).
                // Allocate a stub FnItem for it.
                let fn_item =
                    mk_module_intrinsic(interner, tree, intrinsic_name, pub_name, mod_name_str);
                let idx = tree.functions.alloc(fn_item);
                scope.intrinsic_fn_lookup.insert(intr_name, idx);
                mod_fns.insert(pub_fn_name, idx);
            }
        }

        scope.synthetic_modules.insert(mod_name, mod_fns);
    }
}

/// Scan an item tree's import list and activate any synthetic module imports.
///
/// For each `import io` / `import math` / `import fs` found in the item tree,
/// adds the module name to `scope.imported_modules` so that the resolver and
/// type inference allow module-qualified calls through that module.
pub fn activate_synthetic_imports(
    tree: &ItemTree,
    scope: &mut ModuleScope,
    interner: &mut Interner,
) {
    for import in &tree.imports {
        if import.path.segments.len() == 1 {
            let seg = import.path.segments[0];
            if scope.synthetic_modules.contains_key(&seg) {
                scope.imported_modules.insert(seg);
            }
        }
    }
    // Suppress unused-mut warning when there are no imports.
    let _ = interner;
}

/// Create FnItem for a new module intrinsic (e.g., read_line, read_stdin).
fn mk_module_intrinsic(
    interner: &mut Interner,
    _tree: &mut ItemTree,
    intrinsic_name: &str,
    _pub_name: &str,
    mod_name: &str,
) -> FnItem {
    let name = Name::new(interner, intrinsic_name);
    let string_ty = TypeRef::Path {
        path: Path::single(Name::new(interner, "String")),
        args: Vec::new(),
    };
    let unit_ty = TypeRef::Path {
        path: Path::single(Name::new(interner, "Unit")),
        args: Vec::new(),
    };

    // Build capability refs for io/fs modules.
    let cap_refs = match mod_name {
        "io" => vec![TypeRef::Path {
            path: Path::single(Name::new(interner, "io")),
            args: Vec::new(),
        }],
        "fs" => vec![TypeRef::Path {
            path: Path::single(Name::new(interner, "fs")),
            args: Vec::new(),
        }],
        _ => Vec::new(),
    };

    match intrinsic_name {
        "read_line" => FnItem {
            name,
            is_pub: false,
            type_params: Vec::new(),
            params: Vec::new(),
            ret_type: Some(string_ty),
            with_effects: cap_refs,
            pipe_effects: Vec::new(),
            has_body: false,
            source_range: None,
            receiver_type: None,
        },
        "read_stdin" => FnItem {
            name,
            is_pub: false,
            type_params: Vec::new(),
            params: Vec::new(),
            ret_type: Some(string_ty),
            with_effects: cap_refs,
            pipe_effects: Vec::new(),
            has_body: false,
            source_range: None,
            receiver_type: None,
        },
        // print/println already allocated, but just in case:
        "print" | "println" => FnItem {
            name,
            is_pub: false,
            type_params: Vec::new(),
            params: vec![FnParam {
                name: Name::new(interner, "s"),
                ty: string_ty,
            }],
            ret_type: Some(unit_ty),
            with_effects: cap_refs,
            pipe_effects: Vec::new(),
            has_body: false,
            source_range: None,
            receiver_type: None,
        },
        _ => FnItem {
            name,
            is_pub: false,
            type_params: Vec::new(),
            params: Vec::new(),
            ret_type: Some(unit_ty),
            with_effects: cap_refs,
            pipe_effects: Vec::new(),
            has_body: false,
            source_range: None,
            receiver_type: None,
        },
    }
}

/// Register static methods (`List.new()`, `Map.new()`, `Set.new()`) in
/// `scope.static_methods`.
///
/// Static methods are always available (no import needed) since the types they
/// belong to are always in scope.
pub fn register_static_methods(scope: &mut ModuleScope, interner: &mut Interner) {
    // (intrinsic_fn_name, owner_key, static_method_name)
    let mappings: &[(&str, StaticOwnerKey, &str)] = &[
        ("list_new", StaticOwnerKey::Core(CoreType::List), "new"),
        ("seq_range", StaticOwnerKey::Core(CoreType::Seq), "range"),
        ("seq_unfold", StaticOwnerKey::Core(CoreType::Seq), "unfold"),
        ("map_new", StaticOwnerKey::Core(CoreType::Map), "new"),
        ("set_new", StaticOwnerKey::Core(CoreType::Set), "new"),
    ];

    for &(intrinsic_name, owner_key, method_name) in mappings {
        let intr_name = Name::new(interner, intrinsic_name);
        let meth_name = Name::new(interner, method_name);

        if let Some(&fn_idx) = scope.intrinsic_fn_lookup.get(&intr_name) {
            scope.static_methods.insert((owner_key, meth_name), fn_idx);
        }
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
            with_effects: Vec::new(),
            pipe_effects: Vec::new(),
            has_body: false,
            source_range: None,
            receiver_type: None,
        },
    )
}

/// Build FnItem signatures for each intrinsic.
fn intrinsic_signatures(scope: &ModuleScope, interner: &mut Interner) -> Vec<(Name, FnItem)> {
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

    let list_core_name = scope
        .core_types
        .get(CoreType::List)
        .map(|info| info.type_name)
        .unwrap_or_else(|| Name::new(interner, "List"));
    let seq_core_name = scope
        .core_types
        .get(CoreType::Seq)
        .map(|info| info.type_name)
        .unwrap_or_else(|| Name::new(interner, "Seq"));
    let map_core_name = scope
        .core_types
        .get(CoreType::Map)
        .map(|info| info.type_name)
        .unwrap_or_else(|| Name::new(interner, "Map"));
    let set_core_name = scope
        .core_types
        .get(CoreType::Set)
        .map(|info| info.type_name)
        .unwrap_or_else(|| Name::new(interner, "Set"));
    let option_core_name = scope
        .core_types
        .get(CoreType::Option)
        .map(|info| info.type_name)
        .unwrap_or_else(|| Name::new(interner, "Option"));
    let result_core_name = scope
        .core_types
        .get(CoreType::Result)
        .map(|info| info.type_name)
        .unwrap_or_else(|| Name::new(interner, "Result"));
    let parse_error_core_name = scope
        .core_types
        .get(CoreType::ParseError)
        .map(|info| info.type_name)
        .unwrap_or_else(|| Name::new(interner, "ParseError"));

    // Type parameter names.
    let t_name = Name::new(interner, "T");
    let u_name = Name::new(interner, "U");
    let e_name = Name::new(interner, "E");
    let k_name = Name::new(interner, "K");
    let v_name = Name::new(interner, "V");
    let s_name = Name::new(interner, "S");

    // Generic type refs.
    let t_ref = TypeRef::Path {
        path: Path::single(t_name),
        args: Vec::new(),
    };
    let u_ref = TypeRef::Path {
        path: Path::single(u_name),
        args: Vec::new(),
    };
    let e_ref = TypeRef::Path {
        path: Path::single(e_name),
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
    let s_ref = TypeRef::Path {
        path: Path::single(s_name),
        args: Vec::new(),
    };

    // Composite type refs.
    let list_t = TypeRef::Path {
        path: Path::single(list_core_name),
        args: vec![t_ref.clone()],
    };
    let seq_t = TypeRef::Path {
        path: Path::single(seq_core_name),
        args: vec![t_ref.clone()],
    };
    let seq_u = TypeRef::Path {
        path: Path::single(seq_core_name),
        args: vec![u_ref.clone()],
    };
    let seq_list_t = TypeRef::Path {
        path: Path::single(seq_core_name),
        args: vec![list_t.clone()],
    };
    let seq_k = TypeRef::Path {
        path: Path::single(seq_core_name),
        args: vec![k_ref.clone()],
    };
    let seq_v = TypeRef::Path {
        path: Path::single(seq_core_name),
        args: vec![v_ref.clone()],
    };
    let seq_string = TypeRef::Path {
        path: Path::single(seq_core_name),
        args: vec![string_ty.clone()],
    };
    let seq_char = TypeRef::Path {
        path: Path::single(seq_core_name),
        args: vec![char_ty.clone()],
    };
    let seq_int = TypeRef::Path {
        path: Path::single(seq_core_name),
        args: vec![int_ty.clone()],
    };
    let indexed_t = TypeRef::Record {
        fields: vec![
            (Name::new(interner, "index"), int_ty.clone()),
            (Name::new(interner, "value"), t_ref.clone()),
        ],
    };
    let seq_indexed_t = TypeRef::Path {
        path: Path::single(seq_core_name),
        args: vec![indexed_t],
    };
    let pair_tu = TypeRef::Record {
        fields: vec![
            (Name::new(interner, "left"), t_ref.clone()),
            (Name::new(interner, "right"), u_ref.clone()),
        ],
    };
    let seq_pair_tu = TypeRef::Path {
        path: Path::single(seq_core_name),
        args: vec![pair_tu],
    };
    let map_kv = TypeRef::Path {
        path: Path::single(map_core_name),
        args: vec![k_ref.clone(), v_ref.clone()],
    };
    let set_t = TypeRef::Path {
        path: Path::single(set_core_name),
        args: vec![t_ref.clone()],
    };
    let option_t = TypeRef::Path {
        path: Path::single(option_core_name),
        args: vec![t_ref.clone()],
    };
    let option_u = TypeRef::Path {
        path: Path::single(option_core_name),
        args: vec![u_ref.clone()],
    };
    let result_te = TypeRef::Path {
        path: Path::single(result_core_name),
        args: vec![t_ref.clone(), e_ref.clone()],
    };
    let result_ue = TypeRef::Path {
        path: Path::single(result_core_name),
        args: vec![u_ref.clone(), e_ref.clone()],
    };
    let result_tu = TypeRef::Path {
        path: Path::single(result_core_name),
        args: vec![t_ref.clone(), u_ref.clone()],
    };
    let option_v = TypeRef::Path {
        path: Path::single(option_core_name),
        args: vec![v_ref.clone()],
    };
    let unfold_step_record = TypeRef::Record {
        fields: vec![
            (Name::new(interner, "value"), t_ref.clone()),
            (Name::new(interner, "state"), s_ref.clone()),
        ],
    };
    let option_unfold_step = TypeRef::Path {
        path: Path::single(option_core_name),
        args: vec![unfold_step_record],
    };

    // Function type refs for higher-order intrinsics.
    let fn_t_to_u = TypeRef::Fn {
        params: vec![t_ref.clone()],
        ret: Box::new(u_ref.clone()),
    };
    let fn_t_to_option_u = TypeRef::Fn {
        params: vec![t_ref.clone()],
        ret: Box::new(option_u.clone()),
    };
    let fn_t_to_result_ue = TypeRef::Fn {
        params: vec![t_ref.clone()],
        ret: Box::new(result_ue.clone()),
    };
    let fn_e_to_u = TypeRef::Fn {
        params: vec![e_ref.clone()],
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
    let fn_tt_to_int = TypeRef::Fn {
        params: vec![t_ref.clone(), t_ref.clone()],
        ret: Box::new(int_ty.clone()),
    };
    let fn_s_to_option_unfold_step = TypeRef::Fn {
        params: vec![s_ref.clone()],
        ret: Box::new(option_unfold_step),
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
        // list_seq<T>(xs: List<T>) -> Seq<T>
        mk_intrinsic(
            interner,
            "list_seq",
            vec![t_name],
            vec![("xs", list_t.clone())],
            seq_t.clone(),
        ),
        // seq_map<T, U>(s: Seq<T>, f: fn(T) -> U) -> Seq<U>
        mk_intrinsic(
            interner,
            "seq_map",
            vec![t_name, u_name],
            vec![("s", seq_t.clone()), ("f", fn_t_to_u.clone())],
            seq_u.clone(),
        ),
        // seq_filter<T>(s: Seq<T>, f: fn(T) -> Bool) -> Seq<T>
        mk_intrinsic(
            interner,
            "seq_filter",
            vec![t_name],
            vec![("s", seq_t.clone()), ("f", fn_t_to_bool.clone())],
            seq_t.clone(),
        ),
        // seq_fold<T, U>(s: Seq<T>, init: U, f: fn(U, T) -> U) -> U
        mk_intrinsic(
            interner,
            "seq_fold",
            vec![t_name, u_name],
            vec![
                ("s", seq_t.clone()),
                ("init", u_ref.clone()),
                ("f", fn_ut_to_u.clone()),
            ],
            u_ref.clone(),
        ),
        // seq_scan<T, U>(s: Seq<T>, init: U, f: fn(U, T) -> U) -> Seq<U>
        mk_intrinsic(
            interner,
            "seq_scan",
            vec![t_name, u_name],
            vec![
                ("s", seq_t.clone()),
                ("init", u_ref.clone()),
                ("f", fn_ut_to_u.clone()),
            ],
            seq_u.clone(),
        ),
        // seq_range(start: Int, end: Int) -> Seq<Int>
        mk_intrinsic(
            interner,
            "seq_range",
            vec![],
            vec![("start", int_ty.clone()), ("end", int_ty.clone())],
            seq_int,
        ),
        // seq_unfold<S, T>(seed: S, step: fn(S) -> Option<{ value: T, state: S }>) -> Seq<T>
        mk_intrinsic(
            interner,
            "seq_unfold",
            vec![s_name, t_name],
            vec![
                ("seed", s_ref.clone()),
                ("step", fn_s_to_option_unfold_step.clone()),
            ],
            seq_t.clone(),
        ),
        // seq_enumerate<T>(s: Seq<T>) -> Seq<{ index: Int, value: T }>
        mk_intrinsic(
            interner,
            "seq_enumerate",
            vec![t_name],
            vec![("s", seq_t.clone())],
            seq_indexed_t,
        ),
        // seq_zip<T, U>(s: Seq<T>, other: Seq<U>) -> Seq<{ left: T, right: U }>
        mk_intrinsic(
            interner,
            "seq_zip",
            vec![t_name, u_name],
            vec![("s", seq_t.clone()), ("other", seq_u.clone())],
            seq_pair_tu,
        ),
        // seq_chunks<T>(s: Seq<T>, n: Int) -> Seq<List<T>>
        mk_intrinsic(
            interner,
            "seq_chunks",
            vec![t_name],
            vec![("s", seq_t.clone()), ("n", int_ty.clone())],
            seq_list_t.clone(),
        ),
        // seq_windows<T>(s: Seq<T>, n: Int) -> Seq<List<T>>
        mk_intrinsic(
            interner,
            "seq_windows",
            vec![t_name],
            vec![("s", seq_t.clone()), ("n", int_ty.clone())],
            seq_list_t.clone(),
        ),
        // seq_count<T>(s: Seq<T>) -> Int
        mk_intrinsic(
            interner,
            "seq_count",
            vec![t_name],
            vec![("s", seq_t.clone())],
            int_ty.clone(),
        ),
        // seq_any<T>(s: Seq<T>, f: fn(T) -> Bool) -> Bool
        mk_intrinsic(
            interner,
            "seq_any",
            vec![t_name],
            vec![("s", seq_t.clone()), ("f", fn_t_to_bool.clone())],
            bool_ty.clone(),
        ),
        // seq_all<T>(s: Seq<T>, f: fn(T) -> Bool) -> Bool
        mk_intrinsic(
            interner,
            "seq_all",
            vec![t_name],
            vec![("s", seq_t.clone()), ("f", fn_t_to_bool.clone())],
            bool_ty.clone(),
        ),
        // seq_find<T>(s: Seq<T>, f: fn(T) -> Bool) -> Option<T>
        mk_intrinsic(
            interner,
            "seq_find",
            vec![t_name],
            vec![("s", seq_t.clone()), ("f", fn_t_to_bool.clone())],
            option_t.clone(),
        ),
        // seq_to_list<T>(s: Seq<T>) -> List<T>
        mk_intrinsic(
            interner,
            "seq_to_list",
            vec![t_name],
            vec![("s", seq_t.clone())],
            list_t.clone(),
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
        // map_keys<K, V>(m: Map<K,V>) -> Seq<K>
        mk_intrinsic(
            interner,
            "map_keys",
            vec![k_name, v_name],
            vec![("m", map_kv.clone())],
            seq_k.clone(),
        ),
        // map_values<K, V>(m: Map<K,V>) -> Seq<V>
        mk_intrinsic(
            interner,
            "map_values",
            vec![k_name, v_name],
            vec![("m", map_kv.clone())],
            seq_v.clone(),
        ),
        // map_is_empty<K, V>(m: Map<K,V>) -> Bool
        mk_intrinsic(
            interner,
            "map_is_empty",
            vec![k_name, v_name],
            vec![("m", map_kv.clone())],
            bool_ty.clone(),
        ),
        // ── Set<T> ───────────────────────────────────────────────
        // set_new<T>() -> Set<T>
        mk_intrinsic(interner, "set_new", vec![t_name], vec![], set_t.clone()),
        // set_insert<T>(s: Set<T>, x: T) -> Set<T>
        mk_intrinsic(
            interner,
            "set_insert",
            vec![t_name],
            vec![("s", set_t.clone()), ("x", t_ref.clone())],
            set_t.clone(),
        ),
        // set_contains<T>(s: Set<T>, x: T) -> Bool
        mk_intrinsic(
            interner,
            "set_contains",
            vec![t_name],
            vec![("s", set_t.clone()), ("x", t_ref.clone())],
            bool_ty.clone(),
        ),
        // set_remove<T>(s: Set<T>, x: T) -> Set<T>
        mk_intrinsic(
            interner,
            "set_remove",
            vec![t_name],
            vec![("s", set_t.clone()), ("x", t_ref.clone())],
            set_t.clone(),
        ),
        // set_len<T>(s: Set<T>) -> Int
        mk_intrinsic(
            interner,
            "set_len",
            vec![t_name],
            vec![("s", set_t.clone())],
            int_ty.clone(),
        ),
        // set_is_empty<T>(s: Set<T>) -> Bool
        mk_intrinsic(
            interner,
            "set_is_empty",
            vec![t_name],
            vec![("s", set_t.clone())],
            bool_ty.clone(),
        ),
        // set_values<T>(s: Set<T>) -> Seq<T>
        mk_intrinsic(
            interner,
            "set_values",
            vec![t_name],
            vec![("s", set_t.clone())],
            seq_t.clone(),
        ),
        // result_unwrap_or<T, E>(r: Result<T, E>, fallback: T) -> T
        mk_intrinsic(
            interner,
            "result_unwrap_or",
            vec![t_name, e_name],
            vec![("r", result_te.clone()), ("fallback", t_ref.clone())],
            t_ref.clone(),
        ),
        // option_unwrap_or<T>(o: Option<T>, fallback: T) -> T
        mk_intrinsic(
            interner,
            "option_unwrap_or",
            vec![t_name],
            vec![("o", option_t.clone()), ("fallback", t_ref.clone())],
            t_ref.clone(),
        ),
        // result_map_or<T, E, U>(r: Result<T, E>, fallback: U, f: fn(T) -> U) -> U
        mk_intrinsic(
            interner,
            "result_map_or",
            vec![t_name, e_name, u_name],
            vec![
                ("r", result_te.clone()),
                ("fallback", u_ref.clone()),
                ("f", fn_t_to_u.clone()),
            ],
            u_ref.clone(),
        ),
        // option_map_or<T, U>(o: Option<T>, fallback: U, f: fn(T) -> U) -> U
        mk_intrinsic(
            interner,
            "option_map_or",
            vec![t_name, u_name],
            vec![
                ("o", option_t.clone()),
                ("fallback", u_ref.clone()),
                ("f", fn_t_to_u.clone()),
            ],
            u_ref.clone(),
        ),
        // option_map<T, U>(o: Option<T>, f: fn(T) -> U) -> Option<U>
        mk_intrinsic(
            interner,
            "option_map",
            vec![t_name, u_name],
            vec![("o", option_t.clone()), ("f", fn_t_to_u.clone())],
            option_u.clone(),
        ),
        // option_and_then<T, U>(o: Option<T>, f: fn(T) -> Option<U>) -> Option<U>
        mk_intrinsic(
            interner,
            "option_and_then",
            vec![t_name, u_name],
            vec![("o", option_t.clone()), ("f", fn_t_to_option_u.clone())],
            option_u.clone(),
        ),
        // result_map<T, E, U>(r: Result<T, E>, f: fn(T) -> U) -> Result<U, E>
        mk_intrinsic(
            interner,
            "result_map",
            vec![t_name, e_name, u_name],
            vec![("r", result_te.clone()), ("f", fn_t_to_u.clone())],
            result_ue.clone(),
        ),
        // result_and_then<T, E, U>(r: Result<T, E>, f: fn(T) -> Result<U, E>) -> Result<U, E>
        mk_intrinsic(
            interner,
            "result_and_then",
            vec![t_name, e_name, u_name],
            vec![("r", result_te.clone()), ("f", fn_t_to_result_ue.clone())],
            result_ue,
        ),
        // result_map_err<T, E, U>(r: Result<T, E>, f: fn(E) -> U) -> Result<T, U>
        mk_intrinsic(
            interner,
            "result_map_err",
            vec![t_name, e_name, u_name],
            vec![("r", result_te), ("f", fn_e_to_u)],
            result_tu,
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
        // string_split(s: String, delim: String) -> Seq<String>
        mk_intrinsic(
            interner,
            "string_split",
            vec![],
            vec![("s", string_ty.clone()), ("delim", string_ty.clone())],
            seq_string.clone(),
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
        // int_pow(base: Int, exp: Int) -> Int
        mk_intrinsic(
            interner,
            "int_pow",
            vec![],
            vec![("base", int_ty.clone()), ("exp", int_ty.clone())],
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
        // gcd(a: Int, b: Int) -> Int
        mk_intrinsic(
            interner,
            "gcd",
            vec![],
            vec![("a", int_ty.clone()), ("b", int_ty.clone())],
            int_ty.clone(),
        ),
        // lcm(a: Int, b: Int) -> Int
        mk_intrinsic(
            interner,
            "lcm",
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
        // ── Parsing ──────────────────────────────────────────────
        // parse_int(s: String) -> Result<Int, ParseError>
        {
            let parse_error_ty = TypeRef::Path {
                path: Path::single(parse_error_core_name),
                args: Vec::new(),
            };
            let result_int = TypeRef::Path {
                path: Path::single(result_core_name),
                args: vec![int_ty.clone(), parse_error_ty],
            };
            mk_intrinsic(
                interner,
                "parse_int",
                vec![],
                vec![("s", string_ty.clone())],
                result_int,
            )
        },
        // parse_float(s: String) -> Result<Float, ParseError>
        {
            let parse_error_ty = TypeRef::Path {
                path: Path::single(parse_error_core_name),
                args: Vec::new(),
            };
            let result_float = TypeRef::Path {
                path: Path::single(result_core_name),
                args: vec![float_ty.clone(), parse_error_ty],
            };
            mk_intrinsic(
                interner,
                "parse_float",
                vec![],
                vec![("s", string_ty.clone())],
                result_float,
            )
        },
        // ── String decomposition ─────────────────────────────────
        // string_lines(s: String) -> Seq<String>
        mk_intrinsic(
            interner,
            "string_lines",
            vec![],
            vec![("s", string_ty.clone())],
            seq_string.clone(),
        ),
        // string_chars(s: String) -> Seq<Char>
        mk_intrinsic(
            interner,
            "string_chars",
            vec![],
            vec![("s", string_ty.clone())],
            seq_char,
        ),
        // ── File I/O ─────────────────────────────────────────────
        // read_file(path: String) -> String
        mk_intrinsic(
            interner,
            "read_file",
            vec![],
            vec![("path", string_ty.clone())],
            string_ty,
        ),
        // ── Sorting ──────────────────────────────────────────────
        // list_sort<T>(xs: List<T>) -> List<T>
        mk_intrinsic(
            interner,
            "list_sort",
            vec![t_name],
            vec![("xs", list_t.clone())],
            list_t.clone(),
        ),
        // list_sort_by<T>(xs: List<T>, cmp: fn(T, T) -> Int) -> List<T>
        mk_intrinsic(
            interner,
            "list_sort_by",
            vec![t_name],
            vec![("xs", list_t.clone()), ("cmp", fn_tt_to_int)],
            list_t.clone(),
        ),
        // list_binary_search<T>(xs: List<T>, x: T) -> Int
        mk_intrinsic(
            interner,
            "list_binary_search",
            vec![t_name],
            vec![("xs", list_t.clone()), ("x", t_ref)],
            int_ty,
        ),
    ]
}
