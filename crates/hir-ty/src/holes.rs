//! Typed hole information collection.
//!
//! When the type checker encounters a `Hole` expression (`_`), it records
//! the expected type and available local variables for IDE/diagnostic use.

use kyokara_hir_def::name::Name;
use kyokara_span::Span;

use crate::effects::EffectSet;
use crate::ty::Ty;

/// Information about a typed hole.
#[derive(Debug, Clone)]
pub struct HoleInfo {
    /// The type expected at the hole site, if known.
    pub expected_type: Option<Ty>,
    /// Local variables available in scope with their types.
    pub available_locals: Vec<(Name, Ty)>,
    /// Span of the hole expression.
    pub span: Span,
    /// Effect constraints of the enclosing function.
    pub effect_constraints: EffectSet,
    /// Optional name from `?name(...)` syntax (future).
    pub name: Option<String>,
}
