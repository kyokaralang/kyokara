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
    /// `cap Foo { … }`
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
