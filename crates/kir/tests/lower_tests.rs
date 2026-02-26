//! Integration tests for HIR → KIR lowering.

use kyokara_hir::check_file;
use kyokara_kir::display::{DisplayCtx, display_module};
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
