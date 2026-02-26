//! Scoped environment for variable bindings.

use kyokara_hir_def::name::Name;

use crate::value::Value;

/// A flat-stack environment with scope markers for efficient push/pop.
///
/// Instead of `Vec<Vec<(Name, Value)>>`, this uses a single flat `Vec`
/// with a separate scope-boundary stack. This avoids inner Vec allocations
/// on every scope push.
#[derive(Debug, Clone)]
pub struct Env {
    /// All bindings in a flat list; scopes are delimited by `scope_starts`.
    bindings: Vec<(Name, Value)>,
    /// Stack of indices into `bindings` marking where each scope begins.
    scope_starts: Vec<usize>,
}

impl Default for Env {
    fn default() -> Self {
        Self::new()
    }
}

impl Env {
    pub fn new() -> Self {
        Env {
            bindings: Vec::new(),
            scope_starts: vec![0],
        }
    }

    #[inline(always)]
    pub fn push_scope(&mut self) {
        self.scope_starts.push(self.bindings.len());
    }

    #[inline(always)]
    pub fn pop_scope(&mut self) {
        if let Some(start) = self.scope_starts.pop() {
            self.bindings.truncate(start);
        }
    }

    #[inline(always)]
    pub fn bind(&mut self, name: Name, value: Value) {
        self.bindings.push((name, value));
    }

    /// Look up a name, searching from innermost binding outward.
    #[inline(always)]
    pub fn lookup(&self, name: Name) -> Option<&Value> {
        // Search from the end (innermost scope) backward.
        for (n, v) in self.bindings.iter().rev() {
            if *n == name {
                return Some(v);
            }
        }
        None
    }
}
