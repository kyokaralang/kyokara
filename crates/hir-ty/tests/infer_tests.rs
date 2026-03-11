//! End-to-end type inference tests: parse → collect item tree → lower → type check → assert.
#![allow(clippy::unwrap_used)]

use std::borrow::Cow;

use kyokara_hir_def::builtins::{
    activate_synthetic_imports, register_builtin_intrinsics, register_builtin_methods,
    register_builtin_traits, register_builtin_types, register_static_methods,
    register_synthetic_modules,
};
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

fn normalize_immutable_collection_constructor_import(source: &str) -> Cow<'_, str> {
    let uses_collections_module = source.contains("collections.");
    if uses_collections_module && !source.contains("import collections") {
        Cow::Owned(format!("import collections\n{source}"))
    } else {
        Cow::Borrowed(source)
    }
}

/// Parse, collect, and type-check, returning the result.
fn check(src: &str) -> (TypeCheckResult, Interner) {
    let src = normalize_immutable_collection_constructor_import(src);
    let root = parse_source(src.as_ref());
    let sf = SourceFile::cast(root.clone()).unwrap();
    let mut interner = Interner::new();
    let mut item_result = collect_item_tree(&sf, file_id(), &mut interner);
    register_builtin_types(
        &mut item_result.tree,
        &mut item_result.module_scope,
        &mut interner,
    );
    register_builtin_traits(
        &mut item_result.tree,
        &mut item_result.module_scope,
        &mut interner,
    );
    register_builtin_intrinsics(
        &mut item_result.tree,
        &mut item_result.module_scope,
        &mut interner,
    );
    register_builtin_methods(&mut item_result.module_scope, &mut interner);
    register_synthetic_modules(
        &mut item_result.tree,
        &mut item_result.module_scope,
        &mut interner,
    );
    register_static_methods(&mut item_result.module_scope, &mut interner);
    activate_synthetic_imports(
        &item_result.tree,
        &mut item_result.module_scope,
        &mut interner,
    );
    let result = check_module(
        &root,
        &item_result.tree,
        &item_result.module_scope,
        &[],
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

#[test]
fn infer_expr_hot_paths_do_not_clone_whole_expr_nodes() {
    let src = include_str!("../src/infer/expr.rs");
    assert!(
        !src.contains("let expr = self.body.exprs[idx].clone();"),
        "infer_expr_inner must borrow Expr nodes instead of cloning whole nodes"
    );
    assert!(
        !src.contains("self.body.exprs[callee].clone()"),
        "infer_call hot path must avoid whole Expr clones for field-call dispatch"
    );
}

#[test]
fn infer_pattern_hot_paths_do_not_clone_whole_pattern_nodes() {
    let expr_src = include_str!("../src/infer/expr.rs");
    let pat_src = include_str!("../src/infer/pat.rs");
    assert!(
        !expr_src.contains("self.body.pats[pat_idx].clone()"),
        "irrefutable-let pattern checks must borrow patterns"
    );
    assert!(
        !pat_src.contains("let pat = self.body.pats[pat_idx].clone();"),
        "infer_pat must borrow pattern nodes instead of cloning whole nodes"
    );
}

#[test]
fn infer_large_body_stress_parity() {
    let mut src = String::from("fn foo() -> Int {\n");
    src.push_str("  let seed = 0\n");
    for i in 0..300 {
        src.push_str(&format!("  let v{i} = seed + {i}\n"));
    }
    src.push_str("  seed\n");
    src.push('}');
    check_ok(&src);
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
fn infer_char_code_method() {
    check_ok("fn foo() -> Int { 'a'.code() }");
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
fn infer_binary_equality_rejects_non_comparable_types() {
    check_err(
        "fn foo() -> Bool { collections.List.new() == collections.List.new() }",
        "equality operator requires",
    );
}

#[test]
fn infer_builtin_trait_qualified_call() {
    check_ok("fn foo() -> Int { Ord.compare(1, 2) }");
}

#[test]
fn infer_generic_trait_bound_body() {
    check_ok("fn less<T: Ord>(a: T, b: T) -> Bool { Ord.compare(a, b) < 0 }");
}

#[test]
fn infer_user_impl_trait_call() {
    check_ok(
        "trait Show { fn show(self) -> String }\n\
         type Point = { x: Int }\n\
         impl Show for Point { fn show(self) -> String { \"p\" } }\n\
         fn foo(p: Point) -> String { Show.show(p) }",
    );
}

#[test]
fn infer_derived_trait_call() {
    check_ok(
        "type Point derive(Eq) = { x: Int, y: Int }\n\
         fn foo(a: Point, b: Point) -> Bool { Eq.eq(a, b) }",
    );
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
fn infer_else_if_chain() {
    check_ok("fn foo() -> Int { if (true) { 1 } else if (false) { 2 } else { 3 } }");
}

#[test]
fn infer_else_if_equivalent_to_nested_else_if_form() {
    check_ok("fn foo() -> Int { if (true) { 1 } else if (false) { 2 } else { 3 } }");
    check_ok("fn foo() -> Int { if (true) { 1 } else { if (false) { 2 } else { 3 } } }");
}

#[test]
fn infer_else_if_without_final_else_matches_nested_error() {
    let else_if = "fn foo() -> Int { if (true) { 1 } else if (false) { 2 } }";
    let nested = "fn foo() -> Int { if (true) { 1 } else { if (false) { 2 } } }";
    check_err(else_if, "type mismatch");
    check_err(nested, "type mismatch");
}

#[test]
fn infer_newline_parenthesized_range_after_let_is_separate_expression() {
    check_ok(
        "fn main() -> Int {
           (0..<1).fold(0, fn(acc: Int, i: Int) => {
             let base = i
             ((i + 1)..<4).fold(acc, fn(a: Int, j: Int) => a + j + base)
           })
         }",
    );
}

#[test]
fn infer_if_no_else_is_unit() {
    check_ok("fn foo() { if (true) { 1 } }");
}

#[test]
fn infer_while_and_for_with_traversable_sources() {
    check_ok(
        "fn foo(xs: List<Int>, ys: MutableList<Int>, zs: Deque<Int>) {
           for (x in 0..<3) { x }
           for (x in xs) { x }
           for (y in ys) { y }
           for (z in zs) { z }
           while (true) { break }
         }",
    );
}

#[test]
fn err_for_source_not_traversable() {
    check_err(
        "fn foo() { for (x in 1) { x } }",
        "for source must be traversable",
    );
}

#[test]
fn err_break_outside_loop() {
    check_err("fn foo() { break }", "`break` used outside loop");
}

#[test]
fn err_continue_outside_loop() {
    check_err("fn foo() { continue }", "`continue` used outside loop");
}

#[test]
fn err_for_pattern_must_be_irrefutable() {
    check_err(
        "fn foo(xs: List<Option<Int>>) { for (Option.Some(x) in xs) { x } }",
        "for loop pattern must be irrefutable",
    );
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
        "type Maybe<T> = Just(T) | Nothing
         fn foo() -> Maybe<Int> { Maybe.Just(42) }",
    );
}

#[test]
fn infer_adt_nullary_constructor() {
    check_ok(
        "type Maybe<T> = Just(T) | Nothing
         fn foo() -> Maybe<Int> { Maybe.Nothing }",
    );
}

#[test]
fn infer_match_basic() {
    check_ok(
        "type Bool2 = True | False
         fn foo(x: Bool2) -> Int {
             match (x) {
                 Bool2.True => 1
                 Bool2.False => 0
             }
         }",
    );
}

#[test]
fn infer_match_with_bind() {
    check_ok(
        "type Maybe<T> = Just(T) | Nothing
         fn foo(x: Maybe<Int>) -> Int {
             match (x) {
                 Maybe.Just(v) => v
                 Maybe.Nothing => 0
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
                 Color.Red => 1
                 Color.Green => 2
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
                 Color.Red => 1
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
                 Bool2.True => 1
                 Bool2.False => 0
                 Bool2.True => 2
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
                 Bool2.True => 1
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
    check_ok("fn foo() -> { x: Int, y: Int } { let r = { x: 1, y: 2 }\n r }");
}

#[test]
fn infer_structural_record_field_order_is_irrelevant() {
    check_ok(
        "fn take(p: { x: Int, y: Int }) -> Int { p.x }
         fn main() -> Int { take({ y: 2, x: 1 }) }",
    );
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

// ── Iteration ergonomics inference tests (#259) ────────────────────

#[test]
fn infer_iteration_ergonomics_happy_paths() {
    let cases = ["fn main() -> Bool {
            let xs = (0..<5)
            let e = xs.enumerate().to_list()
            let z = xs.zip(collections.MutableList.new().push(10).push(20)).to_list()
            let c = xs.chunks(2).to_list()
            let w = xs.windows(3).to_list()
            e.len() > 0 && z.len() == 2 && c.len() == 3 && w.len() == 3
        }"];

    for src in cases {
        let (result, _) = check(src);
        assert!(
            result.diagnostics.is_empty(),
            "expected no diagnostics, got: {:?}\nsource:\n{src}",
            result.diagnostics
        );
    }
}

#[test]
fn infer_seq_any_all_find_happy_paths() {
    let cases = [r#"fn main() -> Int {
            let xs = (0..<5)
            let has_three = xs.any(fn(n: Int) => n == 3)
            let all_small = xs.all(fn(n: Int) => n < 5)
            let first_even = xs.find(fn(n: Int) => n % 2 == 0).map_or(-1, fn(n: Int) => n)
            let empty_any = (0..<0).any(fn(_n: Int) => true)
            let empty_all = (0..<0).all(fn(_n: Int) => false)
            let empty_find = (0..<0).find(fn(_n: Int) => true).unwrap_or(-1)

            if (has_three && all_small && empty_all && empty_find == -1 && empty_any == false && first_even == 0) {
                1
            } else {
                0
            }
        }"#];

    for src in cases {
        let (result, _) = check(src);
        assert!(
            result.diagnostics.is_empty(),
            "expected no diagnostics, got: {:?}\nsource:\n{src}",
            result.diagnostics
        );
    }
}

#[test]
fn infer_seq_flat_map_happy_paths() {
    let cases = [
        r#"import collections

fn main() -> Int {
    let a = collections.MutableList.new().push(1).push(2)
        .flat_map(fn(n: Int) => collections.MutableList.new().push(n).push(n + 10))
        .count()
    let b = collections.MutableList.from_list(collections.MutableList.new().push(1).push(2).to_list())
        .flat_map(fn(n: Int) => collections.MutableList.from_list(collections.MutableList.new().push(n).push(n + 1).to_list()))
        .count()
    let c = collections.Deque.new().appended(1).appended(2)
        .flat_map(fn(n: Int) => collections.Deque.new().appended(n).appended(n + 1))
        .count()
    let d = "a,b\nc,d".lines().flat_map(fn(line: String) => line.split(",")).count()
    a + b + c + d
}"#,
        r#"import collections

type Tokens = { items: List<String> }

impl IntoTraversal<String> for Tokens {
    fn into_seq(self) -> Seq<String> {
        self.items.map(fn(x: String) => x)
    }
}

fn tokenize(line: String) -> Tokens {
    Tokens { items: line.split(",").to_list() }
}

fn main() -> Int {
    let count = "a,b\nc,d".lines().flat_map(fn(line: String) => tokenize(line)).count()
    let direct = IntoTraversal.into_seq(Tokens {
        items: collections.MutableList.new().push("x").push("y").to_list()
    }).count()
    count * 10 + direct
}"#,
    ];

    for src in cases {
        let (result, _) = check(src);
        assert!(
            result.diagnostics.is_empty(),
            "expected no diagnostics, got: {:?}\nsource:\n{src}",
            result.diagnostics
        );
    }
}

#[test]
fn infer_seq_flat_map_requires_into_traversal() {
    let (result, _) = check("fn main() -> Int { (0..<3).flat_map(fn(n: Int) => n).count() }");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("IntoTraversal") || d.message.contains("trait")),
        "expected IntoTraversal/trait diagnostic, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn infer_seq_contains_happy_paths() {
    let cases = [r#"import collections

fn main() -> Int {
    let a = collections.MutableList.new().push(1).push(2).contains(2)
    let b = collections.MutableList.from_list(collections.MutableList.new().push(1).push(2).to_list()).contains(1)
    let c = collections.Deque.new().appended(1).appended(2).contains(3)
    let d = (0..<4).contains(3)
    let e = "a,b,c".split(",").contains("b")

    if (a && b && !c && d && e) {
        1
    } else {
        0
    }
}"#];

    for src in cases {
        let (result, _) = check(src);
        assert!(
            result.diagnostics.is_empty(),
            "expected no diagnostics, got: {:?}\nsource:\n{src}",
            result.diagnostics
        );
    }
}

#[test]
fn infer_seq_contains_requires_eq() {
    let (result, _) =
        check("fn main() -> Bool { (0..<3).map(fn(n: Int) => n.to_float()).contains(1.0) }");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("Eq") || d.message.contains("trait")),
        "expected Eq/trait diagnostic, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn infer_seq_scan_unfold_int_pow_happy_paths() {
    let cases = [r#"fn main() -> Int {
            let scanned = (1..<4).scan(0, fn(acc: Int, n: Int) => acc + n).to_list()
            let unfolded = (0).unfold(fn(state: Int) =>
                if (state < 3) {
                    Option.Some({ value: state + 1, state: state + 1 })
                } else {
                    Option.None
                }
            ).to_list()
            let p = 2.pow(10)
            scanned.len() + unfolded.len() + p
        }"#];

    for src in cases {
        let (result, _) = check(src);
        assert!(
            result.diagnostics.is_empty(),
            "expected no diagnostics, got: {:?}\nsource:\n{src}",
            result.diagnostics
        );
    }
}

#[test]
fn infer_seq_unfold_accepts_named_record_alias_payload() {
    let src = r#"type PickStep = { value: Int, state: Int }

fn main() -> Int {
    let unfolded = (0).unfold(fn(state: Int) =>
        if (state < 3) {
            Option.Some(PickStep { value: state + 1, state: state + 1 })
        } else {
            Option.None
        }
    ).to_list()
    unfolded.len()
}"#;

    let (result, _) = check(src);
    assert!(
        result.diagnostics.is_empty(),
        "expected no diagnostics, got: {:?}\nsource:\n{src}",
        result.diagnostics
    );
}

#[test]
fn infer_result_ergonomics_happy_paths() {
    let cases = [
        "fn main() -> Int { \"42\".parse_int().unwrap_or(0) }",
        "fn main() -> Int { \"42\".parse_int().map_or(0, fn(n: Int) => n + 1) }",
        "fn main() -> Int { \"abc\".parse_int().map_or(7, fn(n: Int) => n + 1) }",
        "fn main() -> String { \"abc\".md5() }",
        "import hash\nfn main() -> String { hash.md5(\"abc\") }",
    ];

    for src in cases {
        let (result, _) = check(src);
        assert!(
            result.diagnostics.is_empty(),
            "expected no diagnostics, got: {:?}\nsource:\n{src}",
            result.diagnostics
        );
    }
}

#[test]
fn infer_option_result_combinator_parity_happy_paths() {
    let cases = [
        "fn main() -> Int { collections.MutableList.new().push(41).head().unwrap_or(0) }",
        "fn main() -> Int { collections.MutableList.new().push(41).head().map_or(0, fn(n: Int) => n + 1) }",
        "fn main() -> Int { collections.MutableList.new().push(41).head().map(fn(n: Int) => n + 1).unwrap_or(0) }",
        "fn main() -> Int { collections.MutableList.new().push(41).head().and_then(fn(n: Int) => Option.Some(n + 1)).unwrap_or(0) }",
        "fn main() -> Int { \"41\".parse_int().map(fn(n: Int) => n + 1).unwrap_or(0) }",
        "fn main() -> Int { \"41\".parse_int().and_then(fn(n: Int) => Result.Ok(n + 1)).unwrap_or(0) }",
        "fn main() -> Int {
            match (\"oops\".parse_int().map_err(fn(_e: ParseError) => 7)) {
                Result.Ok(n) => n
                Result.Err(e) => e
            }
        }",
    ];

    for src in cases {
        let (result, _) = check(src);
        assert!(
            result.diagnostics.is_empty(),
            "expected no diagnostics, got: {:?}\nsource:\n{src}",
            result.diagnostics
        );
    }
}

#[test]
fn err_seq_any_all_find_wrong_arity_or_predicate_type() {
    struct Case<'a> {
        src: &'a str,
        expected_fragment: &'a str,
    }

    let cases = [
        Case {
            src: "fn main() -> Bool { (0..<3).any() }",
            expected_fragment: "expected 1 argument(s)",
        },
        Case {
            src: "fn main() -> Bool { (0..<3).all(fn(n: Int) => n) }",
            expected_fragment: "type mismatch",
        },
        Case {
            src: "fn main() -> Int { (0..<3).find(fn(n: Int) => n + 1).unwrap_or(0) }",
            expected_fragment: "type mismatch",
        },
    ];

    for case in cases {
        let (result, _) = check(case.src);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.message.contains(case.expected_fragment)),
            "expected diagnostic containing `{}`; got: {:?}\nsource:\n{}",
            case.expected_fragment,
            result.diagnostics,
            case.src
        );
        assert!(
            result
                .diagnostics
                .iter()
                .all(|d| !d.message.contains("unresolved name")),
            "expected canonical surface to resolve names; got unresolved-name diagnostics: {:?}\nsource:\n{}",
            result.diagnostics,
            case.src
        );
    }
}

#[test]
fn err_seq_count_predicate_wrong_arity_or_predicate_type() {
    struct Case<'a> {
        src: &'a str,
        expected_fragment: &'a str,
    }

    let cases = [
        Case {
            src: "fn main() -> Int { (0..<3).count(fn(n: Int) => n) }",
            expected_fragment: "type mismatch",
        },
        Case {
            src: "fn main() -> Int { (0..<3).count(fn(n: Int) => n == 1, fn(n: Int) => n == 2) }",
            expected_fragment: "expected 0 or 1 argument(s)",
        },
    ];

    for case in cases {
        let (result, _) = check(case.src);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.message.contains(case.expected_fragment)),
            "expected diagnostic containing `{}`; got: {:?}\nsource:\n{}",
            case.expected_fragment,
            result.diagnostics,
            case.src
        );
    }
}

#[test]
fn err_seq_scan_unfold_int_pow_wrong_arity_or_types() {
    struct Case<'a> {
        src: &'a str,
        expected_fragment: &'a str,
    }

    let cases = [
        Case {
            src: "fn main() -> Int { (0..<3).scan(0, fn(acc: Int) => acc).count() }",
            expected_fragment: "type mismatch",
        },
        Case {
            src: "fn main() -> Int { (0).unfold(fn(state: Int) => state + 1).count() }",
            expected_fragment: "type mismatch",
        },
        Case {
            src: "fn main() -> Int { (0).unfold(fn(state: Int) => Option.Some({ value: state + 1 })).count() }",
            expected_fragment: "type mismatch",
        },
        Case {
            src: "fn main() -> Int { 2.pow() }",
            expected_fragment: "expected 1 argument(s)",
        },
        Case {
            src: "fn main() -> Int { 2.pow(true) }",
            expected_fragment: "type mismatch",
        },
    ];

    for case in cases {
        let (result, _) = check(case.src);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.message.contains(case.expected_fragment)),
            "expected diagnostic containing `{}`; got: {:?}\nsource:\n{}",
            case.expected_fragment,
            result.diagnostics,
            case.src
        );
        assert!(
            result
                .diagnostics
                .iter()
                .all(|d| !d.message.contains("unresolved name")),
            "expected canonical surface to resolve names; got unresolved-name diagnostics: {:?}\nsource:\n{}",
            result.diagnostics,
            case.src
        );
    }
}

#[test]
fn err_result_ergonomics_wrong_types_or_arity() {
    struct Case<'a> {
        src: &'a str,
        expected_fragment: &'a str,
    }

    let cases = [
        Case {
            src: "fn main() -> Int { \"42\".parse_int().unwrap_or(\"x\") }",
            expected_fragment: "type mismatch",
        },
        Case {
            src: "fn main() -> Int { \"42\".parse_int().map_or(0) }",
            expected_fragment: "expected 2 argument(s)",
        },
        Case {
            src: "fn main() -> Int { \"42\".parse_int().map_or(0, fn(n: Int) => \"x\") }",
            expected_fragment: "type mismatch",
        },
    ];

    for case in cases {
        let (result, _) = check(case.src);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.message.contains(case.expected_fragment)),
            "expected diagnostic containing `{}`; got: {:?}\nsource:\n{}",
            case.expected_fragment,
            result.diagnostics,
            case.src
        );
        assert!(
            result
                .diagnostics
                .iter()
                .all(|d| !d.message.contains("unresolved name")),
            "expected canonical surface to resolve names; got unresolved-name diagnostics: {:?}\nsource:\n{}",
            result.diagnostics,
            case.src
        );
    }
}

#[test]
fn err_option_result_combinator_wrong_types_or_arity() {
    struct Case<'a> {
        src: &'a str,
        expected_fragment: &'a str,
    }

    let cases = [
        Case {
            src: "fn main() -> Int { collections.List.new().head().unwrap_or() }",
            expected_fragment: "expected 1 argument(s)",
        },
        Case {
            src: "fn main() -> Int { collections.List.new().head().map_or(0) }",
            expected_fragment: "expected 2 argument(s)",
        },
        Case {
            src: "fn main() -> Int { collections.List.new().head().map(fn(n: Int) => n + 1, 0).unwrap_or(0) }",
            expected_fragment: "expected 1 argument(s)",
        },
        Case {
            src: "fn main() -> Int { collections.MutableList.new().push(1).head().and_then(fn(n: Int) => n + 1).unwrap_or(0) }",
            expected_fragment: "type mismatch",
        },
        Case {
            src: "fn main() -> Int { \"42\".parse_int().map_err().unwrap_or(0) }",
            expected_fragment: "expected 1 argument(s)",
        },
        Case {
            src: "fn main() -> Int { \"42\".parse_int().and_then(fn(n: Int) => n + 1).unwrap_or(0) }",
            expected_fragment: "type mismatch",
        },
    ];

    for case in cases {
        let (result, _) = check(case.src);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.message.contains(case.expected_fragment)),
            "expected diagnostic containing `{}`; got: {:?}\nsource:\n{}",
            case.expected_fragment,
            result.diagnostics,
            case.src
        );
        assert!(
            result
                .diagnostics
                .iter()
                .all(|d| !d.message.contains("unresolved name")),
            "expected canonical surface to resolve names; got unresolved-name diagnostics: {:?}\nsource:\n{}",
            result.diagnostics,
            case.src
        );
    }
}

#[test]
fn err_iteration_ergonomics_wrong_arity_or_type() {
    struct Case<'a> {
        src: &'a str,
        expected_fragment: &'a str,
    }

    let cases = [
        Case {
            src: "fn main() -> Int { (true..<3).count() }",
            expected_fragment: "type mismatch",
        },
        Case {
            src: "fn main() -> Int { (0..<false).count() }",
            expected_fragment: "type mismatch",
        },
        Case {
            src: "fn main() -> Int { collections.MutableList.new().push(1).zip(1).count() }",
            expected_fragment: "type mismatch",
        },
        Case {
            src: "fn main() -> Int { collections.MutableList.new().push(1).chunks(true).count() }",
            expected_fragment: "type mismatch",
        },
        Case {
            src: "fn main() -> Int { collections.MutableList.new().push(1).windows(false).count() }",
            expected_fragment: "type mismatch",
        },
    ];

    for case in cases {
        let (result, _) = check(case.src);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.message.contains(case.expected_fragment)),
            "expected diagnostic containing `{}`; got: {:?}\nsource:\n{}",
            case.expected_fragment,
            result.diagnostics,
            case.src
        );
        assert!(
            result
                .diagnostics
                .iter()
                .all(|d| !d.message.contains("unresolved name")),
            "expected canonical surface to resolve names; got unresolved-name diagnostics: {:?}\nsource:\n{}",
            result.diagnostics,
            case.src
        );
    }
}

#[test]
fn infer_deque_and_list_index_update_happy_paths() {
    let cases = [r#"import collections
        fn main() -> Int {
            let q0 = collections.Deque.new().appended(1).appended(2).prepended(0)
            let q1 = match (q0.popped_front()) {
                Option.Some(p) => p.rest.appended(p.value + 10)
                Option.None => q0
            }

            let xs = collections.MutableList.new().push(10).push(20).set(1, 99)
            let ys = xs.update(0, fn(n: Int) => n + 1)
            ys.get(0).unwrap_or(0) + ys.get(1).unwrap_or(0) + q1.len()
        }"#];

    for src in cases {
        let (result, _) = check(src);
        assert!(
            result.diagnostics.is_empty(),
            "expected no diagnostics, got: {:?}\nsource:\n{src}",
            result.diagnostics
        );
    }
}

#[test]
fn infer_deque_pop_back_happy_paths() {
    let cases = [r#"import collections
        fn main() -> Int {
            let q0 = collections.Deque.new().appended(1).appended(2).prepended(0)
            match (q0.popped_back()) {
                Option.Some(p1) => match (p1.rest.popped_back()) {
                    Option.Some(p2) => p1.value + p2.value + p2.rest.len()
                    Option.None => -1
                }
                Option.None => -1
            }
        }"#];

    for src in cases {
        let (result, _) = check(src);
        assert!(
            result.diagnostics.is_empty(),
            "expected no diagnostics, got: {:?}\nsource:\n{src}",
            result.diagnostics
        );
    }
}

#[test]
fn infer_collections_deque_constructor_happy_paths_rfc_0004() {
    let cases = [
        r#"import collections
        fn main() -> Int {
            let q0 = collections.Deque.new().appended(1).appended(2).prepended(0)
            let q1 = match (q0.popped_front()) {
                Option.Some(p) => p.rest.appended(p.value + 10)
                Option.None => q0
            }
            q1.len()
        }"#,
        r#"import collections as c
        fn main() -> Int {
            c.Deque.new().appended(1).appended(2).len()
        }"#,
    ];

    for src in cases {
        let (result, _) = check(src);
        assert!(
            result.diagnostics.is_empty(),
            "expected no diagnostics, got: {:?}\nsource:\n{src}",
            result.diagnostics
        );
    }
}

#[test]
fn err_global_deque_constructor_surface_is_removed_rfc_0004() {
    let (result, _) = check("fn main() -> Int { Deque.new().len() }");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("no method") || d.message.contains("unresolved")),
        "expected removed-surface diagnostic, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn err_global_immutable_list_map_set_constructor_surface_is_removed_rfc_0009() {
    for src in [
        "fn main() -> Int { List.new().len() }",
        "fn main() -> Int { Map.new().len() }",
        "fn main() -> Int { Set.new().len() }",
    ] {
        let (result, _) = check(src);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.message.contains("no method") || d.message.contains("unresolved")),
            "expected removed-surface diagnostic for `{src}`, got: {:?}",
            result.diagnostics
        );
    }
}

#[test]
fn infer_collections_mutable_list_constructor_happy_paths_rfc_0005() {
    let cases = [
        r#"import collections
        fn main() -> Int {
            let xs = collections.MutableList.new().push(1).push(2).set(0, 9)
            let ys = collections.MutableList.from_list(collections.MutableList.new().push(3).push(4).to_list())
            xs.get(0).unwrap_or(0) + ys.len()
        }"#,
        r#"import collections as c
        fn main() -> Int {
            let xs: MutableList<Int> = c.MutableList.new().push(1).push(2)
            xs.update(1, fn(n: Int) => n + 1).len()
        }"#,
        r#"import collections
        fn main() -> Int {
            let xs = collections.MutableList.new().insert(0, 1).insert(1, 3).insert(1, 2)
            let removed = xs.remove_at(0)
            xs.delete_at(1).len() + removed
        }"#,
        r#"import collections
        fn main() -> Int {
            collections.MutableList.from_list(collections.MutableList.new().push(1).push(2).push(3).to_list())
                .map(fn(n: Int) => n * 2)
                .zip(collections.MutableList.new().push(10))
                .count()
        }"#,
        r#"import collections
        fn main() -> Int {
            let xs = collections.MutableList.new().push(1).push(2).push(3)
            let last = xs.last().unwrap_or(0)
            let popped = xs.pop().unwrap_or(0)
            let extended = xs.extend(collections.MutableList.new().push(8).push(9).to_list())
            last + popped + extended.len()
        }"#,
    ];

    for src in cases {
        let (result, _) = check(src);
        assert!(
            result.diagnostics.is_empty(),
            "expected no diagnostics, got: {:?}\nsource:\n{src}",
            result.diagnostics
        );
    }
}

#[test]
fn err_global_mutable_list_constructor_surface_is_removed_rfc_0005() {
    let (result, _) = check("fn main() -> Int { MutableList.new().len() }");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("no method") || d.message.contains("unresolved")),
        "expected removed-surface diagnostic, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn err_mutable_list_wrong_arity_or_type_rfc_0005() {
    struct Case<'a> {
        src: &'a str,
        expected_fragment: &'a str,
    }

    let cases = [
        Case {
            src: "import collections\nfn main() -> Int { collections.MutableList.new(1).len() }",
            expected_fragment: "expected 0 argument(s)",
        },
        Case {
            src: "import collections\nfn main() -> Int { collections.MutableList.from_list(1).len() }",
            expected_fragment: "type mismatch",
        },
        Case {
            src: "import collections\nfn main() -> Int { collections.MutableList.new().push().len() }",
            expected_fragment: "expected 1 argument(s)",
        },
        Case {
            src: "import collections\nfn main() -> Int { collections.MutableList.new().push(1).set(true, 2).len() }",
            expected_fragment: "type mismatch",
        },
        Case {
            src: "import collections\nfn main() -> Int { collections.MutableList.new().push(1).update(0, fn(n: Int) => \"x\").len() }",
            expected_fragment: "type mismatch",
        },
        Case {
            src: "import collections\nfn main() -> Int { collections.MutableList.new().push(1).insert(0, \"x\").len() }",
            expected_fragment: "type mismatch",
        },
        Case {
            src: "import collections\nfn main() -> Int { collections.MutableList.new().insert(false, 1).len() }",
            expected_fragment: "type mismatch",
        },
        Case {
            src: "import collections\nfn main() -> Int { collections.MutableList.new().push(1).get(false).unwrap_or(0) }",
            expected_fragment: "type mismatch",
        },
        Case {
            src: "import collections\nfn main() -> Int { collections.MutableList.new().push(1).delete_at(false).len() }",
            expected_fragment: "type mismatch",
        },
        Case {
            src: "import collections\nfn main() -> Int { collections.MutableList.new().push(1).remove_at(false) }",
            expected_fragment: "type mismatch",
        },
        Case {
            src: "import collections\nfn main() -> Int { collections.MutableList.new().last(1).unwrap_or(0) }",
            expected_fragment: "expected 0 argument(s)",
        },
        Case {
            src: "import collections\nfn main() -> Int { collections.MutableList.new().push(1).pop(1).unwrap_or(0) }",
            expected_fragment: "expected 0 argument(s)",
        },
        Case {
            src: "import collections\nfn main() -> Int { collections.MutableList.new().insert(0).len() }",
            expected_fragment: "expected 2 argument(s)",
        },
        Case {
            src: "import collections\nfn main() -> Int { collections.MutableList.new().push(1).delete_at().len() }",
            expected_fragment: "expected 1 argument(s)",
        },
        Case {
            src: "import collections\nfn main() -> Int { collections.MutableList.new().push(1).remove_at(0, 1) }",
            expected_fragment: "expected 1 argument(s)",
        },
        Case {
            src: "import collections\nfn main() -> Int { collections.MutableList.new().extend(1).len() }",
            expected_fragment: "type mismatch",
        },
    ];

    for case in cases {
        let (result, _) = check(case.src);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.message.contains(case.expected_fragment)),
            "expected diagnostic containing `{}`; got: {:?}\nsource:\n{}",
            case.expected_fragment,
            result.diagnostics,
            case.src
        );
        assert!(
            result
                .diagnostics
                .iter()
                .all(|d| !d.message.contains("unresolved name")),
            "expected canonical surface to resolve names; got unresolved-name diagnostics: {:?}\nsource:\n{}",
            result.diagnostics,
            case.src
        );
    }
}

#[test]
fn err_deque_and_list_index_update_wrong_arity_or_type() {
    struct Case<'a> {
        src: &'a str,
        expected_fragment: &'a str,
    }

    let cases = [
        Case {
            src: "import collections\nfn main() -> Int { collections.Deque.new(1).len() }",
            expected_fragment: "expected 0 argument(s)",
        },
        Case {
            src: "import collections\nfn main() -> Int { collections.Deque.new().appended().len() }",
            expected_fragment: "expected 1 argument(s)",
        },
        Case {
            src: "import collections\nfn main() -> Int { collections.Deque.new().appended(1).popped_back(2).len() }",
            expected_fragment: "expected 0 argument(s)",
        },
        Case {
            src: "fn main() -> Int { collections.MutableList.new().push(1).set(true, 2).len() }",
            expected_fragment: "type mismatch",
        },
        Case {
            src: "fn main() -> Int { collections.MutableList.new().push(1).set(0, \"x\").len() }",
            expected_fragment: "type mismatch",
        },
        Case {
            src: "fn main() -> Int { collections.MutableList.new().push(1).update(0, fn(n: Int) => \"x\").len() }",
            expected_fragment: "type mismatch",
        },
        Case {
            src: "fn main() -> Int { collections.MutableList.new().push(1).update(0).len() }",
            expected_fragment: "expected 2 argument(s)",
        },
    ];

    for case in cases {
        let (result, _) = check(case.src);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.message.contains(case.expected_fragment)),
            "expected diagnostic containing `{}`; got: {:?}\nsource:\n{}",
            case.expected_fragment,
            result.diagnostics,
            case.src
        );
        assert!(
            result
                .diagnostics
                .iter()
                .all(|d| !d.message.contains("unresolved name")),
            "expected canonical surface to resolve names; got unresolved-name diagnostics: {:?}\nsource:\n{}",
            result.diagnostics,
            case.src
        );
    }
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

#[test]
fn err_char_code_wrong_receivers() {
    check_err(
        r#"fn main() -> Int { "a".code() }"#,
        "no method `code` on type `String`",
    );
    check_err(
        "fn main() -> Int { 1.code() }",
        "no method `code` on type `Int`",
    );
}

#[test]
fn err_non_canonical_free_range_function_is_unresolved() {
    check_err("fn main() -> Int { range(0, 3).len() }", "unresolved name");
}

#[test]
fn infer_seq_surface_happy_paths() {
    let cases = [
        r#"fn main() -> Int {
            let xs = (0..<5)
            xs.map(fn(n: Int) => n + 1)
                .filter(fn(n: Int) => n > 2)
                .count()
        }"#,
        r#"fn main() -> Int {
            let xs = collections.MutableList.new().push(1).push(2).push(3)
            xs.map(fn(n: Int) => n * 2).to_list().len()
        }"#,
        r#"fn main() -> Int {
            let keys = collections.MutableMap.new().insert("a", 1).insert("b", 2).keys()
            keys.count()
        }"#,
        r#"fn main() -> Int {
            "a,b,c".split(",").count()
        }"#,
    ];

    for src in cases {
        let (result, _) = check(src);
        assert!(
            result.diagnostics.is_empty(),
            "expected no diagnostics, got: {:?}\nsource:\n{src}",
            result.diagnostics
        );
    }
}

#[test]
fn err_list_traversal_surface_is_removed() {
    let cases = [
        "fn main() -> Int { List.range(0, 5).len() }",
        "fn main() -> Int { collections.MutableList.new().push(1).seq().count() }",
    ];

    for src in cases {
        let (result, _) = check(src);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.message.contains("no method") || d.message.contains("unresolved")),
            "expected removed-surface diagnostic, got: {:?}\nsource:\n{src}",
            result.diagnostics
        );
    }
}

#[test]
fn infer_collection_first_traversal_surface_happy_paths_rfc_0002() {
    let cases = [
        r#"fn main() -> Int {
            let xs = collections.MutableList.new().push(1).push(2).push(3)
            xs.map(fn(n: Int) => n + 1)
                .filter(fn(n: Int) => n > 2)
                .count()
        }"#,
        r#"import collections
        fn main() -> Int {
            let xs = collections.Deque.new().appended(1).appended(2).appended(3)
            xs.map(fn(n: Int) => n * 2).to_list().len()
        }"#,
        r#"import collections
        fn main() -> Int {
            let a = collections.MutableList.new().push(1).push(2).zip((10..<13)).count()
            let b = (0..<3).zip(collections.MutableList.new().push(7).push(8)).count()
            let c = collections.Deque.new().appended(1).appended(2).zip(collections.MutableList.new().push(9)).count()
            a + b + c
        }"#,
    ];

    for src in cases {
        let (result, _) = check(src);
        assert!(
            result.diagnostics.is_empty(),
            "expected no diagnostics, got: {:?}\nsource:\n{src}",
            result.diagnostics
        );
    }
}

#[test]
fn infer_seq_frequencies_happy_paths() {
    let cases = [
        r#"fn main() -> Int {
            let counts = collections.MutableList.new().push(3).push(1).push(3).push(2).frequencies()
            counts.get(3).unwrap_or(0) + counts.len()
        }"#,
        r#"fn main() -> Int {
            let counts = "a,b,a,c".split(",").frequencies()
            counts.get("a").unwrap_or(0) + counts.len()
        }"#,
        r#"import collections

        fn main() -> Int {
            let counts = collections.MutableList.from_list(collections.MutableList.new().push(1).push(2).push(1).to_list())
                .frequencies()
            counts.get(1).unwrap_or(0) + counts.len()
        }"#,
        r#"import collections

        fn main() -> Int {
            let counts = collections.Deque.new().appended(1).appended(2).appended(1).frequencies()
            counts.get(1).unwrap_or(0) + counts.len()
        }"#,
        r#"fn main() -> Int {
            let counts = (0..<4).frequencies()
            counts.get(0).unwrap_or(0) + counts.len()
        }"#,
        r#"type P derive(Eq, Hash) = { x: Int }

        fn main() -> Int {
            let p: P = P { x: 1 }
            let counts = collections.MutableList.new().push(p).push(p).frequencies()
            counts.len()
        }"#,
    ];

    for src in cases {
        let (result, _) = check(src);
        assert!(
            result.diagnostics.is_empty(),
            "expected no diagnostics, got: {:?}\nsource:\n{src}",
            result.diagnostics
        );
    }
}

#[test]
fn infer_list_sort_accepts_derived_ord_elements() {
    check_ok(
        r#"type P derive(Ord) = { x: Int }

        fn main() -> Int {
            let a: P = P { x: 2 }
            let b: P = P { x: 1 }
            let xs = collections.MutableList.new().push(a).push(b).sort()
            xs.len()
        }"#,
    );
}

#[test]
fn infer_mutable_priority_queue_surface_happy_paths_rfc_0012() {
    let cases = [
        r#"import collections

        fn main() -> Int {
            let pq: MutablePriorityQueue<Int, String> = collections.MutablePriorityQueue.new_min()
                .push(5, "far")
                .push(1, "near")
            match (pq.peek()) {
                Option.Some(item) => item.priority + pq.len()
                Option.None => 0
            }
        }"#,
        r#"import collections as c

        fn main() -> Int {
            let pq: MutablePriorityQueue<Int, String> = c.MutablePriorityQueue.new_max()
                .push(1, "low")
                .push(9, "high")
            match (pq.pop()) {
                Option.Some(item) => item.priority
                Option.None => 0
            }
        }"#,
        r#"import collections

        type P derive(Ord) = { score: Int }

        fn main() -> Int {
            let a: P = P { score: 2 }
            let b: P = P { score: 1 }
            let pq: MutablePriorityQueue<P, String> = collections.MutablePriorityQueue.new_min()
                .push(a, "a")
                .push(b, "b")
            match (pq.peek()) {
                Option.Some(item) => item.priority.score
                Option.None => 0
            }
        }"#,
    ];

    for src in cases {
        let (result, _) = check(src);
        assert!(
            result.diagnostics.is_empty(),
            "expected no diagnostics, got: {:?}\nsource:\n{src}",
            result.diagnostics
        );
    }
}

#[test]
fn err_mutable_priority_queue_non_ord_priority_reports_trait_diagnostic() {
    let cases = [
        r#"import collections

        fn main() -> Int {
            let pq: MutablePriorityQueue<Float, Int> = collections.MutablePriorityQueue.new_min()
            pq.len()
        }"#,
        r#"import collections

        type P = { score: Int }

        fn main() -> Int {
            let pq: MutablePriorityQueue<P, String> = collections.MutablePriorityQueue.new_max()
            pq.is_empty()
        }"#,
    ];

    for src in cases {
        let (result, _) = check(src);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.message.contains("Ord") || d.message.contains("trait")),
            "expected Ord-bound diagnostic, got: {:?}\nsource:\n{src}",
            result.diagnostics
        );
    }
}

#[test]
fn err_seq_frequencies_non_hashable_element_reports_e0024() {
    let cases = [
        r#"import collections

        fn main() -> Int {
            let inner = collections.MutableList.from_list(collections.MutableList.new().push(1).to_list())
            let counts = collections.MutableList.new().push(inner).frequencies()
            counts.len()
        }"#,
        r#"fn main() -> Int {
            let counts = collections.MutableList.new().push(fn(x: Int) => x).frequencies()
            counts.len()
        }"#,
    ];

    for src in cases {
        let (result, _) = check(src);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.message.contains("map key")),
            "expected E0024 map key diagnostic, got: {:?}\nsource:\n{src}",
            result.diagnostics
        );
    }
}

#[test]
fn err_seq_frequencies_named_record_without_derive_reports_e0024() {
    let (result, _) = check(
        r#"import collections

        type P = { x: Int }

        fn main() -> Int {
            let counts = collections.MutableList.new().push(P { x: 1 }).frequencies()
            counts.len()
        }"#,
    );

    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("map key")),
        "expected E0024 map key diagnostic, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn infer_opaque_traversal_surface_happy_paths_rfc_0003() {
    let cases = [
        r#"fn main() -> Int {
            let a = (0..<5).count()
            let b = (5..<5).count()
            let c = (5..<2).count()
            a + b + c
        }"#,
        r#"fn main() -> Int {
            let a = (0..<6).count(fn(n: Int) => n % 2 == 0)
            let b = collections.MutableList.new().push(1).push(2).push(3).count(fn(n: Int) => n >= 2)
            let c = "a,b,,c".split(",").count(fn(part: String) => part != "")
            let d = collections.MutableList.from_list(collections.MutableList.new().push(1).push(2).push(3).to_list())
                .count(fn(n: Int) => n != 2)
            let e = collections.Deque.new().appended(1).appended(2).appended(3)
                .count(fn(n: Int) => n < 3)
            a + b + c + d + e
        }"#,
        r#"fn main() -> Int {
            let xs = (0).unfold(fn(state: Int) =>
                if (state < 3) {
                    Option.Some({ value: state + 1, state: state + 1 })
                } else {
                    Option.None
                }
            ).to_list()
            xs.len() + xs[0]
        }"#,
        r#"type Seed = { x: Int }

        fn main() -> Int {
            let xs = Seed { x: 0 }.unfold(fn(state: Seed) =>
                if (state.x < 3) {
                    Option.Some({ value: state.x, state: Seed { x: state.x + 1 } })
                } else {
                    Option.None
                }
            ).to_list()
            xs.len() + xs[0]
        }"#,
    ];

    for src in cases {
        let (result, _) = check(src);
        assert!(
            result.diagnostics.is_empty(),
            "expected no diagnostics, got: {:?}\nsource:\n{src}",
            result.diagnostics
        );
    }
}

#[test]
fn err_seq_static_constructors_are_rejected_rfc_0003() {
    let cases = [
        "fn main() -> Int { Seq.range(0, 3).count() }",
        "fn main() -> Int { Seq.unfold(0, fn(state: Int) => Option.None).count() }",
    ];

    for src in cases {
        let (result, _) = check(src);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.message.contains("no method") || d.message.contains("unresolved")),
            "expected removed Seq constructor diagnostic, got: {:?}\nsource:\n{src}",
            result.diagnostics
        );
    }
}

#[test]
fn infer_seq_type_annotation_happy_path() {
    let (result, _) =
        check("fn takes_seq(xs: Seq<Int>) -> Int { xs.count() }\nfn main() -> Int { 0 }");
    assert!(
        result.diagnostics.is_empty(),
        "expected Seq type annotation to typecheck, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn err_list_seq_bridge_is_removed_rfc_0002() {
    let (result, _) = check("fn main() -> Int { collections.MutableList.new().push(1).seq().count() }");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("no method") || d.message.contains("unresolved")),
        "expected removed .seq() diagnostic, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn err_non_traversal_seq_param_still_rejects_list_rfc_0002() {
    let src = r#"fn takes_seq(xs: Seq<Int>) -> Int { xs.count() }
fn main() -> Int {
    let xs = collections.MutableList.new().push(1).push(2)
    takes_seq(xs)
}"#;
    let (result, _) = check(src);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("type mismatch")
                || d.message.contains("expected `Seq<Int>`")),
        "expected Seq/List mismatch diagnostics, got: {:?}",
        result.diagnostics
    );
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
        "type Maybe<T> = Just(T) | Nothing
         fn foo(o: Maybe<Int>) -> Int {
             let result = match (o) {
                 Maybe.Just(v) => v
                 Maybe.Nothing => 0
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

#[test]
fn exhaustive_option_bool_with_literal_arms() {
    check_ok(
        "fn foo(x: Option<Bool>) -> Int {
             match (x) {
                 Option.Some(true) => 1
                 Option.Some(false) => 0
                 Option.None => 2
             }
         }",
    );
}

#[test]
fn err_non_exhaustive_option_bool_with_partial_literal_arms() {
    check_err(
        "fn foo(x: Option<Bool>) -> Int {
             match (x) {
                 Option.Some(true) => 1
                 Option.None => 2
             }
         }",
        "non-exhaustive",
    );
}

#[test]
fn err_non_exhaustive_option_int_with_literal_arms() {
    check_err(
        "fn foo(x: Option<Int>) -> Int {
             match (x) {
                 Option.Some(1) => 1
                 Option.Some(2) => 2
                 Option.None => 0
             }
         }",
        "non-exhaustive",
    );
}
