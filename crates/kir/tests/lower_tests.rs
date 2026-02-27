//! Integration tests for HIR → KIR lowering.

use kyokara_hir::check_file;
use kyokara_kir::block::Terminator;
use kyokara_kir::display::{DisplayCtx, display_module};
use kyokara_kir::inst::Inst;
use kyokara_kir::lower::lower_module;
use kyokara_kir::validate::validate_function;

/// Parse, type-check, lower to KIR, validate every function, return display text.
fn lower_and_display(source: &str) -> String {
    let result = check_file(source);
    // Use raw_diagnostics (inference errors only) — `diagnostics` includes
    // false-positive "unresolved name" from body-lowering for constructor
    // pattern bindings (known limitation, same workaround as eval crate).
    assert!(
        result.type_check.raw_diagnostics.is_empty(),
        "type errors: {:?}",
        result.type_check.raw_diagnostics
    );

    let mut interner = result.interner;
    let module = lower_module(
        &result.item_tree,
        &result.module_scope,
        &result.type_check,
        &mut interner,
    );

    // Validate every function.
    for (_, func) in module.functions.iter() {
        let diags = validate_function(func, &interner);
        assert!(
            diags.is_empty(),
            "validation errors for @{}: {:?}",
            func.name.resolve(&interner),
            diags
        );
    }

    let ctx = DisplayCtx::new(&interner, &result.item_tree);
    display_module(&module, &ctx)
}

// ── Literals ─────────────────────────────────────────────────────

#[test]
fn test_int_literal() {
    let out = lower_and_display("fn f() -> Int { 42 }");
    assert!(out.contains("const 42 : Int"), "output:\n{out}");
    assert!(out.contains("return"), "output:\n{out}");
}

#[test]
fn test_float_literal() {
    let out = lower_and_display("fn f() -> Float { 3.14 }");
    assert!(out.contains("const 3.14 : Float"), "output:\n{out}");
}

#[test]
fn test_string_literal() {
    let out = lower_and_display(r#"fn f() -> String { "hello" }"#);
    assert!(out.contains(r#"const "hello" : String"#), "output:\n{out}");
}

#[test]
fn test_bool_literal() {
    let out = lower_and_display("fn f() -> Bool { true }");
    assert!(out.contains("const true : Bool"), "output:\n{out}");
}

#[test]
fn test_unit_return() {
    let out = lower_and_display("fn f() { }");
    assert!(out.contains("const () : Unit"), "output:\n{out}");
}

// ── Paths / Names ────────────────────────────────────────────────

#[test]
fn test_param_reference() {
    let out = lower_and_display("fn f(x: Int) -> Int { x }");
    assert!(out.contains("return x"), "output:\n{out}");
}

#[test]
fn test_let_binding() {
    let out = lower_and_display("fn f() -> Int {\n  let x = 42\n  x\n}");
    assert!(out.contains("const 42 : Int"), "output:\n{out}");
    assert!(out.contains("return"), "output:\n{out}");
}

#[test]
fn test_nested_let() {
    let out = lower_and_display("fn f() -> Int {\n  let x = 1\n  let y = x\n  y\n}");
    assert!(out.contains("const 1 : Int"), "output:\n{out}");
}

// ── Binary ops ───────────────────────────────────────────────────

#[test]
fn test_add() {
    let out = lower_and_display("fn f(x: Int, y: Int) -> Int { x + y }");
    assert!(out.contains("add x, y"), "output:\n{out}");
}

#[test]
fn test_comparison() {
    let out = lower_and_display("fn f(x: Int) -> Bool { x > 0 }");
    assert!(out.contains("gt x,"), "output:\n{out}");
}

#[test]
fn test_chained_ops() {
    let out = lower_and_display("fn f(x: Int) -> Int { x + 1 + 2 }");
    let add_count = out.matches("add").count();
    assert!(
        add_count >= 2,
        "expected 2 adds, got {add_count}. output:\n{out}"
    );
}

// ── Unary ops ────────────────────────────────────────────────────

#[test]
fn test_neg() {
    let out = lower_and_display("fn f(x: Int) -> Int { -x }");
    assert!(out.contains("neg x"), "output:\n{out}");
}

#[test]
fn test_not() {
    let out = lower_and_display("fn f(x: Bool) -> Bool { !x }");
    assert!(out.contains("not x"), "output:\n{out}");
}

// ── Calls ────────────────────────────────────────────────────────

#[test]
fn test_direct_call() {
    let out = lower_and_display(
        "fn add(a: Int, b: Int) -> Int { a + b }\nfn main() -> Int { add(1, 2) }",
    );
    assert!(out.contains("call @add("), "output:\n{out}");
}

#[test]
fn test_constructor_call() {
    let out = lower_and_display("type Foo = | Bar(Int)\nfn main() -> Foo { Bar(42) }");
    assert!(out.contains("adt_construct Bar("), "output:\n{out}");
}

#[test]
fn test_nullary_constructor() {
    let out = lower_and_display("type Opt = | Yes | No\nfn main() -> Opt { No }");
    assert!(out.contains("adt_construct No()"), "output:\n{out}");
}

#[test]
fn test_nested_call() {
    let out = lower_and_display("fn f(x: Int) -> Int { x }\nfn g() -> Int { f(f(1)) }");
    let call_count = out.matches("call @f(").count();
    assert!(
        call_count >= 2,
        "expected 2 calls to f, got {call_count}. output:\n{out}"
    );
}

// ── Field access ─────────────────────────────────────────────────

#[test]
fn test_record_field() {
    let out = lower_and_display("type Point = { x: Int, y: Int }\nfn f(p: Point) -> Int { p.x }");
    assert!(out.contains("field_get"), "output:\n{out}");
    assert!(out.contains(", x"), "output:\n{out}");
}

#[test]
fn test_chained_field_access() {
    let out = lower_and_display(
        "type Inner = { v: Int }\ntype Outer = { inner: Inner }\nfn f(o: Outer) -> Int { o.inner.v }",
    );
    let fg_count = out.matches("field_get").count();
    assert!(
        fg_count >= 2,
        "expected 2 field_gets, got {fg_count}. output:\n{out}"
    );
}

// ── If expressions ───────────────────────────────────────────────

#[test]
fn test_if_else() {
    let out = lower_and_display("fn f(x: Int) -> Int { if x > 0 { x } else { -x } }");
    assert!(out.contains("branch"), "output:\n{out}");
    assert!(out.contains("then"), "output:\n{out}");
    assert!(out.contains("else"), "output:\n{out}");
    assert!(out.contains("merge"), "output:\n{out}");
}

#[test]
fn test_if_no_else() {
    let out = lower_and_display("fn f(x: Bool) { if x { } }");
    assert!(out.contains("branch"), "output:\n{out}");
}

#[test]
fn test_nested_if() {
    let out = lower_and_display(
        "fn f(x: Int) -> Int { if x > 0 { if x > 10 { 10 } else { x } } else { 0 } }",
    );
    let branch_count = out.matches("branch").count();
    assert!(
        branch_count >= 2,
        "expected >= 2 branches, got {branch_count}. output:\n{out}"
    );
}

#[test]
fn test_if_in_let() {
    let out =
        lower_and_display("fn f(x: Int) -> Int {\n  let y = if x > 0 { 1 } else { 0 }\n  y\n}");
    assert!(out.contains("branch"), "output:\n{out}");
    assert!(out.contains("merge"), "output:\n{out}");
}

// ── Match expressions ────────────────────────────────────────────

#[test]
fn test_adt_match_switch() {
    let out = lower_and_display(
        "type Wrap<T> = | Val(T) | Empty
         fn f(x: Wrap<Int>) -> Int {
           match x {
             Val(n) => n
             Empty => 0
           }
         }",
    );
    assert!(out.contains("switch"), "output:\n{out}");
    assert!(out.contains("Val"), "output:\n{out}");
    assert!(out.contains("Empty"), "output:\n{out}");
}

#[test]
fn test_literal_match() {
    let out = lower_and_display(
        "fn f(x: Int) -> Int {
           match x {
             0 => 100
             _ => x
           }
         }",
    );
    assert!(
        out.contains("eq") || out.contains("branch"),
        "output:\n{out}"
    );
}

#[test]
fn test_wildcard_match() {
    let out = lower_and_display(
        "type Wrap<T> = | Val(T) | Empty
         fn f(x: Wrap<Int>) -> Int {
           match x {
             Val(n) => n
             _ => 0
           }
         }",
    );
    assert!(out.contains("switch"), "output:\n{out}");
    assert!(out.contains("default"), "output:\n{out}");
}

#[test]
fn test_bind_pattern_match() {
    let out = lower_and_display(
        "type Wrap<T> = | Val(T) | Empty
         fn f(x: Wrap<Int>) -> Int {
           match x {
             Val(n) => n
             other => 0
           }
         }",
    );
    assert!(out.contains("switch"), "output:\n{out}");
}

#[test]
fn test_nested_constructor() {
    let out = lower_and_display(
        "type Wrap<T> = | Val(T) | Empty
         fn f(x: Wrap<Int>) -> Int {
           match x {
             Val(n) => n + 1
             Empty => 0
           }
         }",
    );
    assert!(out.contains("adt_field_get"), "output:\n{out}");
    assert!(out.contains("add"), "output:\n{out}");
}

#[test]
fn test_multiple_arms() {
    let out = lower_and_display(
        "type Color = | Red | Green | Blue
         fn f(c: Color) -> Int {
           match c {
             Red => 1
             Green => 2
             Blue => 3
           }
         }",
    );
    assert!(out.contains("switch"), "output:\n{out}");
    assert!(out.contains("Red"), "output:\n{out}");
    assert!(out.contains("Green"), "output:\n{out}");
    assert!(out.contains("Blue"), "output:\n{out}");
}

// ── Blocks ───────────────────────────────────────────────────────

#[test]
fn test_block_with_statements() {
    let out = lower_and_display(
        "fn f() -> Int {
           let x = 1
           let y = 2
           x + y
         }",
    );
    assert!(out.contains("const 1"), "output:\n{out}");
    assert!(out.contains("const 2"), "output:\n{out}");
    assert!(out.contains("add"), "output:\n{out}");
}

#[test]
fn test_empty_block() {
    let out = lower_and_display("fn f() { }");
    assert!(out.contains("const ()"), "output:\n{out}");
}

#[test]
fn test_discarded_expression() {
    let out = lower_and_display(
        "fn f() -> Int {
           1 + 2
           42
         }",
    );
    assert!(out.contains("add"), "output:\n{out}");
    assert!(out.contains("const 42"), "output:\n{out}");
}

// ── Return ───────────────────────────────────────────────────────

#[test]
fn test_early_return() {
    let out = lower_and_display(
        "fn f(x: Int) -> Int {
           if x > 0 { return x } else { }
           -x
         }",
    );
    assert!(out.contains("return x"), "output:\n{out}");
}

#[test]
fn test_return_unit() {
    let out = lower_and_display("fn f() { return }");
    assert!(out.contains("return"), "output:\n{out}");
}

#[test]
fn test_return_in_match_arm() {
    let out = lower_and_display(
        "type Wrap<T> = | Val(T) | Empty
         fn f(x: Wrap<Int>) -> Int {
           match x {
             Val(n) => return n
             Empty => 0
           }
         }",
    );
    let ret_count = out.matches("return").count();
    assert!(ret_count >= 1, "expected return, output:\n{out}");
}

// ── Record literals ──────────────────────────────────────────────

#[test]
fn test_record_literal() {
    let out = lower_and_display(
        "type Point = { x: Int, y: Int }
         fn f() -> Point { Point { x: 1, y: 2 } }",
    );
    assert!(
        out.contains("record_create") || out.contains("adt_construct"),
        "output:\n{out}"
    );
}

// ── Holes ────────────────────────────────────────────────────────

#[test]
fn test_typed_hole() {
    let out = lower_and_display("fn f() -> Int { _ }");
    assert!(out.contains("hole #"), "output:\n{out}");
}

// ── Contracts ────────────────────────────────────────────────────

#[test]
fn test_requires_clause() {
    let out = lower_and_display("fn f(x: Int) -> Int requires x > 0 { x }");
    assert!(out.contains("assert"), "output:\n{out}");
    assert!(out.contains("requires"), "output:\n{out}");
}

#[test]
fn test_ensures_clause() {
    let out = lower_and_display("fn f(x: Int) -> Int ensures x > 0 { x }");
    assert!(out.contains("assert"), "output:\n{out}");
    assert!(out.contains("ensures"), "output:\n{out}");
}

// ── Module-level ─────────────────────────────────────────────────

#[test]
fn test_multiple_functions() {
    let out = lower_and_display("fn foo() -> Int { 1 }\nfn bar() -> Int { 2 }");
    assert!(out.contains("fn @foo("), "output:\n{out}");
    assert!(out.contains("fn @bar("), "output:\n{out}");
}

#[test]
fn test_entry_detection() {
    let result = check_file("fn main() -> Int { 42 }");
    let mut interner = result.interner;
    let module = lower_module(
        &result.item_tree,
        &result.module_scope,
        &result.type_check,
        &mut interner,
    );
    assert!(
        module.entry.is_some(),
        "expected main to be detected as entry"
    );
}

#[test]
fn test_all_output_validates() {
    let source = "
        fn id(x: Int) -> Int { x }
        fn double(x: Int) -> Int { x + x }
        fn main() -> Int { double(id(21)) }
    ";
    let _ = lower_and_display(source);
}

// ── Bug regressions ─────────────────────────────────────────────

#[test]
fn test_regression_last_literal_arm_uses_branch() {
    // Bug: the last literal arm in a sequential match used an unconditional
    // jump, ignoring the equality check.  After fix, every literal arm
    // (including the last) emits a branch (conditional).
    let out = lower_and_display(
        "fn f(x: Int) -> Int {
           match x {
             0 => 100
             1 => 200
           }
         }",
    );
    // Each literal arm must produce a branch instruction.
    // Before fix: last arm used jump → only 1 branch.
    let branch_count = out.matches("branch").count();
    assert!(
        branch_count >= 2,
        "expected >= 2 branches (one per literal arm), got {branch_count}. output:\n{out}"
    );
}

#[test]
fn test_regression_record_pattern_field_type() {
    // Bug: record pattern destructuring used the whole record type as each
    // field's type instead of resolving individual field types.
    let out = lower_and_display(
        "type Point = { x: Int, y: Int }
         fn f(p: Point) -> Int {
           let { x, y } = p
           0
         }",
    );
    // field_get for x should have type Int, not the whole record type.
    // Before fix: `field_get p, x : { x: Int, y: Int }` (whole record type).
    // After fix:  `field_get p, x : Int` (correct field type).
    for line in out.lines() {
        if line.contains("field_get") && line.contains(", x") {
            assert!(
                !line.contains('{'),
                "field_get for x should have type Int, not the whole record type. got: {line}"
            );
            assert!(
                line.trim().ends_with(": Int"),
                "field_get for x should end with `: Int`. got: {line}"
            );
            return;
        }
    }
    panic!("no field_get for x found in output:\n{out}");
}

// ── Edge cases ───────────────────────────────────────────────────

#[test]
fn test_nested_control_flow() {
    let source = "
        type Wrap<T> = | Val(T) | Empty
        fn f(x: Wrap<Int>) -> Int {
            match x {
                Val(n) => if n > 0 { n } else { 0 }
                Empty => -1
            }
        }
    ";
    let out = lower_and_display(source);
    assert!(out.contains("switch"), "output:\n{out}");
    assert!(out.contains("branch"), "output:\n{out}");
}

#[test]
fn test_recursive_call() {
    let source = "
        fn fib(n: Int) -> Int {
            if n < 2 {
                n
            } else {
                fib(n - 1) + fib(n - 2)
            }
        }
    ";
    let out = lower_and_display(source);
    assert!(out.contains("call @fib("), "output:\n{out}");
}

// ── Bug regression: ensures binds result (#50) ─────────────────

#[test]
fn test_ensures_binds_result_variable() {
    let out = lower_and_display("fn f(x: Int) -> Int ensures result > 0 { x }");
    // The ensures clause should reference the return value, not produce a hole.
    // Before fix: `result` lowered to a hole because it wasn't defined as a local.
    assert!(
        !out.contains("hole"),
        "ensures should reference result, not produce a hole. output:\n{out}"
    );
    assert!(out.contains("assert"), "output:\n{out}");
    assert!(out.contains("ensures"), "output:\n{out}");
}

// ── Bug regression: ensures with early return (#51) ─────────────

#[test]
fn test_ensures_with_early_return() {
    let out = lower_and_display(
        "fn f(x: Int) -> Int ensures result > 0 {
           return x
         }",
    );
    // The ensures assertion should appear before the return terminator,
    // not in dead code after it.
    assert!(
        out.contains("assert"),
        "ensures assert missing. output:\n{out}"
    );
    assert!(
        out.contains("ensures"),
        "ensures label missing. output:\n{out}"
    );
    // The assert should NOT be in a block that starts with unreachable.
    // Check that there is an assert followed eventually by a return.
    let assert_pos = out.find("assert").expect("assert missing");
    let return_pos = out.rfind("return").expect("return missing");
    assert!(
        assert_pos < return_pos,
        "ensures assert should come before return. output:\n{out}"
    );
}

// ── Bug regression: sequential match with record pattern (#52) ──

#[test]
fn test_sequential_match_record_pattern() {
    let out = lower_and_display(
        "type Point = { x: Int, y: Int }
         fn f(p: Point) -> Int {
           match p {
             { x, y } => x + y
           }
         }",
    );
    // The record pattern arm body should be lowered (not silently dropped).
    assert!(
        out.contains("add"),
        "record pattern arm body should produce add. output:\n{out}"
    );
    assert!(
        out.contains("field_get"),
        "record pattern should destructure fields. output:\n{out}"
    );
}

#[test]
fn test_sequential_match_constructor_pattern() {
    let out = lower_and_display(
        "type Wrap<T> = | Val(T) | Empty
         fn f(x: Int) -> Int {
           match x {
             0 => 100
             n => n
           }
         }",
    );
    // Bind pattern arm should still work as before.
    assert!(out.contains("eq"), "output:\n{out}");
}

// ── Bug regression: ensures metadata from early return (#55) ────

#[test]
fn test_ensures_early_return_recorded_in_contracts() {
    // A function with `ensures` + explicit `return` should have the
    // postcondition assert recorded in KirContracts.ensures, not just
    // emitted silently.
    let result = check_file(
        "fn f(x: Int) -> Int ensures result > 0 {
           return x
         }",
    );
    assert!(
        result.type_check.raw_diagnostics.is_empty(),
        "type errors: {:?}",
        result.type_check.raw_diagnostics
    );

    let mut interner = result.interner;
    let module = lower_module(
        &result.item_tree,
        &result.module_scope,
        &result.type_check,
        &mut interner,
    );

    let func = module.functions.iter().next().unwrap().1;
    assert!(
        !func.contracts.ensures.is_empty(),
        "KirContracts.ensures should contain the postcondition assert from return, but it's empty"
    );
}

#[test]
fn test_ensures_implicit_return_recorded_in_contracts() {
    // Baseline: implicit return path should also record ensures in contracts.
    let result = check_file("fn f(x: Int) -> Int ensures result > 0 { x }");
    assert!(
        result.type_check.raw_diagnostics.is_empty(),
        "type errors: {:?}",
        result.type_check.raw_diagnostics
    );

    let mut interner = result.interner;
    let module = lower_module(
        &result.item_tree,
        &result.module_scope,
        &result.type_check,
        &mut interner,
    );

    let func = module.functions.iter().next().unwrap().1;
    assert!(
        !func.contracts.ensures.is_empty(),
        "KirContracts.ensures should contain the postcondition assert from implicit return"
    );
}

#[test]
fn test_constructor_in_if_branches() {
    let source = "
        type Wrap<T> = | Val(T) | Empty
        fn f(x: Int) -> Wrap<Int> {
            if x > 0 {
                Val(x)
            } else {
                Empty
            }
        }
    ";
    let out = lower_and_display(source);
    assert!(out.contains("adt_construct Val("), "output:\n{out}");
    assert!(out.contains("adt_construct Empty()"), "output:\n{out}");
}

// ── Bug regression: old() semantics erased in contracts (#72) ───

#[test]
fn test_old_in_ensures_with_explicit_return() {
    // Bug: old(x) lowers as just `lower_expr(inner)`, resolving x in current
    // scope. With explicit `return` inside a block where x is rebound,
    // ensures is emitted at the return site while the rebinding is still in
    // scope — so old(x) incorrectly references the rebound value.
    let out = lower_and_display(
        "fn f(x: Int) -> Int ensures old(x) > 0 {
           let x = x + 1
           return x
         }",
    );
    assert!(
        out.contains("assert"),
        "ensures assert missing. output:\n{out}"
    );
    assert!(
        out.contains("ensures"),
        "ensures label missing. output:\n{out}"
    );
    // The gt instruction for the ensures should reference param `x` (displayed
    // as `x`), NOT the rebound value `%N`. With the bug, we'd see `gt %2,`
    // instead of `gt x,`.
    let lines: Vec<&str> = out.lines().collect();
    let gt_line = lines
        .iter()
        .find(|l| l.contains(" gt "))
        .unwrap_or_else(|| panic!("no gt instruction found for ensures. output:\n{out}"));
    assert!(
        gt_line.contains("gt x,"),
        "old(x) should reference original param `x`, not a computed value. got: {gt_line}\nfull output:\n{out}"
    );
}

#[test]
fn test_ensures_without_old_still_works() {
    // Guard: ensures clause without old() should continue to work.
    let out = lower_and_display("fn f(x: Int) -> Int ensures result > 0 { x }");
    assert!(out.contains("assert"), "output:\n{out}");
    assert!(out.contains("ensures"), "output:\n{out}");
    assert!(!out.contains("hole"), "output:\n{out}");
}

#[test]
fn test_old_without_rebinding_references_param() {
    // Guard: old(x) where x is NOT rebound — should reference the original
    // param regardless (trivially correct, verifies old() doesn't break).
    let out = lower_and_display("fn f(x: Int) -> Int ensures old(x) > 0 { x }");
    assert!(out.contains("assert"), "output:\n{out}");
    assert!(out.contains("ensures"), "output:\n{out}");
    let lines: Vec<&str> = out.lines().collect();
    let gt_line = lines
        .iter()
        .find(|l| l.contains(" gt "))
        .unwrap_or_else(|| panic!("no gt instruction found. output:\n{out}"));
    assert!(
        gt_line.contains("gt x,"),
        "old(x) should reference param `x`. got: {gt_line}\nfull output:\n{out}"
    );
}

// ── Bug regression: ADT switch default routes to last catch-all (#140) ───

#[test]
fn test_adt_switch_default_uses_first_catchall() {
    // Bug: lower_match_adt overwrites default_target for every wildcard/bind
    // arm, so the last catch-all wins instead of the first.
    let result = check_file(
        "type W = | A | B
         fn f(x: W) -> Int {
           match x {
             A => 1
             _ => 2
             _ => 3
           }
         }",
    );
    // Allow RedundantMatchArm but nothing else.
    let real_errors: Vec<_> = result
        .type_check
        .raw_diagnostics
        .iter()
        .filter(|(d, _)| !matches!(d, kyokara_hir::TyDiagnosticData::RedundantMatchArm))
        .collect();
    assert!(
        real_errors.is_empty(),
        "unexpected type errors: {real_errors:?}"
    );

    let mut interner = result.interner;
    let module = lower_module(
        &result.item_tree,
        &result.module_scope,
        &result.type_check,
        &mut interner,
    );

    // Inspect the switch terminator directly.
    let func = module.functions.iter().next().unwrap().1;
    let entry = &func.blocks[func.entry_block];
    let switch_default_block = match entry.terminator.as_ref().unwrap() {
        Terminator::Switch { default, .. } => default.as_ref().unwrap().block,
        other => panic!("expected Switch terminator, got: {other:?}"),
    };

    // The default target block's first instruction should be const 2
    // (the first catch-all arm), not const 3 (the last).
    let default_block = &func.blocks[switch_default_block];
    let first_val = default_block.body[0];
    match &func.values[first_val].inst {
        Inst::Const(c) => {
            let display = format!("{c:?}");
            assert!(
                display.contains("2"),
                "switch default should route to first catch-all (const 2), got: {display}"
            );
        }
        other => panic!("expected Const in default block, got: {other:?}"),
    }
}

#[test]
fn test_adt_switch_single_catchall_still_works() {
    // Guard: single catch-all arm should work as before.
    let out = lower_and_display(
        "type W = | A | B
         fn f(x: W) -> Int {
           match x {
             A => 1
             _ => 99
           }
         }",
    );
    assert!(out.contains("const 99"), "output:\n{out}");
    assert!(out.contains("default:"), "output:\n{out}");
}

// ── Bug regression: ADT switch violates arm order after catch-all (#141) ───

#[test]
fn test_adt_switch_no_cases_after_catchall() {
    // Bug: constructor arms after a catch-all still get added to the switch,
    // so `A` values branch to the later arm instead of the catch-all.
    let result = check_file(
        "type W = | A | B
         fn f(x: W) -> Int {
           match x {
             other => 1
             A => 2
           }
         }",
    );
    let real_errors: Vec<_> = result
        .type_check
        .raw_diagnostics
        .iter()
        .filter(|(d, _)| !matches!(d, kyokara_hir::TyDiagnosticData::RedundantMatchArm))
        .collect();
    assert!(
        real_errors.is_empty(),
        "unexpected type errors: {real_errors:?}"
    );

    let mut interner = result.interner;
    let module = lower_module(
        &result.item_tree,
        &result.module_scope,
        &result.type_check,
        &mut interner,
    );

    let func = module.functions.iter().next().unwrap().1;
    let entry = &func.blocks[func.entry_block];
    match entry.terminator.as_ref().unwrap() {
        Terminator::Switch { cases, default, .. } => {
            // The catch-all is the first arm, so there should be NO constructor
            // cases in the switch — everything goes to the default.
            assert!(
                cases.is_empty(),
                "switch should have no cases after catch-all, got {} cases",
                cases.len()
            );
            assert!(default.is_some(), "switch should have a default target");
        }
        other => panic!("expected Switch terminator, got: {other:?}"),
    }
}

#[test]
fn test_adt_switch_constructor_before_catchall_still_works() {
    // Guard: constructor arms BEFORE a catch-all should still be cases.
    let out = lower_and_display(
        "type W = | A | B
         fn f(x: W) -> Int {
           match x {
             A => 1
             other => 2
           }
         }",
    );
    // A should have its own case block.
    assert!(out.contains("A:"), "output:\n{out}");
    assert!(out.contains("default:"), "output:\n{out}");
}

// ── Bug regression: ADT switch emits duplicate cases (#142) ───

#[test]
fn test_adt_switch_no_duplicate_cases() {
    // Bug: redundant constructor arms emit duplicate SwitchCase entries.
    let result = check_file(
        "type W = | A | B
         fn f(x: W) -> Int {
           match x {
             A => 1
             A => 2
             B => 3
           }
         }",
    );
    let real_errors: Vec<_> = result
        .type_check
        .raw_diagnostics
        .iter()
        .filter(|(d, _)| !matches!(d, kyokara_hir::TyDiagnosticData::RedundantMatchArm))
        .collect();
    assert!(
        real_errors.is_empty(),
        "unexpected type errors: {real_errors:?}"
    );

    let mut interner = result.interner;
    let module = lower_module(
        &result.item_tree,
        &result.module_scope,
        &result.type_check,
        &mut interner,
    );

    let func = module.functions.iter().next().unwrap().1;
    let entry = &func.blocks[func.entry_block];
    match entry.terminator.as_ref().unwrap() {
        Terminator::Switch { cases, .. } => {
            // Each variant should appear at most once.
            let variant_names: Vec<_> =
                cases.iter().map(|c| c.variant.resolve(&interner)).collect();
            let unique: std::collections::HashSet<_> = variant_names.iter().collect();
            assert_eq!(
                variant_names.len(),
                unique.len(),
                "switch has duplicate cases: {variant_names:?}"
            );
        }
        other => panic!("expected Switch terminator, got: {other:?}"),
    }
}

#[test]
fn test_adt_switch_first_dup_case_wins() {
    // Guard: when deduplicating, the first constructor arm should win.
    let result = check_file(
        "type W = | A | B
         fn f(x: W) -> Int {
           match x {
             A => 10
             A => 20
             B => 30
           }
         }",
    );
    let real_errors: Vec<_> = result
        .type_check
        .raw_diagnostics
        .iter()
        .filter(|(d, _)| !matches!(d, kyokara_hir::TyDiagnosticData::RedundantMatchArm))
        .collect();
    assert!(
        real_errors.is_empty(),
        "unexpected type errors: {real_errors:?}"
    );

    let mut interner = result.interner;
    let module = lower_module(
        &result.item_tree,
        &result.module_scope,
        &result.type_check,
        &mut interner,
    );

    let func = module.functions.iter().next().unwrap().1;
    let entry = &func.blocks[func.entry_block];
    let a_block = match entry.terminator.as_ref().unwrap() {
        Terminator::Switch { cases, .. } => {
            let a_case = cases
                .iter()
                .find(|c| c.variant.resolve(&interner) == "A")
                .expect("should have A case");
            a_case.target.block
        }
        other => panic!("expected Switch, got: {other:?}"),
    };

    // The A case block body should produce const 10 (first arm), not const 20.
    let a_blk = &func.blocks[a_block];
    let first_val = a_blk.body[0];
    match &func.values[first_val].inst {
        Inst::Const(c) => {
            let display = format!("{c:?}");
            assert!(
                display.contains("10"),
                "first A arm should produce const 10, got: {display}"
            );
        }
        other => panic!("expected Const, got: {other:?}"),
    }
}
