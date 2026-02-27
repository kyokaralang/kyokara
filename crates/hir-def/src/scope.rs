//! Scope data structures for name resolution.
//!
//! A `ScopeTree` is an arena of `ScopeData` nodes with parent pointers.
//! Each scope maps names to definitions.

use kyokara_stdx::FxHashMap;
use la_arena::{Arena, Idx};

use crate::expr::{ExprIdx, PatIdx};
use crate::item_tree::{CapItemIdx, FnItemIdx, TypeItemIdx};
use crate::name::Name;

/// Index into the scope arena.
pub type ScopeIdx = Idx<ScopeData>;

/// A tree of nested scopes.
#[derive(Debug, Default, Clone)]
pub struct ScopeTree {
    pub scopes: Arena<ScopeData>,
    pub root: Option<ScopeIdx>,
}

impl ScopeTree {
    pub fn new_root(&mut self) -> ScopeIdx {
        let idx = self.scopes.alloc(ScopeData {
            parent: None,
            entries: FxHashMap::default(),
        });
        self.root = Some(idx);
        idx
    }

    pub fn new_child(&mut self, parent: ScopeIdx) -> ScopeIdx {
        self.scopes.alloc(ScopeData {
            parent: Some(parent),
            entries: FxHashMap::default(),
        })
    }

    pub fn define(&mut self, scope: ScopeIdx, name: Name, def: ScopeDef) {
        self.scopes[scope]
            .entries
            .entry(name)
            .or_default()
            .push(def);
    }

    /// Look up a name starting from the given scope, walking up parents.
    pub fn lookup(&self, mut scope: ScopeIdx, name: Name) -> Option<&ScopeDef> {
        loop {
            if let Some(defs) = self.scopes[scope].entries.get(&name)
                && let Some(def) = defs.last()
            {
                return Some(def);
            }
            scope = self.scopes[scope].parent?;
        }
    }

    /// Look up a name at a specific usage position.
    ///
    /// The predicate is evaluated from innermost to outermost definitions
    /// in each scope; the first visible definition is returned.
    pub fn lookup_at<F>(
        &self,
        mut scope: ScopeIdx,
        name: Name,
        mut is_visible: F,
    ) -> Option<&ScopeDef>
    where
        F: FnMut(&ScopeDef) -> bool,
    {
        loop {
            if let Some(defs) = self.scopes[scope].entries.get(&name)
                && let Some(def) = defs.iter().rev().find(|def| is_visible(def))
            {
                return Some(def);
            }
            scope = self.scopes[scope].parent?;
        }
    }
}

/// Data for a single scope node.
#[derive(Debug, Clone)]
pub struct ScopeData {
    pub parent: Option<ScopeIdx>,
    pub entries: FxHashMap<Name, Vec<ScopeDef>>,
}

/// What a name resolves to.
#[derive(Debug, Clone)]
pub enum ScopeDef {
    /// A local variable introduced by a pattern binding.
    Local(PatIdx),
    /// A function parameter.
    Param(usize),
    /// A function item.
    Fn(FnItemIdx),
    /// A type item.
    Type(TypeItemIdx),
    /// A capability item.
    Cap(CapItemIdx),
    /// An ADT constructor.
    Constructor {
        type_idx: TypeItemIdx,
        variant_idx: usize,
    },
    /// An imported name.
    Import(usize),
    /// A lambda parameter.
    LambdaParam(ExprIdx),
}
