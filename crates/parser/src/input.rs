//! Token input source — maps non-trivia indices to raw token indices.
//!
//! The parser only sees non-trivia tokens. The [`Input`] type pre-processes
//! the raw token stream so the parser can index by non-trivia position.

use crate::SyntaxKind;

/// Pre-processed token input for the parser.
///
/// Stores the kinds of all raw tokens and maintains a mapping from
/// non-trivia index to raw index.
pub struct Input {
    /// All raw token kinds (including trivia).
    raw_kinds: Vec<SyntaxKind>,
    /// Maps non-trivia index → raw index.
    non_trivia_to_raw: Vec<u32>,
    /// Whether there was a line break in trivia immediately before each
    /// non-trivia token.
    line_break_before_non_trivia: Vec<bool>,
}

impl Input {
    /// Build an `Input` from an iterator of raw token kinds.
    pub fn new(raw_kinds: Vec<SyntaxKind>) -> Input {
        let non_trivia_count = raw_kinds.iter().filter(|k| !k.is_trivia()).count();
        Self::new_with_line_breaks(raw_kinds, vec![false; non_trivia_count])
    }

    /// Build an `Input` with explicit line-break-before metadata for each
    /// non-trivia token.
    pub fn new_with_line_breaks(
        raw_kinds: Vec<SyntaxKind>,
        line_break_before_non_trivia: Vec<bool>,
    ) -> Input {
        let non_trivia_to_raw: Vec<u32> = raw_kinds
            .iter()
            .enumerate()
            .filter(|(_, k)| !k.is_trivia())
            .map(|(i, _)| i as u32)
            .collect();
        assert_eq!(
            non_trivia_to_raw.len(),
            line_break_before_non_trivia.len(),
            "line-break metadata must match non-trivia token count"
        );
        Input {
            raw_kinds,
            non_trivia_to_raw,
            line_break_before_non_trivia,
        }
    }

    /// The kind of the non-trivia token at `index`.
    /// Returns `Eof` if `index` is out of range.
    pub fn kind(&self, index: usize) -> SyntaxKind {
        self.non_trivia_to_raw
            .get(index)
            .map(|&raw| self.raw_kinds[raw as usize])
            .unwrap_or(SyntaxKind::Eof)
    }

    /// Total number of non-trivia tokens.
    pub fn len(&self) -> usize {
        self.non_trivia_to_raw.len()
    }

    /// Returns `true` if there are no non-trivia tokens.
    pub fn is_empty(&self) -> bool {
        self.non_trivia_to_raw.is_empty()
    }

    /// Returns whether there is a line break in trivia before non-trivia token
    /// `index`.
    pub fn line_break_before(&self, index: usize) -> bool {
        self.line_break_before_non_trivia
            .get(index)
            .copied()
            .unwrap_or(false)
    }

    /// The raw index corresponding to non-trivia token `index`.
    pub fn raw_index(&self, index: usize) -> usize {
        self.non_trivia_to_raw[index] as usize
    }

    /// The raw token kind at `raw_index`.
    pub fn raw_kind(&self, raw_index: usize) -> SyntaxKind {
        self.raw_kinds
            .get(raw_index)
            .copied()
            .unwrap_or(SyntaxKind::Eof)
    }

    /// Total number of raw tokens.
    pub fn raw_len(&self) -> usize {
        self.raw_kinds.len()
    }
}
