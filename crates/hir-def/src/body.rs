//! Function/expression bodies and CST → HIR lowering.

pub mod lower;

use la_arena::{Arena, ArenaMap};

use crate::expr::{Expr, ExprIdx, PatIdx};
use crate::name::Name;
use crate::pat::Pat;
use crate::resolver::{ModuleScope, ResolvedName, Resolver};
use crate::scope::ScopeTree;
use crate::scope::{ScopeDef, ScopeIdx};

/// A lowered function body containing arenas of expressions and patterns.
#[derive(Debug, Clone)]
pub struct Body {
    pub exprs: Arena<Expr>,
    pub pats: Arena<Pat>,
    /// The root expression of the body.
    pub root: ExprIdx,
    /// `requires` clause expressions in source order.
    pub requires: Vec<ExprIdx>,
    /// `ensures` clause expressions in source order.
    pub ensures: Vec<ExprIdx>,
    /// `invariant` clause expressions in source order.
    pub invariant: Vec<ExprIdx>,
    /// Scope tree for this body.
    pub scopes: ScopeTree,
    /// Map from pattern index to the scope it was introduced in.
    pub pat_scopes: Vec<(PatIdx, crate::scope::ScopeIdx)>,
    /// Map from expression index to the scope it was allocated in.
    pub expr_scopes: ArenaMap<ExprIdx, crate::scope::ScopeIdx>,
    /// Map from expression index to its CST source text range.
    pub expr_source_map: ArenaMap<ExprIdx, kyokara_span::TextRange>,
    /// Map from pattern index to its CST source text range.
    pub pat_source_map: ArenaMap<PatIdx, kyokara_span::TextRange>,
    /// Metadata for local pattern bindings.
    pub local_binding_meta: ArenaMap<PatIdx, LocalBindingMeta>,
}

/// Origin kind of a local binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalBindingOrigin {
    LetPattern,
    MatchArmPattern,
    ForPattern,
    LambdaParam,
    ContractResult,
}

/// Source metadata for a local binding introduced in a body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalBindingMeta {
    pub origin: LocalBindingOrigin,
    pub decl_range: kyokara_span::TextRange,
    pub scope: ScopeIdx,
    pub slot: usize,
    pub mutable: bool,
}

/// Resolved local access coordinates for runtime lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalSlotRef {
    pub depth: usize,
    pub slot: usize,
}

/// Canonical usage-position name-resolution output for body consumers.
#[derive(Debug, Clone)]
pub struct ResolvedNameAt {
    pub resolved: ResolvedName,
    pub local_binding: Option<(PatIdx, LocalBindingMeta)>,
}

impl Body {
    /// Resolve a name at an expression usage site with canonical
    /// position-aware local shadowing.
    pub fn resolve_name_at(
        &self,
        module_scope: &ModuleScope,
        expr_idx: ExprIdx,
        name: Name,
    ) -> Option<ResolvedNameAt> {
        let usage_scope = self.expr_scopes.get(expr_idx).copied().or(self.scopes.root);
        let usage_start = self.expr_source_map.get(expr_idx).map(|r| r.start());
        let resolver = Resolver::new(module_scope, &self.scopes, usage_scope);
        let resolved = resolver.resolve_name_at(name, |def| match def {
            ScopeDef::Local(pat_idx) => {
                if let (Some(usage_start), Some(meta)) =
                    (usage_start, self.local_binding_meta.get(*pat_idx))
                {
                    match meta.origin {
                        LocalBindingOrigin::LetPattern => meta.decl_range.end() <= usage_start,
                        _ => meta.decl_range.start() <= usage_start,
                    }
                } else {
                    true
                }
            }
            // Parameters shadow throughout the function body.
            ScopeDef::Param(_) => true,
            ScopeDef::LambdaParam(_) => true,
            _ => true,
        })?;

        let local_binding = match resolved {
            ResolvedName::Local(ScopeDef::Local(pat_idx)) => self
                .local_binding_meta
                .get(pat_idx)
                .copied()
                .map(|meta| (pat_idx, meta)),
            _ => None,
        };

        Some(ResolvedNameAt {
            resolved,
            local_binding,
        })
    }

    /// Resolve a local or parameter usage to runtime `(depth, slot)` coordinates.
    pub fn resolve_local_access_at(
        &self,
        module_scope: &ModuleScope,
        expr_idx: ExprIdx,
        name: Name,
    ) -> Option<LocalSlotRef> {
        let usage_scope = self
            .expr_scopes
            .get(expr_idx)
            .copied()
            .or(self.scopes.root)?;
        let resolved = self.resolve_name_at(module_scope, expr_idx, name)?;
        match resolved.resolved {
            ResolvedName::Local(ScopeDef::Local(pat_idx)) => {
                let meta = self.local_binding_meta.get(pat_idx)?;
                let depth = self.scope_distance(usage_scope, meta.scope)?;
                Some(LocalSlotRef {
                    depth,
                    slot: meta.slot,
                })
            }
            ResolvedName::Local(ScopeDef::Param(slot)) => {
                let depth = self.scope_distance(usage_scope, self.scopes.root?)?;
                Some(LocalSlotRef { depth, slot })
            }
            _ => None,
        }
    }

    fn scope_distance(&self, mut usage_scope: ScopeIdx, target_scope: ScopeIdx) -> Option<usize> {
        let mut depth = 0;
        loop {
            if usage_scope == target_scope {
                return Some(depth);
            }
            usage_scope = self.scopes.scopes[usage_scope].parent?;
            depth += 1;
        }
    }
}
