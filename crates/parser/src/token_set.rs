//! Compact bitset over [`SyntaxKind`] for parser recovery sets.

use crate::SyntaxKind;

/// A fixed-size bitset that can hold any subset of [`SyntaxKind`] values.
///
/// Used to define recovery sets so the parser knows when to stop
/// consuming error tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenSet([u128; 2]);

impl TokenSet {
    pub const EMPTY: TokenSet = TokenSet([0; 2]);

    pub const fn new(kinds: &[SyntaxKind]) -> TokenSet {
        let mut set = TokenSet([0; 2]);
        let mut i = 0;
        while i < kinds.len() {
            set = set.with(kinds[i]);
            i += 1;
        }
        set
    }

    const fn with(self, kind: SyntaxKind) -> TokenSet {
        let bit = kind as u16;
        let word = (bit / 128) as usize;
        let offset = bit % 128;
        let mut inner = self.0;
        inner[word] |= 1u128 << offset;
        TokenSet(inner)
    }

    pub const fn contains(self, kind: SyntaxKind) -> bool {
        let bit = kind as u16;
        let word = (bit / 128) as usize;
        let offset = bit % 128;
        self.0[word] & (1u128 << offset) != 0
    }

    #[allow(dead_code)]
    pub const fn union(self, other: TokenSet) -> TokenSet {
        TokenSet([self.0[0] | other.0[0], self.0[1] | other.0[1]])
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use SyntaxKind::*;

    #[test]
    fn contains_added_kinds() {
        let set = TokenSet::new(&[LParen, RParen, Comma]);
        assert!(set.contains(LParen));
        assert!(set.contains(RParen));
        assert!(set.contains(Comma));
        assert!(!set.contains(LBrace));
    }

    #[test]
    fn union_merges() {
        let a = TokenSet::new(&[LParen]);
        let b = TokenSet::new(&[RParen]);
        let c = a.union(b);
        assert!(c.contains(LParen));
        assert!(c.contains(RParen));
    }

    #[test]
    fn empty_contains_nothing() {
        assert!(!TokenSet::EMPTY.contains(Eof));
        assert!(!TokenSet::EMPTY.contains(Ident));
    }
}
