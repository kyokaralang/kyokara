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
mod syntax_kind;

pub use event::{Event, TreeSink};
pub use syntax_kind::SyntaxKind;
