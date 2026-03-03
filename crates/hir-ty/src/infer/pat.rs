//! Pattern type inference for all [`Pat`] variants.

use kyokara_hir_def::expr::Literal;
use kyokara_hir_def::item_tree::TypeDefKind;
use kyokara_hir_def::pat::Pat;

use crate::diagnostics::TyDiagnosticData;
use crate::resolve::{TyResolutionEnv, instantiate_constructor};
use crate::ty::Ty;

use super::InferenceCtx;

impl<'a> InferenceCtx<'a> {
    /// Infer the type of a pattern against an expected scrutinee type.
    pub(crate) fn infer_pat(&mut self, pat_idx: la_arena::Idx<Pat>, expected: &Ty) {
        let ty = match &self.body.pats[pat_idx] {
            Pat::Missing => Ty::Error,

            Pat::Wildcard => expected.clone(),

            Pat::Bind { name: _ } => {
                let ty = expected.clone();
                self.local_types.insert(pat_idx, ty.clone());
                ty
            }

            Pat::Literal(lit) => {
                let lit_ty = match lit {
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
                    self.push_pat_diag(
                        pat_idx,
                        TyDiagnosticData::UnresolvedConstructor {
                            name: path
                                .segments
                                .iter()
                                .map(|s| s.resolve(self.interner).to_owned())
                                .collect::<Vec<_>>()
                                .join("."),
                        },
                    );
                    for sub in args {
                        self.infer_pat(*sub, &Ty::Error);
                    }
                    self.pat_types.insert(pat_idx, Ty::Error);
                    return;
                }
                let name = path.segments[0];
                let args = args.clone();

                // Prefer owner-type resolution when expected type is known.
                // This avoids constructor-name collisions across unrelated ADTs.
                let resolved_expected = self.table.resolve_deep(expected);
                if let Ty::Adt {
                    def: expected_def, ..
                } = resolved_expected
                    && let TypeDefKind::Adt { variants } = &self.item_tree.types[expected_def].kind
                    && let Some(variant_idx) = variants.iter().position(|v| v.name == name)
                {
                    let env = Self::make_env(
                        self.item_tree,
                        self.module_scope,
                        self.interner,
                        &self.type_params,
                    );
                    let (field_tys, adt_ty) =
                        instantiate_constructor(expected_def, variant_idx, &env, &mut self.table);

                    self.unify_or_err(expected, &adt_ty);

                    if args.len() != field_tys.len() {
                        self.push_pat_diag(
                            pat_idx,
                            TyDiagnosticData::ArgCountMismatch {
                                expected: field_tys.len(),
                                actual: args.len(),
                            },
                        );
                        for sub in args {
                            self.infer_pat(sub, &Ty::Error);
                        }
                    } else {
                        for (sub, field_ty) in args.iter().zip(field_tys.iter()) {
                            self.infer_pat(*sub, field_ty);
                        }
                    }
                    adt_ty
                } else if let Some(&(type_idx, variant_idx)) =
                    self.module_scope.constructors.get(&name)
                {
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
                        self.push_pat_diag(
                            pat_idx,
                            TyDiagnosticData::ArgCountMismatch {
                                expected: field_tys.len(),
                                actual: args.len(),
                            },
                        );
                        for sub in args {
                            self.infer_pat(sub, &Ty::Error);
                        }
                    } else {
                        for (sub, field_ty) in args.iter().zip(field_tys.iter()) {
                            self.infer_pat(*sub, field_ty);
                        }
                    }
                    adt_ty
                } else {
                    self.push_pat_diag(
                        pat_idx,
                        TyDiagnosticData::UnresolvedConstructor {
                            name: name.resolve(self.interner).to_owned(),
                        },
                    );
                    for sub in args {
                        self.infer_pat(sub, &Ty::Error);
                    }
                    Ty::Error
                }
            }

            Pat::Record { path, fields } => {
                if let Some(path) = path {
                    let path_text = path
                        .segments
                        .iter()
                        .map(|s| s.resolve(self.interner).to_owned())
                        .collect::<Vec<_>>()
                        .join(".");
                    self.push_pat_diag(
                        pat_idx,
                        TyDiagnosticData::UnsupportedRecordPatternPath { path: path_text },
                    );
                }

                let resolved = self.table.resolve(expected);
                match resolved {
                    Ty::Record {
                        fields: ref rec_fields,
                    } => {
                        for field_name in fields {
                            let field_str = field_name.resolve(self.interner);
                            if let Some((_, field_ty)) = rec_fields
                                .iter()
                                .find(|(n, _)| n.resolve(self.interner) == field_str)
                            {
                                let bind_ty = self.table.resolve_deep(field_ty);
                                for bind_pat_idx in
                                    self.record_field_bind_pats_in_range(pat_idx, *field_name)
                                {
                                    self.local_types.insert(bind_pat_idx, bind_ty.clone());
                                    self.pat_types.insert(bind_pat_idx, bind_ty.clone());
                                }
                            } else {
                                self.push_pat_diag(
                                    pat_idx,
                                    TyDiagnosticData::NoSuchField {
                                        field: field_str.to_owned(),
                                        ty: resolved.clone(),
                                    },
                                );
                            }
                        }
                    }
                    Ty::Adt { ref def, ref args } => {
                        let type_item = &self.item_tree.types[*def];
                        let def_fields = match &type_item.kind {
                            TypeDefKind::Record { fields: f } => Some(f),
                            TypeDefKind::Alias(kyokara_hir_def::type_ref::TypeRef::Record {
                                fields: f,
                            }) => Some(f),
                            _ => None,
                        };
                        if let Some(def_fields) = def_fields {
                            let mut tp_map: Vec<(kyokara_hir_def::name::Name, Ty)> =
                                self.type_params.clone();
                            for (param_name, arg) in type_item.type_params.iter().zip(args.iter()) {
                                tp_map.push((*param_name, arg.clone()));
                            }
                            let env = TyResolutionEnv {
                                item_tree: self.item_tree,
                                module_scope: self.module_scope,
                                interner: self.interner,
                                type_params: tp_map,
                                resolving_aliases: vec![],
                            };
                            for field_name in fields {
                                let field_str = field_name.resolve(self.interner);
                                if let Some((_, type_ref)) = def_fields
                                    .iter()
                                    .find(|(n, _)| n.resolve(self.interner) == field_str)
                                {
                                    let field_ty = env.resolve_type_ref(type_ref, &mut self.table);
                                    let bind_ty = self.table.resolve_deep(&field_ty);
                                    for bind_pat_idx in
                                        self.record_field_bind_pats_in_range(pat_idx, *field_name)
                                    {
                                        self.local_types.insert(bind_pat_idx, bind_ty.clone());
                                        self.pat_types.insert(bind_pat_idx, bind_ty.clone());
                                    }
                                } else {
                                    self.push_pat_diag(
                                        pat_idx,
                                        TyDiagnosticData::NoSuchField {
                                            field: field_str.to_owned(),
                                            ty: resolved.clone(),
                                        },
                                    );
                                }
                            }
                        } else {
                            self.push_pat_diag(
                                pat_idx,
                                TyDiagnosticData::TypeMismatch {
                                    expected: Ty::Record { fields: vec![] },
                                    actual: resolved.clone(),
                                },
                            );
                        }
                    }
                    Ty::Error | Ty::Var(_) => {}
                    _ => {
                        self.push_pat_diag(
                            pat_idx,
                            TyDiagnosticData::TypeMismatch {
                                expected: Ty::Record { fields: vec![] },
                                actual: resolved.clone(),
                            },
                        );
                    }
                }
                expected.clone()
            }
        };

        self.pat_types.insert(pat_idx, ty);
    }

    fn record_field_bind_pats_in_range(
        &self,
        record_pat_idx: la_arena::Idx<Pat>,
        field_name: kyokara_hir_def::name::Name,
    ) -> Vec<la_arena::Idx<Pat>> {
        let Some(record_range) = self.body.pat_source_map.get(record_pat_idx).copied() else {
            return Vec::new();
        };

        self.body
            .local_binding_meta
            .iter()
            .filter_map(|(bind_pat_idx, meta)| {
                if meta.decl_range.start() < record_range.start()
                    || meta.decl_range.end() > record_range.end()
                {
                    return None;
                }

                match &self.body.pats[bind_pat_idx] {
                    Pat::Bind { name } if *name == field_name => Some(bind_pat_idx),
                    _ => None,
                }
            })
            .collect()
    }
}
