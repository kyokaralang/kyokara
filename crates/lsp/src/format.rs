//! `textDocument/formatting` — integrate kyokara_fmt.

use text_size::TextSize;
use tower_lsp::lsp_types::{Position, Range, TextEdit};

/// Format a document. Returns a single edit replacing the entire document
/// if the formatted text differs from the input, or an empty vec if unchanged.
pub fn format_document(source: &str) -> Vec<TextEdit> {
    let formatted = kyokara_fmt::format_source(source);
    if formatted == source {
        return Vec::new();
    }

    // Use the same UTF-16-aware conversion as the rest of the LSP bridge.
    let end = crate::position::offset_to_lsp_position(TextSize::from(source.len() as u32), source);

    vec![TextEdit {
        range: Range::new(Position::new(0, 0), end),
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
        assert_eq!(edits.len(), 1, "expected a full-document edit");
    }

    #[test]
    fn format_edit_range_matches_utf16_end_of_document() {
        // Contains an emoji (2 UTF-16 code units) and trailing newline.
        let source = "fn   foo(  )  ->  String  {  \"😀\"  }\n";
        let edits = format_document(source);
        assert_eq!(edits.len(), 1, "expected a full-document edit");
        let expected_end =
            crate::position::offset_to_lsp_position(TextSize::from(source.len() as u32), source);
        assert_eq!(
            edits[0].range,
            Range::new(Position::new(0, 0), expected_end),
            "format range must cover exact UTF-16 document extent"
        );
    }
}
