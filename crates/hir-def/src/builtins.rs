//! Built-in type definitions injected before type-checking.
//!
//! Registers `Option<T>` and `Result<T, E>` as synthetic ADT types
//! in the item tree and module scope. Both the eval pipeline and the
//! hir `check_file` pipeline call [`register_builtin_types`] after
//! item tree collection but before type-checking.

use crate::item_tree::{FnItem, FnParam, ItemTree, TypeDefKind, TypeItem, VariantDef};
use crate::name::Name;
use crate::path::Path;
use crate::resolver::ModuleScope;
use crate::type_ref::TypeRef;
use kyokara_intern::Interner;

/// Inject `Option<T>`, `Result<T, E>`, `List<T>`, `Map<K,V>`, `Set<T>`, and
/// `ParseError` into the item tree
/// and module scope.
///
/// Uses `Vacant` entry checks so that user-defined types with the same
/// names (registered during item tree collection) take precedence.
pub fn register_builtin_types(
    tree: &mut ItemTree,
    scope: &mut ModuleScope,
    interner: &mut Interner,
) {
    register_option(tree, scope, interner);
    register_result(tree, scope, interner);
    register_list(tree, scope, interner);
    register_map(tree, scope, interner);
    register_set(tree, scope, interner);
    register_parse_error(tree, scope, interner);
}

/// `type Option<T> = | Some(T) | None`
fn register_option(tree: &mut ItemTree, scope: &mut ModuleScope, interner: &mut Interner) {
    let option_name = Name::new(interner, "Option");

    // Skip if the user already defined Option.
    if scope.types.contains_key(&option_name) {
        return;
    }

    let t_name = Name::new(interner, "T");
    let some_name = Name::new(interner, "Some");
    let none_name = Name::new(interner, "None");

    let t_ref = TypeRef::Path {
        path: Path::single(t_name),
        args: Vec::new(),
    };

    let idx = tree.types.alloc(TypeItem {
        name: option_name,
        is_pub: false,
        type_params: vec![t_name],
        kind: TypeDefKind::Adt {
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
    });

    scope.types.insert(option_name, idx);

    // Register constructors (only if not already present).
    if let std::collections::hash_map::Entry::Vacant(e) = scope.constructors.entry(some_name) {
        e.insert((idx, 0));
    }
    if let std::collections::hash_map::Entry::Vacant(e) = scope.constructors.entry(none_name) {
        e.insert((idx, 1));
    }
}

/// `List<T>` — opaque builtin type (no variants, no pattern matching).
fn register_list(tree: &mut ItemTree, scope: &mut ModuleScope, interner: &mut Interner) {
    let list_name = Name::new(interner, "List");
    if scope.types.contains_key(&list_name) {
        return;
    }
    let t_name = Name::new(interner, "T");
    let idx = tree.types.alloc(TypeItem {
        name: list_name,
        is_pub: false,
        type_params: vec![t_name],
        kind: TypeDefKind::Adt { variants: vec![] },
    });
    scope.types.insert(list_name, idx);
}

/// `Map<K, V>` — opaque builtin type (no variants, no pattern matching).
fn register_map(tree: &mut ItemTree, scope: &mut ModuleScope, interner: &mut Interner) {
    let map_name = Name::new(interner, "Map");
    if scope.types.contains_key(&map_name) {
        return;
    }
    let k_name = Name::new(interner, "K");
    let v_name = Name::new(interner, "V");
    let idx = tree.types.alloc(TypeItem {
        name: map_name,
        is_pub: false,
        type_params: vec![k_name, v_name],
        kind: TypeDefKind::Adt { variants: vec![] },
    });
    scope.types.insert(map_name, idx);
}

/// `Set<T>` — opaque builtin type (no variants, no pattern matching).
fn register_set(tree: &mut ItemTree, scope: &mut ModuleScope, interner: &mut Interner) {
    let set_name = Name::new(interner, "Set");
    if scope.types.contains_key(&set_name) {
        return;
    }
    let t_name = Name::new(interner, "T");
    let idx = tree.types.alloc(TypeItem {
        name: set_name,
        is_pub: false,
        type_params: vec![t_name],
        kind: TypeDefKind::Adt { variants: vec![] },
    });
    scope.types.insert(set_name, idx);
}

/// `type Result<T, E> = | Ok(T) | Err(E)`
fn register_result(tree: &mut ItemTree, scope: &mut ModuleScope, interner: &mut Interner) {
    let result_name = Name::new(interner, "Result");

    // Skip if the user already defined Result.
    if scope.types.contains_key(&result_name) {
        return;
    }

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

    let idx = tree.types.alloc(TypeItem {
        name: result_name,
        is_pub: false,
        type_params: vec![t_name, e_name],
        kind: TypeDefKind::Adt {
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
    });

    scope.types.insert(result_name, idx);

    // Register constructors (only if not already present).
    if let std::collections::hash_map::Entry::Vacant(e) = scope.constructors.entry(ok_name) {
        e.insert((idx, 0));
    }
    if let std::collections::hash_map::Entry::Vacant(e) = scope.constructors.entry(err_name) {
        e.insert((idx, 1));
    }
}
/// `type ParseError = | InvalidInt(String) | InvalidFloat(String)`
fn register_parse_error(tree: &mut ItemTree, scope: &mut ModuleScope, interner: &mut Interner) {
    let parse_error_name = Name::new(interner, "ParseError");

    if scope.types.contains_key(&parse_error_name) {
        return;
    }

    let invalid_int_name = Name::new(interner, "InvalidInt");
    let invalid_float_name = Name::new(interner, "InvalidFloat");

    let string_ref = TypeRef::Path {
        path: Path::single(Name::new(interner, "String")),
        args: Vec::new(),
    };

    let idx = tree.types.alloc(TypeItem {
        name: parse_error_name,
        is_pub: false,
        type_params: vec![],
        kind: TypeDefKind::Adt {
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
    });

    scope.types.insert(parse_error_name, idx);

    if let std::collections::hash_map::Entry::Vacant(e) = scope.constructors.entry(invalid_int_name)
    {
        e.insert((idx, 0));
    }
    if let std::collections::hash_map::Entry::Vacant(e) =
        scope.constructors.entry(invalid_float_name)
    {
        e.insert((idx, 1));
    }
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
    let intrinsic_sigs = intrinsic_signatures(interner);
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
/// `("String", "len") → FnItemIdx` in `scope.methods`.
///
/// Also populates `scope.well_known_names` with cached primitive type names.
pub fn register_builtin_methods(scope: &mut ModuleScope, interner: &mut Interner) {
    use crate::resolver::WellKnownNames;

    // Cache well-known type names for method resolution in type inference.
    scope.well_known_names = WellKnownNames {
        string: Some(Name::new(interner, "String")),
        int: Some(Name::new(interner, "Int")),
        float: Some(Name::new(interner, "Float")),
        bool_: Some(Name::new(interner, "Bool")),
        char_: Some(Name::new(interner, "Char")),
        list: Some(Name::new(interner, "List")),
        map: Some(Name::new(interner, "Map")),
        set: Some(Name::new(interner, "Set")),
    };

    // (intrinsic_fn_name, receiver_type_name, method_name)
    let mappings: &[(&str, &str, &str)] = &[
        // String methods
        ("string_len", "String", "len"),
        ("string_contains", "String", "contains"),
        ("string_starts_with", "String", "starts_with"),
        ("string_ends_with", "String", "ends_with"),
        ("string_trim", "String", "trim"),
        ("string_split", "String", "split"),
        ("string_substring", "String", "substring"),
        ("string_to_upper", "String", "to_upper"),
        ("string_to_lower", "String", "to_lower"),
        ("string_concat", "String", "concat"),
        ("string_lines", "String", "lines"),
        ("string_chars", "String", "chars"),
        ("parse_int", "String", "parse_int"),
        ("parse_float", "String", "parse_float"),
        // List methods
        ("list_push", "List", "push"),
        ("list_len", "List", "len"),
        ("list_get", "List", "get"),
        ("list_head", "List", "head"),
        ("list_tail", "List", "tail"),
        ("list_is_empty", "List", "is_empty"),
        ("list_reverse", "List", "reverse"),
        ("list_concat", "List", "concat"),
        ("list_map", "List", "map"),
        ("list_filter", "List", "filter"),
        ("list_fold", "List", "fold"),
        ("list_sort", "List", "sort"),
        ("list_sort_by", "List", "sort_by"),
        ("list_binary_search", "List", "binary_search"),
        // Map methods
        ("map_insert", "Map", "insert"),
        ("map_get", "Map", "get"),
        ("map_contains", "Map", "contains"),
        ("map_remove", "Map", "remove"),
        ("map_len", "Map", "len"),
        ("map_keys", "Map", "keys"),
        ("map_values", "Map", "values"),
        ("map_is_empty", "Map", "is_empty"),
        // Set methods
        ("set_insert", "Set", "insert"),
        ("set_contains", "Set", "contains"),
        ("set_remove", "Set", "remove"),
        ("set_len", "Set", "len"),
        ("set_is_empty", "Set", "is_empty"),
        ("set_values", "Set", "values"),
        // Int methods
        ("int_to_string", "Int", "to_string"),
        ("int_to_float", "Int", "to_float"),
        ("abs", "Int", "abs"),
        // Float methods
        ("float_to_int", "Float", "to_int"),
        ("float_abs", "Float", "abs"),
        // Char methods
        ("char_to_string", "Char", "to_string"),
    ];

    for &(intrinsic_name, type_name, method_name) in mappings {
        let intr_name = Name::new(interner, intrinsic_name);
        let ty_name = Name::new(interner, type_name);
        let meth_name = Name::new(interner, method_name);

        if let Some(&fn_idx) = scope.intrinsic_fn_lookup.get(&intr_name) {
            scope.methods.insert((ty_name, meth_name), fn_idx);
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
            with_caps: cap_refs,
            pipe_caps: Vec::new(),
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
            with_caps: cap_refs,
            pipe_caps: Vec::new(),
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
            with_caps: cap_refs,
            pipe_caps: Vec::new(),
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
            with_caps: cap_refs,
            pipe_caps: Vec::new(),
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
    // (intrinsic_fn_name, type_name, static_method_name)
    let mappings: &[(&str, &str, &str)] = &[
        ("list_new", "List", "new"),
        ("map_new", "Map", "new"),
        ("set_new", "Set", "new"),
    ];

    for &(intrinsic_name, type_name, method_name) in mappings {
        let intr_name = Name::new(interner, intrinsic_name);
        let ty_name = Name::new(interner, type_name);
        let meth_name = Name::new(interner, method_name);

        if let Some(&fn_idx) = scope.intrinsic_fn_lookup.get(&intr_name) {
            scope.static_methods.insert((ty_name, meth_name), fn_idx);
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
            with_caps: Vec::new(),
            pipe_caps: Vec::new(),
            has_body: false,
            source_range: None,
            receiver_type: None,
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
    let list_char = TypeRef::Path {
        path: Path::single(Name::new(interner, "List")),
        args: vec![char_ty.clone()],
    };
    let map_kv = TypeRef::Path {
        path: Path::single(Name::new(interner, "Map")),
        args: vec![k_ref.clone(), v_ref.clone()],
    };
    let set_t = TypeRef::Path {
        path: Path::single(Name::new(interner, "Set")),
        args: vec![t_ref.clone()],
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
    let fn_tt_to_int = TypeRef::Fn {
        params: vec![t_ref.clone(), t_ref.clone()],
        ret: Box::new(int_ty.clone()),
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
        // set_values<T>(s: Set<T>) -> List<T>
        mk_intrinsic(
            interner,
            "set_values",
            vec![t_name],
            vec![("s", set_t.clone())],
            list_t.clone(),
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
                path: Path::single(Name::new(interner, "ParseError")),
                args: Vec::new(),
            };
            let result_int = TypeRef::Path {
                path: Path::single(Name::new(interner, "Result")),
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
                path: Path::single(Name::new(interner, "ParseError")),
                args: Vec::new(),
            };
            let result_float = TypeRef::Path {
                path: Path::single(Name::new(interner, "Result")),
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
        // string_lines(s: String) -> List<String>
        mk_intrinsic(
            interner,
            "string_lines",
            vec![],
            vec![("s", string_ty.clone())],
            list_string.clone(),
        ),
        // string_chars(s: String) -> List<Char>
        mk_intrinsic(
            interner,
            "string_chars",
            vec![],
            vec![("s", string_ty.clone())],
            list_char,
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
