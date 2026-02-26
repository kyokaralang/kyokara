//! Pattern type inference for all [`Pat`] variants.

use kyokara_hir_def::expr::Literal;
use kyokara_hir_def::pat::Pat;

use crate::diagnostics::TyDiagnosticData;
use crate::resolve::instantiate_constructor;
use crate::ty::Ty;

use super::InferenceCtx;

impl<'a> InferenceCtx<'a> {
    /// Infer the type of a pattern against an expected scrutinee type.
    pub(crate) fn infer_pat(&mut self, pat_idx: la_arena::Idx<Pat>, expected: &Ty) {
        let pat = self.body.pats[pat_idx].clone();
        let ty = match pat {
            Pat::Missing => Ty::Error,

            Pat::Wildcard => expected.clone(),

            Pat::Bind { name: _ } => {
                let ty = expected.clone();
                self.local_types.insert(pat_idx, ty.clone());
                ty
            }

            Pat::Literal(lit) => {
                let lit_ty = match &lit {
                    Literal::Int(_) => Ty::Int,
                    Literal::Float(_) => Ty::Float,
                    Literal::String(_) => Ty::String,
                    Literal::Char(_) => Ty::Char,
                    Literal::Bool(_) => Ty::Bool,
                };
                self.unify_or_err(expected, &lit_ty);
                lit_ty
            }

            Pat::Constructor { path, args } => {
                if !path.is_single() {
                    self.push_diag(TyDiagnosticData::UnresolvedConstructor {
                        name: path
                            .segments
                            .iter()
                            .map(|s| s.resolve(self.interner).to_owned())
                            .collect::<Vec<_>>()
                            .join("."),
                    });
                    for sub in &args {
                        self.infer_pat(*sub, &Ty::Error);
                    }
                    self.pat_types.insert(pat_idx, Ty::Error);
                    return;
                }
                let name = path.segments[0];

                if let Some(&(type_idx, variant_idx)) = self.module_scope.constructors.get(&name) {
                    let env = Self::make_env(
                        self.item_tree,
                        self.module_scope,
                        self.interner,
                        &self.type_params,
                    );
                    let (field_tys, adt_ty) =
                        instantiate_constructor(type_idx, variant_idx, &env, &mut self.table);

                    self.unify_or_err(expected, &adt_ty);

                    if args.len() != field_tys.len() {
                        self.push_diag(TyDiagnosticData::ArgCountMismatch {
                            expected: field_tys.len(),
                            actual: args.len(),
                        });
                        for sub in &args {
                            self.infer_pat(*sub, &Ty::Error);
                        }
                    } else {
                        for (sub, field_ty) in args.iter().zip(field_tys.iter()) {
                            self.infer_pat(*sub, field_ty);
                        }
                    }
                    adt_ty
                } else {
                    self.push_diag(TyDiagnosticData::UnresolvedConstructor {
                        name: name.resolve(self.interner).to_owned(),
                    });
                    for sub in &args {
                        self.infer_pat(*sub, &Ty::Error);
                    }
                    Ty::Error
                }
            }

            Pat::Record { path: _, fields } => {
                let resolved = self.table.resolve(expected);
                match resolved {
                    Ty::Record {
                        fields: ref rec_fields,
                    } => {
                        for field_name in &fields {
                            let field_str = field_name.resolve(self.interner);
                            if !rec_fields
                                .iter()
                                .any(|(n, _)| n.resolve(self.interner) == field_str)
                            {
                                self.push_diag(TyDiagnosticData::NoSuchField {
                                    field: field_str.to_owned(),
                                    ty: resolved.clone(),
                                });
                            }
                        }
                    }
                    Ty::Error | Ty::Var(_) => {}
                    _ => {
                        self.push_diag(TyDiagnosticData::TypeMismatch {
                            expected: Ty::Record { fields: vec![] },
                            actual: resolved.clone(),
                        });
                    }
                }
                expected.clone()
            }
        };

        self.pat_types.insert(pat_idx, ty);
    }
}
