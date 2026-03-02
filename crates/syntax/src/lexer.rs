//! Logos-based lexer for Kyokara source text.
//!
//! Produces a lossless stream of `LexToken`s — every byte in the input
//! is accounted for by exactly one token. Whitespace and comments are
//! preserved as explicit tokens for the lossless CST.

use kyokara_parser::SyntaxKind;
use logos::Logos;

// ── Internal logos token ─────────────────────────────────────────────

/// Callback for nested block comments (`/* … /* … */ … */`).
///
/// Called by logos after matching the opening `/*`. We continue scanning
/// until the nesting depth returns to zero.
fn block_comment(lex: &mut logos::Lexer<'_, Token>) -> logos::FilterResult<(), ()> {
    let rest = lex.remainder();
    let mut depth: u32 = 1;
    let mut chars = rest.chars();
    let mut consumed = 0;

    while depth > 0 {
        match chars.next() {
            Some('/') => {
                consumed += 1;
                if chars.as_str().starts_with('*') {
                    chars.next();
                    consumed += 1;
                    depth += 1;
                }
            }
            Some('*') => {
                consumed += 1;
                if chars.as_str().starts_with('/') {
                    chars.next();
                    consumed += 1;
                    depth -= 1;
                }
            }
            Some(c) => {
                consumed += c.len_utf8();
            }
            None => {
                // Unterminated block comment — consume everything.
                lex.bump(rest.len());
                return logos::FilterResult::Emit(());
            }
        }
    }
    lex.bump(consumed);
    logos::FilterResult::Emit(())
}

#[derive(Logos)]
#[logos(skip "")]
enum Token {
    // ── Whitespace & comments ────────────────────────────────────────
    #[regex(r"[ \t\n\r]+")]
    Whitespace,

    #[regex(r"//[^\n]*")]
    LineComment,

    #[token("/*", block_comment)]
    BlockComment,

    // ── Literals ─────────────────────────────────────────────────────
    // Float must come before int so logos prefers the longer match.
    #[regex(r"[0-9][0-9_]*\.[0-9][0-9_]*")]
    FloatLiteral,

    #[regex(r"[0-9][0-9_]*")]
    IntLiteral,

    #[regex(r#""([^"\\]|\\.)*""#)]
    StringLiteral,

    #[regex(r"'([^'\\]|\\.)'")]
    CharLiteral,

    // ── Identifier (keywords disambiguated after match) ──────────────
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*")]
    Ident,

    // ── Multi-char operators (ordered longest-first) ─────────────────
    #[token("&&")]
    AmpAmp,
    #[token("||")]
    PipePipe,
    #[token("|>")]
    PipeGt,
    #[token("->")]
    Arrow,
    #[token("=>")]
    FatArrow,
    #[token("==")]
    EqEq,
    #[token("!=")]
    BangEq,
    #[token(">=")]
    GtEq,
    #[token("<-")]
    LeftArrow,
    #[token("<=")]
    LtEq,
    #[token("<<")]
    LtLt,
    #[token(">>")]
    GtGt,

    // ── Single-char operators ────────────────────────────────────────
    #[token("=")]
    Eq,
    #[token("!")]
    Bang,
    #[token(">")]
    Gt,
    #[token("<")]
    Lt,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token("|")]
    Pipe,
    #[token("&")]
    Amp,
    #[token("^")]
    Caret,
    #[token("~")]
    Tilde,
    #[token("?")]
    Question,

    // ── Delimiters ───────────────────────────────────────────────────
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token(",")]
    Comma,
    #[token(":")]
    Colon,
    #[token(";")]
    Semicolon,
    #[token(".")]
    Dot,
}

impl Token {
    /// Map the internal logos token to a `SyntaxKind`.
    fn into_syntax_kind(self, text: &str) -> SyntaxKind {
        match self {
            Token::Whitespace => SyntaxKind::Whitespace,
            Token::LineComment => SyntaxKind::LineComment,
            Token::BlockComment => SyntaxKind::BlockComment,
            Token::FloatLiteral => SyntaxKind::FloatLiteral,
            Token::IntLiteral => SyntaxKind::IntLiteral,
            Token::StringLiteral => SyntaxKind::StringLiteral,
            Token::CharLiteral => SyntaxKind::CharLiteral,
            Token::Ident => {
                if text == "_" {
                    SyntaxKind::Underscore
                } else {
                    SyntaxKind::from_keyword(text).unwrap_or(SyntaxKind::Ident)
                }
            }
            Token::AmpAmp => SyntaxKind::AmpAmp,
            Token::PipePipe => SyntaxKind::PipePipe,
            Token::PipeGt => SyntaxKind::PipeGt,
            Token::Arrow => SyntaxKind::Arrow,
            Token::FatArrow => SyntaxKind::FatArrow,
            Token::EqEq => SyntaxKind::EqEq,
            Token::BangEq => SyntaxKind::BangEq,
            Token::GtEq => SyntaxKind::GtEq,
            Token::LeftArrow => SyntaxKind::LeftArrow,
            Token::LtEq => SyntaxKind::LtEq,
            Token::LtLt => SyntaxKind::LtLt,
            Token::GtGt => SyntaxKind::GtGt,
            Token::Eq => SyntaxKind::Eq,
            Token::Bang => SyntaxKind::Bang,
            Token::Gt => SyntaxKind::Gt,
            Token::Lt => SyntaxKind::Lt,
            Token::Plus => SyntaxKind::Plus,
            Token::Minus => SyntaxKind::Minus,
            Token::Star => SyntaxKind::Star,
            Token::Slash => SyntaxKind::Slash,
            Token::Percent => SyntaxKind::Percent,
            Token::Pipe => SyntaxKind::Pipe,
            Token::Amp => SyntaxKind::Amp,
            Token::Caret => SyntaxKind::Caret,
            Token::Tilde => SyntaxKind::Tilde,
            Token::Question => SyntaxKind::Question,
            Token::LParen => SyntaxKind::LParen,
            Token::RParen => SyntaxKind::RParen,
            Token::LBrace => SyntaxKind::LBrace,
            Token::RBrace => SyntaxKind::RBrace,
            Token::LBracket => SyntaxKind::LBracket,
            Token::RBracket => SyntaxKind::RBracket,
            Token::Comma => SyntaxKind::Comma,
            Token::Colon => SyntaxKind::Colon,
            Token::Semicolon => SyntaxKind::Semicolon,
            Token::Dot => SyntaxKind::Dot,
        }
    }
}

// ── Public API ───────────────────────────────────────────────────────

/// A single lexed token with its kind and source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LexToken<'src> {
    pub kind: SyntaxKind,
    pub text: &'src str,
}

/// Tokenize `source` into a lossless stream of `LexToken`s.
///
/// Every byte in `source` is covered by exactly one token. Unknown bytes
/// produce `SyntaxKind::Error` tokens.
pub fn lex(source: &str) -> Vec<LexToken<'_>> {
    let mut tokens = Vec::new();
    let mut lexer = Token::lexer(source);

    while let Some(result) = lexer.next() {
        match result {
            Ok(tok) => {
                let text = lexer.slice();
                tokens.push(LexToken {
                    kind: tok.into_syntax_kind(text),
                    text,
                });
            }
            Err(()) => {
                // Unknown byte — emit a single-char Error token.
                tokens.push(LexToken {
                    kind: SyntaxKind::Error,
                    text: lexer.slice(),
                });
            }
        }
    }

    tokens
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use SyntaxKind::*;

    /// Helper: lex and return `(kind, text)` pairs.
    fn lex_kinds(src: &str) -> Vec<(SyntaxKind, &str)> {
        lex(src).into_iter().map(|t| (t.kind, t.text)).collect()
    }

    // ── Individual token kinds ───────────────────────────────────────

    #[test]
    fn keywords() {
        let kws = [
            ("module", ModuleKw),
            ("import", ImportKw),
            ("as", AsKw),
            ("type", TypeKw),
            ("fn", FnKw),
            ("let", LetKw),
            ("match", MatchKw),
            ("cap", CapKw),
            ("effect", EffectKw),
            ("with", WithKw),
            ("contract", ContractKw),
            ("requires", RequiresKw),
            ("ensures", EnsuresKw),
            ("invariant", InvariantKw),
            ("property", PropertyKw),
            ("for", ForKw),
            ("all", AllKw),
            ("where", WhereKw),
            ("pipe", PipeKw),
            ("old", OldKw),
            ("true", TrueKw),
            ("false", FalseKw),
            ("if", IfKw),
            ("else", ElseKw),
            ("return", ReturnKw),
        ];
        for (text, expected) in kws {
            let tokens = lex_kinds(text);
            assert_eq!(tokens, [(expected, text)], "keyword: {text}");
        }
    }

    #[test]
    fn keyword_vs_ident() {
        // Prefixes/suffixes of keywords are identifiers, not keywords.
        let idents = ["modules", "letx", "fns", "iff", "returned", "trueish"];
        for text in idents {
            let tokens = lex_kinds(text);
            assert_eq!(tokens, [(Ident, text)], "should be ident: {text}");
        }
    }

    #[test]
    fn delimiters() {
        let tokens = lex_kinds("( ) { } [ ] , : ; .");
        let kinds: Vec<_> = tokens.iter().filter(|(k, _)| *k != Whitespace).collect();
        assert_eq!(
            kinds,
            vec![
                &(LParen, "("),
                &(RParen, ")"),
                &(LBrace, "{"),
                &(RBrace, "}"),
                &(LBracket, "["),
                &(RBracket, "]"),
                &(Comma, ","),
                &(Colon, ":"),
                &(Semicolon, ";"),
                &(Dot, "."),
            ]
        );
    }

    #[test]
    fn operators() {
        let cases = [
            ("->", Arrow),
            ("<-", LeftArrow),
            ("=>", FatArrow),
            ("=", Eq),
            ("==", EqEq),
            ("!", Bang),
            ("!=", BangEq),
            (">=", GtEq),
            ("<=", LtEq),
            (">", Gt),
            ("<", Lt),
            ("+", Plus),
            ("-", Minus),
            ("*", Star),
            ("/", Slash),
            ("%", Percent),
            ("|", Pipe),
            ("|>", PipeGt),
            ("||", PipePipe),
            ("&", Amp),
            ("&&", AmpAmp),
            ("^", Caret),
            ("~", Tilde),
            ("<<", LtLt),
            (">>", GtGt),
            ("?", Question),
        ];
        for (text, expected) in cases {
            let tokens = lex_kinds(text);
            assert_eq!(tokens, [(expected, text)], "operator: {text}");
        }
    }

    // ── Longest-match operator disambiguation ────────────────────────

    #[test]
    fn operator_disambiguation() {
        // `|>` should not be `|` + `>`
        assert_eq!(lex_kinds("|>"), [(PipeGt, "|>")]);
        // `==` should not be `=` + `=`
        assert_eq!(lex_kinds("=="), [(EqEq, "==")]);
        // `!=` should not be `!` + `=`
        assert_eq!(lex_kinds("!="), [(BangEq, "!=")]);
        // `->` should not be `-` + `>`
        assert_eq!(lex_kinds("->"), [(Arrow, "->")]);
        // `<-` should not be `<` + `-`
        assert_eq!(lex_kinds("<-"), [(LeftArrow, "<-")]);
        // `=>` should not be `=` + `>`
        assert_eq!(lex_kinds("=>"), [(FatArrow, "=>")]);
        // `>=` should not be `>` + `=`
        assert_eq!(lex_kinds(">="), [(GtEq, ">=")]);
        // `<=` should not be `<` + `=`
        assert_eq!(lex_kinds("<="), [(LtEq, "<=")]);
        // `&&` should not be `&` + `&`
        assert_eq!(lex_kinds("&&"), [(AmpAmp, "&&")]);
        // `||` should not be `|` + `|`
        assert_eq!(lex_kinds("||"), [(PipePipe, "||")]);
        // `<<` should not be `<` + `<`
        assert_eq!(lex_kinds("<<"), [(LtLt, "<<")]);
        // `>>` should not be `>` + `>`
        assert_eq!(lex_kinds(">>"), [(GtGt, ">>")]);
    }

    // ── Literals ─────────────────────────────────────────────────────

    #[test]
    fn int_literals() {
        assert_eq!(lex_kinds("42"), [(IntLiteral, "42")]);
        assert_eq!(lex_kinds("10_000"), [(IntLiteral, "10_000")]);
        assert_eq!(lex_kinds("0"), [(IntLiteral, "0")]);
    }

    #[test]
    fn float_literals() {
        assert_eq!(lex_kinds("3.14"), [(FloatLiteral, "3.14")]);
        assert_eq!(lex_kinds("1_000.5"), [(FloatLiteral, "1_000.5")]);
        assert_eq!(lex_kinds("0.0"), [(FloatLiteral, "0.0")]);
    }

    #[test]
    fn int_dot_ident() {
        // `42.field` should lex as int + dot + ident (not float).
        let tokens = lex_kinds("42.field");
        assert_eq!(tokens, [(IntLiteral, "42"), (Dot, "."), (Ident, "field"),]);
    }

    #[test]
    fn string_literals() {
        assert_eq!(lex_kinds(r#""hello""#), [(StringLiteral, r#""hello""#)]);
        // Escaped quote inside string
        assert_eq!(
            lex_kinds(r#""he said \"hi\"""#),
            [(StringLiteral, r#""he said \"hi\"""#)]
        );
        // Empty string
        assert_eq!(lex_kinds(r#""""#), [(StringLiteral, r#""""#)]);
    }

    #[test]
    fn char_literals() {
        assert_eq!(lex_kinds("'a'"), [(CharLiteral, "'a'")]);
        assert_eq!(lex_kinds(r"'\n'"), [(CharLiteral, r"'\n'")]);
    }

    // ── Comments ─────────────────────────────────────────────────────

    #[test]
    fn line_comment() {
        let tokens = lex_kinds("// hello world\n42");
        assert_eq!(
            tokens,
            [
                (LineComment, "// hello world"),
                (Whitespace, "\n"),
                (IntLiteral, "42"),
            ]
        );
    }

    #[test]
    fn block_comment_simple() {
        assert_eq!(
            lex_kinds("/* comment */"),
            [(BlockComment, "/* comment */")]
        );
    }

    #[test]
    fn block_comment_nested() {
        let src = "/* outer /* inner */ still outer */";
        assert_eq!(lex_kinds(src), [(BlockComment, src)]);
    }

    #[test]
    fn block_comment_deeply_nested() {
        let src = "/* a /* b /* c */ d */ e */";
        assert_eq!(lex_kinds(src), [(BlockComment, src)]);
    }

    #[test]
    fn block_comment_unterminated() {
        let src = "/* unterminated";
        let tokens = lex_kinds(src);
        assert_eq!(tokens, [(BlockComment, src)]);
    }

    // ── Error recovery ───────────────────────────────────────────────

    #[test]
    fn unknown_chars() {
        let tokens = lex_kinds("#");
        assert_eq!(tokens, [(Error, "#")]);

        let tokens = lex_kinds("@");
        assert_eq!(tokens, [(Error, "@")]);
    }

    #[test]
    fn error_interspersed() {
        // Error tokens don't swallow neighbors.
        let tokens = lex_kinds("a#b");
        assert_eq!(tokens, [(Ident, "a"), (Error, "#"), (Ident, "b")]);
    }

    // ── Lossless roundtrip ───────────────────────────────────────────

    #[test]
    fn lossless_roundtrip() {
        let sources = [
            "",
            "fn main() -> Int { 42 }",
            "let x = 10_000\nlet y = 3.14",
            "// comment\n/* block */\n  \t\n",
            r#"let s = "hello \"world\"""#,
            "match x { Some(v) => v, None => 0 }",
            "a |> b |> c",
            "x >= 10 && y <= 20",
            "fn foo(x: Int) -> Bool requires x > 0 ensures old(x) == x { true }",
            "# @ ~ ` $", // all unknown
        ];
        for src in sources {
            let reconstructed: String = lex(src).iter().map(|t| t.text).collect();
            assert_eq!(reconstructed, src, "roundtrip failed for: {src:?}");
        }
    }

    // ── Full program smoke test ──────────────────────────────────────

    #[test]
    fn smoke_program() {
        let src = r#"
module Main

import Std.IO as IO

type Option<T> =
    | Some(T)
    | None

fn map<T, U>(opt: Option<T>, f: fn(T) -> U) -> Option<U> {
    match opt {
        Some(x) => Some(f(x)),
        None => None,
    }
}

let result = Some(42) |> map(fn(x) => x + 1)
"#;
        let tokens = lex(src);

        // Lossless
        let reconstructed: String = tokens.iter().map(|t| t.text).collect();
        assert_eq!(reconstructed, src);

        // No error tokens in valid source
        let errors: Vec<_> = tokens.iter().filter(|t| t.kind == Error).collect();
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");

        // Spot-check some tokens
        let non_trivia: Vec<_> = tokens.iter().filter(|t| !t.kind.is_trivia()).collect();

        assert_eq!(non_trivia[0].kind, ModuleKw);
        assert_eq!(non_trivia[1].kind, Ident); // Main
        assert_eq!(non_trivia[2].kind, ImportKw);
    }
}
