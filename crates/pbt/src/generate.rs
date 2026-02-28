//! Type-driven value generators.
//!
//! Given a `TypeRef` from a function parameter, generates a random `Value`
//! by drawing from a `ChoiceSource`.

use kyokara_eval::value::Value;
use kyokara_hir_def::item_tree::{ItemTree, TypeDefKind, TypeItemIdx};
use kyokara_hir_def::name::Name;
use kyokara_hir_def::resolver::ModuleScope;
use kyokara_hir_def::type_ref::TypeRef;
use kyokara_intern::Interner;

use crate::choice::ChoiceSource;

/// Maximum string length for generated strings.
const MAX_STR_LEN: u64 = 32;
/// Maximum list length for generated lists.
const MAX_LIST_LEN: u64 = 16;
/// Maximum map size for generated maps.
const MAX_MAP_LEN: u64 = 8;
/// Maximum recursion depth for nested types.
const MAX_DEPTH: usize = 5;

/// "Interesting" integer values for biased generation.
const INTERESTING_INTS: &[i64] = &[0, 1, -1, i64::MAX, i64::MIN, 42, -42, 100, -100];

/// Result of attempting to generate a value.
pub enum GenResult {
    /// Successfully generated a value.
    Ok(Value),
    /// This type cannot be generated (e.g., function types, unresolved generics).
    Unsupported,
    /// Choice source exhausted during generation.
    Exhausted,
}

/// Check if a `TypeRef` is generatable (no function types or unresolved generics).
pub fn is_generatable(ty: &TypeRef, item_tree: &ItemTree, interner: &Interner) -> bool {
    is_generatable_inner(ty, item_tree, interner, 0)
}

fn is_generatable_inner(
    ty: &TypeRef,
    item_tree: &ItemTree,
    interner: &Interner,
    depth: usize,
) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    match ty {
        TypeRef::Fn { .. } => false,
        TypeRef::Error => false,
        TypeRef::Refined { base, .. } => is_generatable_inner(base, item_tree, interner, depth + 1),
        TypeRef::Record { fields } => fields
            .iter()
            .all(|(_, t)| is_generatable_inner(t, item_tree, interner, depth + 1)),
        TypeRef::Path { path, args } => {
            let Some(name) = path.last() else {
                return false;
            };
            let resolved = name.resolve(interner);
            match resolved {
                "Int" | "Float" | "Bool" | "String" | "Char" | "Unit" => true,
                "Option" => {
                    args.len() == 1
                        && is_generatable_inner(&args[0], item_tree, interner, depth + 1)
                }
                "Result" => {
                    args.len() == 2
                        && is_generatable_inner(&args[0], item_tree, interner, depth + 1)
                        && is_generatable_inner(&args[1], item_tree, interner, depth + 1)
                }
                "List" => {
                    args.len() == 1
                        && is_generatable_inner(&args[0], item_tree, interner, depth + 1)
                }
                "Map" => {
                    args.len() == 2
                        && is_generatable_inner(&args[0], item_tree, interner, depth + 1)
                        && is_generatable_inner(&args[1], item_tree, interner, depth + 1)
                }
                _ => {
                    // User-defined type — look up in item_tree.
                    find_type_by_name(item_tree, name).is_some()
                }
            }
        }
    }
}

/// Generate a value for the given type.
pub fn generate(
    ty: &TypeRef,
    source: &mut dyn ChoiceSource,
    item_tree: &ItemTree,
    module_scope: &ModuleScope,
    interner: &Interner,
) -> GenResult {
    generate_inner(ty, source, item_tree, module_scope, interner, 0)
}

fn generate_inner(
    ty: &TypeRef,
    source: &mut dyn ChoiceSource,
    item_tree: &ItemTree,
    module_scope: &ModuleScope,
    interner: &Interner,
    depth: usize,
) -> GenResult {
    if depth > MAX_DEPTH {
        return GenResult::Unsupported;
    }

    match ty {
        TypeRef::Fn { .. } | TypeRef::Error => GenResult::Unsupported,

        TypeRef::Refined { base, .. } => {
            // Tier 1: generate from base type, ignore predicate.
            generate_inner(base, source, item_tree, module_scope, interner, depth)
        }

        TypeRef::Record { fields } => {
            gen_record(fields, source, item_tree, module_scope, interner, depth)
        }

        TypeRef::Path { path, args } => {
            let Some(name) = path.last() else {
                return GenResult::Unsupported;
            };
            let resolved = name.resolve(interner);
            match resolved {
                "Int" => gen_int(source),
                "Float" => gen_float(source),
                "Bool" => gen_bool(source),
                "String" => gen_string(source),
                "Char" => gen_char(source),
                "Unit" => GenResult::Ok(Value::Unit),
                "Option" if args.len() == 1 => {
                    gen_option(&args[0], source, item_tree, module_scope, interner, depth)
                }
                "Result" if args.len() == 2 => gen_result(
                    &args[0],
                    &args[1],
                    source,
                    item_tree,
                    module_scope,
                    interner,
                    depth,
                ),
                "List" if args.len() == 1 => {
                    gen_list(&args[0], source, item_tree, module_scope, interner, depth)
                }
                "Map" if args.len() == 2 => gen_map(
                    &args[0],
                    &args[1],
                    source,
                    item_tree,
                    module_scope,
                    interner,
                    depth,
                ),
                _ => gen_user_type(name, args, source, item_tree, module_scope, interner, depth),
            }
        }
    }
}

fn gen_int(source: &mut dyn ChoiceSource) -> GenResult {
    // 1/8 chance of an interesting value.
    let Some(bias) = source.draw(7) else {
        return GenResult::Exhausted;
    };
    if bias == 0 {
        let Some(idx) = source.draw((INTERESTING_INTS.len() - 1) as u64) else {
            return GenResult::Exhausted;
        };
        GenResult::Ok(Value::Int(INTERESTING_INTS[idx as usize]))
    } else {
        let Some(raw) = source.draw(u32::MAX as u64) else {
            return GenResult::Exhausted;
        };
        // Interpret as signed: spread across negative and positive range.
        let val = raw as i64 - (u32::MAX as i64 / 2);
        GenResult::Ok(Value::Int(val))
    }
}

fn gen_float(source: &mut dyn ChoiceSource) -> GenResult {
    // Bias toward small values: generate mantissa and optional exponent.
    let Some(raw) = source.draw(u32::MAX as u64) else {
        return GenResult::Exhausted;
    };
    let Some(sign_bit) = source.draw(1) else {
        return GenResult::Exhausted;
    };
    let sign = if sign_bit == 0 { 1.0_f64 } else { -1.0_f64 };
    // Scale to [-1000.0, 1000.0] range (biased toward smaller values).
    let val = (raw as f64 / u32::MAX as f64) * 1000.0 * sign;
    GenResult::Ok(Value::Float(val))
}

fn gen_bool(source: &mut dyn ChoiceSource) -> GenResult {
    let Some(val) = source.draw(1) else {
        return GenResult::Exhausted;
    };
    GenResult::Ok(Value::Bool(val == 1))
}

fn gen_string(source: &mut dyn ChoiceSource) -> GenResult {
    let Some(len) = source.draw(MAX_STR_LEN) else {
        return GenResult::Exhausted;
    };
    let mut s = String::with_capacity(len as usize);
    for _ in 0..len {
        let Some(ch) = source.draw(127) else {
            return GenResult::Exhausted;
        };
        // Printable ASCII range 32..=126, plus space.
        let c = if ch < 95 {
            (ch + 32) as u8 as char
        } else {
            ' '
        };
        s.push(c);
    }
    GenResult::Ok(Value::String(s))
}

fn gen_char(source: &mut dyn ChoiceSource) -> GenResult {
    let Some(val) = source.draw(127) else {
        return GenResult::Exhausted;
    };
    let c = if val < 95 {
        (val + 32) as u8 as char
    } else {
        ' '
    };
    GenResult::Ok(Value::Char(c))
}

fn gen_option(
    inner: &TypeRef,
    source: &mut dyn ChoiceSource,
    item_tree: &ItemTree,
    module_scope: &ModuleScope,
    interner: &Interner,
    depth: usize,
) -> GenResult {
    let Some(tag) = source.draw(1) else {
        return GenResult::Exhausted;
    };
    // Look up Option's type_idx and variant indices.
    let (type_idx, none_variant, some_variant) = match find_option_variants(module_scope) {
        Some(v) => v,
        None => return GenResult::Unsupported,
    };
    if tag == 0 {
        // None
        GenResult::Ok(Value::Adt {
            type_idx,
            variant: none_variant,
            fields: vec![],
        })
    } else {
        // Some(inner)
        match generate_inner(inner, source, item_tree, module_scope, interner, depth + 1) {
            GenResult::Ok(val) => GenResult::Ok(Value::Adt {
                type_idx,
                variant: some_variant,
                fields: vec![val],
            }),
            other => other,
        }
    }
}

fn gen_result(
    ok_ty: &TypeRef,
    err_ty: &TypeRef,
    source: &mut dyn ChoiceSource,
    item_tree: &ItemTree,
    module_scope: &ModuleScope,
    interner: &Interner,
    depth: usize,
) -> GenResult {
    let Some(tag) = source.draw(1) else {
        return GenResult::Exhausted;
    };
    let (type_idx, ok_variant, err_variant) = match find_result_variants(module_scope) {
        Some(v) => v,
        None => return GenResult::Unsupported,
    };
    if tag == 0 {
        match generate_inner(ok_ty, source, item_tree, module_scope, interner, depth + 1) {
            GenResult::Ok(val) => GenResult::Ok(Value::Adt {
                type_idx,
                variant: ok_variant,
                fields: vec![val],
            }),
            other => other,
        }
    } else {
        match generate_inner(err_ty, source, item_tree, module_scope, interner, depth + 1) {
            GenResult::Ok(val) => GenResult::Ok(Value::Adt {
                type_idx,
                variant: err_variant,
                fields: vec![val],
            }),
            other => other,
        }
    }
}

fn gen_list(
    elem_ty: &TypeRef,
    source: &mut dyn ChoiceSource,
    item_tree: &ItemTree,
    module_scope: &ModuleScope,
    interner: &Interner,
    depth: usize,
) -> GenResult {
    let Some(len) = source.draw(MAX_LIST_LEN) else {
        return GenResult::Exhausted;
    };
    let mut items = Vec::with_capacity(len as usize);
    for _ in 0..len {
        match generate_inner(
            elem_ty,
            source,
            item_tree,
            module_scope,
            interner,
            depth + 1,
        ) {
            GenResult::Ok(val) => items.push(val),
            other => return other,
        }
    }
    GenResult::Ok(Value::List(items))
}

fn gen_map(
    key_ty: &TypeRef,
    val_ty: &TypeRef,
    source: &mut dyn ChoiceSource,
    item_tree: &ItemTree,
    module_scope: &ModuleScope,
    interner: &Interner,
    depth: usize,
) -> GenResult {
    let Some(len) = source.draw(MAX_MAP_LEN) else {
        return GenResult::Exhausted;
    };
    let mut entries = Vec::with_capacity(len as usize);
    for _ in 0..len {
        let k = match generate_inner(key_ty, source, item_tree, module_scope, interner, depth + 1) {
            GenResult::Ok(val) => val,
            other => return other,
        };
        let v = match generate_inner(val_ty, source, item_tree, module_scope, interner, depth + 1) {
            GenResult::Ok(val) => val,
            other => return other,
        };
        entries.push((k, v));
    }
    GenResult::Ok(Value::Map(entries))
}

fn gen_record(
    fields: &[(Name, TypeRef)],
    source: &mut dyn ChoiceSource,
    item_tree: &ItemTree,
    module_scope: &ModuleScope,
    interner: &Interner,
    depth: usize,
) -> GenResult {
    let mut result_fields = Vec::with_capacity(fields.len());
    for (name, ty) in fields {
        match generate_inner(ty, source, item_tree, module_scope, interner, depth + 1) {
            GenResult::Ok(val) => result_fields.push((*name, val)),
            other => return other,
        }
    }
    GenResult::Ok(Value::Record {
        fields: result_fields,
        type_idx: None,
    })
}

fn gen_user_type(
    name: Name,
    _args: &[TypeRef],
    source: &mut dyn ChoiceSource,
    item_tree: &ItemTree,
    module_scope: &ModuleScope,
    interner: &Interner,
    depth: usize,
) -> GenResult {
    let Some(type_idx) = find_type_by_name(item_tree, name) else {
        return GenResult::Unsupported;
    };
    let type_item = &item_tree.types[type_idx];
    match &type_item.kind {
        TypeDefKind::Alias(inner) => {
            generate_inner(inner, source, item_tree, module_scope, interner, depth)
        }
        TypeDefKind::Record { fields } => {
            gen_record(fields, source, item_tree, module_scope, interner, depth)
        }
        TypeDefKind::Adt { variants } => {
            if variants.is_empty() {
                return GenResult::Unsupported;
            }
            let Some(variant_idx) = source.draw((variants.len() - 1) as u64) else {
                return GenResult::Exhausted;
            };
            let variant = &variants[variant_idx as usize];
            let mut fields = Vec::with_capacity(variant.fields.len());
            for field_ty in &variant.fields {
                match generate_inner(
                    field_ty,
                    source,
                    item_tree,
                    module_scope,
                    interner,
                    depth + 1,
                ) {
                    GenResult::Ok(val) => fields.push(val),
                    other => return other,
                }
            }
            GenResult::Ok(Value::Adt {
                type_idx,
                variant: variant_idx as usize,
                fields,
            })
        }
    }
}

/// Generate a value for the given type using a generator spec.
///
/// `Auto` delegates to existing `generate()` (identical distribution).
/// Explicit specs generate values constrained to the spec.
pub fn generate_from_spec(
    spec: &kyokara_hir_def::item_tree::GenSpec,
    declared_ty: &TypeRef,
    source: &mut dyn ChoiceSource,
    item_tree: &ItemTree,
    module_scope: &ModuleScope,
    interner: &Interner,
) -> GenResult {
    use kyokara_hir_def::item_tree::GenSpec;

    match spec {
        GenSpec::Auto => generate(declared_ty, source, item_tree, module_scope, interner),
        GenSpec::Int => gen_int(source),
        GenSpec::IntRange { min, max } => gen_int_range(source, *min, *max),
        GenSpec::Float => gen_float(source),
        GenSpec::FloatRange { min, max } => gen_float_range(source, *min, *max),
        GenSpec::Bool => gen_bool(source),
        GenSpec::String => gen_string(source),
        GenSpec::Char => gen_char(source),
        GenSpec::List(inner) => {
            // Extract inner type from declared_ty if possible.
            let inner_ty = match declared_ty {
                TypeRef::Path { args, .. } if !args.is_empty() => &args[0],
                _ => return GenResult::Unsupported,
            };
            let Some(len) = source.draw(MAX_LIST_LEN) else {
                return GenResult::Exhausted;
            };
            let mut items = Vec::with_capacity(len as usize);
            for _ in 0..len {
                match generate_from_spec(inner, inner_ty, source, item_tree, module_scope, interner)
                {
                    GenResult::Ok(val) => items.push(val),
                    other => return other,
                }
            }
            GenResult::Ok(Value::List(items))
        }
        GenSpec::Map(key_spec, val_spec) => {
            let (key_ty, val_ty) = match declared_ty {
                TypeRef::Path { args, .. } if args.len() >= 2 => (&args[0], &args[1]),
                _ => return GenResult::Unsupported,
            };
            let Some(len) = source.draw(MAX_MAP_LEN) else {
                return GenResult::Exhausted;
            };
            let mut entries = Vec::with_capacity(len as usize);
            for _ in 0..len {
                let k = match generate_from_spec(
                    key_spec,
                    key_ty,
                    source,
                    item_tree,
                    module_scope,
                    interner,
                ) {
                    GenResult::Ok(val) => val,
                    other => return other,
                };
                let v = match generate_from_spec(
                    val_spec,
                    val_ty,
                    source,
                    item_tree,
                    module_scope,
                    interner,
                ) {
                    GenResult::Ok(val) => val,
                    other => return other,
                };
                entries.push((k, v));
            }
            GenResult::Ok(Value::Map(entries))
        }
        GenSpec::OptionOf(inner_spec) => {
            let inner_ty = match declared_ty {
                TypeRef::Path { args, .. } if !args.is_empty() => &args[0],
                _ => return GenResult::Unsupported,
            };
            let Some(tag) = source.draw(1) else {
                return GenResult::Exhausted;
            };
            let (type_idx, none_variant, some_variant) = match find_option_variants(module_scope) {
                Some(v) => v,
                None => return GenResult::Unsupported,
            };
            if tag == 0 {
                GenResult::Ok(Value::Adt {
                    type_idx,
                    variant: none_variant,
                    fields: vec![],
                })
            } else {
                match generate_from_spec(
                    inner_spec,
                    inner_ty,
                    source,
                    item_tree,
                    module_scope,
                    interner,
                ) {
                    GenResult::Ok(val) => GenResult::Ok(Value::Adt {
                        type_idx,
                        variant: some_variant,
                        fields: vec![val],
                    }),
                    other => other,
                }
            }
        }
        GenSpec::ResultOf(ok_spec, err_spec) => {
            let (ok_ty, err_ty) = match declared_ty {
                TypeRef::Path { args, .. } if args.len() >= 2 => (&args[0], &args[1]),
                _ => return GenResult::Unsupported,
            };
            let Some(tag) = source.draw(1) else {
                return GenResult::Exhausted;
            };
            let (type_idx, ok_variant, err_variant) = match find_result_variants(module_scope) {
                Some(v) => v,
                None => return GenResult::Unsupported,
            };
            if tag == 0 {
                match generate_from_spec(ok_spec, ok_ty, source, item_tree, module_scope, interner)
                {
                    GenResult::Ok(val) => GenResult::Ok(Value::Adt {
                        type_idx,
                        variant: ok_variant,
                        fields: vec![val],
                    }),
                    other => other,
                }
            } else {
                match generate_from_spec(
                    err_spec,
                    err_ty,
                    source,
                    item_tree,
                    module_scope,
                    interner,
                ) {
                    GenResult::Ok(val) => GenResult::Ok(Value::Adt {
                        type_idx,
                        variant: err_variant,
                        fields: vec![val],
                    }),
                    other => other,
                }
            }
        }
    }
}

fn gen_int_range(source: &mut dyn ChoiceSource, min: i64, max: i64) -> GenResult {
    if min > max {
        return GenResult::Unsupported;
    }
    let range = (max - min) as u64;
    let Some(offset) = source.draw(range) else {
        return GenResult::Exhausted;
    };
    GenResult::Ok(Value::Int(min + offset as i64))
}

fn gen_float_range(source: &mut dyn ChoiceSource, min: f64, max: f64) -> GenResult {
    if min > max {
        return GenResult::Unsupported;
    }
    let Some(raw) = source.draw(u32::MAX as u64) else {
        return GenResult::Exhausted;
    };
    let frac = raw as f64 / u32::MAX as f64;
    GenResult::Ok(Value::Float(min + frac * (max - min)))
}

/// Find a type definition by Name in the item tree.
fn find_type_by_name(item_tree: &ItemTree, name: Name) -> Option<TypeItemIdx> {
    for (idx, ty) in item_tree.types.iter() {
        if ty.name == name {
            return Some(idx);
        }
    }
    None
}

/// Find Option's type_idx, None variant index, Some variant index.
fn find_option_variants(scope: &ModuleScope) -> Option<(TypeItemIdx, usize, usize)> {
    // Look through constructors for "None" and "Some".
    let mut none_info = None;
    let mut some_info = None;
    for (&_name, &(type_idx, variant_idx)) in &scope.constructors {
        // We can't easily resolve the name here without an interner,
        // but we know the builtins register None at variant 0, Some at variant 1.
        // Use the type_idx to correlate.
        if none_info.is_none() {
            none_info = Some((type_idx, variant_idx));
        } else if some_info.is_none() {
            let (t, _) = none_info?;
            if type_idx == t {
                some_info = Some((type_idx, variant_idx));
            }
        }
    }
    // Fallback: use well-known layout — None=0, Some=1.
    // The builtins module always registers them this way.
    let (type_idx, _) = none_info.or(some_info)?;
    Some((type_idx, 0, 1))
}

/// Find Result's type_idx, Ok variant index, Err variant index.
fn find_result_variants(scope: &ModuleScope) -> Option<(TypeItemIdx, usize, usize)> {
    // Result builtins: Ok=0, Err=1.
    for (&_name, &(type_idx, variant_idx)) in &scope.constructors {
        if variant_idx == 0 {
            // Check if there's a variant_idx=1 for the same type.
            for (&_n2, &(t2, v2)) in &scope.constructors {
                if t2 == type_idx && v2 == 1 {
                    return Some((type_idx, 0, 1));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::choice::ChoiceRecorder;

    /// Helper to make a Path type ref using the given interner.
    fn make_path_type(name: &str, interner: &mut Interner) -> TypeRef {
        let n = Name::new(interner, name);
        TypeRef::Path {
            path: kyokara_hir_def::path::Path::single(n),
            args: vec![],
        }
    }

    #[test]
    fn gen_int_produces_value() {
        let mut rec = ChoiceRecorder::new(42);
        let item_tree = ItemTree::default();
        let scope = ModuleScope::default();
        let mut interner = Interner::new();
        let ty = make_path_type("Int", &mut interner);
        match generate(&ty, &mut rec, &item_tree, &scope, &interner) {
            GenResult::Ok(Value::Int(_)) => {}
            _ => panic!("expected Int value"),
        }
    }

    #[test]
    fn gen_bool_produces_value() {
        let mut rec = ChoiceRecorder::new(42);
        let item_tree = ItemTree::default();
        let scope = ModuleScope::default();
        let mut interner = Interner::new();
        let ty = make_path_type("Bool", &mut interner);
        match generate(&ty, &mut rec, &item_tree, &scope, &interner) {
            GenResult::Ok(Value::Bool(_)) => {}
            _ => panic!("expected Bool value"),
        }
    }

    #[test]
    fn gen_float_produces_value() {
        let mut rec = ChoiceRecorder::new(42);
        let item_tree = ItemTree::default();
        let scope = ModuleScope::default();
        let mut interner = Interner::new();
        let ty = make_path_type("Float", &mut interner);
        match generate(&ty, &mut rec, &item_tree, &scope, &interner) {
            GenResult::Ok(Value::Float(_)) => {}
            _ => panic!("expected Float value"),
        }
    }

    #[test]
    fn gen_string_produces_value() {
        let mut rec = ChoiceRecorder::new(42);
        let item_tree = ItemTree::default();
        let scope = ModuleScope::default();
        let mut interner = Interner::new();
        let ty = make_path_type("String", &mut interner);
        match generate(&ty, &mut rec, &item_tree, &scope, &interner) {
            GenResult::Ok(Value::String(_)) => {}
            _ => panic!("expected String value"),
        }
    }

    #[test]
    fn gen_unit_produces_value() {
        let mut rec = ChoiceRecorder::new(42);
        let item_tree = ItemTree::default();
        let scope = ModuleScope::default();
        let mut interner = Interner::new();
        let ty = make_path_type("Unit", &mut interner);
        match generate(&ty, &mut rec, &item_tree, &scope, &interner) {
            GenResult::Ok(Value::Unit) => {}
            _ => panic!("expected Unit value"),
        }
    }

    #[test]
    fn fn_type_is_unsupported() {
        let mut rec = ChoiceRecorder::new(42);
        let item_tree = ItemTree::default();
        let scope = ModuleScope::default();
        let mut interner = Interner::new();
        let ret = make_path_type("Int", &mut interner);
        let ty = TypeRef::Fn {
            params: vec![],
            ret: Box::new(ret),
        };
        assert!(!is_generatable(&ty, &item_tree, &interner));
        match generate(&ty, &mut rec, &item_tree, &scope, &interner) {
            GenResult::Unsupported => {}
            _ => panic!("expected Unsupported for Fn type"),
        }
    }
}
