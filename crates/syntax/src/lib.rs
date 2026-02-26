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

pub mod ast;
mod bridge;
mod language;
pub mod lexer;

pub use language::{KyokaraLanguage, SyntaxNode, SyntaxToken};
pub use lexer::{LexToken, lex};

/// The result of parsing source text.
pub struct Parse {
    pub green: rowan::GreenNode,
    pub errors: Vec<kyokara_parser::ParseError>,
}

/// Parse source text into a lossless CST.
///
/// This is the main entry point for the syntax crate. It lexes, parses,
/// and builds a rowan green tree in one shot.
pub fn parse(source: &str) -> Parse {
    // 1. Lex into raw tokens.
    let tokens = lexer::lex(source);

    // 2. Build parser input (trivia-filtered view).
    let raw_kinds: Vec<SyntaxKind> = tokens.iter().map(|t| t.kind).collect();
    let input = kyokara_parser::Input::new(raw_kinds);

    // 3. Run the parser to get events.
    let (events, mut errors) = kyokara_parser::parse(&input);

    // 4. Compute byte offsets for parse errors from token positions.
    let mut byte_offsets: Vec<u32> = Vec::with_capacity(tokens.len() + 1);
    let mut offset = 0u32;
    for tok in &tokens {
        byte_offsets.push(offset);
        offset += tok.text.len() as u32;
    }
    byte_offsets.push(offset);

    for error in &mut errors {
        let pos = error.token_pos;
        if pos < input.len() {
            let raw = input.raw_index(pos);
            error.range_start = byte_offsets[raw];
            error.range_end = byte_offsets[raw] + tokens[raw].text.len() as u32;
        } else {
            // Past end — point to source end.
            error.range_start = offset;
            error.range_end = offset;
        }
    }

    // 5. Build the rowan green tree.
    let green = bridge::build_tree(events, &tokens);

    Parse { green, errors }
}
