//! Integration tests for HIR → KIR lowering.
#![allow(clippy::unwrap_used)]

use kyokara_hir::check_file;
use kyokara_hir_def::name::Name;
use kyokara_hir_ty::effects::EffectSet;
use kyokara_hir_ty::ty::Ty;
use kyokara_intern::Interner;
use kyokara_kir::block::{BranchTarget, SwitchCase, Terminator};
use kyokara_kir::build::KirBuilder;
use kyokara_kir::display::{DisplayCtx, display_module};
use kyokara_kir::function::KirContracts;
use kyokara_kir::inst::{CallTarget, Constant, Inst};
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

#[test]
fn test_range_until_lowers_via_seq_range_intrinsic_path_rfc_0003() {
    let out = lower_and_display("fn f() -> Int { (0..<3).count() }");
    assert!(out.contains("call intrinsic:seq_range("), "output:\n{out}");
    assert!(out.contains("call intrinsic:seq_count("), "output:\n{out}");
}

#[test]
fn test_count_predicate_lowers_via_seq_count_by_intrinsic() {
    let out = lower_and_display("fn f() -> Int { (0..<3).count(fn(n: Int) => n % 2 == 0) }");
    assert!(out.contains("call intrinsic:seq_range("), "output:\n{out}");
    assert!(
        out.contains("call intrinsic:seq_count_by("),
        "output:\n{out}"
    );
}

#[test]
fn test_loop_statements_lower_with_minimal_kir_compatibility_rfc_0006() {
    let out = lower_and_display(
        "import collections\n\
         fn f() -> Int {\n\
           let acc = collections.MutableList.new().push(0)\n\
           for (x in 0..<10) {\n\
             if (x == 7) { break }\n\
             if ((x % 2) == 0) { continue }\n\
             acc.set(0, acc[0] + x)\n\
           }\n\
           acc[0]\n\
         }",
    );
    assert!(out.contains("@f"), "output:\n{out}");
}

#[test]
fn test_mutable_reassignment_and_while_lower_without_placeholder_holes() {
    let out = lower_and_display(
        "fn f() -> Int {\n\
           var acc = 0\n\
           while (acc < 4) {\n\
             acc = acc + 1\n\
           }\n\
           acc\n\
         }",
    );
    assert!(
        !out.contains("hole"),
        "while/assignment lowering should not leave placeholder holes. output:\n{out}"
    );
    assert!(out.contains("branch "), "output:\n{out}");
    assert!(out.contains("jump "), "output:\n{out}");
}

#[test]
fn test_short_circuit_while_condition_lowers_without_placeholder_holes() {
    let out = lower_and_display(
        "fn f() -> Int {\n\
           var i = 0\n\
           while (i < 7 && i < 10) {\n\
             i = i + 1\n\
           }\n\
           i\n\
         }",
    );
    assert!(
        !out.contains("hole"),
        "short-circuit while lowering should not leave placeholder holes. output:\n{out}"
    );
    assert!(out.contains("branch "), "output:\n{out}");
    assert!(out.contains("jump "), "output:\n{out}");
}

#[test]
fn test_nested_short_circuit_while_index_condition_lowers_without_placeholder_holes() {
    let out = lower_and_display(
        "from collections import MutableList\n\
         fn f() -> Int {\n\
           let blocks = MutableList.new().push(0).push(-1).push(1).push(-1).push(2)\n\
           let left = MutableList.new().push(0)\n\
           let right = MutableList.new().push(blocks.len() - 1)\n\
           while (left[0] < right[0]) {\n\
             while (left[0] < blocks.len() && blocks[left[0]] != -1) {\n\
               let _l = left.set(0, left[0] + 1)\n\
             }\n\
             while (right[0] >= 0 && blocks[right[0]] == -1) {\n\
               let _r = right.set(0, right[0] - 1)\n\
             }\n\
             if (left[0] < right[0]) {\n\
               let value = blocks[right[0]]\n\
               let _a = blocks.set(left[0], value)\n\
               let _b = blocks.set(right[0], -1)\n\
               let _l = left.set(0, left[0] + 1)\n\
               let _r = right.set(0, right[0] - 1)\n\
             }\n\
           }\n\
           var total = 0\n\
           for (i in 0..<blocks.len()) {\n\
             let value = blocks[i]\n\
             if (value >= 0) {\n\
               total = total + i * value\n\
             }\n\
           }\n\
           total\n\
         }",
    );
    assert!(!out.contains("hole"), "output:\n{out}");
}

#[test]
fn test_for_over_seq_lowers_without_placeholder_holes() {
    let out = lower_and_display(
        "fn f() -> Int {\n\
           let text = \"1234567890\"\n\
           var sum = 0\n\
           for (ch in text.chars()) {\n\
             sum = sum + (ch.code() - '0'.code())\n\
           }\n\
           sum\n\
         }",
    );
    assert!(
        !out.contains("hole"),
        "for-over-seq lowering should not leave placeholder holes. output:\n{out}"
    );
    assert!(out.contains("intrinsic:seq_to_list"), "output:\n{out}");
    assert!(out.contains("intrinsic:list_index"), "output:\n{out}");
}

#[test]
fn test_for_loop_if_merge_threads_mutable_locals() {
    let out = lower_and_display(
        "fn f(flag: Bool) -> Int {\n\
           var total = 0\n\
           for (step in (0..<4).to_list()) {\n\
             var x0 = step\n\
             var x1 = step\n\
             var x2 = step\n\
             var valid = true\n\
             if (flag) {\n\
               x0 = x0 + 0\n\
               x1 = x1 + 0\n\
               x2 = x2 + 0\n\
               if (x0 > 1 || x1 > 1 || x2 > 1) {\n\
                 valid = false\n\
               }\n\
             }\n\
             if (valid) {\n\
               total = total + 1\n\
             }\n\
           }\n\
           total\n\
         }",
    );
    assert!(!out.contains("hole"), "output:\n{out}");
}

#[test]
fn test_string_index_lowers_without_placeholder_holes() {
    let out = lower_and_display(r#"fn f() -> Int { "abc"[1].code() }"#);
    assert!(
        !out.contains("hole"),
        "string index lowering should not leave placeholder holes. output:\n{out}"
    );
    assert!(out.contains("intrinsic:string_index"), "output:\n{out}");
}

#[test]
fn test_while_break_continue_lower_without_placeholder_holes() {
    let out = lower_and_display(
        "fn f() -> Int {\n\
           var i = 0\n\
           var acc = 0\n\
           while (i < 8) {\n\
             i = i + 1\n\
             if (i == 6) { break }\n\
             if ((i % 2) == 0) { continue }\n\
             acc = acc + i\n\
           }\n\
           acc\n\
         }",
    );
    assert!(
        !out.contains("hole"),
        "break/continue lowering should not leave placeholder holes. output:\n{out}"
    );
    assert!(out.contains("branch "), "output:\n{out}");
    assert!(out.contains("jump "), "output:\n{out}");
}

#[test]
fn test_loop_with_multiple_returns_and_indexed_record_fields_lowers_without_placeholder_holes() {
    let out = lower_and_display(
        "from collections import MutableList\n\
         type Table = { keys: MutableList<Int>, states: MutableList<Int>, size: Int }\n\
         fn helper(table: Table, key: Int) -> Int {\n\
           var idx = 0\n\
           while (true) {\n\
             let cur = table.keys[idx]\n\
             if (cur == -1) {\n\
               return 0\n\
             }\n\
             if (cur == key) {\n\
               return table.states[idx]\n\
             }\n\
             idx = (idx + 1) % table.size\n\
           }\n\
           0\n\
         }\n\
         fn main() -> Int { 1 }\n",
    );
    assert!(
        !out.contains("hole"),
        "multi-return indexed loop lowering should not leave placeholder holes. output:\n{out}"
    );
    assert!(out.contains("@helper"), "output:\n{out}");
}

#[test]
fn test_loop_with_nested_unit_merges_and_mutations_lowers_without_placeholder_holes() {
    let out = lower_and_display(
        "from collections import MutableList\n\
         type Table = { keys: MutableList<Int>, states: MutableList<Int>, size: Int }\n\
         fn set_state(table: Table, key: Int, state: Int) -> Unit {\n\
           var idx = 0\n\
           while (true) {\n\
             let cur = table.keys[idx]\n\
             if (cur == -1 || cur == key) {\n\
               if (cur == -1) {\n\
                 let _k = table.keys.set(idx, key)\n\
               }\n\
               let _s = table.states.set(idx, state)\n\
               return\n\
             }\n\
             idx = (idx + 1) % table.size\n\
           }\n\
         }\n\
         fn main() -> Int { 1 }\n",
    );
    assert!(
        !out.contains("hole"),
        "nested unit-merge loop lowering should not leave placeholder holes. output:\n{out}"
    );
    assert!(out.contains("@set_state"), "output:\n{out}");
}

#[test]
fn test_first8_short_circuit_loop_shape_lowers_without_placeholder_holes() {
    let out = lower_and_display(
        "from collections import MutableList\n\
         fn first8(xs: MutableList<Int>) -> String {\n\
           var s = \"\"\n\
           var i = 0\n\
           while (i < 8 && i < xs.len()) {\n\
             s = s.concat(xs[i].to_string())\n\
             i = i + 1\n\
           }\n\
           s\n\
         }\n\
         fn main() -> Int { 1 }\n",
    );
    assert!(
        !out.contains("hole"),
        "first8 short-circuit loop lowering should not leave placeholder holes. output:\n{out}"
    );
    assert!(out.contains("@first8"), "output:\n{out}");
}

#[test]
fn test_for_range_break_continue_lower_without_placeholder_holes() {
    let out = lower_and_display(
        "fn f() -> Int {\n\
           var acc = 0\n\
           for (x in 0..<8) {\n\
             if (x == 6) { break }\n\
             if ((x % 2) == 0) { continue }\n\
             acc = acc + x\n\
           }\n\
           acc\n\
         }",
    );
    assert!(
        !out.contains("hole"),
        "range for-loop lowering should not leave placeholder holes. output:\n{out}"
    );
    assert!(out.contains("branch "), "output:\n{out}");
    assert!(out.contains("jump "), "output:\n{out}");
}

#[test]
fn test_match_inside_while_carries_mutable_locals_to_merge() {
    let out = lower_and_display(
        "type Step = Keep | Skip\n\
         fn step(i: Int) -> Step {\n\
           if ((i % 2) == 0) { Step.Skip } else { Step.Keep }\n\
         }\n\
         fn f() -> Int {\n\
           var i = 0\n\
           var acc = 0\n\
           while (i < 8) {\n\
             i = i + 1\n\
             let ignored = match (step(i)) {\n\
               Step.Keep => { acc = acc + i\n 0 }\n\
               Step.Skip => { 0 }\n\
             }\n\
             ignored\n\
         }\n\
           acc\n\
         }",
    );
    assert!(
        out.lines()
            .any(|line| line.trim_start().starts_with("merge(") && line.contains("acc: Int")),
        "match merge should carry mutable local state. output:\n{out}"
    );
    assert!(!out.contains("hole"), "output:\n{out}");
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

// ── Modulo ──────────────────────────────────────────────────────

#[test]
fn test_rem() {
    let out = lower_and_display("fn f(x: Int, y: Int) -> Int { x % y }");
    assert!(out.contains("rem x, y"), "output:\n{out}");
}

// ── Bitwise binary ops ──────────────────────────────────────────

#[test]
fn test_bit_and() {
    let out = lower_and_display("fn f(x: Int, y: Int) -> Int { x & y }");
    assert!(out.contains("bit_and x, y"), "output:\n{out}");
}

#[test]
fn test_bit_or() {
    let out = lower_and_display("fn f(x: Int, y: Int) -> Int { x | y }");
    assert!(out.contains("bit_or x, y"), "output:\n{out}");
}

#[test]
fn test_bit_xor() {
    let out = lower_and_display("fn f(x: Int, y: Int) -> Int { x ^ y }");
    assert!(out.contains("bit_xor x, y"), "output:\n{out}");
}

#[test]
fn test_shl() {
    let out = lower_and_display("fn f(x: Int, y: Int) -> Int { x << y }");
    assert!(out.contains("shl x, y"), "output:\n{out}");
}

#[test]
fn test_shr() {
    let out = lower_and_display("fn f(x: Int, y: Int) -> Int { x >> y }");
    assert!(out.contains("shr x, y"), "output:\n{out}");
}

// ── Bitwise unary ───────────────────────────────────────────────

#[test]
fn test_bit_not() {
    let out = lower_and_display("fn f(x: Int) -> Int { ~x }");
    assert!(out.contains("bit_not x"), "output:\n{out}");
}

// ── Logical operators in KIR ─────────────────────────────────────
// `&&` and `||` lower to explicit control flow so short-circuit behavior
// is preserved in KIR-based backends.

#[test]
fn test_logical_and() {
    let out = lower_and_display("fn f(a: Bool, b: Bool) -> Bool { a && b }");
    assert!(out.contains("branch a ->"), "output:\n{out}");
    assert!(out.contains("const false : Bool"), "output:\n{out}");
    assert!(!out.contains("and a, b"), "output:\n{out}");
}

#[test]
fn test_logical_or() {
    let out = lower_and_display("fn f(a: Bool, b: Bool) -> Bool { a || b }");
    assert!(out.contains("branch a ->"), "output:\n{out}");
    assert!(out.contains("const true : Bool"), "output:\n{out}");
    assert!(!out.contains("or a, b"), "output:\n{out}");
}

// ── Chained bitwise ops ─────────────────────────────────────────

#[test]
fn test_chained_bitwise_and() {
    let out = lower_and_display("fn f(x: Int, y: Int, z: Int) -> Int { x & y & z }");
    let count = out.matches("bit_and").count();
    assert!(
        count >= 2,
        "expected 2 bit_and ops, got {count}. output:\n{out}"
    );
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
    let out = lower_and_display("type Foo = Bar(Int)\nfn main() -> Foo { Foo.Bar(42) }");
    assert!(out.contains("adt_construct Bar("), "output:\n{out}");
}

#[test]
fn test_nullary_constructor() {
    let out = lower_and_display("type Opt = Yes | No\nfn main() -> Opt { Opt.No }");
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

#[test]
fn test_named_args_reordered_direct_call() {
    let out = lower_and_display(
        "fn sub(x: Int, y: Int) -> Int { x - y }\nfn main() -> Int { sub(y: 10, x: 3) }",
    );
    assert!(out.contains("call @sub(%1, %0)"), "output:\n{out}");
}

#[test]
fn test_named_args_reordered_module_call() {
    let out = lower_and_display("import math\nfn main() -> Int { math.min(b: 10, a: 3) }");
    assert!(out.contains("call intrinsic:min(%1, %0)"), "output:\n{out}");
}

#[test]
fn test_from_imported_synthetic_module_call_keeps_intrinsic_identity() {
    let out = lower_and_display("from hash import md5\nfn main() -> String { md5(\"abc\") }");
    assert!(out.contains("call intrinsic:string_md5"), "output:\n{out}");
}

#[test]
fn test_unimported_module_call_is_not_lowered_as_intrinsic() {
    let result = check_file("fn main() -> Int { math.min(1, 2) }");
    let mut interner = result.interner;
    let module = lower_module(
        &result.item_tree,
        &result.module_scope,
        &result.type_check,
        &mut interner,
    );
    let ctx = DisplayCtx::new(&interner, &result.item_tree);
    let out = display_module(&module, &ctx);
    assert!(
        !out.contains("call intrinsic:min"),
        "unimported module call must not lower as intrinsic. output:\n{out}"
    );
}

#[test]
fn test_named_args_reordered_method_call() {
    let out = lower_and_display("fn main() -> String { \"abcd\".substring(end: 3, start: 1) }");
    assert!(
        out.contains("call intrinsic:string_substring(%0, %2, %1)"),
        "output:\n{out}"
    );
}

#[test]
fn test_char_code_method_lowers_to_intrinsic() {
    let out = lower_and_display("fn main() -> Int { 'é'.code() }");
    assert!(out.contains("call intrinsic:char_code"), "output:\n{out}");
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
    let out = lower_and_display("fn f(x: Int) -> Int { if (x > 0) { x } else { -x } }");
    assert!(out.contains("branch"), "output:\n{out}");
    assert!(out.contains("then"), "output:\n{out}");
    assert!(out.contains("else"), "output:\n{out}");
    assert!(out.contains("merge"), "output:\n{out}");
}

#[test]
fn test_if_no_else() {
    let out = lower_and_display("fn f(x: Bool) { if (x) { } }");
    assert!(out.contains("branch"), "output:\n{out}");
}

#[test]
fn test_nested_if() {
    let out = lower_and_display(
        "fn f(x: Int) -> Int { if (x > 0) { if (x > 10) { 10 } else { x } } else { 0 } }",
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
        lower_and_display("fn f(x: Int) -> Int {\n  let y = if (x > 0) { 1 } else { 0 }\n  y\n}");
    assert!(out.contains("branch"), "output:\n{out}");
    assert!(out.contains("merge"), "output:\n{out}");
}

// ── Match expressions ────────────────────────────────────────────

#[test]
fn test_adt_match_switch() {
    let out = lower_and_display(
        "type Wrap<T> = Val(T) | Empty
         fn f(x: Wrap<Int>) -> Int {
           match (x) {
             Wrap.Val(n) => n
             Wrap.Empty => 0
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
           match (x) {
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
        "type Wrap<T> = Val(T) | Empty
         fn f(x: Wrap<Int>) -> Int {
           match (x) {
             Wrap.Val(n) => n
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
        "type Wrap<T> = Val(T) | Empty
         fn f(x: Wrap<Int>) -> Int {
           match (x) {
             Wrap.Val(n) => n
             other => 0
           }
         }",
    );
    assert!(out.contains("switch"), "output:\n{out}");
}

#[test]
fn test_nested_constructor() {
    let out = lower_and_display(
        "type Wrap<T> = Val(T) | Empty
         fn f(x: Wrap<Int>) -> Int {
           match (x) {
             Wrap.Val(n) => n + 1
             Wrap.Empty => 0
           }
         }",
    );
    assert!(out.contains("adt_field_get"), "output:\n{out}");
    assert!(out.contains("add"), "output:\n{out}");
}

#[test]
fn test_multiple_arms() {
    let out = lower_and_display(
        "type Color = Red | Green | Blue
         fn f(c: Color) -> Int {
           match (c) {
             Color.Red => 1
             Color.Green => 2
             Color.Blue => 3
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
           if (x > 0) { return x } else { }
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
        "type Wrap<T> = Val(T) | Empty
         fn f(x: Wrap<Int>) -> Int {
           match (x) {
             Wrap.Val(n) => return n
             Wrap.Empty => 0
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
    let out = lower_and_display("fn f(x: Int) -> Int contract requires (x > 0) { x }");
    assert!(out.contains("assert"), "output:\n{out}");
    assert!(out.contains("requires"), "output:\n{out}");
}

#[test]
fn test_ensures_clause() {
    let out = lower_and_display("fn f(x: Int) -> Int contract ensures (x > 0) { x }");
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
           match (x) {
             0 => 100
             1 => 200
             _ => -1
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
        type Wrap<T> = Val(T) | Empty
        fn f(x: Wrap<Int>) -> Int {
            match (x) {
                Wrap.Val(n) => if (n > 0) { n } else { 0 }
                Wrap.Empty => -1
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
            if (n < 2) {
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
    let out = lower_and_display("fn f(x: Int) -> Int contract ensures (result > 0) { x }");
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
        "fn f(x: Int) -> Int contract ensures (result > 0) {
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
           match (p) {
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
        "type Wrap<T> = Val(T) | Empty
         fn f(x: Int) -> Int {
           match (x) {
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
        "fn f(x: Int) -> Int contract ensures (result > 0) {
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
    let result = check_file("fn f(x: Int) -> Int contract ensures (result > 0) { x }");
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
        type Wrap<T> = Val(T) | Empty
        fn f(x: Int) -> Wrap<Int> {
            if (x > 0) {
                Wrap.Val(x)
            } else {
                Wrap.Empty
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
        "fn f(x: Int) -> Int contract ensures (old(x) > 0) {
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
    let out = lower_and_display("fn f(x: Int) -> Int contract ensures (result > 0) { x }");
    assert!(out.contains("assert"), "output:\n{out}");
    assert!(out.contains("ensures"), "output:\n{out}");
    assert!(!out.contains("hole"), "output:\n{out}");
}

#[test]
fn test_old_without_rebinding_references_param() {
    // Guard: old(x) where x is NOT rebound — should reference the original
    // param regardless (trivially correct, verifies old() doesn't break).
    let out = lower_and_display("fn f(x: Int) -> Int contract ensures (old(x) > 0) { x }");
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
        "type W = A | B
         fn f(x: W) -> Int {
           match (x) {
             W.A => 1
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
        "type W = A | B
         fn f(x: W) -> Int {
           match (x) {
             W.A => 1
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
        "type W = A | B
         fn f(x: W) -> Int {
           match (x) {
             other => 1
             W.A => 2
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
        "type W = A | B
         fn f(x: W) -> Int {
           match (x) {
             W.A => 1
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
        "type W = A | B
         fn f(x: W) -> Int {
           match (x) {
             W.A => 1
             W.A => 2
             W.B => 3
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
        "type W = A | B
         fn f(x: W) -> Int {
           match (x) {
             W.A => 10
             W.A => 20
             W.B => 30
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

// ── Validator: duplicate switch-case check (#143) ───

#[test]
fn test_validator_rejects_duplicate_switch_cases() {
    // Build a KIR function with a Switch that has duplicate variants.
    let mut interner = Interner::new();
    let fn_name = Name::new(&mut interner, "test_fn");
    let variant_a = Name::new(&mut interner, "A");

    let mut builder = KirBuilder::new();
    let entry_name = Name::new(&mut interner, "entry");
    let entry = builder.new_block(Some(entry_name));
    builder.switch_to(entry);

    let scr = builder.alloc_value(Ty::Int, Inst::FnParam { index: 0 });
    let case1_blk = builder.new_block(None);
    let case2_blk = builder.new_block(None);

    // Two switch cases with the same variant name.
    builder.set_terminator(Terminator::Switch {
        scrutinee: scr,
        cases: vec![
            SwitchCase {
                variant: variant_a,
                target: BranchTarget {
                    block: case1_blk,
                    args: vec![],
                },
            },
            SwitchCase {
                variant: variant_a,
                target: BranchTarget {
                    block: case2_blk,
                    args: vec![],
                },
            },
        ],
        default: None,
    });

    // Terminate case blocks.
    builder.switch_to(case1_blk);
    let c1 = builder.push_const(Constant::Int(1), Ty::Int);
    builder.set_return(c1);
    builder.switch_to(case2_blk);
    let c2 = builder.push_const(Constant::Int(2), Ty::Int);
    builder.set_return(c2);

    let func = builder.build(
        fn_name,
        vec![(Name::new(&mut interner, "x"), Ty::Int)],
        Ty::Int,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let diags = validate_function(&func, &interner);
    assert!(
        diags.iter().any(|d| d.message.contains("duplicate case")),
        "validator should reject duplicate switch cases, got: {diags:?}"
    );
}

#[test]
fn test_validator_accepts_unique_switch_cases() {
    // Guard: unique switch cases should pass validation.
    let out = lower_and_display(
        "type W = A | B
         fn f(x: W) -> Int {
           match (x) {
             W.A => 1
             W.B => 2
           }
         }",
    );
    // lower_and_display already validates; just confirm output is well-formed.
    assert!(out.contains("switch"), "output:\n{out}");
}

// ── Bug regression: param types lowered as <error> (#146) ───

#[test]
fn test_param_types_not_error() {
    // Bug: resolve_param_types scanned pat_scopes for params, which didn't
    // include top-level function params. Result: `x: <error>`.
    let out = lower_and_display("fn f(x: Int) -> Int { x }");
    // The param type should be Int, not <error>.
    assert!(
        out.contains("x: Int"),
        "param type should be Int, not <error>. output:\n{out}"
    );
    assert!(
        !out.contains("<error>"),
        "should have no <error> types. output:\n{out}"
    );
}

#[test]
fn test_multi_param_types_resolved() {
    // Guard: multiple params should all get correct types.
    let out = lower_and_display("fn add(a: Int, b: Int) -> Int { a + b }");
    assert!(
        out.contains("a: Int") && out.contains("b: Int"),
        "all param types should be resolved. output:\n{out}"
    );
}

// ── Bug regression: ret_ty is Never for explicit return (#147) ───

#[test]
fn test_explicit_return_ret_ty_not_never() {
    // Bug: lower_function used expr_ty(body.root) for ret_ty. When the root
    // expression is `return x`, its type is Never, not the declared type.
    let out = lower_and_display("fn f(x: Int) -> Int { return x }");
    assert!(
        out.contains("-> Int"),
        "return type should be Int, not Never. output:\n{out}"
    );
    assert!(
        !out.contains("-> Never"),
        "return type should not be Never. output:\n{out}"
    );
}

#[test]
fn test_implicit_return_ret_ty_preserved() {
    // Guard: implicit return should still work correctly.
    let out = lower_and_display("fn f(x: Int) -> Int { x }");
    assert!(out.contains("-> Int"), "output:\n{out}");
}

// ── Bug regression: sequential match unbound merge param (#148) ───

#[test]
fn test_sequential_match_all_return_marks_merge_unreachable() {
    // Bug: lower_match_sequential always creates merge block + param, but
    // when all arms return, no one jumps to merge, leaving an unbound param.
    let out = lower_and_display(
        "fn f(x: Int) -> Int {
           match (x) {
             0 => return 1
             _ => return 2
           }
         }",
    );
    // The merge block should be marked unreachable, not have `return %N`.
    let lines: Vec<&str> = out.lines().collect();
    let merge_idx = lines
        .iter()
        .position(|l| l.trim().starts_with("merge("))
        .expect("merge block should exist");
    let merge_next = lines[merge_idx + 1].trim();
    assert!(
        merge_next == "unreachable",
        "merge block should be unreachable when all arms return, got: `{merge_next}`\noutput:\n{out}"
    );
}

#[test]
fn test_sequential_match_partial_return_keeps_merge() {
    // Guard: if some arms don't return, merge should be reachable.
    let out = lower_and_display(
        "fn f(x: Int) -> Int {
           match (x) {
             0 => return 1
             _ => 2
           }
         }",
    );
    // merge block should have a return (not unreachable).
    let lines: Vec<&str> = out.lines().collect();
    let merge_idx = lines
        .iter()
        .position(|l| l.trim().starts_with("merge("))
        .expect("merge block should exist");
    // Lines after merge should eventually have `return`, not `unreachable`.
    let merge_body: Vec<&&str> = lines[merge_idx + 1..]
        .iter()
        .take_while(|l| !l.trim().is_empty() && !l.starts_with("  bb") && !l.starts_with("  merge"))
        .collect();
    let has_return = merge_body.iter().any(|l| l.contains("return"));
    assert!(
        has_return,
        "merge should have return. merge body: {merge_body:?}\noutput:\n{out}"
    );
}

// ── Bug regression: sequential match ignores early catch-all (#149) ───

#[test]
fn test_sequential_match_wildcard_stops_dispatch() {
    // Bug: wildcard arm doesn't stop the sequential dispatch loop, so
    // later literal arms can still affect control flow.
    let result = check_file(
        "fn f(x: Int) -> Int {
           match (x) {
             _ => 1
             0 => 2
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
    let ctx = DisplayCtx::new(&interner, &result.item_tree);
    let out = display_module(&module, &ctx);

    // After the catch-all `_ => 1`, there should be no branch/eq for `0 => 2`.
    assert!(
        !out.contains("eq "),
        "should not have equality check after catch-all. output:\n{out}"
    );
    assert!(
        !out.contains("branch"),
        "should not have branch after catch-all. output:\n{out}"
    );
}

#[test]
fn test_sequential_match_bind_stops_dispatch() {
    // Same but with a bind pattern.
    let result = check_file(
        "fn f(x: Int) -> Int {
           match (x) {
             n => n
             0 => 2
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
    let ctx = DisplayCtx::new(&interner, &result.item_tree);
    let out = display_module(&module, &ctx);

    assert!(
        !out.contains("eq "),
        "should not have equality check after bind catch-all. output:\n{out}"
    );
}

// ── Bug regression: ADT switch ignores nested subpatterns (#150) ───

#[test]
fn test_adt_match_nested_literal_check() {
    // Bug: ADT switch dispatches on outer constructor but doesn't emit
    // equality checks for nested literal subpatterns like `Some(1)`.
    let out = lower_and_display(
        "type O = Some(Int) | None
         fn f(x: O) -> Int {
           match (x) {
             O.Some(1) => 10
             _ => 0
           }
         }",
    );
    // After extracting the field from Some, there should be an equality check.
    assert!(
        out.contains("eq "),
        "should have equality check for nested literal `1`. output:\n{out}"
    );
}

#[test]
fn test_adt_match_nested_bind_still_works() {
    // Guard: nested bind patterns (no literal) should NOT produce eq checks.
    let out = lower_and_display(
        "type O = Some(Int) | None
         fn f(x: O) -> Int {
           match (x) {
             O.Some(n) => n
             O.None => 0
           }
         }",
    );
    // No equality checks needed — just field extraction and binding.
    assert!(
        !out.contains("eq "),
        "should not have equality check for bind pattern. output:\n{out}"
    );
}

// ── #155: callable values should emit fn_ref, not hole ──────────────

#[test]
fn test_fn_ref_emitted_for_function_value() {
    // Bug: `let f = inc` emits `hole` instead of `fn_ref @inc`.
    let out = lower_and_display(
        "fn inc(x: Int) -> Int { x + 1 }
         fn apply(x: Int) -> Int {
           let f = inc
           f(x)
         }",
    );
    assert!(
        out.contains("fn_ref @inc"),
        "should emit fn_ref for function reference. output:\n{out}"
    );
    assert!(
        !out.contains("hole #"),
        "function reference should not be a hole. output:\n{out}"
    );
}

#[test]
fn test_capturing_lambda_does_not_lower_to_placeholder_hole() {
    let out = lower_and_display(
        "fn f() -> Int {\n\
           let base = 5\n\
           let g = fn(x: Int) => x + base\n\
           g(7)\n\
         }",
    );
    assert!(
        !out.contains("hole #"),
        "capturing lambdas should lower without placeholder holes. output:\n{out}"
    );
}

#[test]
fn test_fn_ref_guard_hole_stays_hole() {
    // Guard: typed holes should still emit hole instructions, not fn_ref.
    let out = lower_and_display("fn f(x: Int) -> Int { x + _ }");
    assert!(
        out.contains("hole #"),
        "typed hole should remain a hole. output:\n{out}"
    );
}

// ── #151: validator should detect unbound block params ──────────────

#[test]
fn test_validator_rejects_block_params_with_no_predecessors() {
    // Bug: a block with params but no incoming edges passes validation.
    // Construct KIR manually: entry -> return, orphan block with a param.
    let mut interner = Interner::new();
    let mut builder = KirBuilder::new();

    let entry = builder.new_block(None);
    let orphan = builder.new_block(None);

    builder.switch_to(entry);
    let unit = builder.push_const(Constant::Unit, Ty::Unit);
    builder.set_return(unit);

    // orphan block has a param but nobody jumps to it
    let _param = builder.add_block_param(orphan, None, Ty::Int);
    builder.switch_to(orphan);
    let dead = builder.push_const(Constant::Int(0), Ty::Int);
    builder.set_return(dead);

    let func = builder.build(
        Name::new(&mut interner, "test"),
        vec![],
        Ty::Unit,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let diags = validate_function(&func, &interner);
    assert!(
        diags.iter().any(|d| d.message.contains("no predecessor")),
        "should reject block with params but no predecessors. diags: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn test_validator_accepts_block_params_with_predecessors() {
    // Guard: block with params that has an incoming jump should be fine.
    let mut interner = Interner::new();
    let mut builder = KirBuilder::new();

    let entry = builder.new_block(None);
    let target = builder.new_block(None);

    builder.switch_to(entry);
    let val = builder.push_const(Constant::Int(42), Ty::Int);
    builder.set_jump(BranchTarget {
        block: target,
        args: vec![val],
    });

    let param = builder.add_block_param(target, None, Ty::Int);
    builder.switch_to(target);
    builder.set_return(param);

    let func = builder.build(
        Name::new(&mut interner, "test"),
        vec![],
        Ty::Int,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let diags = validate_function(&func, &interner);
    assert!(
        diags.is_empty(),
        "block with params and a predecessor should pass. diags: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ── #152: ADT match must not silently drop unsupported patterns ─────

#[test]
fn test_adt_match_unsupported_pattern_falls_back_to_sequential() {
    // Bug: a literal pattern arm on an ADT match gets silently dropped.
    // With the fix, is_adt_match returns false and sequential lowering
    // handles it, so both arms produce code.
    //
    // This source has a type error (literal on ADT) but lowering runs anyway.
    let source = "type O = Some(Int) | None
         fn f(x: O) -> Int {
           match (x) {
             1 => 1
             O.Some(n) => n
             _ => 0
           }
         }";
    let result = check_file(source);
    // Has type errors — skip the assertion on diagnostics.
    let mut interner = result.interner;
    let module = lower_module(
        &result.item_tree,
        &result.module_scope,
        &result.type_check,
        &mut interner,
    );

    let ctx = DisplayCtx::new(&interner, &result.item_tree);
    let out = display_module(&module, &ctx);

    // The output should NOT contain a switch (would mean ADT path was used).
    // Sequential lowering uses eq/branch.
    assert!(
        !out.contains("switch "),
        "should fall back to sequential lowering, not ADT switch. output:\n{out}"
    );
}

#[test]
fn test_adt_match_all_supported_still_uses_switch() {
    // Guard: a normal ADT match with only constructor/wildcard/bind should
    // still use the switch-based lowering path.
    let out = lower_and_display(
        "type O = Some(Int) | None
         fn f(x: O) -> Int {
           match (x) {
             O.Some(n) => n
             O.None => 0
           }
         }",
    );
    assert!(
        out.contains("switch "),
        "normal ADT match should use switch lowering. output:\n{out}"
    );
}

// ── #153: validator should check Bool types for branch/assert ───────

#[test]
fn test_validator_rejects_non_bool_branch_condition() {
    // Bug: branch with Int condition passes validation.
    let mut interner = Interner::new();
    let mut builder = KirBuilder::new();

    let entry = builder.new_block(None);
    let then_blk = builder.new_block(None);
    let else_blk = builder.new_block(None);

    builder.switch_to(entry);
    let int_val = builder.push_const(Constant::Int(1), Ty::Int);
    builder.set_branch(
        int_val,
        BranchTarget {
            block: then_blk,
            args: vec![],
        },
        BranchTarget {
            block: else_blk,
            args: vec![],
        },
    );

    builder.switch_to(then_blk);
    let unit1 = builder.push_const(Constant::Unit, Ty::Unit);
    builder.set_return(unit1);

    builder.switch_to(else_blk);
    let unit2 = builder.push_const(Constant::Unit, Ty::Unit);
    builder.set_return(unit2);

    let func = builder.build(
        Name::new(&mut interner, "test"),
        vec![],
        Ty::Unit,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let diags = validate_function(&func, &interner);
    assert!(
        diags.iter().any(|d| d.message.contains("Bool")),
        "should reject non-Bool branch condition. diags: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn test_validator_accepts_bool_branch_condition() {
    // Guard: branch with Bool condition should pass.
    let mut interner = Interner::new();
    let mut builder = KirBuilder::new();

    let entry = builder.new_block(None);
    let then_blk = builder.new_block(None);
    let else_blk = builder.new_block(None);

    builder.switch_to(entry);
    let bool_val = builder.push_const(Constant::Bool(true), Ty::Bool);
    builder.set_branch(
        bool_val,
        BranchTarget {
            block: then_blk,
            args: vec![],
        },
        BranchTarget {
            block: else_blk,
            args: vec![],
        },
    );

    builder.switch_to(then_blk);
    let unit1 = builder.push_const(Constant::Unit, Ty::Unit);
    builder.set_return(unit1);

    builder.switch_to(else_blk);
    let unit2 = builder.push_const(Constant::Unit, Ty::Unit);
    builder.set_return(unit2);

    let func = builder.build(
        Name::new(&mut interner, "test"),
        vec![],
        Ty::Unit,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let diags = validate_function(&func, &interner);
    assert!(
        diags.is_empty(),
        "Bool branch condition should pass. diags: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn test_validator_rejects_non_bool_assert_condition() {
    // Bug: assert with Int condition passes validation.
    let mut interner = Interner::new();
    let mut builder = KirBuilder::new();

    let entry = builder.new_block(None);
    builder.switch_to(entry);

    let int_val = builder.push_const(Constant::Int(1), Ty::Int);
    let _assert = builder.push_assert(int_val, "test".to_string(), Ty::Unit);
    let unit = builder.push_const(Constant::Unit, Ty::Unit);
    builder.set_return(unit);

    let func = builder.build(
        Name::new(&mut interner, "test"),
        vec![],
        Ty::Unit,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let diags = validate_function(&func, &interner);
    assert!(
        diags.iter().any(|d| d.message.contains("Bool")),
        "should reject non-Bool assert condition. diags: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn test_validator_accepts_bool_assert_condition() {
    // Guard: assert with Bool condition should pass.
    let mut interner = Interner::new();
    let mut builder = KirBuilder::new();

    let entry = builder.new_block(None);
    builder.switch_to(entry);

    let bool_val = builder.push_const(Constant::Bool(true), Ty::Bool);
    let _assert = builder.push_assert(bool_val, "test".to_string(), Ty::Unit);
    let unit = builder.push_const(Constant::Unit, Ty::Unit);
    builder.set_return(unit);

    let func = builder.build(
        Name::new(&mut interner, "test"),
        vec![],
        Ty::Unit,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let diags = validate_function(&func, &interner);
    assert!(
        diags.is_empty(),
        "Bool assert condition should pass. diags: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ── #156: validator should check base types for field_get/adt_field_get ──

#[test]
fn test_validator_rejects_field_get_on_int() {
    // Bug: field_get on Int base passes validation.
    let mut interner = Interner::new();
    let mut builder = KirBuilder::new();

    let entry = builder.new_block(None);
    builder.switch_to(entry);

    let int_val = builder.push_const(Constant::Int(1), Ty::Int);
    let field = Name::new(&mut interner, "x");
    let _fg = builder.push_field_get(int_val, field, Ty::Error);
    let unit = builder.push_const(Constant::Unit, Ty::Unit);
    builder.set_return(unit);

    let func = builder.build(
        Name::new(&mut interner, "test"),
        vec![],
        Ty::Unit,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let diags = validate_function(&func, &interner);
    assert!(
        diags.iter().any(|d| d.message.contains("field_get")),
        "should reject field_get on Int base. diags: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn test_validator_accepts_field_get_on_record() {
    // Guard: field_get on Record base should pass.
    let mut interner = Interner::new();
    let mut builder = KirBuilder::new();

    let entry = builder.new_block(None);
    builder.switch_to(entry);

    let field_name = Name::new(&mut interner, "x");
    let record_ty = Ty::Record {
        fields: vec![(field_name, Ty::Int)],
    };
    let rec = builder.push_const(Constant::Unit, record_ty); // placeholder const
    let _fg = builder.push_field_get(rec, field_name, Ty::Int);
    let unit = builder.push_const(Constant::Unit, Ty::Unit);
    builder.set_return(unit);

    let func = builder.build(
        Name::new(&mut interner, "test"),
        vec![],
        Ty::Unit,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let diags = validate_function(&func, &interner);
    let field_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.message.contains("field_get"))
        .collect();
    assert!(
        field_diags.is_empty(),
        "field_get on Record base should not produce field_get errors. diags: {:?}",
        field_diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn test_validator_rejects_adt_field_get_on_int() {
    // Bug: adt_field_get on Int base passes validation.
    let mut interner = Interner::new();
    let mut builder = KirBuilder::new();

    let entry = builder.new_block(None);
    builder.switch_to(entry);

    let int_val = builder.push_const(Constant::Int(1), Ty::Int);
    let _afg = builder.push_adt_field_get(int_val, 0, Ty::Error);
    let unit = builder.push_const(Constant::Unit, Ty::Unit);
    builder.set_return(unit);

    let func = builder.build(
        Name::new(&mut interner, "test"),
        vec![],
        Ty::Unit,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let diags = validate_function(&func, &interner);
    assert!(
        diags.iter().any(|d| d.message.contains("adt_field_get")),
        "should reject adt_field_get on Int base. diags: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn test_validator_accepts_adt_field_get_on_adt() {
    // Guard: adt_field_get on Adt base should pass.
    use kyokara_hir_def::item_tree::TypeItemIdx;

    let mut interner = Interner::new();
    let mut builder = KirBuilder::new();

    let entry = builder.new_block(None);
    builder.switch_to(entry);

    let adt_ty = Ty::Adt {
        def: TypeItemIdx::from_raw(la_arena::RawIdx::from_u32(0)),
        args: vec![],
    };
    let adt_val = builder.push_const(Constant::Unit, adt_ty); // placeholder
    let _afg = builder.push_adt_field_get(adt_val, 0, Ty::Int);
    let unit = builder.push_const(Constant::Unit, Ty::Unit);
    builder.set_return(unit);

    let func = builder.build(
        Name::new(&mut interner, "test"),
        vec![],
        Ty::Unit,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let diags = validate_function(&func, &interner);
    let afg_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.message.contains("adt_field_get"))
        .collect();
    assert!(
        afg_diags.is_empty(),
        "adt_field_get on Adt base should not produce adt_field_get errors. diags: {:?}",
        afg_diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ── #157: validator should check indirect call target is Fn type ────

#[test]
fn test_validator_rejects_indirect_call_on_int() {
    // Bug: indirect call with Int target passes validation.
    let mut interner = Interner::new();
    let mut builder = KirBuilder::new();

    let entry = builder.new_block(None);
    builder.switch_to(entry);

    let int_val = builder.push_const(Constant::Int(1), Ty::Int);
    let _call = builder.push_call(CallTarget::Indirect(int_val), vec![], Ty::Error);
    let unit = builder.push_const(Constant::Unit, Ty::Unit);
    builder.set_return(unit);

    let func = builder.build(
        Name::new(&mut interner, "test"),
        vec![],
        Ty::Unit,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let diags = validate_function(&func, &interner);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("indirect call target")),
        "should reject indirect call on Int. diags: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn test_validator_accepts_indirect_call_on_fn() {
    // Guard: indirect call with Fn target should pass.
    let mut interner = Interner::new();
    let mut builder = KirBuilder::new();

    let entry = builder.new_block(None);
    builder.switch_to(entry);

    let fn_ty = Ty::Fn {
        params: vec![],
        ret: Box::new(Ty::Unit),
    };
    let fn_val = builder.push_const(Constant::Unit, fn_ty); // placeholder
    let _call = builder.push_call(CallTarget::Indirect(fn_val), vec![], Ty::Unit);
    let unit = builder.push_const(Constant::Unit, Ty::Unit);
    builder.set_return(unit);

    let func = builder.build(
        Name::new(&mut interner, "test"),
        vec![],
        Ty::Unit,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let diags = validate_function(&func, &interner);
    let call_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.message.contains("indirect call"))
        .collect();
    assert!(
        call_diags.is_empty(),
        "indirect call on Fn should pass. diags: {:?}",
        call_diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}
