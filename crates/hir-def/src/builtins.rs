//! Built-in type definitions injected before type-checking.
//!
//! Registers `Option<T>` and `Result<T, E>` as synthetic ADT types
//! in the item tree and module scope. Both the eval pipeline and the
//! hir `check_file` pipeline call [`register_builtin_types`] after
//! item tree collection but before type-checking.

use crate::item_tree::{ItemTree, TypeDefKind, TypeItem, VariantDef};
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
