//! IR function definitions.

use la_arena::{Arena, ArenaMap};

use kyokara_hir_def::name::Name;
use kyokara_hir_ty::effects::EffectSet;
use kyokara_hir_ty::ty::Ty;
use kyokara_span::TextRange;

use crate::block::{Block, BlockId};
use crate::value::{ValueDef, ValueId};

/// Pre/post-condition contracts lowered to value references.
#[derive(Debug, Clone, Default)]
pub struct KirContracts {
    /// `requires` assertions (preconditions).
    pub requires: Vec<ValueId>,
    /// `ensures` assertions (postconditions).
    pub ensures: Vec<ValueId>,
}

/// A function in the KIR.
#[derive(Debug, Clone)]
pub struct KirFunction {
    pub name: Name,
    pub params: Vec<(Name, Ty)>,
    pub closure_capture_tys: Vec<Ty>,
    pub ret_ty: Ty,
    pub effects: EffectSet,
    pub blocks: Arena<Block>,
    pub values: Arena<ValueDef>,
    pub entry_block: BlockId,
    pub contracts: KirContracts,
    /// Maps values back to source text ranges for diagnostics.
    pub source_map: ArenaMap<ValueId, TextRange>,
}
