//! Interned name type used throughout the HIR.

use kyokara_intern::Interner;
use kyokara_intern::lasso::Spur;

/// An interned identifier name.
///
/// `Copy + Eq + Hash` — string lookup requires an `&Interner`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Name(pub Spur);

impl Name {
    pub fn new(interner: &mut Interner, s: &str) -> Self {
        Name(interner.intern(s))
    }

    pub fn resolve(self, interner: &Interner) -> &str {
        interner.resolve(self.0)
    }
}

impl std::fmt::Display for Name {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Name({:?})", self.0)
    }
}
