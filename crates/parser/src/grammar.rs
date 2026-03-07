//! Grammar entry point and shared utilities.
//!
//! This module wires together the sub-grammar modules (items, types,
//! expressions, patterns) and provides the top-level `source_file()`
//! parsing function.

pub(crate) mod expressions;
pub(crate) mod items;
pub(crate) mod patterns;
pub(crate) mod types;

use crate::SyntaxKind::*;
use crate::parser::{CompletedMarker, IdentifierRole, Parser};

/// Parse a complete source file.
///
/// ```peg
/// SourceFile <- ModuleDecl? ImportDecl* Item* EOF
/// ```
pub(crate) fn source_file(p: &mut Parser<'_>) {
    let m = p.open();

    // Optional module declaration.
    if p.at(ModuleKw) {
        items::module_decl(p);
    }

    // Import declarations.
    while p.at(ImportKw) {
        items::import_decl(p);
    }

    // Items until EOF.
    while !p.at_eof() {
        let start_pos = p.token_pos();
        let _ = items::item(p);
        if p.token_pos() == start_pos && !p.at_eof() {
            p.bump();
        }
    }

    m.complete(p, SourceFile);
}

/// Parse a dotted path: `Ident ('.' Ident)*`
pub(crate) fn parse_path(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.open();
    p.expect_identifier(IdentifierRole::PathSegment);
    while p.eat(Dot) {
        p.expect_identifier(IdentifierRole::PathSegment);
    }
    m.complete(p, Path)
}

/// Parse a single-segment path (just one `Ident`).
///
/// Used in value expression position so that `p.x` is parsed as a field
/// access (`FieldExpr`) via the postfix-`.` loop rather than a
/// two-segment path.
pub(crate) fn parse_single_path(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.open();
    p.expect_identifier(IdentifierRole::PathSegment);
    m.complete(p, Path)
}
