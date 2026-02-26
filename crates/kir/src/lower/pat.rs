//! Pattern binding: destructure HIR patterns into KIR values.

use kyokara_hir_def::expr::PatIdx;
use kyokara_hir_def::name::Name;
use kyokara_hir_def::pat::Pat;
use kyokara_hir_ty::ty::Ty;

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
                    let field_ty = resolve_record_field_ty(&record_ty, *field_name);
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
/// directly. For named record types (`Ty::Adt` pointing to a
/// `TypeDefKind::Record`), the definition only has `TypeRef` (not `Ty`),
/// so we fall back to `Ty::Error` — this is a type annotation only and
/// does not affect runtime semantics.
fn resolve_record_field_ty(record_ty: &Ty, field: Name) -> Ty {
    if let Ty::Record { fields } = record_ty {
        for (name, ty) in fields {
            if *name == field {
                return ty.clone();
            }
        }
    }
    // Named record types (Ty::Adt) store field types as TypeRef, not Ty.
    // Without inference context we can't resolve them, so use Error.
    Ty::Error
}
