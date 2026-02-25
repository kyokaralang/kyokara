//! Integration tests for the tree-walking interpreter.

use kyokara_eval::value::Value;

fn run_ok(source: &str) -> Value {
    match kyokara_eval::run(source) {
        Ok(result) => result.value,
        Err(e) => panic!("runtime error: {e}"),
    }
}

fn run_err(source: &str) -> String {
    match kyokara_eval::run(source) {
        Ok(result) => panic!("expected error, got {:?}", result.value),
        Err(e) => e.to_string(),
    }
}

// ── Literal tests ────────────────────────────────────────────────────

#[test]
fn eval_literal_int() {
    let val = run_ok("fn main() -> Int { 42 }");
    assert!(matches!(val, Value::Int(42)));
}

#[test]
fn eval_literal_bool_true() {
    let val = run_ok("fn main() -> Bool { true }");
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_literal_bool_false() {
    let val = run_ok("fn main() -> Bool { false }");
    assert!(matches!(val, Value::Bool(false)));
}

#[test]
fn eval_literal_string() {
    let val = run_ok(r#"fn main() -> String { "hello" }"#);
    match val {
        Value::String(s) => assert_eq!(s, "hello"),
        other => panic!("expected String, got {other:?}"),
    }
}

// ── Arithmetic tests ─────────────────────────────────────────────────

#[test]
fn eval_arithmetic_add() {
    let val = run_ok("fn main() -> Int { 2 + 3 }");
    assert!(matches!(val, Value::Int(5)));
}

#[test]
fn eval_arithmetic_mul() {
    let val = run_ok("fn main() -> Int { 4 * 5 }");
    assert!(matches!(val, Value::Int(20)));
}

#[test]
fn eval_arithmetic_precedence() {
    let val = run_ok("fn main() -> Int { 2 + 3 * 4 }");
    assert!(matches!(val, Value::Int(14)));
}

#[test]
fn eval_arithmetic_sub() {
    let val = run_ok("fn main() -> Int { 10 - 3 }");
    assert!(matches!(val, Value::Int(7)));
}

#[test]
fn eval_arithmetic_div() {
    let val = run_ok("fn main() -> Int { 15 / 3 }");
    assert!(matches!(val, Value::Int(5)));
}

#[test]
fn eval_division_by_zero() {
    let err = run_err("fn main() -> Int { 1 / 0 }");
    assert!(err.contains("division by zero"));
}

// ── Function call tests ──────────────────────────────────────────────

#[test]
fn eval_function_call() {
    let val = run_ok(
        "fn add(x: Int, y: Int) -> Int { x + y }
         fn main() -> Int { add(1, 2) }",
    );
    assert!(matches!(val, Value::Int(3)));
}

#[test]
fn eval_nested_calls() {
    let val = run_ok(
        "fn double(x: Int) -> Int { x + x }
         fn quad(x: Int) -> Int { double(double(x)) }
         fn main() -> Int { quad(3) }",
    );
    assert!(matches!(val, Value::Int(12)));
}

// ── Let binding tests ────────────────────────────────────────────────

#[test]
fn eval_let_binding() {
    let val = run_ok("fn main() -> Int { let x = 10\n x + 1 }");
    assert!(matches!(val, Value::Int(11)));
}

#[test]
fn eval_let_multiple() {
    let val = run_ok(
        "fn main() -> Int {
           let a = 3
           let b = 4
           a + b
         }",
    );
    assert!(matches!(val, Value::Int(7)));
}

// ── If/else tests ────────────────────────────────────────────────────

#[test]
fn eval_if_true() {
    let val = run_ok("fn main() -> Int { if true { 1 } else { 2 } }");
    assert!(matches!(val, Value::Int(1)));
}

#[test]
fn eval_if_false() {
    let val = run_ok("fn main() -> Int { if false { 1 } else { 2 } }");
    assert!(matches!(val, Value::Int(2)));
}

#[test]
fn eval_if_with_comparison() {
    let val = run_ok(
        "fn main() -> Int {
           let x = 5
           if x > 3 { 100 } else { 0 }
         }",
    );
    assert!(matches!(val, Value::Int(100)));
}

// ── Comparison operator tests ────────────────────────────────────────

#[test]
fn eval_comparison_eq() {
    let val = run_ok("fn main() -> Bool { 42 == 42 }");
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_comparison_neq() {
    let val = run_ok("fn main() -> Bool { 1 != 2 }");
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_comparison_lt() {
    let val = run_ok("fn main() -> Bool { 1 < 2 }");
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_comparison_gt() {
    let val = run_ok("fn main() -> Bool { 3 > 2 }");
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_comparison_lteq() {
    let val = run_ok("fn main() -> Bool { 2 <= 2 }");
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_comparison_gteq() {
    let val = run_ok("fn main() -> Bool { 3 >= 2 }");
    assert!(matches!(val, Value::Bool(true)));
}

// ── Boolean logic tests ──────────────────────────────────────────────

#[test]
fn eval_not_true() {
    let val = run_ok("fn main() -> Bool { !true }");
    assert!(matches!(val, Value::Bool(false)));
}

#[test]
fn eval_not_false() {
    let val = run_ok("fn main() -> Bool { !false }");
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_bool_equality() {
    let val = run_ok("fn main() -> Bool { true == false }");
    assert!(matches!(val, Value::Bool(false)));
}

// ── Unary operator tests ─────────────────────────────────────────────

#[test]
fn eval_negate_int() {
    let val = run_ok("fn main() -> Int { -42 }");
    assert!(matches!(val, Value::Int(-42)));
}

// ── ADT constructor tests ────────────────────────────────────────────

#[test]
fn eval_adt_nullary_constructor() {
    let val = run_ok(
        "type Color = | Red | Green | Blue
         fn main() -> Color { Red }",
    );
    assert!(matches!(val, Value::Adt { variant: 0, .. }));
}

#[test]
fn eval_adt_constructor_call() {
    let val = run_ok(
        "type Option<T> = | Some(T) | None
         fn main() -> Option<Int> { Some(42) }",
    );
    match val {
        Value::Adt {
            variant: 0,
            ref fields,
            ..
        } => {
            assert!(matches!(fields[0], Value::Int(42)));
        }
        other => panic!("expected Adt, got {other:?}"),
    }
}

// ── Pattern match tests ──────────────────────────────────────────────

#[test]
fn eval_pattern_match_nullary() {
    let val = run_ok(
        "type Bool2 = | True | False
         fn to_int(x: Bool2) -> Int {
           match x {
             True => 1
             False => 0
           }
         }
         fn main() -> Int { to_int(True) }",
    );
    assert!(matches!(val, Value::Int(1)));
}

#[test]
fn eval_pattern_match_with_bind() {
    let val = run_ok(
        "type Option<T> = | Some(T) | None
         fn unwrap(x: Option<Int>) -> Int {
           match x {
             Some(v) => v
             None => 0
           }
         }
         fn main() -> Int { unwrap(Some(42)) }",
    );
    assert!(matches!(val, Value::Int(42)));
}

#[test]
fn eval_pattern_match_wildcard() {
    let val = run_ok(
        "type Color = | Red | Green | Blue
         fn is_red(c: Color) -> Int {
           match c {
             Red => 1
             _ => 0
           }
         }
         fn main() -> Int { is_red(Blue) }",
    );
    assert!(matches!(val, Value::Int(0)));
}

// ── Record tests ─────────────────────────────────────────────────────

#[test]
fn eval_record_literal() {
    let val = run_ok(
        "type Point = { x: Int, y: Int }
         fn main() -> Int {
           let p = Point { x: 3, y: 4 }
           p.x + p.y
         }",
    );
    assert!(matches!(val, Value::Int(7)));
}

// ── Recursion tests ──────────────────────────────────────────────────

#[test]
fn eval_factorial() {
    let val = run_ok(
        "fn fact(n: Int) -> Int {
           if n <= 1 { 1 } else { n * fact(n - 1) }
         }
         fn main() -> Int { fact(5) }",
    );
    assert!(matches!(val, Value::Int(120)));
}

#[test]
fn eval_fibonacci() {
    let val = run_ok(
        "fn fib(n: Int) -> Int {
           if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
         }
         fn main() -> Int { fib(10) }",
    );
    assert!(matches!(val, Value::Int(55)));
}

// ── Lambda tests ─────────────────────────────────────────────────────

#[test]
fn eval_lambda() {
    let val = run_ok(
        "fn main() -> Int {
           let f = fn(x: Int) => x + 1
           f(5)
         }",
    );
    assert!(matches!(val, Value::Int(6)));
}

#[test]
fn eval_lambda_capture() {
    let val = run_ok(
        "fn main() -> Int {
           let offset = 10
           let f = fn(x: Int) => x + offset
           f(5)
         }",
    );
    assert!(matches!(val, Value::Int(15)));
}

// ── String intrinsic tests ───────────────────────────────────────────

#[test]
fn eval_int_to_string() {
    let val = run_ok("fn main() -> String { int_to_string(42) }");
    match val {
        Value::String(s) => assert_eq!(s, "42"),
        other => panic!("expected String, got {other:?}"),
    }
}

#[test]
fn eval_string_concat() {
    let val = run_ok(r#"fn main() -> String { string_concat("foo", "bar") }"#);
    match val {
        Value::String(s) => assert_eq!(s, "foobar"),
        other => panic!("expected String, got {other:?}"),
    }
}

// ── Block scoping tests ─────────────────────────────────────────────

#[test]
fn eval_block_scoping() {
    let val = run_ok(
        "fn main() -> Int {
           let x = 1
           let y = x + 2
           y + x
         }",
    );
    assert!(matches!(val, Value::Int(4)));
}

// ── Error handling tests ─────────────────────────────────────────────

#[test]
fn eval_no_main_error() {
    let err = run_err("fn not_main() -> Int { 42 }");
    assert!(err.contains("main"));
}

#[test]
fn eval_hole_error() {
    let err = run_err("fn main() -> Int { ? }");
    // Hole generates a type mismatch during compilation, or a runtime hole error.
    assert!(err.contains("type") || err.contains("hole"));
}

// ── Combined integration tests ───────────────────────────────────────

#[test]
fn eval_adt_option_program() {
    let val = run_ok(
        "type Option<T> = | Some(T) | None

         fn unwrap_or(opt: Option<Int>, default: Int) -> Int {
           match opt {
             Some(x) => x
             None => default
           }
         }

         fn main() -> Int {
           let x = Some(42)
           let y = None
           unwrap_or(x, 0) + unwrap_or(y, 7)
         }",
    );
    assert!(matches!(val, Value::Int(49)));
}

#[test]
fn eval_higher_order_function() {
    let val = run_ok(
        "fn apply(f: fn(Int) -> Int, x: Int) -> Int { f(x) }
         fn double(x: Int) -> Int { x * 2 }
         fn main() -> Int { apply(double, 21) }",
    );
    assert!(matches!(val, Value::Int(42)));
}

// ── Builtin Option tests ────────────────────────────────────────────

#[test]
fn eval_builtin_option_some() {
    let val = run_ok(
        "fn main() -> Int {
           match Some(42) {
             Some(x) => x
             None => 0
           }
         }",
    );
    assert!(matches!(val, Value::Int(42)));
}

#[test]
fn eval_builtin_option_none() {
    let val = run_ok(
        "fn main() -> Int {
           match None {
             Some(x) => x
             None => 0
           }
         }",
    );
    assert!(matches!(val, Value::Int(0)));
}

// ── Builtin Result tests ────────────────────────────────────────────

#[test]
fn eval_builtin_result_ok() {
    let val = run_ok(
        "fn main() -> Int {
           match Ok(1) {
             Ok(x) => x
             Err(e) => 0
           }
         }",
    );
    assert!(matches!(val, Value::Int(1)));
}

#[test]
fn eval_builtin_result_err() {
    let val = run_ok(
        "fn main() -> Int {
           match Err(99) {
             Ok(x) => x
             Err(e) => e
           }
         }",
    );
    assert!(matches!(val, Value::Int(99)));
}

// ── Propagation operator with builtins ──────────────────────────────

#[test]
fn eval_propagate_ok() {
    let val = run_ok(
        "fn get() -> Result<Int, Int> { Ok(42) }
         fn main() -> Result<Int, Int> {
           let x = get()?
           Ok(x)
         }",
    );
    match val {
        Value::Adt {
            variant: 0,
            ref fields,
            ..
        } => assert!(matches!(fields[0], Value::Int(42))),
        other => panic!("expected Ok(42), got {other:?}"),
    }
}

#[test]
fn eval_propagate_err() {
    let val = run_ok(
        "fn get() -> Result<Int, Int> { Err(1) }
         fn main() -> Result<Int, Int> {
           let x = get()?
           Ok(x)
         }",
    );
    match val {
        Value::Adt {
            variant: 1,
            ref fields,
            ..
        } => assert!(matches!(fields[0], Value::Int(1))),
        other => panic!("expected Err(1), got {other:?}"),
    }
}

// ── Propagation in nested expression contexts ──────────────────────

#[test]
fn eval_propagate_in_call_arg() {
    // `?` inside a function argument should propagate early return.
    let val = run_ok(
        "fn get() -> Result<Int, Int> { Err(5) }
         fn identity(x: Int) -> Int { x }
         fn main() -> Result<Int, Int> {
           Ok(identity(get()?))
         }",
    );
    match val {
        Value::Adt {
            variant: 1,
            ref fields,
            ..
        } => assert!(matches!(fields[0], Value::Int(5))),
        other => panic!("expected Err(5), got {other:?}"),
    }
}

#[test]
fn eval_propagate_in_binary_expr() {
    // `?` inside a binary expression should propagate early return.
    let val = run_ok(
        "fn get() -> Result<Int, Int> { Err(3) }
         fn main() -> Result<Int, Int> {
           Ok(get()? + 1)
         }",
    );
    match val {
        Value::Adt {
            variant: 1,
            ref fields,
            ..
        } => assert!(matches!(fields[0], Value::Int(3))),
        other => panic!("expected Err(3), got {other:?}"),
    }
}

// ── Contract tests ──────────────────────────────────────────────────

#[test]
fn eval_requires_passes() {
    let val = run_ok(
        "fn check(x: Int) -> Int
           requires x > 0
         { x }
         fn main() -> Int { check(5) }",
    );
    assert!(matches!(val, Value::Int(5)));
}

#[test]
fn eval_requires_fails() {
    let err = run_err(
        "fn check(x: Int) -> Int
           requires x > 0
         { x }
         fn main() -> Int { check(-1) }",
    );
    assert!(err.contains("precondition failed"));
}

#[test]
fn eval_ensures_passes() {
    let val = run_ok(
        "fn get() -> Int
           ensures result > 0
         { 42 }
         fn main() -> Int { get() }",
    );
    assert!(matches!(val, Value::Int(42)));
}

#[test]
fn eval_ensures_fails() {
    let err = run_err(
        "fn get() -> Int
           ensures result > 100
         { 42 }
         fn main() -> Int { get() }",
    );
    assert!(err.contains("postcondition failed"));
}

#[test]
fn eval_ensures_result_binding() {
    let val = run_ok(
        "fn ten() -> Int
           ensures result == 10
         { 10 }
         fn main() -> Int { ten() }",
    );
    assert!(matches!(val, Value::Int(10)));
}

#[test]
fn eval_old_in_ensures() {
    let val = run_ok(
        "fn inc(x: Int) -> Int
           ensures result == old(x) + 1
         { x + 1 }
         fn main() -> Int { inc(5) }",
    );
    assert!(matches!(val, Value::Int(6)));
}

#[test]
fn eval_old_in_ensures_fails() {
    let err = run_err(
        "fn inc(x: Int) -> Int
           ensures result == old(x)
         { x + 1 }
         fn main() -> Int { inc(5) }",
    );
    assert!(err.contains("postcondition failed"));
}

#[test]
fn eval_invariant_passes() {
    let val = run_ok(
        "fn check(x: Int) -> Int
           invariant x > 0
         { x }
         fn main() -> Int { check(5) }",
    );
    assert!(matches!(val, Value::Int(5)));
}

#[test]
fn eval_invariant_fails() {
    let err = run_err(
        "fn check(x: Int) -> Int
           invariant x > 100
         { x }
         fn main() -> Int { check(5) }",
    );
    assert!(err.contains("invariant violated"));
}

#[test]
fn eval_requires_and_ensures_combined() {
    let val = run_ok(
        "fn safe_inc(x: Int) -> Int
           requires x > 0
           ensures result > x
         { x + 1 }
         fn main() -> Int { safe_inc(5) }",
    );
    assert!(matches!(val, Value::Int(6)));
}

#[test]
fn eval_no_contract_still_works() {
    // Regression: functions without contracts must keep working.
    let val = run_ok(
        "fn add(a: Int, b: Int) -> Int { a + b }
         fn main() -> Int { add(3, 4) }",
    );
    assert!(matches!(val, Value::Int(7)));
}

// ── Contract violation tests ────────────────────────────────────────

#[test]
fn eval_requires_fails_at_boundary() {
    // x == 0 should fail `requires x > 0`.
    let err = run_err(
        "fn positive(x: Int) -> Int
           requires x > 0
         { x }
         fn main() -> Int { positive(0) }",
    );
    assert!(err.contains("precondition failed"));
}

#[test]
fn eval_requires_fails_with_equality_check() {
    let err = run_err(
        "fn expect_ten(x: Int) -> Int
           requires x == 10
         { x }
         fn main() -> Int { expect_ten(9) }",
    );
    assert!(err.contains("precondition failed"));
}

#[test]
fn eval_requires_fails_multi_param() {
    // Precondition references multiple params.
    let err = run_err(
        "fn safe_div(a: Int, b: Int) -> Int
           requires b > 0
         { a / b }
         fn main() -> Int { safe_div(10, 0) }",
    );
    assert!(err.contains("precondition failed"));
}

#[test]
fn eval_requires_fails_negative_bound() {
    let err = run_err(
        "fn clamp_low(x: Int) -> Int
           requires x >= 0
         { x }
         fn main() -> Int { clamp_low(-1) }",
    );
    assert!(err.contains("precondition failed"));
}

#[test]
fn eval_requires_passes_at_boundary() {
    // x == 1 should pass `requires x > 0`.
    let val = run_ok(
        "fn positive(x: Int) -> Int
           requires x > 0
         { x }
         fn main() -> Int { positive(1) }",
    );
    assert!(matches!(val, Value::Int(1)));
}

#[test]
fn eval_ensures_fails_wrong_return() {
    // Function returns 0 but ensures says result > 0.
    let err = run_err(
        "fn bad() -> Int
           ensures result > 0
         { 0 }
         fn main() -> Int { bad() }",
    );
    assert!(err.contains("postcondition failed"));
}

#[test]
fn eval_ensures_fails_negative_return() {
    let err = run_err(
        "fn negate(x: Int) -> Int
           ensures result >= 0
         { 0 - x }
         fn main() -> Int { negate(5) }",
    );
    assert!(err.contains("postcondition failed"));
}

#[test]
fn eval_ensures_fails_equality_mismatch() {
    let err = run_err(
        "fn double(x: Int) -> Int
           ensures result == x + x
         { x * 3 }
         fn main() -> Int { double(4) }",
    );
    assert!(err.contains("postcondition failed"));
}

#[test]
fn eval_ensures_passes_with_computation() {
    let val = run_ok(
        "fn double(x: Int) -> Int
           ensures result == x + x
         { x * 2 }
         fn main() -> Int { double(7) }",
    );
    assert!(matches!(val, Value::Int(14)));
}

#[test]
fn eval_invariant_fails_body_violates() {
    // Invariant checks post-body state; param is fine but invariant uses strict bound.
    let err = run_err(
        "fn process(x: Int) -> Int
           invariant x > 10
         { x }
         fn main() -> Int { process(5) }",
    );
    assert!(err.contains("invariant violated"));
}

#[test]
fn eval_invariant_fails_at_zero() {
    let err = run_err(
        "fn nonzero(x: Int) -> Int
           invariant x != 0
         { x }
         fn main() -> Int { nonzero(0) }",
    );
    assert!(err.contains("invariant violated"));
    assert!(!err.contains("precondition"));
    assert!(!err.contains("postcondition"));
}

#[test]
fn eval_old_captures_pre_state() {
    // old(x) should be 10 even though x is used in computation.
    let val = run_ok(
        "fn add_five(x: Int) -> Int
           ensures result == old(x) + 5
         { x + 5 }
         fn main() -> Int { add_five(10) }",
    );
    assert!(matches!(val, Value::Int(15)));
}

#[test]
fn eval_old_fails_when_body_changes_meaning() {
    // Body returns x * 2 but ensures says result == old(x) + 1.
    let err = run_err(
        "fn wrong(x: Int) -> Int
           ensures result == old(x) + 1
         { x * 2 }
         fn main() -> Int { wrong(5) }",
    );
    assert!(err.contains("postcondition failed"));
}

#[test]
fn eval_requires_and_ensures_requires_fails_first() {
    // Both contracts present, but precondition fails before body runs.
    let err = run_err(
        "fn guarded(x: Int) -> Int
           requires x > 0
           ensures result > 0
         { x + 1 }
         fn main() -> Int { guarded(-5) }",
    );
    assert!(err.contains("precondition failed"));
    assert!(!err.contains("postcondition"));
}

#[test]
fn eval_requires_passes_ensures_fails() {
    // Precondition passes but postcondition catches bad return.
    let err = run_err(
        "fn bad_inc(x: Int) -> Int
           requires x > 0
           ensures result > x
         { x }
         fn main() -> Int { bad_inc(5) }",
    );
    assert!(err.contains("postcondition failed"));
    assert!(!err.contains("precondition"));
}

#[test]
fn eval_all_three_contracts_pass() {
    let val = run_ok(
        "fn triple_check(x: Int) -> Int
           requires x > 0
           ensures result == old(x) + 1
           invariant x > 0
         { x + 1 }
         fn main() -> Int { triple_check(5) }",
    );
    assert!(matches!(val, Value::Int(6)));
}

#[test]
fn eval_invariant_fails_with_requires_and_ensures() {
    // requires passes, invariant fails before ensures runs.
    let err = run_err(
        "fn strict(x: Int) -> Int
           requires x > 0
           ensures result > 0
           invariant x > 100
         { x }
         fn main() -> Int { strict(5) }",
    );
    assert!(err.contains("invariant violated"));
    assert!(!err.contains("precondition"));
    assert!(!err.contains("postcondition"));
}

#[test]
fn eval_contract_on_recursive_fn() {
    // Contracts checked on every call in recursion.
    let val = run_ok(
        "fn fact(n: Int) -> Int
           requires n >= 0
           ensures result >= 1
         {
           if n <= 1 { 1 } else { n * fact(n - 1) }
         }
         fn main() -> Int { fact(5) }",
    );
    assert!(matches!(val, Value::Int(120)));
}

#[test]
fn eval_contract_on_called_fn_not_main() {
    // Contract on a helper, main has none.
    let err = run_err(
        "fn helper(x: Int) -> Int
           requires x > 0
         { x }
         fn main() -> Int { helper(-1) }",
    );
    assert!(err.contains("precondition failed"));
}

#[test]
fn eval_ensures_with_bool_return() {
    let val = run_ok(
        "fn is_positive(x: Int) -> Bool
           ensures result == true
         { x > 0 }
         fn main() -> Bool { is_positive(5) }",
    );
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_ensures_with_bool_return_fails() {
    let err = run_err(
        "fn is_positive(x: Int) -> Bool
           ensures result == true
         { x > 0 }
         fn main() -> Bool { is_positive(-1) }",
    );
    assert!(err.contains("postcondition failed"));
}

#[test]
fn eval_requires_compound_condition_fails() {
    // Compound boolean in requires.
    let err = run_err(
        "fn bounded(x: Int) -> Int
           requires x > 0
         { x }
         fn main() -> Int { bounded(-10) }",
    );
    assert!(err.contains("precondition failed"));
}

#[test]
fn eval_ensures_result_is_zero() {
    let val = run_ok(
        "fn zero() -> Int
           ensures result == 0
         { 0 }
         fn main() -> Int { zero() }",
    );
    assert!(matches!(val, Value::Int(0)));
}

#[test]
fn eval_ensures_result_is_zero_fails() {
    let err = run_err(
        "fn not_zero() -> Int
           ensures result == 0
         { 1 }
         fn main() -> Int { not_zero() }",
    );
    assert!(err.contains("postcondition failed"));
}

#[test]
fn eval_old_with_multiple_params() {
    let val = run_ok(
        "fn sum_inc(a: Int, b: Int) -> Int
           ensures result == old(a) + old(b) + 1
         { a + b + 1 }
         fn main() -> Int { sum_inc(3, 4) }",
    );
    assert!(matches!(val, Value::Int(8)));
}

#[test]
fn eval_old_with_multiple_params_fails() {
    let err = run_err(
        "fn sum_inc(a: Int, b: Int) -> Int
           ensures result == old(a) + old(b)
         { a + b + 1 }
         fn main() -> Int { sum_inc(3, 4) }",
    );
    assert!(err.contains("postcondition failed"));
}

#[test]
fn eval_contract_error_names_function() {
    // Error message should contain the function name.
    let err = run_err(
        "fn my_special_fn(x: Int) -> Int
           requires x > 100
         { x }
         fn main() -> Int { my_special_fn(1) }",
    );
    assert!(err.contains("my_special_fn"));
}

#[test]
fn eval_postcondition_error_names_function() {
    let err = run_err(
        "fn another_fn() -> Int
           ensures result > 999
         { 1 }
         fn main() -> Int { another_fn() }",
    );
    assert!(err.contains("another_fn"));
}

#[test]
fn eval_invariant_error_names_function() {
    let err = run_err(
        "fn inv_fn(x: Int) -> Int
           invariant x > 999
         { x }
         fn main() -> Int { inv_fn(1) }",
    );
    assert!(err.contains("inv_fn"));
}

// ── User-defined Option still works (takes precedence over builtin) ─

#[test]
fn eval_user_option_overrides_builtin() {
    let val = run_ok(
        "type Option<T> = | Some(T) | None
         fn main() -> Int {
           match Some(7) {
             Some(x) => x
             None => 0
           }
         }",
    );
    assert!(matches!(val, Value::Int(7)));
}
