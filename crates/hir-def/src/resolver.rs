//! Name resolution engine.
//!
//! `ModuleScope` holds module-level names (items, constructors, imports).
//! `Resolver` combines module scope with local scope chains for full lookup.

use kyokara_stdx::{FxHashMap, FxHashSet};

use crate::item_tree::{CapItemIdx, FnItemIdx, TypeItemIdx};
use crate::name::Name;
use crate::scope::{ScopeDef, ScopeIdx, ScopeTree};

/// Cached names for built-in primitive types, used during method resolution
/// so that type inference can map `Ty::String` → `Name("String")` without
/// requiring a mutable interner reference.
#[derive(Debug, Default, Clone)]
pub struct WellKnownNames {
    pub string: Option<Name>,
    pub int: Option<Name>,
    pub float: Option<Name>,
    pub bool_: Option<Name>,
    pub char_: Option<Name>,
    pub list: Option<Name>,
    pub map: Option<Name>,
    pub set: Option<Name>,
}

/// Module-level scope: items + constructors + imports.
#[derive(Debug, Default, Clone)]
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
    /// Method definitions: `(receiver_type_name, method_name)` → `FnItemIdx`.
    pub methods: FxHashMap<(Name, Name), FnItemIdx>,
    /// Synthetic modules: `module_name` → `{ fn_name → FnItemIdx }`.
    /// Module-qualified calls like `io.println(s)` resolve through this.
    pub synthetic_modules: FxHashMap<Name, FxHashMap<Name, FnItemIdx>>,
    /// Static methods: `(type_name, method_name)` → `FnItemIdx`.
    /// Type-namespaced constructors like `List.new()` resolve through this.
    pub static_methods: FxHashMap<(Name, Name), FnItemIdx>,
    /// Internal lookup table: intrinsic name → FnItemIdx.
    /// Populated by `register_builtin_intrinsics`, used by method/module/static registration.
    /// Not part of the public name resolution — intrinsics are only reachable through
    /// methods, synthetic modules, or static methods.
    pub intrinsic_fn_lookup: FxHashMap<Name, FnItemIdx>,
    /// Cached primitive type names for method resolution.
    pub well_known_names: WellKnownNames,
    /// Synthetic modules that have been explicitly imported (e.g., `import io`).
    /// Module-qualified calls only resolve if the module name is in this set.
    pub imported_modules: FxHashSet<Name>,
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
    /// A synthetic module (io, math, fs).
    Module(Name),
    /// A static method target type (e.g., `List` in `List.new()`).
    StaticMethodType(Name),
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

        // 2b. Synthetic modules (io, math, fs) — only if explicitly imported.
        if self.module.imported_modules.contains(&name)
            && self.module.synthetic_modules.contains_key(&name)
        {
            return Some(ResolvedName::Module(name));
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

    /// Look up a name with usage-position visibility for locals.
    ///
    /// The `local_visible` predicate decides whether each candidate local
    /// scope definition is visible at the usage site.
    pub fn resolve_name_at<F>(&self, name: Name, mut local_visible: F) -> Option<ResolvedName>
    where
        F: FnMut(&ScopeDef) -> bool,
    {
        // 1. Local scopes (innermost → outermost), position-aware.
        if let Some(scope) = self.current_scope
            && let Some(def) = self
                .scope_tree
                .lookup_at(scope, name, |def| local_visible(def))
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

        // 2b. Synthetic modules — only if explicitly imported.
        if self.module.imported_modules.contains(&name)
            && self.module.synthetic_modules.contains_key(&name)
        {
            return Some(ResolvedName::Module(name));
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
