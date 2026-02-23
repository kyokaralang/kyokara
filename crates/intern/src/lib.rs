//! `kyokara-intern` — String interning for the Kyokara compiler.
//!
//! Wraps [`lasso`] to provide a global interner that maps strings to
//! compact keys. All identifier and keyword text flows through here
//! so that string comparisons become integer comparisons.

pub use lasso;
pub use smol_str::SmolStr;

use lasso::{Rodeo, Spur};

/// A thread-local string interner.
///
/// In v0.0 we use a simple `Rodeo`. When salsa lands (v0.3) this will
/// be swapped for a concurrent interner.
pub struct Interner {
    rodeo: Rodeo,
}

impl Interner {
    pub fn new() -> Self {
        Self {
            rodeo: Rodeo::default(),
        }
    }

    /// Intern a string, returning its key.
    pub fn intern(&mut self, s: &str) -> Spur {
        self.rodeo.get_or_intern(s)
    }

    /// Look up a previously interned string.
    pub fn resolve(&self, key: Spur) -> &str {
        self.rodeo.resolve(&key)
    }
}

impl Default for Interner {
    fn default() -> Self {
        Self::new()
    }
}
