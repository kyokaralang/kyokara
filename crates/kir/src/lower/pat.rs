//! Pattern binding: destructure HIR patterns into KIR values.

use kyokara_hir_def::expr::PatIdx;
use kyokara_hir_def::pat::Pat;

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
                for field_name in &fields {
                    let field_ty = self.pat_ty(pat_idx);
                    let field_val = self.builder.push_field_get(value, *field_name, field_ty);
                    self.define_local(*field_name, field_val);
                }
            }
            Pat::Literal(_) | Pat::Missing => {}
        }
    }
}
