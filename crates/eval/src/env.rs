//! Scoped environment for variable bindings.

use kyokara_hir_def::name::Name;

use crate::value::Value;

/// A stack of scopes, each holding name-value bindings.
#[derive(Debug, Clone)]
pub struct Env {
    scopes: Vec<Vec<(Name, Value)>>,
}

impl Default for Env {
    fn default() -> Self {
        Self::new()
    }
}

impl Env {
    pub fn new() -> Self {
        Env {
            scopes: vec![Vec::new()],
        }
    }

    pub fn push_scope(&mut self) {
        self.scopes.push(Vec::new());
    }

    pub fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    pub fn bind(&mut self, name: Name, value: Value) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.push((name, value));
        }
    }

    /// Look up a name, searching from innermost scope outward.
    pub fn lookup(&self, name: Name) -> Option<&Value> {
        for scope in self.scopes.iter().rev() {
            for (n, v) in scope.iter().rev() {
                if *n == name {
                    return Some(v);
                }
            }
        }
        None
    }
}
