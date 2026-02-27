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

/// Inject `Option<T>` and `Result<T, E>` into the item tree and module scope.
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
/// Inject intrinsic function signatures into the item tree and module scope.
///
/// Uses `Vacant` entry checks so user-defined functions with the same name
/// (collected during item-tree lowering) keep precedence.
pub fn register_builtin_intrinsics(
    tree: &mut ItemTree,
    scope: &mut ModuleScope,
    interner: &mut Interner,
) {
    let intrinsic_sigs = intrinsic_signatures(interner);

    for (name, fn_item) in intrinsic_sigs {
        if let std::collections::hash_map::Entry::Vacant(e) = scope.functions.entry(name) {
            let idx = tree.functions.alloc(fn_item);
            e.insert(idx);
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
