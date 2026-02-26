//! Function/expression bodies and CST → HIR lowering.

pub mod lower;

use la_arena::{Arena, ArenaMap};

use crate::expr::{Expr, ExprIdx, PatIdx};
use crate::pat::Pat;
use crate::scope::ScopeTree;

/// A lowered function body containing arenas of expressions and patterns.
#[derive(Debug, Clone)]
pub struct Body {
    pub exprs: Arena<Expr>,
    pub pats: Arena<Pat>,
    /// The root expression of the body.
    pub root: ExprIdx,
    /// Optional `requires` clause expression.
    pub requires: Option<ExprIdx>,
    /// Optional `ensures` clause expression.
    pub ensures: Option<ExprIdx>,
    /// Optional `invariant` clause expression.
    pub invariant: Option<ExprIdx>,
    /// Scope tree for this body.
    pub scopes: ScopeTree,
    /// Map from pattern index to the scope it was introduced in.
    pub pat_scopes: Vec<(PatIdx, crate::scope::ScopeIdx)>,
    /// Map from expression index to the scope it was allocated in.
    pub expr_scopes: ArenaMap<ExprIdx, crate::scope::ScopeIdx>,
    /// Map from expression index to its CST source text range.
    pub expr_source_map: ArenaMap<ExprIdx, kyokara_span::TextRange>,
}
