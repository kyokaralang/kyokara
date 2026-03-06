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

    /// Look up a value by lexical `(depth, slot)` coordinates.
    #[inline(always)]
    pub fn lookup_slot(&self, depth: usize, slot: usize) -> Option<&Value> {
        let scope_idx = self.scope_starts.len().checked_sub(depth + 1)?;
        let binding_idx = self.scope_starts[scope_idx].checked_add(slot)?;
        let scope_end = self
            .scope_starts
            .get(scope_idx + 1)
            .copied()
            .unwrap_or(self.bindings.len());
        if binding_idx >= scope_end {
            return None;
        }
        self.bindings.get(binding_idx).map(|(_, value)| value)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use kyokara_intern::Interner;

    use super::*;

    #[test]
    fn lookup_slot_reads_current_and_outer_scopes() {
        let mut interner = Interner::new();
        let outer = Name::new(&mut interner, "outer");
        let inner = Name::new(&mut interner, "inner");

        let mut env = Env::new();
        env.push_scope();
        env.bind(outer, Value::Int(10));
        env.push_scope();
        env.bind(inner, Value::Int(20));

        assert_eq!(env.lookup_slot(0, 0), Some(&Value::Int(20)));
        assert_eq!(env.lookup_slot(1, 0), Some(&Value::Int(10)));
    }

    #[test]
    fn lookup_slot_returns_none_for_out_of_bounds_slot() {
        let mut interner = Interner::new();
        let x = Name::new(&mut interner, "x");

        let mut env = Env::new();
        env.push_scope();
        env.bind(x, Value::Int(1));

        assert_eq!(env.lookup_slot(0, 1), None);
        assert_eq!(env.lookup_slot(1, 0), None);
    }
}
