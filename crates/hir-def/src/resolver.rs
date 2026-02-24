//! Name resolution engine.
//!
//! `ModuleScope` holds module-level names (items, constructors, imports).
//! `Resolver` combines module scope with local scope chains for full lookup.

use kyokara_stdx::FxHashMap;

use crate::item_tree::{CapItemIdx, FnItemIdx, TypeItemIdx};
use crate::name::Name;
use crate::scope::{ScopeDef, ScopeIdx, ScopeTree};

/// Module-level scope: items + constructors + imports.
#[derive(Debug, Default)]
pub struct ModuleScope {
    /// Top-level function definitions.
    pub functions: FxHashMap<Name, FnItemIdx>,
    /// Type definitions.
    pub types: FxHashMap<Name, TypeItemIdx>,
    /// Capability definitions.
    pub caps: FxHashMap<Name, CapItemIdx>,
    /// ADT constructors: `VariantName -> (type_idx, variant_idx)`.
    pub constructors: FxHashMap<Name, (TypeItemIdx, usize)>,
    /// Imported names: `local_name -> import_index`.
    pub imports: FxHashMap<Name, usize>,
}

/// The full resolver used during body lowering.
///
/// Lookup order: local scopes (innermost→outermost) → module scope.
pub struct Resolver<'a> {
    pub module: &'a ModuleScope,
    pub scope_tree: &'a ScopeTree,
    pub current_scope: Option<ScopeIdx>,
}

/// What a name resolved to.
#[derive(Debug, Clone)]
pub enum ResolvedName {
    /// Found in a local scope.
    Local(ScopeDef),
    /// A function item at module level.
    Fn(FnItemIdx),
    /// A type at module level.
    Type(TypeItemIdx),
    /// A capability at module level.
    Cap(CapItemIdx),
    /// An ADT constructor.
    Constructor {
        type_idx: TypeItemIdx,
        variant_idx: usize,
    },
    /// An import.
    Import(usize),
}

impl<'a> Resolver<'a> {
    pub fn new(
        module: &'a ModuleScope,
        scope_tree: &'a ScopeTree,
        scope: Option<ScopeIdx>,
    ) -> Self {
        Self {
            module,
            scope_tree,
            current_scope: scope,
        }
    }

    /// Look up a single-segment name.
    pub fn resolve_name(&self, name: Name) -> Option<ResolvedName> {
        // 1. Local scopes (innermost → outermost)
        if let Some(scope) = self.current_scope
            && let Some(def) = self.scope_tree.lookup(scope, name)
        {
            return Some(ResolvedName::Local(def.clone()));
        }

        // 2. Module-level items
        if let Some(&idx) = self.module.functions.get(&name) {
            return Some(ResolvedName::Fn(idx));
        }
        if let Some(&idx) = self.module.types.get(&name) {
            return Some(ResolvedName::Type(idx));
        }
        if let Some(&idx) = self.module.caps.get(&name) {
            return Some(ResolvedName::Cap(idx));
        }

        // 3. Constructors
        if let Some(&(type_idx, variant_idx)) = self.module.constructors.get(&name) {
            return Some(ResolvedName::Constructor {
                type_idx,
                variant_idx,
            });
        }

        // 4. Imports
        if let Some(&idx) = self.module.imports.get(&name) {
            return Some(ResolvedName::Import(idx));
        }

        None
    }
}
