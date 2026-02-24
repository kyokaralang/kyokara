//! Top-level item parsing.
//!
//! Handles module declarations, imports, type definitions, function
//! definitions, capability definitions, property definitions, and
//! let bindings.

use crate::SyntaxKind::*;
use crate::parser::{CompletedMarker, Parser};
use crate::token_set::TokenSet;

/// Tokens that can start an item — used for error recovery.
pub(super) const ITEM_RECOVERY: TokenSet =
    TokenSet::new(&[ModuleKw, ImportKw, TypeKw, FnKw, CapKw, PropertyKw, LetKw]);

pub(super) fn item(p: &mut Parser<'_>) -> Option<CompletedMarker> {
    let cm = match p.current() {
        TypeKw => type_def(p),
        FnKw => fn_def(p),
        CapKw => cap_def(p),
        PropertyKw => property_def(p),
        LetKw => let_binding(p),
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

/// `type Ident TypeParamList? '=' TypeBody`
fn type_def(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.open();
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

/// `fn Ident TypeParamList? ParamList ReturnType? FnContract? BlockExpr`
pub(super) fn fn_def(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.open();
    p.bump(); // fn
    p.expect(Ident);
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
    p.expect(Colon);
    super::types::type_expr(p);
    m.complete(p, Param);
}

fn return_type(p: &mut Parser<'_>) {
    let m = p.open();
    p.bump(); // ->
    super::types::type_expr(p);
    m.complete(p, ReturnType);
}

/// Parse optional contract clauses: with, pipe, requires, ensures, invariant.
fn fn_contract(p: &mut Parser<'_>) {
    if p.at(WithKw) {
        with_clause(p);
    }
    if p.at(PipeKw) {
        pipe_clause(p);
    }
    if p.at(RequiresKw) {
        requires_clause(p);
    }
    if p.at(EnsuresKw) {
        ensures_clause(p);
    }
    if p.at(InvariantKw) {
        invariant_clause(p);
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

/// `cap Ident TypeParamList? '{' FnDef* '}'`
fn cap_def(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.open();
    p.bump(); // cap
    p.expect(Ident);
    if p.at(Lt) {
        type_param_list(p);
    }
    p.expect(LBrace);
    while !p.at(RBrace) && !p.at_eof() {
        if p.at(FnKw) {
            fn_def(p);
        } else {
            p.error_recover("expected fn in cap body", TokenSet::new(&[FnKw, RBrace]));
        }
    }
    p.expect(RBrace);
    m.complete(p, CapDef)
}

// ── Property Definition ─────────────────────────────────────────────

/// `property Ident ParamList BlockExpr`
fn property_def(p: &mut Parser<'_>) -> CompletedMarker {
    let m = p.open();
    p.bump(); // property
    p.expect(Ident);
    param_list(p);
    if p.at(LBrace) {
        super::expressions::block_expr(p);
    }
    m.complete(p, PropertyDef)
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
