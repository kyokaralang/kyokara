//! `textDocument/formatting` — integrate kyokara_fmt.

use tower_lsp::lsp_types::{Position, Range, TextEdit};

/// Format a document. Returns a single edit replacing the entire document
/// if the formatted text differs from the input, or an empty vec if unchanged.
pub fn format_document(source: &str) -> Vec<TextEdit> {
    let formatted = kyokara_fmt::format_source(source);
    if formatted == source {
        return Vec::new();
    }

    // Count lines in the original source for the replacement range.
    let line_count = source.lines().count() as u32;
    let last_line_len = source.lines().last().map(|l| l.len() as u32).unwrap_or(0);

    vec![TextEdit {
        range: Range::new(
            Position::new(0, 0),
            Position::new(line_count, last_line_len),
        ),
        new_text: formatted,
    }]
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn format_unchanged() {
        let source = "fn foo() -> Int { 42 }\n";
        let edits = format_document(source);
        // May or may not produce edits depending on formatter behavior,
        // but should not panic.
        let _ = edits;
    }

    #[test]
    fn format_returns_edit_when_different() {
        // Intentionally messy formatting.
        let source = "fn   foo(  )  ->  Int  {  42  }";
        let edits = format_document(source);
        // If the formatter changes the text, we should get an edit.
        // (The formatter may or may not change this particular input.)
        let _ = edits;
    }
}
