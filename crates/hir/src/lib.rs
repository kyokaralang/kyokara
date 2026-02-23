//! `kyokara-hir` — High-level facade over the semantic model.
//!
//! This crate is the **public API** for semantic queries. It ties
//! together `hir-def` (data) and `hir-ty` (checking) behind a simple
//! interface that `api` and `cli` consume.
//!
//! When salsa lands (v0.3), the incremental database will live here.

// TODO: expose semantic query facade
