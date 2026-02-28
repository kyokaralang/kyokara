//! Wadler-Lindig pretty-printer — renders Doc IR to a string.
//!
//! Uses a stack-based approach: each entry tracks (indent, mode, doc).
//! The printer greedily tries to fit groups on one line; if they don't
//! fit, it switches to break mode.

use crate::doc::Doc;

/// Rendering mode for a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Try to render everything on one line.
    Flat,
    /// Render line breaks as actual newlines.
    Break,
}

/// Render a Doc to a string with the given maximum line width.
pub fn print(doc: &Doc, width: i32) -> String {
    let mut out = String::new();
    // Stack of (indent, mode, doc)
    let mut stack: Vec<(i32, Mode, &Doc)> = vec![(0, Mode::Break, doc)];
    // Current column position.
    let mut col: i32 = 0;

    while let Some((indent, mode, doc)) = stack.pop() {
        match doc {
            Doc::Nil => {}
            Doc::Text(s) => {
                out.push_str(s);
                col += s.len() as i32;
            }
            Doc::Line => match mode {
                Mode::Flat => {
                    out.push(' ');
                    col += 1;
                }
                Mode::Break => {
                    out.push('\n');
                    for _ in 0..indent {
                        out.push(' ');
                    }
                    col = indent;
                }
            },
            Doc::HardLine => {
                out.push('\n');
                for _ in 0..indent {
                    out.push(' ');
                }
                col = indent;
            }
            Doc::SoftLine => match mode {
                Mode::Flat => {
                    // Nothing — zero width in flat mode.
                }
                Mode::Break => {
                    out.push('\n');
                    for _ in 0..indent {
                        out.push(' ');
                    }
                    col = indent;
                }
            },
            Doc::Indent(n, inner) => {
                stack.push((indent + n, mode, inner));
            }
            Doc::Group(inner) => {
                if fits(width - col, &[(indent, Mode::Flat, inner)]) {
                    stack.push((indent, Mode::Flat, inner));
                } else {
                    stack.push((indent, Mode::Break, inner));
                }
            }
            Doc::Concat(left, right) => {
                // Push right first so left is processed first (stack is LIFO).
                stack.push((indent, mode, right));
                stack.push((indent, mode, left));
            }
            Doc::IfBreak(break_doc, flat_doc) => match mode {
                Mode::Flat => stack.push((indent, mode, flat_doc)),
                Mode::Break => stack.push((indent, mode, break_doc)),
            },
        }
    }

    // Trim trailing whitespace from each line.
    let trimmed: Vec<&str> = out.lines().map(|l| l.trim_end()).collect();
    let mut result = trimmed.join("\n");
    // Preserve final newline if the original had one.
    if out.ends_with('\n') {
        result.push('\n');
    }
    result
}

/// Check whether the document fits in the remaining width.
/// Returns early on encountering a hard line break.
fn fits(mut remaining: i32, initial: &[(i32, Mode, &Doc)]) -> bool {
    let mut stack: Vec<(i32, Mode, &Doc)> = initial.iter().rev().cloned().collect();

    while let Some((indent, mode, doc)) = stack.pop() {
        if remaining < 0 {
            return false;
        }
        match doc {
            Doc::Nil => {}
            Doc::Text(s) => {
                remaining -= s.len() as i32;
            }
            Doc::Line => match mode {
                Mode::Flat => {
                    remaining -= 1; // space
                }
                Mode::Break => return true, // newline always "fits"
            },
            Doc::HardLine => return true,
            Doc::SoftLine => match mode {
                Mode::Flat => {} // zero width
                Mode::Break => return true,
            },
            Doc::Indent(n, inner) => {
                stack.push((indent + n, mode, inner));
            }
            Doc::Group(inner) => {
                // In fits check, groups are always tried flat.
                stack.push((indent, Mode::Flat, inner));
            }
            Doc::Concat(left, right) => {
                stack.push((indent, mode, right));
                stack.push((indent, mode, left));
            }
            Doc::IfBreak(break_doc, flat_doc) => match mode {
                Mode::Flat => stack.push((indent, mode, flat_doc)),
                Mode::Break => stack.push((indent, mode, break_doc)),
            },
        }
    }

    remaining >= 0
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::doc::Doc;

    #[test]
    fn test_simple_text() {
        let doc = Doc::text("hello");
        assert_eq!(print(&doc, 80), "hello");
    }

    #[test]
    fn test_group_fits_on_one_line() {
        // group("a" + Line + "b") with enough width → "a b"
        let doc = Doc::group(Doc::concat(vec![Doc::text("a"), Doc::Line, Doc::text("b")]));
        assert_eq!(print(&doc, 80), "a b");
    }

    #[test]
    fn test_group_breaks() {
        // group("aaa" + Line + "bbb") with width=5 → breaks
        let doc = Doc::group(Doc::concat(vec![
            Doc::text("aaa"),
            Doc::Line,
            Doc::text("bbb"),
        ]));
        assert_eq!(print(&doc, 5), "aaa\nbbb");
    }

    #[test]
    fn test_indent() {
        let doc = Doc::concat(vec![
            Doc::text("{"),
            Doc::indent(2, Doc::concat(vec![Doc::HardLine, Doc::text("x")])),
            Doc::HardLine,
            Doc::text("}"),
        ]);
        assert_eq!(print(&doc, 80), "{\n  x\n}");
    }

    #[test]
    fn test_hard_line() {
        let doc = Doc::concat(vec![Doc::text("a"), Doc::HardLine, Doc::text("b")]);
        assert_eq!(print(&doc, 80), "a\nb");
    }

    #[test]
    fn test_soft_line_flat() {
        let doc = Doc::group(Doc::concat(vec![
            Doc::text("a"),
            Doc::SoftLine,
            Doc::text("b"),
        ]));
        // Fits, so SoftLine renders as nothing.
        assert_eq!(print(&doc, 80), "ab");
    }

    #[test]
    fn test_soft_line_break() {
        let doc = Doc::group(Doc::concat(vec![
            Doc::text("aaaa"),
            Doc::SoftLine,
            Doc::text("bbbb"),
        ]));
        // Doesn't fit on width=6.
        assert_eq!(print(&doc, 6), "aaaa\nbbbb");
    }

    #[test]
    fn test_if_break() {
        // In flat mode: no trailing comma. In break mode: trailing comma.
        let items = Doc::group(Doc::concat(vec![
            Doc::text("("),
            Doc::indent(
                2,
                Doc::concat(vec![
                    Doc::SoftLine,
                    Doc::text("a"),
                    Doc::text(","),
                    Doc::Line,
                    Doc::text("b"),
                    Doc::trailing_comma(),
                ]),
            ),
            Doc::SoftLine,
            Doc::text(")"),
        ]));

        // Fits on one line: "(a, b)"
        assert_eq!(print(&items, 80), "(a, b)");

        // Doesn't fit: breaks with trailing comma
        assert_eq!(print(&items, 5), "(\n  a,\n  b,\n)");
    }

    #[test]
    fn test_nested_groups() {
        let inner = Doc::group(Doc::concat(vec![Doc::text("x"), Doc::Line, Doc::text("y")]));
        let outer = Doc::group(Doc::concat(vec![Doc::text("f("), inner, Doc::text(")")]));
        assert_eq!(print(&outer, 80), "f(x y)");
    }

    #[test]
    fn test_trailing_whitespace_trimmed() {
        // Text with trailing spaces should be trimmed.
        let doc = Doc::concat(vec![
            Doc::text("hello   "),
            Doc::HardLine,
            Doc::text("world"),
        ]);
        assert_eq!(print(&doc, 80), "hello\nworld");
    }

    #[test]
    fn test_nil() {
        assert_eq!(print(&Doc::Nil, 80), "");
    }
}
