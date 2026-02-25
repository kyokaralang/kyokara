//! Comment/trivia classification and Doc emission.
//!
//! Trivia tokens (whitespace, comments) in rowan are siblings of the
//! nodes they appear next to, not children of those nodes. This module
//! provides helpers to walk `children_with_tokens()` and produce Docs
//! with comments in the right places.

use kyokara_parser::SyntaxKind;
use kyokara_syntax::SyntaxNode;

use crate::doc::Doc;

/// Emit a leading comment as a Doc: the comment text followed by a hard line.
pub fn leading_comment_doc(text: &str) -> Doc {
    Doc::concat(vec![Doc::text(text), Doc::HardLine])
}

/// Emit a trailing comment as a Doc: space, comment text.
pub fn trailing_comment_doc(text: &str) -> Doc {
    Doc::concat(vec![Doc::text(" "), Doc::text(text)])
}

/// Walk a node's `children_with_tokens()` and yield formatted children
/// with their attached comments. Each returned Doc includes any leading
/// comments (on previous lines) prepended, and trailing comments (on
/// the same line) appended.
///
/// `format_child` is called for each non-trivia child node.
/// `filter` determines which child nodes to process.
pub fn format_children_with_comments<F, G>(
    node: &SyntaxNode,
    mut filter: F,
    mut format_child: G,
) -> Vec<Doc>
where
    F: FnMut(&SyntaxNode) -> bool,
    G: FnMut(&SyntaxNode) -> Doc,
{
    let mut result: Vec<Doc> = Vec::new();
    let mut pending_leading: Vec<Doc> = Vec::new();
    let mut prev_was_newline = true; // Start of file counts as newline

    for element in node.children_with_tokens() {
        match element {
            rowan::NodeOrToken::Token(tok) => {
                let kind = tok.kind();
                if kind == SyntaxKind::Whitespace {
                    if tok.text().contains('\n') {
                        prev_was_newline = true;
                    }
                } else if kind == SyntaxKind::LineComment || kind == SyntaxKind::BlockComment {
                    let text = tok.text().to_string();
                    if prev_was_newline {
                        // Leading comment — attach to next item.
                        pending_leading.push(leading_comment_doc(&text));
                    } else {
                        // Trailing comment — attach to previous item.
                        if let Some(last) = result.last_mut() {
                            *last = Doc::concat(vec![last.clone(), trailing_comment_doc(&text)]);
                        } else {
                            // No previous item, treat as leading.
                            pending_leading.push(leading_comment_doc(&text));
                        }
                    }
                    prev_was_newline = false;
                } else {
                    // Non-trivia token — skip (handled by format_child).
                    prev_was_newline = false;
                }
            }
            rowan::NodeOrToken::Node(child) => {
                if filter(&child) {
                    let formatted = format_child(&child);
                    let doc = if pending_leading.is_empty() {
                        formatted
                    } else {
                        let mut parts = std::mem::take(&mut pending_leading);
                        parts.push(formatted);
                        Doc::concat(parts)
                    };
                    result.push(doc);
                }
                prev_was_newline = false;
            }
        }
    }

    // Any trailing leading comments (comments after the last item)
    // get appended as standalone items.
    for comment in pending_leading {
        result.push(comment);
    }

    result
}
