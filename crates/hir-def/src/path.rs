//! Module-qualified paths in the HIR.

use crate::name::Name;

/// A dotted path like `Foo.Bar.baz`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Path {
    pub segments: Vec<Name>,
}

impl Path {
    pub fn single(name: Name) -> Self {
        Path {
            segments: vec![name],
        }
    }

    pub fn is_single(&self) -> bool {
        self.segments.len() == 1
    }

    /// The last segment (the "leaf" name).
    pub fn last(&self) -> Option<Name> {
        self.segments.last().copied()
    }
}
