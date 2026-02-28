//! Block ordering and structured control flow recovery.

use kyokara_kir::block::{BlockId, Terminator};
use kyokara_kir::function::KirFunction;
use rustc_hash::{FxHashMap, FxHashSet};

/// Compute reverse postorder of blocks starting from entry.
pub fn reverse_postorder(func: &KirFunction) -> Vec<BlockId> {
    let mut visited = FxHashSet::default();
    let mut postorder = Vec::new();

    fn dfs(
        block: BlockId,
        func: &KirFunction,
        visited: &mut FxHashSet<BlockId>,
        postorder: &mut Vec<BlockId>,
    ) {
        if !visited.insert(block) {
            return;
        }

        let blk = &func.blocks[block];
        if let Some(term) = &blk.terminator {
            match term {
                Terminator::Return(_) | Terminator::Unreachable => {}
                Terminator::Jump(target) => {
                    dfs(target.block, func, visited, postorder);
                }
                Terminator::Branch {
                    then_target,
                    else_target,
                    ..
                } => {
                    dfs(then_target.block, func, visited, postorder);
                    dfs(else_target.block, func, visited, postorder);
                }
                Terminator::Switch { cases, default, .. } => {
                    for case in cases {
                        dfs(case.target.block, func, visited, postorder);
                    }
                    if let Some(def) = default {
                        dfs(def.block, func, visited, postorder);
                    }
                }
            }
        }

        postorder.push(block);
    }

    dfs(func.entry_block, func, &mut visited, &mut postorder);
    postorder.reverse();
    postorder
}

/// Build a map from BlockId to its index in the RPO ordering.
pub fn rpo_index_map(rpo: &[BlockId]) -> FxHashMap<BlockId, usize> {
    rpo.iter().enumerate().map(|(i, &b)| (b, i)).collect()
}
