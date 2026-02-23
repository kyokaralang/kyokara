//! The `SyntaxKind` enum — every token and node kind in the Kyokara grammar.
//!
//! This is the single source of truth for the grammar's terminal and
//! non-terminal symbols. The lexer produces token kinds; the parser
//! groups them into node kinds.

/// A tag for every kind of token or syntax node in Kyokara.
///
/// Token kinds (leaves) and node kinds (interior nodes) share the same
/// enum so that `rowan::SyntaxKind` can be implemented as a trivial
/// `From` conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
#[allow(clippy::manual_non_exhaustive)]
pub enum SyntaxKind {
    // — Tokens (leaves) —
    /// End of file.
    Eof = 0,
    /// Unrecognised byte sequence.
    Error,
    /// Whitespace (spaces, tabs, newlines).
    Whitespace,
    /// Line comment (`//`).
    LineComment,
    /// Block comment (`/* … */`).
    BlockComment,

    // Literals
    IntLiteral,
    FloatLiteral,
    StringLiteral,
    CharLiteral,

    // Identifiers & keywords
    Ident,
    // Keywords will be added as the grammar grows.

    // Punctuation / operators (stubs)
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Semicolon,
    Dot,
    Arrow,    // ->
    FatArrow, // =>
    Eq,       // =
    EqEq,     // ==
    Bang,     // !
    BangEq,   // !=
    Plus,
    Minus,
    Star,
    Slash,
    Pipe,     // |
    PipeGt,   // |>
    Amp,      // &
    Question, // ?

    // — Nodes (interior) —
    /// Root node of every source file.
    SourceFile,
    /// A generic error-recovery wrapper.
    ErrorNode,

    // Sentinel — keep last.
    #[doc(hidden)]
    __Last,
}

impl SyntaxKind {
    /// Returns `true` for trivia tokens (whitespace, comments).
    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            Self::Whitespace | Self::LineComment | Self::BlockComment
        )
    }
}
