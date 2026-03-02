//! Top-level item parsing.
//!
//! Handles module declarations, imports, type definitions, function
//! definitions, capability definitions, property definitions, and
//! let bindings.

use crate::SyntaxKind::*;
use crate::parser::{CompletedMarker, Parser};
use crate::token_set::TokenSet;

/// Tokens that can start an item — used for error recovery.
pub(super) const ITEM_RECOVERY: TokenSet = TokenSet::new(&[
    ModuleKw, ImportKw, TypeKw, FnKw, CapKw, PropertyKw, LetKw, PubKw,
]);

pub(super) fn item(p: &mut Parser<'_>) -> Option<CompletedMarker> {
    // `pub` can precede fn, type, or cap.
    let is_pub = p.at(PubKw);
    let start = if is_pub {
        p.current_after_pub()
    } else {
        p.current()
    };

    let cm = match start {
        TypeKw => type_def(p, is_pub),
        FnKw => fn_def(p, is_pub, false),
        CapKw => cap_def(p, is_pub),
        PropertyKw => {
            if is_pub {
                p.error_recover("expected item", ITEM_RECOVERY);
                return None;
            }
            property_def(p)
        }
        LetKw => {
            if is_pub {
                p.error_recover("expected item", ITEM_RECOVERY);
                return None;
            }
            let_binding(p)
        }
        _ => {
            p.error_recover("expected item", ITEM_RECOVERY);
            return None;
        }
    };
    Some(cm)
}

// ── Module & Imports ────────────────────────────────────────────────

/// `module Path`
pub(super) fn module_decl(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.open();
    p.bump(); // module
    super::parse_path(p);
    m.complete(p, ModuleDecl)
}

/// `import Path ImportAlias?`
pub(super) fn import_decl(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.open();
    p.bump(); // import
    super::parse_path(p);
    if p.at(AsKw) {
        import_alias(p);
    }
    m.complete(p, ImportDecl)
}

fn import_alias(p: &mut Parser<'_>) {
    let m = p.open();
    p.bump(); // as
    p.expect(Ident);
    m.complete(p, ImportAlias);
}

// ── Type Definition ─────────────────────────────────────────────────

/// `pub? type Ident TypeParamList? '=' TypeBody`
fn type_def(p: &mut Parser<'_>, is_pub: bool) -> CompletedMarker {
    let m = p.open();
    if is_pub {
        p.bump(); // pub
    }
    p.bump(); // type
    p.expect(Ident);
    if p.at(Lt) {
        type_param_list(p);
    }
    p.expect(Eq);
    type_body(p);
    m.complete(p, TypeDef)
}

/// `VariantList / TypeExpr`
fn type_body(p: &mut Parser<'_>) {
    if p.at(Pipe) {
        variant_list(p);
    } else {
        super::types::type_expr(p);
    }
}

/// `('|' Variant)+`
fn variant_list(p: &mut Parser<'_>) {
    let m = p.open();
    while p.at(Pipe) {
        variant(p);
    }
    m.complete(p, VariantList);
}

/// `'|' Ident VariantFieldList?`
fn variant(p: &mut Parser<'_>) {
    let m = p.open();
    p.bump(); // |
    p.expect(Ident);
    if p.at(LParen) {
        variant_field_list(p);
    }
    m.complete(p, Variant);
}

/// `'(' TypeExpr (',' TypeExpr)* ','? ')'`
fn variant_field_list(p: &mut Parser<'_>) {
    let m = p.open();
    p.bump(); // (
    if !p.at(RParen) {
        let fm = p.open();
        super::types::type_expr(p);
        fm.complete(p, VariantField);
        while p.eat(Comma) {
            if p.at(RParen) {
                break;
            }
            let fm = p.open();
            super::types::type_expr(p);
            fm.complete(p, VariantField);
        }
    }
    p.expect(RParen);
    m.complete(p, VariantFieldList);
}

// ── Function Definition ─────────────────────────────────────────────

/// `pub? fn Ident TypeParamList? ParamList ReturnType? FnContract? BlockExpr`
/// or method: `pub? fn Ident '.' Ident TypeParamList? ParamList ReturnType? FnContract? BlockExpr`
///
/// Capability member signatures are the only bodyless `fn` forms currently
/// allowed (`allow_bodyless = true`).
pub(super) fn fn_def(
    p: &mut Parser<'_>,
    is_pub: bool,
    allow_bodyless: bool,
) -> CompletedMarker {
    let m = p.open();
    if is_pub {
        p.bump(); // pub
    }
    p.bump(); // fn
    p.expect(Ident); // function name, or receiver type name for methods
    // Method syntax: fn Type.method(...)
    if p.at(Dot) {
        p.bump(); // .
        p.expect(Ident); // method name
    }
    if p.at(Lt) {
        type_param_list(p);
    }
    param_list(p);
    if p.at(Arrow) {
        return_type(p);
    }
    fn_contract(p);
    if p.at(LBrace) {
        super::expressions::block_expr(p);
    } else if !allow_bodyless {
        p.error("expected function body");
    }
    m.complete(p, FnDef)
}

fn param_list(p: &mut Parser<'_>) {
    let m = p.open();
    p.expect(LParen);
    if !p.at(RParen) {
        param(p);
        while p.eat(Comma) {
            if p.at(RParen) {
                break;
            }
            param(p);
        }
    }
    p.expect(RParen);
    m.complete(p, ParamList);
}

fn param(p: &mut Parser<'_>) {
    let m = p.open();
    p.expect(Ident);
    // The `: Type` part is optional to support bare `self` in method defs.
    // Semantic validation ensures only `self` can omit the type annotation.
    if p.eat(Colon) {
        super::types::type_expr(p);
    }
    m.complete(p, Param);
}

fn return_type(p: &mut Parser<'_>) {
    let m = p.open();
    p.bump(); // ->
    super::types::type_expr(p);
    m.complete(p, ReturnType);
}

/// Parse optional contract clauses in strict canonical order:
/// with, pipe, requires, ensures, invariant.
///
/// Out-of-order clauses produce a targeted diagnostic and are still parsed so
/// the parser can recover without cascading item-level errors.
fn fn_contract(p: &mut Parser<'_>) {
    let mut max_seen_rank: Option<u8> = None;
    let mut seen_mask: u8 = 0;

    while let Some((rank, name)) = contract_clause_rank_and_name(p.current()) {
        let bit = 1u8 << rank;
        if (seen_mask & bit) != 0 {
            p.error(&format!("duplicate `{name}` clause"));
        } else {
            seen_mask |= bit;
        }

        if let Some(prev_rank) = max_seen_rank
            && rank < prev_rank
            && let Some(prev_name) = contract_clause_name_by_rank(prev_rank)
        {
            p.error(&format!(
                "{name} cannot appear after {prev_name} (contract clause order: with, pipe, requires, ensures, invariant)"
            ));
        }

        max_seen_rank = Some(max_seen_rank.map_or(rank, |r| r.max(rank)));

        match p.current() {
            WithKw => with_clause(p),
            PipeKw => pipe_clause(p),
            RequiresKw => requires_clause(p),
            EnsuresKw => ensures_clause(p),
            InvariantKw => invariant_clause(p),
            _ => unreachable!("contract clause dispatch mismatch"),
        }
    }
}

fn contract_clause_rank_and_name(kind: crate::SyntaxKind) -> Option<(u8, &'static str)> {
    match kind {
        WithKw => Some((0, "with")),
        PipeKw => Some((1, "pipe")),
        RequiresKw => Some((2, "requires")),
        EnsuresKw => Some((3, "ensures")),
        InvariantKw => Some((4, "invariant")),
        _ => None,
    }
}

fn contract_clause_name_by_rank(rank: u8) -> Option<&'static str> {
    match rank {
        0 => Some("with"),
        1 => Some("pipe"),
        2 => Some("requires"),
        3 => Some("ensures"),
        4 => Some("invariant"),
        _ => None,
    }
}

fn with_clause(p: &mut Parser<'_>) {
    let m = p.open();
    p.bump(); // with
    super::types::type_expr(p);
    while p.eat(Comma) {
        if p.at(LBrace) || p.at(RequiresKw) || p.at(EnsuresKw) || p.at(InvariantKw) || p.at(PipeKw)
        {
            break;
        }
        super::types::type_expr(p);
    }
    m.complete(p, WithClause);
}

fn pipe_clause(p: &mut Parser<'_>) {
    let m = p.open();
    p.bump(); // pipe
    super::types::type_expr(p);
    while p.eat(Comma) {
        if p.at(LBrace) || p.at(RequiresKw) || p.at(EnsuresKw) || p.at(InvariantKw) {
            break;
        }
        super::types::type_expr(p);
    }
    m.complete(p, PipeClause);
}

fn requires_clause(p: &mut Parser<'_>) {
    let m = p.open();
    p.bump(); // requires
    super::expressions::expr_no_record(p);
    m.complete(p, RequiresClause);
}

fn ensures_clause(p: &mut Parser<'_>) {
    let m = p.open();
    p.bump(); // ensures
    super::expressions::expr_no_record(p);
    m.complete(p, EnsuresClause);
}

fn invariant_clause(p: &mut Parser<'_>) {
    let m = p.open();
    p.bump(); // invariant
    super::expressions::expr_no_record(p);
    m.complete(p, InvariantClause);
}

// ── Capability Definition ───────────────────────────────────────────

/// `pub? cap Ident TypeParamList? '{' FnDef* '}'`
fn cap_def(p: &mut Parser<'_>, is_pub: bool) -> CompletedMarker {
    let m = p.open();
    if is_pub {
        p.bump(); // pub
    }
    p.bump(); // cap
    p.expect(Ident);
    if p.at(Lt) {
        type_param_list(p);
    }
    p.expect(LBrace);
    while !p.at(RBrace) && !p.at_eof() {
        if p.at(FnKw) {
            fn_def(p, false, true);
        } else if p.at(PubKw) && p.current_after_pub() == FnKw {
            fn_def(p, true, true);
        } else {
            p.error_recover("expected fn in cap body", TokenSet::new(&[FnKw, RBrace]));
        }
    }
    p.expect(RBrace);
    m.complete(p, CapDef)
}

// ── Property Definition ─────────────────────────────────────────────

/// `property Ident PropertyParamList WhereClause? BlockExpr`
fn property_def(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.open();
    p.bump(); // property
    p.expect(Ident);
    property_param_list(p);
    if p.at(WhereKw) {
        where_clause(p);
    }
    if p.at(LBrace) {
        super::expressions::block_expr(p);
    }
    m.complete(p, PropertyDef)
}

/// `'(' PropertyParam (',' PropertyParam)* ','? ')'`
fn property_param_list(p: &mut Parser<'_>) {
    let m = p.open();
    p.expect(LParen);
    if !p.at(RParen) {
        property_param(p);
        while p.eat(Comma) {
            if p.at(RParen) {
                break;
            }
            property_param(p);
        }
    }
    p.expect(RParen);
    m.complete(p, PropertyParamList);
}

/// `Ident ':' TypeExpr '<-' Expr`
fn property_param(p: &mut Parser<'_>) {
    let m = p.open();
    p.expect(Ident);
    p.expect(Colon);
    super::types::type_expr(p);
    if !p.eat(LeftArrow) {
        p.error_recover(
            "property parameters must use `<-` generator binding, e.g. `x: Int <- Gen.auto()`",
            TokenSet::new(&[Comma, RParen]),
        );
    } else {
        super::expressions::expr(p);
    }
    m.complete(p, PropertyParam);
}

/// `where Expr`
fn where_clause(p: &mut Parser<'_>) {
    let m = p.open();
    p.bump(); // where
    super::expressions::expr_no_record(p);
    m.complete(p, WhereClause);
}

// ── Let Binding ─────────────────────────────────────────────────────

/// `let Pattern (':' TypeExpr)? '=' Expr`
pub(super) fn let_binding(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.open();
    p.bump(); // let
    super::patterns::pattern(p);
    if p.eat(Colon) {
        super::types::type_expr(p);
    }
    p.expect(Eq);
    super::expressions::expr(p);
    m.complete(p, LetBinding)
}

// ── Generics ────────────────────────────────────────────────────────

/// `< TypeParam (',' TypeParam)* ','? >`
pub(super) fn type_param_list(p: &mut Parser<'_>) {
    let m = p.open();
    p.bump(); // <
    type_param(p);
    while p.eat(Comma) {
        if p.at(Gt) {
            break;
        }
        type_param(p);
    }
    p.expect(Gt);
    m.complete(p, TypeParamList);
}

fn type_param(p: &mut Parser<'_>) {
    let m = p.open();
    p.expect(Ident);
    m.complete(p, TypeParam);
}
