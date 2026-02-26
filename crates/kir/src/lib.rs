//! KyokaraIR (KIR) — SSA-based intermediate representation.
//!
//! KIR sits between the typed HIR and backend code generation.
//! It uses block parameters (not phi nodes) and reuses `hir_ty::Ty`
//! directly as its type system.

pub mod block;
pub mod build;
pub mod display;
pub mod function;
pub mod inst;
pub mod lower;
pub mod validate;
pub mod value;

use la_arena::{Arena, Idx};

use crate::function::KirFunction;

/// Index into the module's function arena.
pub type FnId = Idx<KirFunction>;

/// Top-level IR module containing all functions.
#[derive(Debug, Clone)]
pub struct KirModule {
    pub functions: Arena<KirFunction>,
    /// Optional entry-point function.
    pub entry: Option<FnId>,
}

impl KirModule {
    pub fn new() -> Self {
        Self {
            functions: Arena::default(),
            entry: None,
        }
    }
}

impl Default for KirModule {
    fn default() -> Self {
        Self::new()
    }
}
