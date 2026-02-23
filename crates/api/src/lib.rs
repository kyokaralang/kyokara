//! `kyokara-api` — Compiler-as-API with JSON serialization.
//!
//! This crate owns all serialization. Internal compiler types stay
//! `serde`-free; instead, `api` defines its own DTO types that mirror
//! the internal structures and derive `Serialize`/`Deserialize`.
//!
//! Outputs (v0.0):
//! - `diagnostics.json`
//! - `typed_ast.json` (placeholder)
//! - `symbol_graph.json` (placeholder)

use serde::Serialize;

/// A serialisable diagnostic for JSON output.
#[derive(Debug, Serialize)]
pub struct DiagnosticDto {
    pub severity: String,
    pub message: String,
    pub file: String,
    pub start: u32,
    pub end: u32,
}

/// Run the full check pipeline on source text and return diagnostics.
pub fn check(_source: &str) -> Vec<DiagnosticDto> {
    // TODO: lex → parse → lower → type-check → collect diagnostics
    Vec::new()
}
