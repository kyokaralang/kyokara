//! `kyokara-syntax` — Lossless concrete syntax tree (CST).
//!
//! This crate ties together:
//! - A [`logos`]-based lexer that tokenises Kyokara source text.
//! - The tree-agnostic parser from `kyokara-parser`.
//! - [`rowan`] to build a lossless green/red tree from parser events.
//! - Typed AST wrapper types for convenient, safe traversal.
//!
//! Downstream crates (`hir-def`, `hir`) work with the typed AST
//! wrappers; they never touch rowan directly.

pub use kyokara_parser::SyntaxKind;

mod language;
pub mod lexer;

pub use language::KyokaraLanguage;
pub use lexer::{LexToken, lex};

/// Parse source text into a CST.
///
/// This is the main entry point for the syntax crate. It lexes, parses,
/// and builds a rowan green tree in one shot.
pub fn parse(_source: &str) -> rowan::GreenNode {
    // TODO: lex → parse → build green tree
    rowan::GreenNode::new(rowan::SyntaxKind(SyntaxKind::SourceFile as u16), [])
}
