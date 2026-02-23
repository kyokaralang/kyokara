//! CST → HIR lowering.
//!
//! This module walks the typed AST from `kyokara-syntax` and produces
//! HIR data structures. Desugaring of `|>` (pipeline) and `?`
//! (propagation) happens here, keeping the CST fully lossless.

// TODO: implement lowering
