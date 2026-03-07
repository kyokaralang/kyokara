//! Top-level item parsing.
//!
//! Handles module declarations, imports, type definitions, function
//! definitions, effect definitions, property definitions, and
//! let bindings.

use crate::SyntaxKind::*;
use crate::parser::{CompletedMarker, Parser};
use crate::token_set::TokenSet;

/// Tokens that can start an item — used for error recovery.
pub(super) const ITEM_RECOVERY: TokenSet = TokenSet::new(&[
    ModuleKw, ImportKw, TypeKw, FnKw, CapKw, EffectKw, PropertyKw, LetKw, PubKw,
]);
const CLAUSE_EXPR_RECOVERY: TokenSet = TokenSet::new(&[
    LBrace,
    RBrace,
    Semicolon,
    Comma,
    ContractKw,
    WithKw,
    RequiresKw,
    EnsuresKw,
    InvariantKw,
    WhereKw,
]);

pub(super) fn item(p: &mut Parser<'_>) -> Option<CompletedMarker> {
    // `pub` can precede fn, type, or effect.
    let is_pub = p.at(PubKw);
    let start = if is_pub {
        p.current_after_pub()
    } else {
        p.current()
    };

    let cm = match start {
        TypeKw => type_def(p, is_pub),
        FnKw => fn_def(p, is_pub, false),
        CapKw => {
            p.error("`cap` is no longer supported; use `effect`");
            p.bump();
            return None;
        }
        EffectKw => effect_def(p, is_pub),
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
    // Canonical ADT syntax does not use a leading `|` before the first variant.
    // We still consume a stray leading pipe for recovery, but report it as invalid.
    if p.at(Pipe) {
        p.error("leading `|` is not allowed in type variants");
        p.bump();
    }

    if starts_variant_list(p) {
        variant_list(p);
    } else {
        super::types::type_expr(p);
    }
}

/// `Variant ('|' Variant)*`
fn variant_list(p: &mut Parser<'_>) {
    let m = p.open();
    variant(p);
    while p.eat(Pipe) {
        if p.at(Pipe) {
            p.error("expected variant after `|`");
            break;
        }
        variant(p);
    }
    m.complete(p, VariantList);
}

/// `Ident VariantFieldList?`
fn variant(p: &mut Parser<'_>) {
    let m = p.open();
    p.expect(Ident);
    if p.at(LParen) {
        variant_field_list(p);
    }
    m.complete(p, Variant);
}

fn starts_variant_list(p: &Parser<'_>) -> bool {
    p.at(Ident) && matches!(p.nth(1), Pipe | LParen)
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
pub(super) fn fn_def(p: &mut Parser<'_>, is_pub: bool, allow_bodyless: bool) -> CompletedMarker {
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

/// Parse optional function-level clauses in canonical order:
/// `with`, then an optional `contract` section.
///
/// Legacy direct `requires`/`ensures`/`invariant` clauses are rejected and
/// consumed for recovery.
fn fn_contract(p: &mut Parser<'_>) {
    let mut seen_with = false;

    while p.at(WithKw) {
        match p.current() {
            WithKw => {
                if seen_with {
                    p.error("duplicate `with` clause");
                }
                with_clause(p);
                seen_with = true;
            }
            _ => unreachable!("with clause dispatch mismatch"),
        }
    }

    if p.at(ContractKw) {
        contract_section(p);
    }

    while matches!(p.current(), RequiresKw | EnsuresKw | InvariantKw) {
        misplaced_contract_clause(p);
    }
}

fn contract_section(p: &mut Parser<'_>) {
    let m = p.open();
    p.bump(); // contract

    if !matches!(p.current(), RequiresKw | EnsuresKw | InvariantKw) {
        p.error("contract section must contain at least one clause");
        m.complete(p, ContractSection);
        return;
    }

    let mut max_seen_rank: Option<u8> = None;
    while let Some((rank, name)) = contract_clause_rank_and_name(p.current()) {
        if let Some(prev_rank) = max_seen_rank
            && rank < prev_rank
            && let Some(prev_name) = contract_clause_name_by_rank(prev_rank)
        {
            p.error(&format!(
                "{name} cannot appear after {prev_name} (contract clause order: requires, ensures, invariant)"
            ));
        }

        max_seen_rank = Some(max_seen_rank.map_or(rank, |r| r.max(rank)));

        match p.current() {
            RequiresKw => requires_clause(p),
            EnsuresKw => ensures_clause(p),
            InvariantKw => invariant_clause(p),
            _ => unreachable!("contract section clause dispatch mismatch"),
        }
    }

    m.complete(p, ContractSection);
}

fn contract_clause_rank_and_name(kind: crate::SyntaxKind) -> Option<(u8, &'static str)> {
    match kind {
        RequiresKw => Some((0, "requires")),
        EnsuresKw => Some((1, "ensures")),
        InvariantKw => Some((2, "invariant")),
        _ => None,
    }
}

fn contract_clause_name_by_rank(rank: u8) -> Option<&'static str> {
    match rank {
        0 => Some("requires"),
        1 => Some("ensures"),
        2 => Some("invariant"),
        _ => None,
    }
}

fn misplaced_contract_clause(p: &mut Parser<'_>) {
    match p.current() {
        RequiresKw => misplaced_contract_clause_with_name(p, "requires", RequiresClause),
        EnsuresKw => misplaced_contract_clause_with_name(p, "ensures", EnsuresClause),
        InvariantKw => misplaced_contract_clause_with_name(p, "invariant", InvariantClause),
        _ => unreachable!("misplaced contract clause dispatch mismatch"),
    }
}

fn misplaced_contract_clause_with_name(
    p: &mut Parser<'_>,
    name: &str,
    node_kind: crate::SyntaxKind,
) {
    let m = p.open();
    p.error(&format!(
        "`{name}` clause must appear inside `contract` section"
    ));
    p.bump(); // clause keyword
    consume_misplaced_clause_expr(p);
    m.complete(p, node_kind);
}

fn consume_misplaced_clause_expr(p: &mut Parser<'_>) {
    if p.eat(LParen) {
        super::expressions::expr(p);
        p.expect(RParen);
        return;
    }
    p.recover_parenthesized_head_content(CLAUSE_EXPR_RECOVERY);
}

fn with_clause(p: &mut Parser<'_>) {
    let m = p.open();
    p.bump(); // with
    super::types::type_expr(p);
    while p.eat(Comma) {
        if p.at(LBrace)
            || p.at(ContractKw)
            || p.at(RequiresKw)
            || p.at(EnsuresKw)
            || p.at(InvariantKw)
        {
            break;
        }
        super::types::type_expr(p);
    }
    m.complete(p, WithClause);
}

fn parse_parenthesized_clause_expr(p: &mut Parser<'_>, message: &str) {
    if p.eat(LParen) {
        super::expressions::expr(p);
        p.expect(RParen);
    } else {
        p.error_recover_parenthesized_head(message, CLAUSE_EXPR_RECOVERY);
    }
}

fn requires_clause(p: &mut Parser<'_>) {
    let m = p.open();
    p.bump(); // requires
    parse_parenthesized_clause_expr(p, "requires clause expression must be parenthesized");
    m.complete(p, RequiresClause);
}

fn ensures_clause(p: &mut Parser<'_>) {
    let m = p.open();
    p.bump(); // ensures
    parse_parenthesized_clause_expr(p, "ensures clause expression must be parenthesized");
    m.complete(p, EnsuresClause);
}

fn invariant_clause(p: &mut Parser<'_>) {
    let m = p.open();
    p.bump(); // invariant
    parse_parenthesized_clause_expr(p, "invariant clause expression must be parenthesized");
    m.complete(p, InvariantClause);
}

// ── Effect Definition ───────────────────────────────────────────────

/// `pub? effect Ident`
fn effect_def(p: &mut Parser<'_>, is_pub: bool) -> CompletedMarker {
    let m = p.open();
    if is_pub {
        p.bump(); // pub
    }
    p.bump(); // effect
    p.expect(Ident);

    if p.at(Lt) {
        p.error("effect declarations cannot have type parameters");
        type_param_list(p);
    }

    if p.at(LBrace) {
        let err = p.open();
        p.error("effect declarations are labels only; remove body");
        p.bump(); // {
        while !p.at(RBrace) && !p.at_eof() {
            p.bump();
        }
        p.expect(RBrace);
        err.complete(p, ErrorNode);
    }
    m.complete(p, EffectDef)
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

/// `where '(' Expr ')'`
fn where_clause(p: &mut Parser<'_>) {
    let m = p.open();
    p.bump(); // where
    parse_parenthesized_clause_expr(p, "where clause expression must be parenthesized");
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
