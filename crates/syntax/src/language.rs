//! Rowan [`Language`] implementation for Kyokara.

use kyokara_parser::SyntaxKind;

/// The rowan `Language` tag for Kyokara.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KyokaraLanguage {}

impl rowan::Language for KyokaraLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
        assert!(raw.0 < SyntaxKind::__Last as u16);
        // SAFETY: SyntaxKind is repr(u16) and we checked bounds.
        unsafe { std::mem::transmute::<u16, SyntaxKind>(raw.0) }
    }

    fn kind_to_raw(kind: SyntaxKind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind as u16)
    }
}

/// Convenience aliases.
#[allow(dead_code)]
pub type SyntaxNode = rowan::SyntaxNode<KyokaraLanguage>;
#[allow(dead_code)]
pub type SyntaxToken = rowan::SyntaxToken<KyokaraLanguage>;
