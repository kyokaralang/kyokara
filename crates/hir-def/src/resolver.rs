//! Name resolution engine.
//!
//! `ModuleScope` holds module-level names (items, constructors, imports).
//! `Resolver` combines module scope with local scope chains for full lookup.

use kyokara_stdx::{FxHashMap, FxHashSet};

use kyokara_intern::Interner;

use crate::item_tree::{EffectItemIdx, FnItemIdx, TypeItemIdx};
use crate::name::Name;
use crate::scope::{ScopeDef, ScopeIdx, ScopeTree};

/// Primitive receiver categories (for method dispatch).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrimitiveType {
    String,
    Int,
    Float,
    Bool,
    Char,
}

/// Core stdlib types that must resolve by identity, not by surface name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoreType {
    Option,
    Result,
    List,
    BitSet,
    MutableList,
    MutableMap,
    MutableSet,
    MutableBitSet,
    Deque,
    Seq,
    Map,
    Set,
    ParseError,
}

/// Method receiver identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReceiverKey {
    Any,
    Primitive(PrimitiveType),
    Core(CoreType),
    User(TypeItemIdx),
}

/// Static method owner identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StaticOwnerKey {
    Core(CoreType),
    User(TypeItemIdx),
}

/// Concrete identity of a registered core type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CoreTypeInfo {
    pub type_idx: TypeItemIdx,
    pub type_name: Name,
}

/// Identity registry for core types.
#[derive(Debug, Default, Clone)]
pub struct CoreTypes {
    pub option: Option<CoreTypeInfo>,
    pub result: Option<CoreTypeInfo>,
    pub list: Option<CoreTypeInfo>,
    pub bitset: Option<CoreTypeInfo>,
    pub mutable_list: Option<CoreTypeInfo>,
    pub mutable_map: Option<CoreTypeInfo>,
    pub mutable_set: Option<CoreTypeInfo>,
    pub mutable_bitset: Option<CoreTypeInfo>,
    pub deque: Option<CoreTypeInfo>,
    pub seq: Option<CoreTypeInfo>,
    pub map: Option<CoreTypeInfo>,
    pub set: Option<CoreTypeInfo>,
    pub parse_error: Option<CoreTypeInfo>,
}

impl CoreTypes {
    pub fn get(&self, core: CoreType) -> Option<CoreTypeInfo> {
        match core {
            CoreType::Option => self.option,
            CoreType::Result => self.result,
            CoreType::List => self.list,
            CoreType::BitSet => self.bitset,
            CoreType::MutableList => self.mutable_list,
            CoreType::MutableMap => self.mutable_map,
            CoreType::MutableSet => self.mutable_set,
            CoreType::MutableBitSet => self.mutable_bitset,
            CoreType::Deque => self.deque,
            CoreType::Seq => self.seq,
            CoreType::Map => self.map,
            CoreType::Set => self.set,
            CoreType::ParseError => self.parse_error,
        }
    }

    pub fn set(&mut self, core: CoreType, info: CoreTypeInfo) {
        match core {
            CoreType::Option => self.option = Some(info),
            CoreType::Result => self.result = Some(info),
            CoreType::List => self.list = Some(info),
            CoreType::BitSet => self.bitset = Some(info),
            CoreType::MutableList => self.mutable_list = Some(info),
            CoreType::MutableMap => self.mutable_map = Some(info),
            CoreType::MutableSet => self.mutable_set = Some(info),
            CoreType::MutableBitSet => self.mutable_bitset = Some(info),
            CoreType::Deque => self.deque = Some(info),
            CoreType::Seq => self.seq = Some(info),
            CoreType::Map => self.map = Some(info),
            CoreType::Set => self.set = Some(info),
            CoreType::ParseError => self.parse_error = Some(info),
        }
    }

    pub fn kind_for_idx(&self, type_idx: TypeItemIdx) -> Option<CoreType> {
        [
            CoreType::Option,
            CoreType::Result,
            CoreType::List,
            CoreType::BitSet,
            CoreType::MutableList,
            CoreType::MutableMap,
            CoreType::MutableSet,
            CoreType::MutableBitSet,
            CoreType::Deque,
            CoreType::Seq,
            CoreType::Map,
            CoreType::Set,
            CoreType::ParseError,
        ]
        .into_iter()
        .find(|&core| self.get(core).is_some_and(|info| info.type_idx == type_idx))
    }
}

/// Map user-facing core type names to core categories.
pub fn core_type_from_public_name(name: Name, interner: &Interner) -> Option<CoreType> {
    match name.resolve(interner) {
        "Option" => Some(CoreType::Option),
        "Result" => Some(CoreType::Result),
        "List" => Some(CoreType::List),
        "BitSet" => Some(CoreType::BitSet),
        "MutableList" => Some(CoreType::MutableList),
        "MutableMap" => Some(CoreType::MutableMap),
        "MutableSet" => Some(CoreType::MutableSet),
        "MutableBitSet" => Some(CoreType::MutableBitSet),
        "Deque" => Some(CoreType::Deque),
        "Map" => Some(CoreType::Map),
        "Set" => Some(CoreType::Set),
        "ParseError" => Some(CoreType::ParseError),
        _ => None,
    }
}

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
    pub bitset: Option<Name>,
    pub mutable_list: Option<Name>,
    pub mutable_map: Option<Name>,
    pub mutable_set: Option<Name>,
    pub mutable_bitset: Option<Name>,
    pub deque: Option<Name>,
    pub seq: Option<Name>,
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
    pub effects: FxHashMap<Name, EffectItemIdx>,
    /// ADT constructors: `VariantName -> (type_idx, variant_idx)`.
    pub constructors: FxHashMap<Name, (TypeItemIdx, usize)>,
    /// Imported names: `local_name -> import_index`.
    pub imports: FxHashMap<Name, usize>,
    /// Method definitions: `(receiver_identity, method_name)` → candidate `FnItemIdx`s.
    ///
    /// Most entries contain exactly one method. A small fixed arity family such as
    /// `count()` / `count(predicate)` stores multiple candidates under the same
    /// receiver/name key and is selected mechanically by arity.
    pub methods: FxHashMap<(ReceiverKey, Name), Vec<FnItemIdx>>,
    /// Synthetic modules: `module_name` → `{ fn_name → FnItemIdx }`.
    /// Module-qualified calls like `io.println(s)` resolve through this.
    pub synthetic_modules: FxHashMap<Name, FxHashMap<Name, FnItemIdx>>,
    /// Synthetic module static methods:
    /// `(module_name, type_name, static_method_name)` -> `FnItemIdx`.
    /// Used for nested module-qualified constructor calls like
    /// `collections.Deque.new()`.
    pub synthetic_module_static_methods: FxHashMap<(Name, Name, Name), FnItemIdx>,
    /// Static methods: `(owner_identity, method_name)` → `FnItemIdx`.
    /// Type-owned static methods resolve through this.
    pub static_methods: FxHashMap<(StaticOwnerKey, Name), FnItemIdx>,
    /// Internal lookup table: intrinsic name → FnItemIdx.
    /// Populated by `register_builtin_intrinsics`, used by method/module/static registration.
    /// Not part of the public name resolution — intrinsics are only reachable through
    /// methods, synthetic modules, or static methods.
    pub intrinsic_fn_lookup: FxHashMap<Name, FnItemIdx>,
    /// Identity registry for core stdlib types.
    pub core_types: CoreTypes,
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
    Effect(EffectItemIdx),
    /// An ADT constructor.
    Constructor {
        type_idx: TypeItemIdx,
        variant_idx: usize,
    },
    /// An import.
    Import(usize),
    /// A synthetic module (io, math, fs).
    Module(Name),
    /// A static method target type (e.g., `List` in `collections.List.new()`).
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
        if let Some(&idx) = self.module.effects.get(&name) {
            return Some(ResolvedName::Effect(idx));
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
        if let Some(&idx) = self.module.effects.get(&name) {
            return Some(ResolvedName::Effect(idx));
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
