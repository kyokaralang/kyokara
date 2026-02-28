//! Pattern binding: destructure HIR patterns into KIR values.

use kyokara_hir_def::expr::PatIdx;
use kyokara_hir_def::item_tree::{ItemTree, TypeDefKind};
use kyokara_hir_def::name::Name;
use kyokara_hir_def::pat::Pat;
use kyokara_hir_def::type_ref::TypeRef;
use kyokara_hir_ty::ty::{Ty, resolve_builtin};
use kyokara_intern::Interner;

use crate::value::ValueId;

use super::LoweringCtx;

impl<'a> LoweringCtx<'a> {
    /// Bind a pattern to a value, defining locals as needed.
    ///
    /// Used for `let` bindings and match-arm destructuring.
    pub(crate) fn bind_pattern(&mut self, pat_idx: PatIdx, value: ValueId) {
        let pat = self.body.pats[pat_idx].clone();
        match pat {
            Pat::Bind { name } => {
                self.define_local(name, value);
            }
            Pat::Wildcard => {}
            Pat::Constructor { args, .. } => {
                for (i, sub_pat) in args.iter().enumerate() {
                    let field_ty = self.pat_ty(*sub_pat);
                    let field_val = self.builder.push_adt_field_get(value, i as u32, field_ty);
                    self.bind_pattern(*sub_pat, field_val);
                }
            }
            Pat::Record { fields, .. } => {
                let record_ty = self.pat_ty(pat_idx);
                for field_name in &fields {
                    let field_ty = resolve_record_field_ty(
                        &record_ty,
                        *field_name,
                        self.item_tree,
                        self.interner,
                    );
                    let field_val = self.builder.push_field_get(value, *field_name, field_ty);
                    self.define_local(*field_name, field_val);
                }
            }
            Pat::Literal(_) | Pat::Missing => {}
        }
    }
}

/// Look up the type of a field within a record type.
///
/// For structural records (`Ty::Record`), the field types are available
/// directly. For named record types (`Ty::Adt` pointing to a record
/// definition), we resolve the field's `TypeRef` to `Ty` for simple
/// builtin types (Int, String, etc.). Complex types fall back to
/// `Ty::Error`, which is acceptable since KIR type annotations are
/// advisory and don't affect runtime semantics.
fn resolve_record_field_ty(
    record_ty: &Ty,
    field: Name,
    item_tree: &ItemTree,
    interner: &Interner,
) -> Ty {
    match record_ty {
        Ty::Record { fields } => {
            for (name, ty) in fields {
                if *name == field {
                    return ty.clone();
                }
            }
        }
        Ty::Adt { def, .. } => {
            let type_item = &item_tree.types[*def];
            let def_fields = match &type_item.kind {
                TypeDefKind::Record { fields } => Some(fields),
                TypeDefKind::Alias(TypeRef::Record { fields }) => Some(fields),
                _ => None,
            };
            if let Some(def_fields) = def_fields {
                for (n, type_ref) in def_fields {
                    if *n == field {
                        return resolve_type_ref_simple(type_ref, interner);
                    }
                }
            }
        }
        _ => {}
    }
    Ty::Error
}

/// Lightweight TypeRef → Ty resolution for simple/builtin types.
/// Falls back to Ty::Error for anything complex (generics, records, etc.).
fn resolve_type_ref_simple(type_ref: &TypeRef, interner: &Interner) -> Ty {
    match type_ref {
        TypeRef::Path { path, args } if path.is_single() && args.is_empty() => {
            let name_str = path.segments[0].resolve(interner);
            resolve_builtin(name_str).unwrap_or(Ty::Error)
        }
        _ => Ty::Error,
    }
}
