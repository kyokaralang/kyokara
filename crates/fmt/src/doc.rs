//! Wadler-Lindig Doc IR for pretty-printing.
//!
//! The Doc type represents a document that can be rendered at different
//! widths. The printer decides where to break lines based on group
//! boundaries and available width.

/// A document IR node.
#[derive(Debug, Clone)]
pub enum Doc {
    /// Empty document.
    Nil,
    /// Literal text (no newlines).
    Text(String),
    /// A line break. In flat mode, rendered as a single space.
    /// In break mode, rendered as a newline + current indentation.
    Line,
    /// A hard line break — always rendered as a newline, even in flat mode.
    HardLine,
    /// A soft line break — in flat mode, rendered as empty string.
    /// In break mode, rendered as a newline + current indentation.
    SoftLine,
    /// Increase indentation by `n` spaces for the inner doc.
    Indent(i32, Box<Doc>),
    /// A group that the printer tries to fit on one line. If it doesn't
    /// fit, Line breaks inside are expanded to newlines.
    Group(Box<Doc>),
    /// Concatenation of two documents.
    Concat(Box<Doc>, Box<Doc>),
    /// Conditional: first doc in flat mode, second doc in break mode.
    IfBreak(Box<Doc>, Box<Doc>),
}

// ── Constructors ────────────────────────────────────────────────────

impl Doc {
    /// Literal text.
    pub fn text(s: impl Into<String>) -> Doc {
        let s = s.into();
        if s.is_empty() { Doc::Nil } else { Doc::Text(s) }
    }

    /// Group: try to fit the inner doc on one line.
    pub fn group(inner: Doc) -> Doc {
        Doc::Group(Box::new(inner))
    }

    /// Indent the inner doc by `n` extra spaces.
    pub fn indent(n: i32, inner: Doc) -> Doc {
        Doc::Indent(n, Box::new(inner))
    }

    /// Conditional rendering based on enclosing group's mode.
    pub fn if_break(break_doc: Doc, flat_doc: Doc) -> Doc {
        Doc::IfBreak(Box::new(break_doc), Box::new(flat_doc))
    }

    /// Concatenate a sequence of docs.
    pub fn concat(docs: Vec<Doc>) -> Doc {
        let mut result = Doc::Nil;
        for doc in docs {
            result = Doc::Concat(Box::new(result), Box::new(doc));
        }
        result
    }

    /// Join docs with a separator between each pair.
    pub fn join(docs: Vec<Doc>, sep: Doc) -> Doc {
        let mut parts = Vec::new();
        let len = docs.len();
        for (i, doc) in docs.into_iter().enumerate() {
            parts.push(doc);
            if i + 1 < len {
                parts.push(sep.clone());
            }
        }
        Doc::concat(parts)
    }

    /// Trailing comma in break mode, nothing in flat mode.
    pub fn trailing_comma() -> Doc {
        Doc::if_break(Doc::text(","), Doc::Nil)
    }
}
