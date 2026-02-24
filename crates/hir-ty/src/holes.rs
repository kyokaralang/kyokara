//! Typed hole information collection.
//!
//! When the type checker encounters a `Hole` expression (`_`), it records
//! the expected type and available local variables for IDE/diagnostic use.

use kyokara_hir_def::name::Name;

use crate::ty::Ty;

/// Information about a typed hole.
#[derive(Debug, Clone)]
pub struct HoleInfo {
    /// The type expected at the hole site, if known.
    pub expected_type: Option<Ty>,
    /// Local variables available in scope with their types.
    pub available_locals: Vec<(Name, Ty)>,
}
