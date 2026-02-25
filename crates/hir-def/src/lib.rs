//! `kyokara-hir-def` — HIR data definitions.
//!
//! This crate defines the **data types** for the High-level Intermediate
//! Representation: items, bodies, expressions, patterns, types, effects,
//! and holes. It also contains the CST → HIR lowering pass
//! (`body::lower`) and item tree collection (`item_tree::lower`).
//!
//! The type checker lives in `kyokara-hir-ty`; this crate is data-only
//! so that the interpreter (v0.1) can depend on it without pulling in
//! the checker.

pub mod body;
pub mod builtins;
pub mod expr;
pub mod item_tree;
pub mod module_graph;
pub mod name;
pub mod pat;
pub mod path;
pub mod resolver;
pub mod scope;
pub mod type_ref;
