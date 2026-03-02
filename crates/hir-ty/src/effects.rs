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
    pub effects: FxHashSet<Name>,
}

impl EffectSet {
    /// Build an effect set from `with_effects` TypeRefs on a function signature.
    pub(crate) fn from_with_effects(
        with_effects: &[TypeRef],
        _env: &TyResolutionEnv<'_>,
        _table: &mut UnificationTable,
        interner: &Interner,
    ) -> Self {
        let mut effects = FxHashSet::default();
        for cap_ref in with_effects {
            if let TypeRef::Path { path, .. } = cap_ref
                && let Some(name) = path.last()
            {
                let _ = name.resolve(interner); // validate it's interned
                effects.insert(name);
            }
        }
        EffectSet { effects }
    }

    /// Return capabilities that are in `self` but not in `allowed`.
    pub fn missing_from(&self, allowed: &EffectSet) -> Vec<Name> {
        self.effects
            .iter()
            .filter(|c| !allowed.effects.contains(c))
            .copied()
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }
}
