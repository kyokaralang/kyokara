//! Unification table for Hindley-Milner type inference.
//!
//! Provides fresh type variable allocation, union-find resolution,
//! Robinson's unification with occurs check, and deep resolution.

use crate::ty::{Ty, TyVarId};

/// Union-find based unification table.
#[derive(Clone)]
pub struct UnificationTable {
    /// Each slot is either `None` (unbound variable) or `Some(ty)` (bound).
    vars: Vec<Option<Ty>>,
}

impl Default for UnificationTable {
    fn default() -> Self {
        Self::new()
    }
}

impl UnificationTable {
    pub fn new() -> Self {
        Self { vars: Vec::new() }
    }

    /// Allocate a fresh inference variable.
    pub fn fresh_var(&mut self) -> Ty {
        let id = TyVarId(self.vars.len() as u32);
        self.vars.push(None);
        Ty::Var(id)
    }

    fn resolve_var_chain(&self, id: TyVarId) -> (Ty, Vec<TyVarId>) {
        let mut current = id;
        let mut path = Vec::new();
        let mut steps = 0usize;

        loop {
            steps += 1;
            if steps > self.vars.len().saturating_add(1) {
                debug_assert!(false, "type variable cycle detected during resolve");
                return (Ty::Var(current), path);
            }

            match &self.vars[current.0 as usize] {
                Some(Ty::Var(next)) => {
                    path.push(current);
                    current = *next;
                }
                Some(bound) => {
                    path.push(current);
                    return (bound.clone(), path);
                }
                None => return (Ty::Var(current), path),
            }
        }
    }

    fn compress_path(&mut self, path: &[TyVarId], resolved: &Ty) {
        for id in path {
            self.vars[id.0 as usize] = Some(resolved.clone());
        }
    }

    fn resolve_with_compression(&mut self, ty: &Ty) -> Ty {
        match ty {
            Ty::Var(id) => {
                let (resolved, path) = self.resolve_var_chain(*id);
                self.compress_path(&path, &resolved);
                resolved
            }
            _ => ty.clone(),
        }
    }

    /// Shallow resolve: follow the binding chain iteratively.
    pub fn resolve(&self, ty: &Ty) -> Ty {
        match ty {
            Ty::Var(id) => self.resolve_var_chain(*id).0,
            _ => ty.clone(),
        }
    }

    /// Deep resolve: recursively resolve all type variables inside a type.
    pub fn resolve_deep(&self, ty: &Ty) -> Ty {
        let ty = self.resolve(ty);
        match ty {
            Ty::Var(_) => ty,
            Ty::Int | Ty::Float | Ty::String | Ty::Char | Ty::Bool | Ty::Unit => ty,
            Ty::Error | Ty::Never => ty,
            Ty::Adt { def, args } => Ty::Adt {
                def,
                args: args.iter().map(|a| self.resolve_deep(a)).collect(),
            },
            Ty::Record { fields } => Ty::Record {
                fields: fields
                    .iter()
                    .map(|(n, t)| (*n, self.resolve_deep(t)))
                    .collect(),
            },
            Ty::Fn { params, ret } => Ty::Fn {
                params: params.iter().map(|p| self.resolve_deep(p)).collect(),
                ret: Box::new(self.resolve_deep(&ret)),
            },
        }
    }

    /// Unify two types, returning `true` on success and `false` on failure.
    pub fn unify(&mut self, a: &Ty, b: &Ty) -> bool {
        let a = self.resolve_with_compression(a);
        let b = self.resolve_with_compression(b);

        // Poison types unify with everything.
        if a.is_poison() || b.is_poison() {
            return true;
        }

        match (&a, &b) {
            // Same variable — trivially unified.
            (Ty::Var(va), Ty::Var(vb)) if va == vb => true,

            // Bind variable to type (with occurs check).
            (Ty::Var(v), _) => {
                if self.occurs(*v, &b) {
                    return false;
                }
                self.vars[v.0 as usize] = Some(b);
                true
            }
            (_, Ty::Var(v)) => {
                if self.occurs(*v, &a) {
                    return false;
                }
                self.vars[v.0 as usize] = Some(a);
                true
            }

            // Primitives.
            (Ty::Int, Ty::Int)
            | (Ty::Float, Ty::Float)
            | (Ty::String, Ty::String)
            | (Ty::Char, Ty::Char)
            | (Ty::Bool, Ty::Bool)
            | (Ty::Unit, Ty::Unit) => true,

            // ADT: same def + pairwise args.
            (Ty::Adt { def: d1, args: a1 }, Ty::Adt { def: d2, args: a2 }) => {
                d1 == d2 && a1.len() == a2.len() && {
                    for (x, y) in a1.iter().zip(a2.iter()) {
                        if !self.unify(x, y) {
                            return false;
                        }
                    }
                    true
                }
            }

            // Structural record: same named fields, independent of declaration order.
            (Ty::Record { fields: f1 }, Ty::Record { fields: f2 }) => {
                if f1.len() != f2.len() {
                    return false;
                }

                let mut used = vec![false; f2.len()];
                for (n1, t1) in f1 {
                    let mut match_idx = None;
                    for (idx, (n2, _)) in f2.iter().enumerate() {
                        if !used[idx] && n1 == n2 {
                            match_idx = Some(idx);
                            break;
                        }
                    }

                    let Some(idx) = match_idx else {
                        return false;
                    };
                    used[idx] = true;

                    let t2 = &f2[idx].1;
                    if !self.unify(t1, t2) {
                        return false;
                    }
                }

                true
            }

            // Function type: params + ret.
            (
                Ty::Fn {
                    params: p1,
                    ret: r1,
                },
                Ty::Fn {
                    params: p2,
                    ret: r2,
                },
            ) => {
                p1.len() == p2.len()
                    && p1.iter().zip(p2.iter()).all(|(a, b)| self.unify(a, b))
                    && self.unify(r1, r2)
            }

            _ => false,
        }
    }

    /// Occurs check: does variable `v` appear anywhere in `ty`?
    fn occurs(&mut self, v: TyVarId, ty: &Ty) -> bool {
        let ty = self.resolve_with_compression(ty);
        match &ty {
            Ty::Var(id) => *id == v,
            Ty::Int | Ty::Float | Ty::String | Ty::Char | Ty::Bool | Ty::Unit => false,
            Ty::Error | Ty::Never => false,
            Ty::Adt { args, .. } => args.iter().any(|a| self.occurs(v, a)),
            Ty::Record { fields } => fields.iter().any(|(_, t)| self.occurs(v, t)),
            Ty::Fn { params, ret } => {
                params.iter().any(|p| self.occurs(v, p)) || self.occurs(v, ret)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn fresh_vars_are_distinct() {
        let mut table = UnificationTable::new();
        let a = table.fresh_var();
        let b = table.fresh_var();
        assert_ne!(a, b);
    }

    #[test]
    fn unify_same_primitive() {
        let mut table = UnificationTable::new();
        assert!(table.unify(&Ty::Int, &Ty::Int));
        assert!(table.unify(&Ty::Bool, &Ty::Bool));
    }

    #[test]
    fn unify_different_primitives_fails() {
        let mut table = UnificationTable::new();
        assert!(!table.unify(&Ty::Int, &Ty::Bool));
    }

    #[test]
    fn unify_var_with_concrete() {
        let mut table = UnificationTable::new();
        let v = table.fresh_var();
        assert!(table.unify(&v, &Ty::Int));
        assert_eq!(table.resolve_deep(&v), Ty::Int);
    }

    #[test]
    fn unify_two_vars() {
        let mut table = UnificationTable::new();
        let a = table.fresh_var();
        let b = table.fresh_var();
        assert!(table.unify(&a, &b));
        assert!(table.unify(&b, &Ty::String));
        assert_eq!(table.resolve_deep(&a), Ty::String);
    }

    #[test]
    fn unify_fn_types() {
        let mut table = UnificationTable::new();
        let f1 = Ty::Fn {
            params: vec![Ty::Int],
            ret: Box::new(Ty::Bool),
        };
        let v = table.fresh_var();
        let f2 = Ty::Fn {
            params: vec![Ty::Int],
            ret: Box::new(v.clone()),
        };
        assert!(table.unify(&f1, &f2));
        assert_eq!(table.resolve_deep(&v), Ty::Bool);
    }

    #[test]
    fn error_unifies_with_anything() {
        let mut table = UnificationTable::new();
        assert!(table.unify(&Ty::Error, &Ty::Int));
        assert!(table.unify(&Ty::Bool, &Ty::Error));
    }

    #[test]
    fn never_unifies_with_anything() {
        let mut table = UnificationTable::new();
        assert!(table.unify(&Ty::Never, &Ty::Int));
    }

    #[test]
    fn occurs_check_prevents_infinite_type() {
        let mut table = UnificationTable::new();
        let v = table.fresh_var();
        let circular = Ty::Fn {
            params: vec![v.clone()],
            ret: Box::new(Ty::Int),
        };
        assert!(!table.unify(&v, &circular));
    }

    fn var_id(ty: &Ty) -> TyVarId {
        match ty {
            Ty::Var(id) => *id,
            _ => panic!("expected type variable"),
        }
    }

    #[test]
    fn resolve_with_compression_shortens_var_chains() {
        let mut table = UnificationTable::new();
        let vars: Vec<_> = (0..64).map(|_| table.fresh_var()).collect();

        for pair in vars.windows(2) {
            let from = var_id(&pair[0]);
            table.vars[from.0 as usize] = Some(pair[1].clone());
        }
        let last = var_id(vars.last().unwrap());
        table.vars[last.0 as usize] = Some(Ty::Int);

        let resolved = table.resolve_with_compression(&vars[0]);
        assert_eq!(resolved, Ty::Int);

        for var in &vars {
            let id = var_id(var);
            assert_eq!(table.vars[id.0 as usize], Some(Ty::Int));
        }
    }

    #[test]
    fn resolve_deep_handles_long_var_chains_iteratively() {
        let mut table = UnificationTable::new();
        let vars: Vec<_> = (0..4096).map(|_| table.fresh_var()).collect();

        for pair in vars.windows(2) {
            let from = var_id(&pair[0]);
            table.vars[from.0 as usize] = Some(pair[1].clone());
        }
        let last = var_id(vars.last().unwrap());
        table.vars[last.0 as usize] = Some(Ty::Bool);

        assert_eq!(table.resolve_deep(&vars[0]), Ty::Bool);
    }
}
