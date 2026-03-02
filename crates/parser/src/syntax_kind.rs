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
    // ── Tokens (leaves) ──────────────────────────────────────────────

    // Special
    /// End of file.
    Eof = 0,
    /// Unrecognised byte sequence.
    Error,
    /// Whitespace (spaces, tabs, newlines).
    Whitespace,
    /// Line comment (`// …`).
    LineComment,
    /// Block comment (`/* … */`), possibly nested.
    BlockComment,

    // Literals
    /// Integer literal (`42`, `10_000`).
    IntLiteral,
    /// Float literal (`3.14`, `1_000.5`).
    FloatLiteral,
    /// String literal (`"hello"`).
    StringLiteral,
    /// Character literal (`'a'`).
    CharLiteral,

    // Identifier
    /// Identifier (`foo`, `MyType`, `_unused`).
    Ident,

    // ── Keywords ─────────────────────────────────────────────────────
    /// `module`
    ModuleKw,
    /// `import`
    ImportKw,
    /// `as`
    AsKw,
    /// `type`
    TypeKw,
    /// `fn`
    FnKw,
    /// `let`
    LetKw,
    /// `match`
    MatchKw,
    /// `cap`
    CapKw,
    /// `effect`
    EffectKw,
    /// `with`
    WithKw,
    /// `requires`
    RequiresKw,
    /// `ensures`
    EnsuresKw,
    /// `invariant`
    InvariantKw,
    /// `property`
    PropertyKw,
    /// `for`
    ForKw,
    /// `all`
    AllKw,
    /// `where`
    WhereKw,
    /// `pipe`
    PipeKw,
    /// `old`
    OldKw,
    /// `true`
    TrueKw,
    /// `false`
    FalseKw,
    /// `if`
    IfKw,
    /// `else`
    ElseKw,
    /// `return`
    ReturnKw,
    /// `pub`
    PubKw,

    // ── Delimiters ───────────────────────────────────────────────────
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// `,`
    Comma,
    /// `:`
    Colon,
    /// `;`
    Semicolon,
    /// `.`
    Dot,

    // ── Operators ────────────────────────────────────────────────────
    /// `->`
    Arrow,
    /// `<-`
    LeftArrow,
    /// `=>`
    FatArrow,
    /// `=`
    Eq,
    /// `==`
    EqEq,
    /// `!`
    Bang,
    /// `!=`
    BangEq,
    /// `>=`
    GtEq,
    /// `<=`
    LtEq,
    /// `>`
    Gt,
    /// `<`
    Lt,
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// `|`
    Pipe,
    /// `|>`
    PipeGt,
    /// `&`
    Amp,
    /// `?`
    Question,
    /// `&&`
    AmpAmp,
    /// `||`
    PipePipe,
    /// `%`
    Percent,
    /// `^`
    Caret,
    /// `~`
    Tilde,
    /// `<<`
    LtLt,
    /// `>>`
    GtGt,
    /// `_` (text-agnostic wildcard / hole token).
    Underscore,

    // ── Nodes (interior) ─────────────────────────────────────────────

    // Top-level
    /// Root node of every source file.
    SourceFile,
    /// `module Foo`
    ModuleDecl,
    /// `import Foo.Bar`
    ImportDecl,
    /// Dotted path (`Foo.Bar.baz`).
    Path,
    /// `as` rename in imports.
    ImportAlias,

    // Items
    /// `type Foo = …`
    TypeDef,
    /// `fn foo(…) -> … { … }`
    FnDef,
    /// `effect Foo`
    CapDef,
    /// `property foo(…) { … }`
    PropertyDef,
    /// `let x = …`
    LetBinding,

    // Type-def sub-nodes
    /// `{ field: Type, … }`
    RecordFieldList,
    /// `field: Type`
    RecordField,
    /// `| Variant(…) | …`
    VariantList,
    /// `Variant(Type, …)`
    Variant,
    /// Field list inside a variant.
    VariantFieldList,
    /// A single variant field.
    VariantField,

    // Function sub-nodes
    /// `(param: Type, …)`
    ParamList,
    /// `param: Type`
    Param,
    /// `-> Type`
    ReturnType,
    /// `with Cap, …`
    WithClause,
    /// `pipe …`
    PipeClause,
    /// `requires …`
    RequiresClause,
    /// `ensures …`
    EnsuresClause,
    /// `invariant …`
    InvariantClause,

    // Generics
    /// `<T, U>`
    TypeParamList,
    /// `T`
    TypeParam,
    /// `<Int, String>`
    TypeArgList,

    // Type expressions
    /// Named type (`Int`, `Option<T>`).
    NameType,
    /// Function type (`fn(A) -> B`).
    FnType,
    /// Record type (`{ x: Int, y: Int }`).
    RecordType,
    /// Refined type (`{ x: Int | x > 0 }`).
    RefinedType,

    // Expressions
    /// Literal expression (`42`, `"hello"`, `true`).
    LiteralExpr,
    /// Bare identifier expression.
    IdentExpr,
    /// Qualified path expression (`Foo.bar`).
    PathExpr,
    /// Binary operation (`a + b`).
    BinaryExpr,
    /// Unary operation (`!x`, `-x`).
    UnaryExpr,
    /// Function call (`f(x, y)`).
    CallExpr,
    /// Named argument (`name: value`).
    NamedArg,
    /// Argument list.
    ArgList,
    /// Field access (`expr.field`).
    FieldExpr,
    /// Index access (`expr[index]`).
    IndexExpr,
    /// Pipeline (`expr |> f`).
    PipelineExpr,
    /// Error propagation (`expr?`).
    PropagateExpr,
    /// `match expr { … }`
    MatchExpr,
    /// Single match arm.
    MatchArm,
    /// List of match arms.
    MatchArmList,
    /// `if cond { … } else { … }`
    IfExpr,
    /// `{ … }` block.
    BlockExpr,
    /// Record construction (`Foo { x: 1, y: 2 }`).
    RecordExpr,
    /// Field in record expression.
    RecordExprField,
    /// Field list in record expression.
    RecordExprFieldList,
    /// `return expr`
    ReturnExpr,
    /// `_` hole expression.
    HoleExpr,
    /// `old(expr)`
    OldExpr,
    /// Parenthesised expression.
    ParenExpr,
    /// Lambda / closure (`|x| x + 1`).
    LambdaExpr,

    // Patterns
    /// Identifier pattern (`x`).
    IdentPat,
    /// Constructor pattern (`Some(x)`).
    ConstructorPat,
    /// Wildcard pattern (`_`).
    WildcardPat,
    /// Literal pattern (`42`).
    LiteralPat,
    /// Record pattern (`{ x, y }`).
    RecordPat,
    /// Pattern list (inside constructor/record patterns).
    PatList,

    // Property
    /// `(param: T <- gen, ...)` in property def.
    PropertyParamList,
    /// `name: Type <- GenExpr`
    PropertyParam,
    /// `where expr`
    WhereClause,
    /// `for all x: T.` binder.
    ForAllBinder,

    // Recovery
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

    /// Returns `true` for keyword tokens.
    pub fn is_keyword(self) -> bool {
        matches!(
            self,
            Self::ModuleKw
                | Self::ImportKw
                | Self::AsKw
                | Self::TypeKw
                | Self::FnKw
                | Self::LetKw
                | Self::MatchKw
                | Self::CapKw
                | Self::EffectKw
                | Self::WithKw
                | Self::RequiresKw
                | Self::EnsuresKw
                | Self::InvariantKw
                | Self::PropertyKw
                | Self::ForKw
                | Self::AllKw
                | Self::WhereKw
                | Self::PipeKw
                | Self::OldKw
                | Self::TrueKw
                | Self::FalseKw
                | Self::IfKw
                | Self::ElseKw
                | Self::ReturnKw
                | Self::PubKw
        )
    }

    /// Returns `true` for literal tokens.
    pub fn is_literal(self) -> bool {
        matches!(
            self,
            Self::IntLiteral | Self::FloatLiteral | Self::StringLiteral | Self::CharLiteral
        )
    }

    /// Returns `true` for unary prefix operator tokens.
    pub fn is_unary_prefix_operator(self) -> bool {
        matches!(self, Self::Bang | Self::Minus | Self::Tilde)
    }

    /// Returns `true` for binary expression operator tokens.
    ///
    /// This intentionally excludes pipeline (`|>`), which lowers to
    /// `PipelineExpr` instead of `BinaryExpr`.
    pub fn is_binary_operator(self) -> bool {
        matches!(
            self,
            Self::Plus
                | Self::Minus
                | Self::Star
                | Self::Slash
                | Self::Percent
                | Self::EqEq
                | Self::BangEq
                | Self::Lt
                | Self::Gt
                | Self::LtEq
                | Self::GtEq
                | Self::Amp
                | Self::Pipe
                | Self::Caret
                | Self::LtLt
                | Self::GtGt
                | Self::AmpAmp
                | Self::PipePipe
        )
    }

    /// Returns Pratt parser binding power for infix operators.
    ///
    /// Includes pipeline (`|>`) and binary operators.
    pub fn infix_binding_power(self) -> Option<(u8, u8)> {
        match self {
            Self::PipeGt => Some((1, 2)),
            Self::PipePipe => Some((3, 4)),
            Self::AmpAmp => Some((5, 6)),
            Self::EqEq | Self::BangEq => Some((7, 8)),
            Self::Lt | Self::Gt | Self::LtEq | Self::GtEq => Some((9, 10)),
            Self::Pipe => Some((11, 12)),
            Self::Caret => Some((13, 14)),
            Self::Amp => Some((15, 16)),
            Self::LtLt | Self::GtGt => Some((17, 18)),
            Self::Plus | Self::Minus => Some((19, 20)),
            Self::Star | Self::Slash | Self::Percent => Some((21, 22)),
            _ => None,
        }
    }

    /// Look up a keyword kind from its text. Returns `None` for
    /// identifiers that are not keywords.
    pub fn from_keyword(text: &str) -> Option<SyntaxKind> {
        match text {
            "module" => Some(Self::ModuleKw),
            "import" => Some(Self::ImportKw),
            "as" => Some(Self::AsKw),
            "type" => Some(Self::TypeKw),
            "fn" => Some(Self::FnKw),
            "let" => Some(Self::LetKw),
            "match" => Some(Self::MatchKw),
            "cap" => Some(Self::CapKw),
            "effect" => Some(Self::EffectKw),
            "with" => Some(Self::WithKw),
            "requires" => Some(Self::RequiresKw),
            "ensures" => Some(Self::EnsuresKw),
            "invariant" => Some(Self::InvariantKw),
            "property" => Some(Self::PropertyKw),
            "for" => Some(Self::ForKw),
            "all" => Some(Self::AllKw),
            "where" => Some(Self::WhereKw),
            "pipe" => Some(Self::PipeKw),
            "old" => Some(Self::OldKw),
            "true" => Some(Self::TrueKw),
            "false" => Some(Self::FalseKw),
            "if" => Some(Self::IfKw),
            "else" => Some(Self::ElseKw),
            "return" => Some(Self::ReturnKw),
            "pub" => Some(Self::PubKw),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SyntaxKind;

    #[test]
    fn unary_prefix_operators_are_classified() {
        assert!(SyntaxKind::Bang.is_unary_prefix_operator());
        assert!(SyntaxKind::Minus.is_unary_prefix_operator());
        assert!(SyntaxKind::Tilde.is_unary_prefix_operator());
        assert!(!SyntaxKind::Plus.is_unary_prefix_operator());
        assert!(!SyntaxKind::PipeGt.is_unary_prefix_operator());
    }

    #[test]
    fn binary_operators_exclude_pipeline() {
        for kind in [
            SyntaxKind::Plus,
            SyntaxKind::Minus,
            SyntaxKind::Star,
            SyntaxKind::Slash,
            SyntaxKind::Percent,
            SyntaxKind::EqEq,
            SyntaxKind::BangEq,
            SyntaxKind::Lt,
            SyntaxKind::Gt,
            SyntaxKind::LtEq,
            SyntaxKind::GtEq,
            SyntaxKind::Amp,
            SyntaxKind::Pipe,
            SyntaxKind::Caret,
            SyntaxKind::LtLt,
            SyntaxKind::GtGt,
            SyntaxKind::AmpAmp,
            SyntaxKind::PipePipe,
        ] {
            assert!(kind.is_binary_operator(), "{kind:?} should be binary");
        }

        assert!(!SyntaxKind::PipeGt.is_binary_operator());
    }

    #[test]
    fn infix_binding_power_matches_parser_contract() {
        assert_eq!(SyntaxKind::PipeGt.infix_binding_power(), Some((1, 2)));
        assert_eq!(SyntaxKind::PipePipe.infix_binding_power(), Some((3, 4)));
        assert_eq!(SyntaxKind::AmpAmp.infix_binding_power(), Some((5, 6)));
        assert_eq!(SyntaxKind::Plus.infix_binding_power(), Some((19, 20)));
        assert_eq!(SyntaxKind::Percent.infix_binding_power(), Some((21, 22)));
        assert_eq!(SyntaxKind::Ident.infix_binding_power(), None);
    }
}
