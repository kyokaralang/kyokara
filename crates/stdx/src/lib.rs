//! `kyokara-stdx` — Utility extensions shared across all Kyokara crates.
//!
//! This is the leaf crate in the dependency graph: it has no Kyokara
//! dependencies. It re-exports commonly used types and provides small
//! helpers that don't warrant their own crate.

pub use rustc_hash::{FxHashMap, FxHashSet};
