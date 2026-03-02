//! End-to-end type inference tests: parse → collect item tree → lower → type check → assert.
#![allow(clippy::unwrap_used)]

use kyokara_hir_def::item_tree::lower::collect_item_tree;
use kyokara_hir_ty::ty::Ty;
use kyokara_hir_ty::{TypeCheckResult, check_module};
use kyokara_intern::Interner;
use kyokara_span::FileId;
use kyokara_syntax::SyntaxNode;
use kyokara_syntax::ast::AstNode;
use kyokara_syntax::ast::nodes::SourceFile;

fn file_id() -> FileId {
    FileId(0)
}

fn parse_source(src: &str) -> SyntaxNode {
    let parse = kyokara_syntax::parse(src);
    SyntaxNode::new_root(parse.green)
}

/// Parse, collect, and type-check, returning the result.
fn check(src: &str) -> (TypeCheckResult, Interner) {
    let root = parse_source(src);
    let sf = SourceFile::cast(root.clone()).unwrap();
    let mut interner = Interner::new();
    let item_result = collect_item_tree(&sf, file_id(), &mut interner);
    let result = check_module(
        &root,
        &item_result.tree,
        &item_result.module_scope,
        file_id(),
        &mut interner,
    );
    (result, interner)
}

/// Assert type-checking produces no diagnostics.
fn check_ok(src: &str) -> (TypeCheckResult, Interner) {
    let (result, interner) = check(src);
    let ty_diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| !d.message.contains("unresolved name"))
        .collect();
    assert!(
        ty_diags.is_empty(),
        "expected no type diagnostics, got: {ty_diags:#?}\nsource:\n{src}"
    );
    (result, interner)
}

/// Assert type-checking produces at least one diagnostic containing `needle`.
fn check_err(src: &str, needle: &str) {
    let (result, _) = check(src);
    let has = result
        .diagnostics
        .iter()
        .any(|d| d.message.contains(needle));
    assert!(
        has,
        "expected diagnostic containing `{needle}`, got: {:?}\nsource:\n{src}",
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ── Basic inference tests ────────────────────────────────────────────

#[test]
fn infer_int_literal() {
    check_ok("fn foo() -> Int { 42 }");
}

#[test]
fn infer_bool_literal() {
    check_ok("fn foo() -> Bool { true }");
}

#[test]
fn infer_string_literal() {
    check_ok("fn foo() -> String { \"hello\" }");
}

#[test]
fn infer_float_literal() {
    check_ok("fn foo() -> Float { 3.14 }");
}

#[test]
fn infer_char_literal() {
    check_ok("fn foo() -> Char { 'a' }");
}

#[test]
fn infer_unit_return() {
    check_ok("fn foo() { }");
}

#[test]
fn infer_let_binding() {
    check_ok("fn foo() -> Int { let x = 42\n x }");
}

#[test]
fn infer_let_with_annotation() {
    check_ok("fn foo() -> Int { let x: Int = 42\n x }");
}

#[test]
fn infer_binary_add() {
    check_ok("fn foo() -> Int { 1 + 2 }");
}

#[test]
fn infer_binary_comparison() {
    check_ok("fn foo() -> Bool { 1 < 2 }");
}

#[test]
fn infer_binary_equality() {
    check_ok("fn foo() -> Bool { 1 == 2 }");
}

#[test]
fn infer_unary_neg() {
    check_ok("fn foo() -> Int { -42 }");
}

#[test]
fn infer_unary_not() {
    check_ok("fn foo() -> Bool { !true }");
}

#[test]
fn infer_if_else() {
    check_ok("fn foo() -> Int { if (true) { 1 } else { 2 } }");
}

#[test]
fn infer_if_no_else_is_unit() {
    check_ok("fn foo() { if (true) { 1 } }");
}

#[test]
fn infer_function_call() {
    check_ok("fn bar(x: Int) -> Int { x }\nfn foo() -> Int { bar(42) }");
}

#[test]
fn infer_function_call_multi_args() {
    check_ok("fn add(x: Int, y: Int) -> Int { x + y }\nfn foo() -> Int { add(1, 2) }");
}

#[test]
fn infer_return_expr() {
    check_ok("fn foo() -> Int { return 42 }");
}

#[test]
fn infer_param_types() {
    check_ok("fn foo(x: Int, y: Bool) -> Int { x }");
}

#[test]
fn infer_block_with_stmts() {
    check_ok(
        "fn foo() -> Int {
            let a = 1
            let b = 2
            a + b
        }",
    );
}

// ── Modulo type inference ────────────────────────────────────────────

#[test]
fn infer_modulo_int() {
    check_ok("fn foo() -> Int { 10 % 3 }");
}

#[test]
fn infer_modulo_float() {
    check_ok("fn foo() -> Float { 3.14 % 1.0 }");
}

#[test]
fn err_modulo_on_bool() {
    check_err("fn foo() -> Bool { true % false }", "arithmetic");
}

#[test]
fn err_modulo_on_string() {
    check_err(r#"fn foo() -> String { "a" % "b" }"#, "arithmetic");
}

// ── Logical operator type inference ─────────────────────────────────

#[test]
fn infer_logical_and() {
    check_ok("fn foo() -> Bool { true && false }");
}

#[test]
fn infer_logical_or() {
    check_ok("fn foo() -> Bool { true || false }");
}

#[test]
fn err_logical_and_on_int() {
    check_err("fn foo() -> Bool { 1 && 2 }", "type mismatch");
}

#[test]
fn err_logical_or_on_int() {
    check_err("fn foo() -> Bool { 1 || 2 }", "type mismatch");
}

// ── Bitwise operator type inference ─────────────────────────────────

#[test]
fn infer_bitwise_and() {
    check_ok("fn foo() -> Int { 12 & 10 }");
}

#[test]
fn infer_bitwise_or() {
    check_ok("fn foo() -> Int { 12 | 10 }");
}

#[test]
fn infer_bitwise_xor() {
    check_ok("fn foo() -> Int { 12 ^ 10 }");
}

#[test]
fn infer_shl() {
    check_ok("fn foo() -> Int { 1 << 3 }");
}

#[test]
fn infer_shr() {
    check_ok("fn foo() -> Int { 8 >> 2 }");
}

#[test]
fn infer_bitwise_not() {
    check_ok("fn foo() -> Int { ~42 }");
}

#[test]
fn err_bitwise_and_on_float() {
    check_err("fn foo() -> Float { 1.0 & 2.0 }", "arithmetic");
}

#[test]
fn err_bitwise_or_on_bool() {
    check_err("fn foo() -> Bool { true | false }", "arithmetic");
}

#[test]
fn err_bitwise_xor_on_string() {
    check_err(r#"fn foo() -> String { "a" ^ "b" }"#, "arithmetic");
}

#[test]
fn err_shl_on_float() {
    check_err("fn foo() -> Float { 1.0 << 2 }", "arithmetic");
}

#[test]
fn err_bitwise_not_on_bool() {
    check_err("fn foo() -> Bool { ~true }", "arithmetic");
}

#[test]
fn err_bitwise_not_on_float() {
    check_err("fn foo() -> Float { ~1.0 }", "arithmetic");
}

// ── Combined operator type inference ────────────────────────────────

#[test]
fn infer_bitwise_in_comparison() {
    // (a & b) == c — bitwise result is Int, comparison returns Bool
    check_ok("fn foo() -> Bool { (3 & 1) == 1 }");
}

#[test]
fn infer_bitwise_with_logical() {
    check_ok("fn foo() -> Bool { (3 & 1) == 1 && (4 | 2) == 6 }");
}

#[test]
fn infer_shift_with_addition() {
    check_ok("fn foo() -> Int { 1 + (1 << 3) }");
}

#[test]
fn infer_tilde_in_expression() {
    check_ok("fn foo() -> Int { ~0 + 1 }");
}

// ── Type error tests ─────────────────────────────────────────────────

#[test]
fn err_type_mismatch_return() {
    check_err("fn foo() -> Int { true }", "type mismatch");
}

#[test]
fn err_type_mismatch_if_condition() {
    check_err("fn foo() { if (42) { 1 } else { 2 } }", "type mismatch");
}

#[test]
fn err_type_mismatch_if_branches() {
    check_err(
        "fn foo() -> Int { if (true) { 1 } else { true } }",
        "type mismatch",
    );
}

#[test]
fn err_arithmetic_on_bool() {
    check_err("fn foo() -> Bool { true + false }", "arithmetic");
}

#[test]
fn err_not_on_int() {
    check_err("fn foo() -> Bool { !42 }", "logical not requires");
}

#[test]
fn err_not_a_function() {
    check_err("fn foo() -> Int { let x = 42\n x(1) }", "not a function");
}

#[test]
fn err_wrong_arg_count() {
    check_err(
        "fn bar(x: Int) -> Int { x }\nfn foo() -> Int { bar(1, 2) }",
        "expected 1 argument",
    );
}

#[test]
fn err_negation_on_bool() {
    check_err("fn foo() -> Bool { -true }", "negation requires");
}

// ── ADT / constructor tests ──────────────────────────────────────────

#[test]
fn infer_adt_constructor_call() {
    check_ok(
        "type Option<T> = Some(T) | None
         fn foo() -> Option<Int> { Some(42) }",
    );
}

#[test]
fn infer_adt_nullary_constructor() {
    check_ok(
        "type Option<T> = Some(T) | None
         fn foo() -> Option<Int> { None }",
    );
}

#[test]
fn infer_match_basic() {
    check_ok(
        "type Bool2 = True | False
         fn foo(x: Bool2) -> Int {
             match (x) {
                 True => 1
                 False => 0
             }
         }",
    );
}

#[test]
fn infer_match_with_bind() {
    check_ok(
        "type Option<T> = Some(T) | None
         fn foo(x: Option<Int>) -> Int {
             match (x) {
                 Some(v) => v
                 None => 0
             }
         }",
    );
}

// ── Exhaustiveness tests ─────────────────────────────────────────────

#[test]
fn err_non_exhaustive_match() {
    check_err(
        "type Color = Red | Green | Blue
         fn foo(c: Color) -> Int {
             match (c) {
                 Red => 1
                 Green => 2
             }
         }",
        "non-exhaustive",
    );
}

#[test]
fn exhaustive_with_wildcard() {
    check_ok(
        "type Color = Red | Green | Blue
         fn foo(c: Color) -> Int {
             match (c) {
                 Red => 1
                 _ => 0
             }
         }",
    );
}

#[test]
fn err_redundant_arm() {
    check_err(
        "type Bool2 = True | False
         fn foo(x: Bool2) -> Int {
             match (x) {
                 True => 1
                 False => 0
                 True => 2
             }
         }",
        "redundant",
    );
}

#[test]
fn err_redundant_arm_after_wildcard() {
    check_err(
        "type Bool2 = True | False
         fn foo(x: Bool2) -> Int {
             match (x) {
                 _ => 0
                 True => 1
             }
         }",
        "redundant",
    );
}

// ── Effect checking tests ────────────────────────────────────────────

#[test]
fn effect_pure_calling_pure_ok() {
    check_ok(
        "fn pure_fn(x: Int) -> Int { x }
         fn foo() -> Int { pure_fn(42) }",
    );
}

#[test]
fn effect_with_cap_calling_effectful_ok() {
    check_ok(
        "effect Console
         fn effectful() -> Unit with Console { }
         fn foo() -> Unit with Console { effectful() }",
    );
}

#[test]
fn err_effect_violation() {
    check_err(
        "effect Console
         fn effectful() -> Unit with Console { }
         fn foo() -> Unit { effectful() }",
        "effect violation",
    );
}

// ── Hole tests ───────────────────────────────────────────────────────

#[test]
fn hole_infers_expected_type() {
    let (result, _interner) = check("fn foo() -> Int { _ }");
    // Should have a hole recorded.
    let has_hole = result.fn_results.values().any(|r| !r.holes.is_empty());
    assert!(has_hole, "expected at least one hole to be recorded");
}

#[test]
fn hole_records_expected_type() {
    let (result, _interner) = check("fn foo() -> Int { _ }");
    for r in result.fn_results.values() {
        for hole in &r.holes {
            if let Some(ty) = &hole.expected_type {
                assert_eq!(*ty, Ty::Int);
                return;
            }
        }
    }
    panic!("expected hole with expected_type = Int");
}

// ── Record tests ─────────────────────────────────────────────────────

#[test]
fn infer_record_literal() {
    check_ok(
        "type Point = { x: Int, y: Int }
         fn foo() -> Point { Point { x: 1, y: 2 } }",
    );
}

#[test]
fn infer_structural_record() {
    // Structural record without a named type — just returns Unit (since
    // we can't express the return type for anonymous records yet).
    check_ok("fn foo() { let r = { x: 1, y: 2 }\n r }");
}

// ── Lambda tests ─────────────────────────────────────────────────────

#[test]
fn infer_lambda_with_annotation() {
    check_ok("fn foo() -> fn(Int) -> Int { fn(x: Int) => x }");
}

// ── Pipeline desugaring + type check ─────────────────────────────────

#[test]
fn infer_pipeline() {
    check_ok(
        "fn double(x: Int) -> Int { x + x }
         fn foo() -> Int { 21 |> double }",
    );
}

#[test]
fn infer_pipeline_with_args() {
    check_ok(
        "fn add(x: Int, y: Int) -> Int { x + y }
         fn foo() -> Int { 1 |> add(2) }",
    );
}

// ── Edge cases ───────────────────────────────────────────────────────

#[test]
fn error_propagation_no_cascade() {
    // A type error in one place shouldn't cause cascading errors.
    // The function has a return type mismatch but the inner expressions
    // should still get consistent types.
    let (result, _) = check("fn foo() -> Int { true }");
    // Should get exactly one type mismatch, not multiple.
    let type_mismatches: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("type mismatch"))
        .collect();
    assert_eq!(
        type_mismatches.len(),
        1,
        "expected exactly 1 type mismatch, got: {type_mismatches:?}"
    );
}

#[test]
fn missing_expr_produces_error_type() {
    // Functions with parse errors should not panic.
    let (result, _) = check("fn foo() -> Int { }");
    // Should get a mismatch since empty block returns Unit.
    let has_mismatch = result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("type mismatch"));
    assert!(has_mismatch);
}

#[test]
fn multiple_functions_checked() {
    check_ok(
        "fn a() -> Int { 1 }
         fn b() -> Bool { true }
         fn c() -> Int { a() }",
    );
}

#[test]
fn comparison_returns_bool() {
    check_ok("fn foo() -> Bool { 1 >= 2 }");
}

// ── Unresolved name tests ───────────────────────────────────────────

#[test]
fn err_unresolved_name() {
    check_err("fn main() -> Int { foo }", "unresolved name");
}

#[test]
fn err_unresolved_name_in_expr() {
    check_err("fn main() -> Int { foo + 1 }", "unresolved name");
}

// ── Unresolved-name diagnostics ─────────────────────────────────────

#[test]
fn err_unresolved_name_has_no_suggestion_suffix_for_unknown_name() {
    let (result, _) = check("fn main() -> Int { totally_unknown() }");
    let diag = result
        .diagnostics
        .iter()
        .find(|d| d.message.contains("unresolved name"))
        .expect("expected unresolved name diagnostic");
    assert!(
        !diag.message.contains(';'),
        "unresolved-name diagnostic should not append suggestion suffix: {:?}",
        diag.message
    );
}

#[test]
fn err_unresolved_name_has_no_suggestion_suffix_for_non_canonical_guess() {
    let (result, _) = check("fn main() -> Int { binary_search(1, 2) }");
    let diag = result
        .diagnostics
        .iter()
        .find(|d| d.message.contains("unresolved name"))
        .expect("expected unresolved name diagnostic");
    assert!(
        !diag.message.contains(';'),
        "unresolved-name diagnostic should not append suggestion suffix: {:?}",
        diag.message
    );
}

// ── Scope resolution tests ──────────────────────────────────────────

#[test]
fn nested_shadowing_resolves_correctly() {
    // Outer x is Int; inner block shadows x as Bool; after the block, x should still be Int.
    check_ok(
        "fn foo() -> Int {
            let x: Int = 1
            let y: Bool = { let x = true\n x }
            x + 1
        }",
    );
}

#[test]
fn match_arm_scope_isolation() {
    // Bindings in one match arm must not leak to the next arm or after the match.
    check_ok(
        "type Option<T> = Some(T) | None
         fn foo(o: Option<Int>) -> Int {
             let result = match (o) {
                 Some(v) => v
                 None => 0
             }
             result
         }",
    );
}

// ── Diagnostic span precision tests ─────────────────────────────────

#[test]
fn diagnostic_span_is_expression_precise() {
    // A multi-expression function with a type error in only one expression.
    // The raw diagnostic span should be smaller than the full function range.
    let src = "fn foo() -> Int {
        let a = 1
        let b: Int = true
        a
    }";
    let (result, _) = check(src);

    // Find the function's full text range.
    let root = parse_source(src);
    let fn_def = root
        .descendants()
        .find_map(kyokara_syntax::ast::nodes::FnDef::cast)
        .expect("should find fn def");
    let fn_range = fn_def.syntax().text_range();

    // Find the raw diagnostic for the type mismatch.
    let mismatch_diag = result
        .raw_diagnostics
        .iter()
        .find(|(d, _)| {
            matches!(
                d,
                kyokara_hir_ty::diagnostics::TyDiagnosticData::TypeMismatch { .. }
            )
        })
        .expect("expected a TypeMismatch diagnostic");

    let diag_range = mismatch_diag.1.range;
    assert!(
        diag_range.len() < fn_range.len(),
        "diagnostic span ({diag_range:?}) should be smaller than function span ({fn_range:?})"
    );
}

// ── Named argument tests ────────────────────────────────────────────

#[test]
fn named_args_satisfy_arity() {
    // All-named call should type-check without arity errors.
    check_ok(
        "fn add(x: Int, y: Int) -> Int { x + y }
         fn main() -> Int { add(x: 1, y: 2) }",
    );
}

#[test]
fn named_args_reordered_type_checks() {
    // Reordered named args should still satisfy arity and type-check.
    check_ok(
        "fn sub(x: Int, y: Int) -> Int { x - y }
         fn main() -> Int { sub(y: 10, x: 3) }",
    );
}

#[test]
fn positional_args_still_work() {
    // Guard: positional args should continue to work as before.
    check_ok(
        "fn add(x: Int, y: Int) -> Int { x + y }
         fn main() -> Int { add(1, 2) }",
    );
}

#[test]
fn named_args_unknown_name_is_diagnostic() {
    check_err(
        "fn add(x: Int, y: Int) -> Int { x + y }
         fn main() -> Int { add(z: 1, y: 2) }",
        "unknown named argument",
    );
}

#[test]
fn named_args_duplicate_name_is_diagnostic() {
    check_err(
        "fn add(x: Int, y: Int) -> Int { x + y }
         fn main() -> Int { add(x: 1, x: 2) }",
        "duplicate named argument",
    );
}

#[test]
fn named_args_missing_parameter_is_diagnostic() {
    check_err(
        "fn add(x: Int, y: Int) -> Int { x + y }
         fn main() -> Int { add(x: 1, x: 2) }",
        "missing argument for parameter `y`",
    );
}

#[test]
fn named_args_reordered_on_direct_lambda_type_checks() {
    check_ok(
        "fn main() -> Int {
           (fn(x: Int, y: Int) => x - y)(y: 10, x: 3)
         }",
    );
}

#[test]
fn named_args_reordered_on_local_fn_value_type_checks() {
    check_ok(
        "fn sub(x: Int, y: Int) -> Int { x - y }
         fn main() -> Int {
           let f = sub
           f(y: 10, x: 3)
         }",
    );
}

#[test]
fn named_args_reordered_on_local_lambda_value_type_checks() {
    check_ok(
        "fn main() -> Int {
           let f = fn(x: Int, y: Int) => x - y
           f(y: 10, x: 3)
         }",
    );
}

#[test]
fn named_args_unknown_on_direct_lambda_is_diagnostic() {
    check_err(
        "fn main() -> Int {
           (fn(x: Int, y: Int) => x + y)(z: 1, y: 2)
         }",
        "unknown named argument",
    );
}

#[test]
fn named_args_duplicate_on_direct_lambda_is_diagnostic() {
    check_err(
        "fn main() -> Int {
           (fn(x: Int, y: Int) => x + y)(x: 1, x: 2)
         }",
        "duplicate named argument",
    );
}

#[test]
fn named_args_missing_on_direct_lambda_is_diagnostic() {
    check_err(
        "fn main() -> Int {
           (fn(x: Int, y: Int) => x + y)(x: 1, x: 2)
         }",
        "missing argument for parameter `y`",
    );
}

#[test]
fn named_args_positional_after_named_on_direct_lambda_is_diagnostic() {
    check_err(
        "fn main() -> Int {
           (fn(x: Int, y: Int) => x + y)(x: 1, 2)
         }",
        "positional argument cannot appear after named argument",
    );
}

#[test]
fn named_args_unknown_on_local_fn_value_is_diagnostic() {
    check_err(
        "fn add(x: Int, y: Int) -> Int { x + y }
         fn main() -> Int {
           let f = add
           f(z: 1, y: 2)
         }",
        "unknown named argument",
    );
}

#[test]
fn named_args_duplicate_on_local_fn_value_is_diagnostic() {
    check_err(
        "fn add(x: Int, y: Int) -> Int { x + y }
         fn main() -> Int {
           let f = add
           f(x: 1, x: 2)
         }",
        "duplicate named argument",
    );
}

#[test]
fn named_args_missing_on_local_fn_value_is_diagnostic() {
    check_err(
        "fn add(x: Int, y: Int) -> Int { x + y }
         fn main() -> Int {
           let f = add
           f(x: 1, x: 2)
         }",
        "missing argument for parameter `y`",
    );
}

#[test]
fn named_args_positional_after_named_on_local_fn_value_is_diagnostic() {
    check_err(
        "fn add(x: Int, y: Int) -> Int { x + y }
         fn main() -> Int {
           let f = add
           f(x: 1, 2)
         }",
        "positional argument cannot appear after named argument",
    );
}

#[test]
fn named_args_unknown_on_local_lambda_value_is_diagnostic() {
    check_err(
        "fn main() -> Int {
           let f = fn(x: Int, y: Int) => x + y
           f(z: 1, y: 2)
         }",
        "unknown named argument",
    );
}

#[test]
fn named_args_duplicate_on_local_lambda_value_is_diagnostic() {
    check_err(
        "fn main() -> Int {
           let f = fn(x: Int, y: Int) => x + y
           f(x: 1, x: 2)
         }",
        "duplicate named argument",
    );
}

#[test]
fn named_args_missing_on_local_lambda_value_is_diagnostic() {
    check_err(
        "fn main() -> Int {
           let f = fn(x: Int, y: Int) => x + y
           f(x: 1, x: 2)
         }",
        "missing argument for parameter `y`",
    );
}

#[test]
fn named_args_positional_after_named_on_local_lambda_value_is_diagnostic() {
    check_err(
        "fn main() -> Int {
           let f = fn(x: Int, y: Int) => x + y
           f(x: 1, 2)
         }",
        "positional argument cannot appear after named argument",
    );
}

#[test]
fn named_args_positional_after_named_is_diagnostic() {
    check_err(
        "fn add(x: Int, y: Int) -> Int { x + y }
         fn main() -> Int { add(x: 1, 2) }",
        "positional argument cannot appear after named argument",
    );
}

// ── Path-qualified record literal validation (#126) ─────────────────

#[test]
fn path_record_lit_non_record_type_is_error() {
    // Foo is an ADT (enum), not a record type — should emit a diagnostic.
    check_err(
        "type Foo = A | B
         fn main() -> Int {
           let r = Foo { x: 1 }
           0
         }",
        "not a record type",
    );
}

#[test]
fn path_record_lit_valid_record_still_works() {
    // Guard: legitimate named record literals still work.
    check_ok(
        "type Point = { x: Int, y: Int }
         fn main() -> Int {
           let p = Point { x: 1, y: 2 }
           p.x + p.y
         }",
    );
}
