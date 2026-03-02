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
    LambdaParam,
    ContractResult,
}

/// Source metadata for a local binding introduced in a body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalBindingMeta {
    pub origin: LocalBindingOrigin,
    pub decl_range: kyokara_span::TextRange,
    pub scope: ScopeIdx,
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
                    meta.decl_range.start() <= usage_start
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
}
