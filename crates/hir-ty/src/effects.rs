//! Effect/capability checking.
//!
//! Each function declares its effects via `with` clauses. At call sites,
//! we check that the callee's effects are a subset of the caller's.

use kyokara_hir_def::name::Name;
use kyokara_hir_def::type_ref::TypeRef;
use kyokara_intern::Interner;
use kyokara_stdx::FxHashSet;

use crate::resolve::TyResolutionEnv;
use crate::unify::UnificationTable;

/// A set of capability/effect names.
#[derive(Debug, Clone, Default)]
pub struct EffectSet {
    pub caps: FxHashSet<Name>,
}

impl EffectSet {
    /// Build an effect set from `with_caps` TypeRefs on a function signature.
    pub(crate) fn from_with_caps(
        with_caps: &[TypeRef],
        _env: &TyResolutionEnv<'_>,
        _table: &mut UnificationTable,
        interner: &Interner,
    ) -> Self {
        let mut caps = FxHashSet::default();
        for cap_ref in with_caps {
            if let TypeRef::Path { path, .. } = cap_ref
                && let Some(name) = path.last()
            {
                let _ = name.resolve(interner); // validate it's interned
                caps.insert(name);
            }
        }
        EffectSet { caps }
    }

    /// Return capabilities that are in `self` but not in `allowed`.
    pub fn missing_from(&self, allowed: &EffectSet) -> Vec<Name> {
        self.caps
            .iter()
            .filter(|c| !allowed.caps.contains(c))
            .copied()
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.caps.is_empty()
    }
}
