//! `kyokara-fmt` — Canonical code formatter for Kyokara.
//!
//! Produces a single deterministic style with no configuration.
//! Operates on the lossless rowan CST, preserving comments.
//!
//! Architecture: CST → Doc IR → String (Wadler-Lindig).

pub mod comments;
pub mod doc;
pub mod format;
pub mod print;

use kyokara_syntax::SyntaxNode;

/// The default maximum line width.
const LINE_WIDTH: i32 = 100;

/// Format Kyokara source code, returning the formatted string.
///
/// Parses the source, builds a Doc IR from the CST, and renders it.
/// Comments are preserved. Import declarations are sorted alphabetically.
/// Error/unknown nodes fall back to verbatim text.
pub fn format_source(source: &str) -> String {
    let parse = kyokara_syntax::parse(source);
    let root = SyntaxNode::new_root(parse.green);
    let doc = format::format_node(&root);
    print::print(&doc, LINE_WIDTH)
}
