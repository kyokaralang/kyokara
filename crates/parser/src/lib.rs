//! `kyokara-parser` — Tree-agnostic recursive-descent parser.
//!
//! This crate is deliberately independent of any concrete tree library
//! (rowan, etc.). It defines the [`SyntaxKind`] enum and emits an
//! [`Event`] stream that a downstream crate (`syntax`) converts into a
//! concrete CST.
//!
//! This design (borrowed from rust-analyzer) lets us unit-test the
//! parser without pulling in rowan.

mod event;
mod grammar;
mod input;
mod parser;
mod syntax_kind;
mod token_set;

pub use event::Event;
pub use input::Input;
pub use parser::ParseError;
pub use syntax_kind::SyntaxKind;

/// Parse a pre-processed token input into an event stream.
///
/// This is the main entry point. The `syntax` crate calls this after
/// lexing and building an [`Input`].
pub fn parse(input: &Input) -> (Vec<Event>, Vec<ParseError>) {
    let mut p = parser::Parser::new(input);
    grammar::source_file(&mut p);
    p.finish()
}
