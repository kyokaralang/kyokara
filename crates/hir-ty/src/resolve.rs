//! Resolution of surface-level [`TypeRef`] into fully-resolved [`Ty`].

use kyokara_hir_def::item_tree::{ItemTree, TypeDefKind, TypeItemIdx};
use kyokara_hir_def::name::Name;
use kyokara_hir_def::resolver::ModuleScope;
use kyokara_hir_def::type_ref::TypeRef;
use kyokara_intern::Interner;

use crate::ty::{Ty, resolve_builtin};
use crate::unify::UnificationTable;

/// Environment for resolving type references.
pub(crate) struct TyResolutionEnv<'a> {
    pub item_tree: &'a ItemTree,
    pub module_scope: &'a ModuleScope,
    pub interner: &'a Interner,
    /// Type parameter names → inference variables (for generic instantiation).
    pub type_params: Vec<(Name, Ty)>,
    /// Alias indices currently being resolved (cycle detection).
    pub resolving_aliases: Vec<TypeItemIdx>,
}

impl<'a> TyResolutionEnv<'a> {
    /// Resolve a [`TypeRef`] to a [`Ty`].
    pub fn resolve_type_ref(&self, ty: &TypeRef, table: &mut UnificationTable) -> Ty {
        match ty {
            TypeRef::Error => Ty::Error,

            TypeRef::Path { path, args } => {
                if path.is_single() {
                    let name = path.segments[0];
                    let name_str = name.resolve(self.interner);

                    // 1. Check built-in primitives.
                    if let Some(builtin) = resolve_builtin(name_str) {
                        return builtin;
                    }

                    // 2. Check type parameters in scope.
                    if let Some((_, ty)) = self.type_params.iter().find(|(n, _)| *n == name) {
                        return ty.clone();
                    }

                    // 3. Check module-scope types.
                    if let Some(&type_idx) = self.module_scope.types.get(&name) {
                        return self.resolve_type_item(type_idx, args, table);
                    }

                    // Unresolved — return error.
                    Ty::Error
                } else {
                    // Multi-segment paths not yet supported in v0.0.
                    Ty::Error
                }
            }

            TypeRef::Fn { params, ret } => {
                let params: Vec<Ty> = params
                    .iter()
                    .map(|p| self.resolve_type_ref(p, table))
                    .collect();
                let ret = self.resolve_type_ref(ret, table);
                Ty::Fn {
                    params,
                    ret: Box::new(ret),
                }
            }

            TypeRef::Record { fields } => {
                let fields: Vec<(Name, Ty)> = fields
                    .iter()
                    .map(|(n, t)| (*n, self.resolve_type_ref(t, table)))
                    .collect();
                Ty::Record { fields }
            }

            TypeRef::Refined { base, .. } => {
                // v0.0: ignore the refinement predicate, just resolve the base type.
                self.resolve_type_ref(base, table)
            }
        }
    }

    /// Resolve a type definition reference into a [`Ty`].
    fn resolve_type_item(
        &self,
        type_idx: TypeItemIdx,
        args: &[TypeRef],
        table: &mut UnificationTable,
    ) -> Ty {
        // Cycle detection: if we're already resolving this alias, bail out.
        if self.resolving_aliases.contains(&type_idx) {
            return Ty::Error;
        }

        let type_item = &self.item_tree.types[type_idx];

        // For aliases, resolve the underlying type (substituting type args).
        // Exception: alias-to-record types (e.g., `type Point = { x: Int, y: Int }`)
        // are treated as named types (Ty::Adt) so that method resolution can
        // find the type name. Other aliases (path aliases) are expanded normally.
        if let TypeDefKind::Alias(inner) = &type_item.kind
            && !matches!(inner, TypeRef::Record { .. })
        {
            let mut env = self.with_type_args(&type_item.type_params, args, table);
            env.resolving_aliases.push(type_idx);
            return env.resolve_type_ref(inner, table);
        }

        // For ADTs and records, produce Ty::Adt with resolved type arguments.
        let resolved_args: Vec<Ty> = if args.is_empty() && !type_item.type_params.is_empty() {
            // Inferred type args: create fresh variables.
            type_item
                .type_params
                .iter()
                .map(|_| table.fresh_var())
                .collect()
        } else {
            args.iter()
                .map(|a| self.resolve_type_ref(a, table))
                .collect()
        };

        Ty::Adt {
            def: type_idx,
            args: resolved_args,
        }
    }

    /// Create a child environment that binds type parameters to either
    /// the given explicit arguments or fresh inference variables.
    fn with_type_args(
        &self,
        param_names: &[Name],
        args: &[TypeRef],
        table: &mut UnificationTable,
    ) -> TyResolutionEnv<'a> {
        let mut type_params = self.type_params.clone();
        for (i, name) in param_names.iter().enumerate() {
            let ty = if i < args.len() {
                self.resolve_type_ref(&args[i], table)
            } else {
                table.fresh_var()
            };
            type_params.push((*name, ty));
        }
        TyResolutionEnv {
            item_tree: self.item_tree,
            module_scope: self.module_scope,
            interner: self.interner,
            type_params,
            resolving_aliases: self.resolving_aliases.clone(),
        }
    }
}

/// Instantiate a function's signature, returning (param types, return type).
/// Type parameters get fresh inference variables.
pub(crate) fn instantiate_fn_sig(
    fn_idx: kyokara_hir_def::item_tree::FnItemIdx,
    env: &TyResolutionEnv<'_>,
    table: &mut UnificationTable,
) -> (Vec<Ty>, Ty) {
    let fn_item = &env.item_tree.functions[fn_idx];

    // Create fresh type vars for the function's type parameters.
    let mut inner_env = TyResolutionEnv {
        item_tree: env.item_tree,
        module_scope: env.module_scope,
        interner: env.interner,
        type_params: env.type_params.clone(),
        resolving_aliases: vec![],
    };
    for &name in &fn_item.type_params {
        let var = table.fresh_var();
        inner_env.type_params.push((name, var));
    }

    let param_tys: Vec<Ty> = fn_item
        .params
        .iter()
        .map(|p| inner_env.resolve_type_ref(&p.ty, table))
        .collect();

    let ret_ty = fn_item
        .ret_type
        .as_ref()
        .map(|t| inner_env.resolve_type_ref(t, table))
        .unwrap_or(Ty::Unit);

    (param_tys, ret_ty)
}

/// Instantiate an ADT constructor variant, returning (field types, resulting Adt type).
pub(crate) fn instantiate_constructor(
    type_idx: TypeItemIdx,
    variant_idx: usize,
    env: &TyResolutionEnv<'_>,
    table: &mut UnificationTable,
) -> (Vec<Ty>, Ty) {
    let type_item = &env.item_tree.types[type_idx];

    // Create fresh type vars for the type's type parameters.
    let mut inner_env = TyResolutionEnv {
        item_tree: env.item_tree,
        module_scope: env.module_scope,
        interner: env.interner,
        type_params: env.type_params.clone(),
        resolving_aliases: vec![],
    };
    let mut args = Vec::new();
    for &name in &type_item.type_params {
        let var = table.fresh_var();
        inner_env.type_params.push((name, var.clone()));
        args.push(var);
    }

    let field_tys = match &type_item.kind {
        TypeDefKind::Adt { variants } => variants[variant_idx]
            .fields
            .iter()
            .map(|f| inner_env.resolve_type_ref(f, table))
            .collect(),
        TypeDefKind::Record { fields } => fields
            .iter()
            .map(|(_, f)| inner_env.resolve_type_ref(f, table))
            .collect(),
        TypeDefKind::Alias(_) => vec![],
    };

    let adt_ty = Ty::Adt {
        def: type_idx,
        args,
    };

    (field_tys, adt_ty)
}
