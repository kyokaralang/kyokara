//! `kyokara-span` — Source locations and file identity.
//!
//! Defines [`FileId`], [`Span`], and re-exports `text-size` types
//! ([`TextRange`], [`TextSize`]) used throughout the compiler to track
//! where things come from.

pub use text_size::{TextRange, TextSize};

/// Opaque handle identifying a source file within a compilation session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId(pub u32);

/// A source span: a file plus a range within that file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub file: FileId,
    pub range: TextRange,
}
