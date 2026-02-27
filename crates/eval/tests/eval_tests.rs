//! Integration tests for the tree-walking interpreter.

use kyokara_eval::manifest::CapabilityManifest;
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

fn check_has_compile_errors(source: &str) -> bool {
    let result = kyokara_hir::check_file(source);
    !result.parse_errors.is_empty()
        || result
            .lowering_diagnostics
            .iter()
            .any(|d| d.severity == kyokara_diagnostics::Severity::Error)
        || result
            .type_check
            .body_lowering_diagnostics
            .iter()
            .any(|d| d.severity == kyokara_diagnostics::Severity::Error)
        || !result.type_check.raw_diagnostics.is_empty()
}

fn run_with_manifest_ok(source: &str, manifest: Option<CapabilityManifest>) -> Value {
    match kyokara_eval::run_with_manifest(source, manifest) {
        Ok(result) => result.value,
        Err(e) => panic!("runtime error: {e}"),
    }
}

fn run_with_manifest_err(source: &str, manifest: Option<CapabilityManifest>) -> String {
    match kyokara_eval::run_with_manifest(source, manifest) {
        Ok(result) => panic!("expected error, got {:?}", result.value),
        Err(e) => e.to_string(),
    }
}

fn manifest_from_json(json: &str) -> CapabilityManifest {
    CapabilityManifest::from_json(json).unwrap()
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

// ── List tests ──────────────────────────────────────────────────────

#[test]
fn eval_list_new_and_push() {
    let val = run_ok(
        "fn main() -> Int {
           let xs = list_new()
           let xs = list_push(xs, 1)
           let xs = list_push(xs, 2)
           list_len(xs)
         }",
    );
    assert!(matches!(val, Value::Int(2)));
}

#[test]
fn eval_list_len_empty() {
    let val = run_ok(
        "fn main() -> Int {
           list_len(list_new())
         }",
    );
    assert!(matches!(val, Value::Int(0)));
}

#[test]
fn eval_list_get_some() {
    let val = run_ok(
        "fn main() -> Int {
           let xs = list_push(list_push(list_new(), 10), 20)
           match list_get(xs, 1) {
             Some(x) => x
             None => 0
           }
         }",
    );
    assert!(matches!(val, Value::Int(20)));
}

#[test]
fn eval_list_get_none() {
    let val = run_ok(
        "fn main() -> Int {
           let xs = list_push(list_new(), 10)
           match list_get(xs, 5) {
             Some(x) => x
             None => -1
           }
         }",
    );
    assert!(matches!(val, Value::Int(-1)));
}

#[test]
fn eval_list_head_some() {
    let val = run_ok(
        "fn main() -> Int {
           let xs = list_push(list_push(list_new(), 10), 20)
           match list_head(xs) {
             Some(x) => x
             None => 0
           }
         }",
    );
    assert!(matches!(val, Value::Int(10)));
}

#[test]
fn eval_list_head_none() {
    let val = run_ok(
        "fn main() -> Int {
           match list_head(list_new()) {
             Some(x) => x
             None => -1
           }
         }",
    );
    assert!(matches!(val, Value::Int(-1)));
}

#[test]
fn eval_list_tail() {
    let val = run_ok(
        "fn main() -> Int {
           let xs = list_push(list_push(list_push(list_new(), 1), 2), 3)
           list_len(list_tail(xs))
         }",
    );
    assert!(matches!(val, Value::Int(2)));
}

#[test]
fn eval_list_tail_empty() {
    let val = run_ok(
        "fn main() -> Int {
           list_len(list_tail(list_new()))
         }",
    );
    assert!(matches!(val, Value::Int(0)));
}

#[test]
fn eval_list_is_empty() {
    let val = run_ok(
        "fn main() -> Bool {
           list_is_empty(list_new())
         }",
    );
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_list_is_empty_false() {
    let val = run_ok(
        "fn main() -> Bool {
           list_is_empty(list_push(list_new(), 1))
         }",
    );
    assert!(matches!(val, Value::Bool(false)));
}

#[test]
fn eval_list_reverse() {
    let val = run_ok(
        "fn main() -> Int {
           let xs = list_push(list_push(list_push(list_new(), 1), 2), 3)
           let rev = list_reverse(xs)
           match list_head(rev) {
             Some(x) => x
             None => 0
           }
         }",
    );
    assert!(matches!(val, Value::Int(3)));
}

#[test]
fn eval_list_concat() {
    let val = run_ok(
        "fn main() -> Int {
           let a = list_push(list_push(list_new(), 1), 2)
           let b = list_push(list_push(list_new(), 3), 4)
           list_len(list_concat(a, b))
         }",
    );
    assert!(matches!(val, Value::Int(4)));
}

#[test]
fn eval_list_map_lambda() {
    let val = run_ok(
        "fn main() -> Int {
           let xs = list_push(list_push(list_push(list_new(), 1), 2), 3)
           let ys = list_map(xs, fn(x: Int) => x * 2)
           match list_get(ys, 2) {
             Some(x) => x
             None => 0
           }
         }",
    );
    assert!(matches!(val, Value::Int(6)));
}

#[test]
fn eval_list_map_named_fn() {
    let val = run_ok(
        "fn double(x: Int) -> Int { x * 2 }
         fn main() -> Int {
           let xs = list_push(list_push(list_new(), 5), 10)
           let ys = list_map(xs, double)
           match list_head(ys) {
             Some(x) => x
             None => 0
           }
         }",
    );
    assert!(matches!(val, Value::Int(10)));
}

#[test]
fn eval_list_filter() {
    let val = run_ok(
        "fn main() -> Int {
           let xs = list_push(list_push(list_push(list_push(list_new(), 1), 2), 3), 4)
           let evens = list_filter(xs, fn(x: Int) => x > 2)
           list_len(evens)
         }",
    );
    assert!(matches!(val, Value::Int(2)));
}

#[test]
fn eval_list_fold_sum() {
    let val = run_ok(
        "fn main() -> Int {
           let xs = list_push(list_push(list_push(list_new(), 1), 2), 3)
           list_fold(xs, 0, fn(acc: Int, x: Int) => acc + x)
         }",
    );
    assert!(matches!(val, Value::Int(6)));
}

// ── Map tests ───────────────────────────────────────────────────────

#[test]
fn eval_map_insert_and_get() {
    let val = run_ok(
        r#"fn main() -> Int {
           let m = map_insert(map_new(), "a", 1)
           match map_get(m, "a") {
             Some(x) => x
             None => 0
           }
         }"#,
    );
    assert!(matches!(val, Value::Int(1)));
}

#[test]
fn eval_map_get_missing() {
    let val = run_ok(
        r#"fn main() -> Int {
           let m = map_insert(map_new(), "a", 1)
           match map_get(m, "b") {
             Some(x) => x
             None => -1
           }
         }"#,
    );
    assert!(matches!(val, Value::Int(-1)));
}

#[test]
fn eval_map_contains() {
    let val = run_ok(
        r#"fn main() -> Bool {
           let m = map_insert(map_new(), "key", 42)
           map_contains(m, "key")
         }"#,
    );
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_map_remove() {
    let val = run_ok(
        r#"fn main() -> Int {
           let m = map_insert(map_insert(map_new(), "a", 1), "b", 2)
           let m2 = map_remove(m, "a")
           map_len(m2)
         }"#,
    );
    assert!(matches!(val, Value::Int(1)));
}

#[test]
fn eval_map_len() {
    let val = run_ok(
        r#"fn main() -> Int {
           let m = map_insert(map_insert(map_new(), "a", 1), "b", 2)
           map_len(m)
         }"#,
    );
    assert!(matches!(val, Value::Int(2)));
}

#[test]
fn eval_map_keys() {
    let val = run_ok(
        r#"fn main() -> Int {
           let m = map_insert(map_insert(map_new(), "a", 1), "b", 2)
           list_len(map_keys(m))
         }"#,
    );
    assert!(matches!(val, Value::Int(2)));
}

#[test]
fn eval_map_values() {
    let val = run_ok(
        r#"fn main() -> Int {
           let m = map_insert(map_insert(map_new(), "a", 10), "b", 20)
           let vals = map_values(m)
           list_fold(vals, 0, fn(acc: Int, x: Int) => acc + x)
         }"#,
    );
    assert!(matches!(val, Value::Int(30)));
}

#[test]
fn eval_map_is_empty() {
    let val = run_ok(
        "fn main() -> Bool {
           map_is_empty(map_new())
         }",
    );
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_map_insert_overwrite() {
    let val = run_ok(
        r#"fn main() -> Int {
           let m = map_insert(map_new(), "a", 1)
           let m = map_insert(m, "a", 99)
           match map_get(m, "a") {
             Some(x) => x
             None => 0
           }
         }"#,
    );
    assert!(matches!(val, Value::Int(99)));
}

// ── String ops tests ────────────────────────────────────────────────

#[test]
fn eval_string_len() {
    let val = run_ok(r#"fn main() -> Int { string_len("hello") }"#);
    assert!(matches!(val, Value::Int(5)));
}

#[test]
fn eval_string_contains() {
    let val = run_ok(r#"fn main() -> Bool { string_contains("hello world", "world") }"#);
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_string_starts_with() {
    let val = run_ok(r#"fn main() -> Bool { string_starts_with("hello", "hel") }"#);
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_string_ends_with() {
    let val = run_ok(r#"fn main() -> Bool { string_ends_with("hello", "llo") }"#);
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_string_trim() {
    let val = run_ok(r#"fn main() -> Int { string_len(string_trim("  hi  ")) }"#);
    assert!(matches!(val, Value::Int(2)));
}

#[test]
fn eval_string_split() {
    let val = run_ok(
        r#"fn main() -> Int {
           let parts = string_split("a,b,c", ",")
           list_len(parts)
         }"#,
    );
    assert!(matches!(val, Value::Int(3)));
}

#[test]
fn eval_string_substring() {
    let val = run_ok(r#"fn main() -> String { string_substring("hello world", 0, 5) }"#);
    match val {
        Value::String(s) => assert_eq!(s, "hello"),
        other => panic!("expected String, got {other:?}"),
    }
}

#[test]
fn eval_string_to_upper() {
    let val = run_ok(r#"fn main() -> String { string_to_upper("hello") }"#);
    match val {
        Value::String(s) => assert_eq!(s, "HELLO"),
        other => panic!("expected String, got {other:?}"),
    }
}

#[test]
fn eval_string_to_lower() {
    let val = run_ok(r#"fn main() -> String { string_to_lower("HELLO") }"#);
    match val {
        Value::String(s) => assert_eq!(s, "hello"),
        other => panic!("expected String, got {other:?}"),
    }
}

#[test]
fn eval_char_to_string() {
    let val = run_ok("fn main() -> String { char_to_string('A') }");
    match val {
        Value::String(s) => assert_eq!(s, "A"),
        other => panic!("expected String, got {other:?}"),
    }
}

// ── Int/Float math tests ────────────────────────────────────────────

#[test]
fn eval_abs() {
    let val = run_ok("fn main() -> Int { abs(-5) }");
    assert!(matches!(val, Value::Int(5)));
}

#[test]
fn eval_min() {
    let val = run_ok("fn main() -> Int { min(3, 7) }");
    assert!(matches!(val, Value::Int(3)));
}

#[test]
fn eval_max() {
    let val = run_ok("fn main() -> Int { max(3, 7) }");
    assert!(matches!(val, Value::Int(7)));
}

#[test]
fn eval_int_to_float() {
    let val = run_ok("fn main() -> Float { int_to_float(42) }");
    match val {
        Value::Float(f) => assert!((f - 42.0).abs() < f64::EPSILON),
        other => panic!("expected Float, got {other:?}"),
    }
}

#[test]
fn eval_float_to_int() {
    let val = run_ok("fn main() -> Int { float_to_int(3.7) }");
    assert!(matches!(val, Value::Int(3)));
}

#[test]
fn eval_float_abs() {
    let val = run_ok("fn main() -> Float { float_abs(-2.5) }");
    match val {
        Value::Float(f) => assert!((f - 2.5).abs() < f64::EPSILON),
        other => panic!("expected Float, got {other:?}"),
    }
}

// ── Integration tests ───────────────────────────────────────────────

#[test]
fn eval_list_map_fold_composition() {
    let val = run_ok(
        "fn main() -> Int {
           let xs = list_push(list_push(list_push(list_new(), 1), 2), 3)
           let doubled = list_map(xs, fn(x: Int) => x * 2)
           list_fold(doubled, 0, fn(acc: Int, x: Int) => acc + x)
         }",
    );
    assert!(matches!(val, Value::Int(12)));
}

#[test]
fn eval_map_list_interop() {
    let val = run_ok(
        r#"fn main() -> Int {
           let m = map_insert(map_insert(map_new(), "x", 10), "y", 20)
           let keys = map_keys(m)
           let vals = map_values(m)
           list_len(keys) + list_fold(vals, 0, fn(acc: Int, x: Int) => acc + x)
         }"#,
    );
    assert!(matches!(val, Value::Int(32)));
}

// ── Capability manifest enforcement ─────────────────────────────────

#[test]
fn no_manifest_print_works() {
    // No manifest = allow all (backward compat).
    let val = run_with_manifest_ok(
        r#"fn main() -> Unit {
            println("hello")
        }"#,
        None,
    );
    assert!(matches!(val, Value::Unit));
}

#[test]
fn manifest_with_io_print_works() {
    let manifest = manifest_from_json(r#"{"caps": {"IO": {}}}"#);
    let val = run_with_manifest_ok(
        r#"fn main() -> Unit {
            println("hello")
        }"#,
        Some(manifest),
    );
    assert!(matches!(val, Value::Unit));
}

#[test]
fn manifest_without_io_print_denied() {
    let manifest = manifest_from_json(r#"{"caps": {"Net": {}}}"#);
    let err = run_with_manifest_err(
        r#"fn main() -> Unit {
            print("hello")
        }"#,
        Some(manifest),
    );
    assert!(err.contains("capability denied"));
    assert!(err.contains("IO"));
}

#[test]
fn manifest_without_io_println_denied() {
    let manifest = manifest_from_json(r#"{"caps": {"Net": {}}}"#);
    let err = run_with_manifest_err(
        r#"fn main() -> Unit {
            println("hello")
        }"#,
        Some(manifest),
    );
    assert!(err.contains("capability denied"));
    assert!(err.contains("IO"));
}

#[test]
fn manifest_with_io_pure_intrinsics_work() {
    let manifest = manifest_from_json(r#"{"caps": {"IO": {}}}"#);
    let val = run_with_manifest_ok(
        r#"fn main() -> String {
            int_to_string(42)
        }"#,
        Some(manifest),
    );
    assert!(matches!(val, Value::String(s) if s == "42"));
}

#[test]
fn empty_manifest_denies_io() {
    let manifest = manifest_from_json(r#"{"caps": {}}"#);
    let err = run_with_manifest_err(
        r#"fn main() -> Unit {
            println("hello")
        }"#,
        Some(manifest),
    );
    assert!(err.contains("capability denied"));
}

#[test]
fn pure_program_no_manifest_works() {
    let val = run_with_manifest_ok(
        r#"fn main() -> Int {
            1 + 2
        }"#,
        None,
    );
    assert!(matches!(val, Value::Int(3)));
}

#[test]
fn pure_program_empty_manifest_works() {
    let manifest = manifest_from_json(r#"{"caps": {}}"#);
    let val = run_with_manifest_ok(
        r#"fn main() -> Int {
            1 + 2
        }"#,
        Some(manifest),
    );
    assert!(matches!(val, Value::Int(3)));
}

#[test]
fn manifest_grants_user_cap() {
    // main must also declare `with Console` to satisfy the type checker.
    let manifest = manifest_from_json(r#"{"caps": {"Console": {}}}"#);
    let val = run_with_manifest_ok(
        r#"
        cap Console { fn log(msg: String) -> Unit }
        fn greet(name: String) -> String with Console {
            string_concat("hi ", name)
        }
        fn main() -> String with Console {
            greet("world")
        }
        "#,
        Some(manifest),
    );
    assert!(matches!(val, Value::String(s) if s == "hi world"));
}

#[test]
fn manifest_denies_user_cap() {
    // Type-checks fine (main declares Console), but manifest doesn't grant Console.
    let manifest = manifest_from_json(r#"{"caps": {"IO": {}}}"#);
    let err = run_with_manifest_err(
        r#"
        cap Console { fn log(msg: String) -> Unit }
        fn greet(name: String) -> String with Console {
            string_concat("hi ", name)
        }
        fn main() -> String with Console {
            greet("world")
        }
        "#,
        Some(manifest),
    );
    assert!(err.contains("capability denied"));
    assert!(err.contains("Console"));
}

#[test]
fn manifest_grants_multiple_caps() {
    let manifest = manifest_from_json(r#"{"caps": {"Console": {}, "Logger": {}}}"#);
    let val = run_with_manifest_ok(
        r#"
        cap Console { fn log(msg: String) -> Unit }
        cap Logger { fn trace(msg: String) -> Unit }
        fn do_stuff(x: Int) -> Int with Console, Logger {
            x + 1
        }
        fn main() -> Int with Console, Logger {
            do_stuff(41)
        }
        "#,
        Some(manifest),
    );
    assert!(matches!(val, Value::Int(42)));
}

#[test]
fn manifest_missing_one_of_multiple_caps() {
    let manifest = manifest_from_json(r#"{"caps": {"Console": {}}}"#);
    let err = run_with_manifest_err(
        r#"
        cap Console { fn log(msg: String) -> Unit }
        cap Logger { fn trace(msg: String) -> Unit }
        fn do_stuff(x: Int) -> Int with Console, Logger {
            x + 1
        }
        fn main() -> Int with Console, Logger {
            do_stuff(41)
        }
        "#,
        Some(manifest),
    );
    assert!(err.contains("capability denied"));
    assert!(err.contains("Logger"));
}

#[test]
fn pure_function_with_restrictive_manifest() {
    let manifest = manifest_from_json(r#"{"caps": {}}"#);
    let val = run_with_manifest_ok(
        r#"
        fn add(a: Int, b: Int) -> Int { a + b }
        fn main() -> Int { add(3, 4) }
        "#,
        Some(manifest),
    );
    assert!(matches!(val, Value::Int(7)));
}

#[test]
fn manifest_grants_unused_cap() {
    // Manifest grants Net, program only uses IO — that's fine.
    let manifest = manifest_from_json(r#"{"caps": {"Net": {}, "IO": {}}}"#);
    let val = run_with_manifest_ok(
        r#"fn main() -> Unit {
            println("hello")
        }"#,
        Some(manifest),
    );
    assert!(matches!(val, Value::Unit));
}

#[test]
fn capability_denied_error_message_format() {
    let manifest = manifest_from_json(r#"{"caps": {}}"#);
    let err = run_with_manifest_err(r#"fn main() -> Unit { println("x") }"#, Some(manifest));
    // Should contain both the capability name and the function name.
    assert!(err.contains("IO"));
    assert!(err.contains("Println"));
}

#[test]
fn run_with_manifest_none_allows_all() {
    let val = run_with_manifest_ok(r#"fn main() -> Unit { println("ok") }"#, None);
    assert!(matches!(val, Value::Unit));
}

// ── Multi-file project diagnostics ──────────────────────────────────

#[test]
fn run_project_rejects_type_error_in_imported_module() {
    use std::io::Write;

    let dir = tempfile::tempdir().unwrap();

    // util.ky has a type error: returns Bool where Int is declared.
    let util_path = dir.path().join("util.ky");
    let mut util_file = std::fs::File::create(&util_path).unwrap();
    writeln!(util_file, "pub fn util() -> Int {{ true }}").unwrap();

    // main.ky imports util and calls it.
    let main_path = dir.path().join("main.ky");
    let mut main_file = std::fs::File::create(&main_path).unwrap();
    writeln!(main_file, "import util").unwrap();
    writeln!(main_file, "fn main() -> Int {{ util() }}").unwrap();

    let result = kyokara_eval::run_project(&main_path);
    match result {
        Ok(_) => panic!("expected type error from imported module"),
        Err(e) => {
            let err = e.to_string();
            assert!(
                err.contains("type error"),
                "expected 'type error' in message, got: {err}"
            );
        }
    }
}

#[test]
fn run_project_rejects_parse_error_in_sibling_module() {
    use std::io::Write;

    let dir = tempfile::tempdir().unwrap();

    // bad.ky has a syntax error (missing param name).
    let bad_path = dir.path().join("bad.ky");
    let mut bad_file = std::fs::File::create(&bad_path).unwrap();
    writeln!(bad_file, "pub fn bad( -> Int {{ 42 }}").unwrap();

    // main.ky is valid and doesn't import bad.
    let main_path = dir.path().join("main.ky");
    let mut main_file = std::fs::File::create(&main_path).unwrap();
    writeln!(main_file, "fn main() -> Int {{ 42 }}").unwrap();

    let result = kyokara_eval::run_project(&main_path);
    match result {
        Ok(_) => panic!("expected parse error from sibling module"),
        Err(e) => {
            let err = e.to_string();
            assert!(
                err.contains("parse error"),
                "expected 'parse error' in message, got: {err}"
            );
        }
    }
}

#[test]
fn run_project_rejects_lowering_error_in_sibling_module() {
    use std::io::Write;

    let dir = tempfile::tempdir().unwrap();

    // dup.ky has a lowering error: duplicate function definition.
    let dup_path = dir.path().join("dup.ky");
    let mut dup_file = std::fs::File::create(&dup_path).unwrap();
    writeln!(dup_file, "pub fn foo() -> Int {{ 1 }}").unwrap();
    writeln!(dup_file, "pub fn foo() -> Int {{ 2 }}").unwrap();

    // main.ky is valid.
    let main_path = dir.path().join("main.ky");
    let mut main_file = std::fs::File::create(&main_path).unwrap();
    writeln!(main_file, "fn main() -> Int {{ 42 }}").unwrap();

    let result = kyokara_eval::run_project(&main_path);
    match result {
        Ok(_) => panic!("expected lowering error from sibling module"),
        Err(e) => {
            let err = e.to_string();
            assert!(
                err.contains("lowering error") || err.contains("duplicate"),
                "expected lowering/duplicate error in message, got: {err}"
            );
        }
    }
}

#[test]
fn run_rejects_body_lowering_errors_before_execution() {
    // A program with an unresolved name should be rejected at compile time
    // (as a TypeError), not reach the interpreter and fail at runtime
    // (as an UnresolvedName).
    let src = "fn main() -> Int { unknown_name }";
    let result = kyokara_eval::run(src);
    let err = match result {
        Ok(_) => panic!("expected error for unresolved name, but program executed"),
        Err(e) => e,
    };
    let msg = err.to_string();
    // Compile-time errors are "lowering errors: ..." or "type error at compile time: ..."
    // Runtime errors are "unresolved name: ..."
    // The error should be a compile-time rejection, NOT "unresolved name: unknown_name".
    assert!(
        !msg.starts_with("unresolved name:"),
        "should be rejected at compile time, not at runtime; got: {msg}"
    );
    assert!(
        msg.contains("lowering") || msg.contains("type error"),
        "expected compile-time error message, got: {msg}"
    );
}

#[test]
fn run_rejects_user_variable_named_result() {
    // Bug test (#159): user code with `result` as a variable name should be
    // rejected at compile time, not suppressed by the ensures false-positive filter.
    let src = "fn main() -> Int { result }";
    let result = kyokara_eval::run(src);
    let err = match result {
        Ok(_) => panic!("expected error for unresolved `result`, but program executed"),
        Err(e) => e,
    };
    let msg = err.to_string();
    assert!(
        !msg.starts_with("unresolved name:"),
        "should be rejected at compile time, not at runtime; got: {msg}"
    );
}

#[test]
fn run_ensures_with_result_still_works() {
    // Guard test: ensures clauses that use `result` should still run fine.
    let val = run_ok("fn get() -> Int ensures result > 0 { 42 }\nfn main() -> Int { get() }");
    assert!(matches!(val, Value::Int(42)));
}

#[test]
fn run_ensures_result_and_user_result_coexist() {
    // Edge case: one function has ensures (with implicit `result`),
    // another function references an undefined `result` — the second
    // should still be caught at compile time.
    let src = "fn get() -> Int ensures result > 0 { 42 }\nfn main() -> Int { result }";
    let result = kyokara_eval::run(src);
    let err = match result {
        Ok(_) => panic!("expected error for unresolved `result` in main"),
        Err(e) => e,
    };
    let msg = err.to_string();
    // Must be a compile-time error, not a runtime "unresolved name:" error.
    assert!(
        !msg.starts_with("unresolved name:"),
        "should be caught at compile time, not runtime; got: {msg}"
    );
}

#[test]
fn run_project_rejects_body_lowering_error_in_sibling_module() {
    use std::io::Write;

    let dir = tempfile::tempdir().unwrap();

    // bad.ky has an unresolved name in the body.
    let bad_path = dir.path().join("bad.ky");
    let mut bad_file = std::fs::File::create(&bad_path).unwrap();
    writeln!(bad_file, "pub fn oops() -> Int {{ unknown_name }}").unwrap();

    // main.ky is valid and doesn't import bad.
    let main_path = dir.path().join("main.ky");
    let mut main_file = std::fs::File::create(&main_path).unwrap();
    writeln!(main_file, "fn main() -> Int {{ 42 }}").unwrap();

    let result = kyokara_eval::run_project(&main_path);
    match result {
        Ok(_) => panic!("expected body lowering error from sibling module"),
        Err(e) => {
            let err = e.to_string();
            assert!(
                err.contains("unresolved") || err.contains("lowering"),
                "expected unresolved/lowering error in message, got: {err}"
            );
        }
    }
}

// ── Duplicate binding rejection (#161) ─────────────────────────────

#[test]
fn run_rejects_duplicate_binding_in_pattern() {
    // Bug test: duplicate binding in constructor pattern must be rejected.
    let src = r#"
type Pair = | Pair(Int, Int)
fn main() -> Int {
  let Pair(x, x) = Pair(1, 2)
  x
}
"#;
    let err = run_err(src);
    assert!(
        err.contains("duplicate binding"),
        "expected duplicate binding error, got: {err}"
    );
}

#[test]
fn run_accepts_distinct_bindings_in_pattern() {
    // Guard test: distinct bindings in constructor pattern should work fine.
    let src = r#"
type Pair = | Pair(Int, Int)
fn main() -> Int {
  let Pair(a, b) = Pair(1, 2)
  a + b
}
"#;
    let result = run_ok(src);
    assert_eq!(result, Value::Int(3));
}

#[test]
fn run_rejects_duplicate_binding_in_match_arm() {
    // Edge case: duplicate binding in match arm pattern.
    let src = r#"
type Pair = | Pair(Int, Int)
fn main() -> Int {
  match Pair(1, 2) {
    Pair(x, x) => x,
  }
}
"#;
    let err = run_err(src);
    assert!(
        err.contains("duplicate binding"),
        "expected duplicate binding error in match arm, got: {err}"
    );
}

#[test]
fn run_rejects_compile_invalid_programs_detected_by_check() {
    struct Case<'a> {
        name: &'a str,
        src: &'a str,
        run_fragment: &'a str,
    }

    let cases = [
        Case {
            name: "parse error",
            src: "fn main( -> Int { 1 }",
            run_fragment: "parse errors:",
        },
        Case {
            name: "unresolved name",
            src: "fn main() -> Int { unknown_name }",
            run_fragment: "lowering errors:",
        },
        Case {
            name: "duplicate pattern binding",
            src: "type Pair = | Pair(Int, Int)\nfn main() -> Int {\n  let Pair(x, x) = Pair(1, 2)\n  x\n}",
            run_fragment: "duplicate binding",
        },
        Case {
            name: "invalid numeric underscore",
            src: "fn main() -> Int { 1__2 }",
            run_fragment: "invalid underscore placement",
        },
        Case {
            name: "type mismatch",
            src: "fn main() -> Int { \"x\" }",
            run_fragment: "type mismatch",
        },
        Case {
            name: "unresolved return type",
            src: "fn main() -> Foo { 1 }",
            run_fragment: "unresolved type",
        },
    ];

    for case in cases {
        assert!(
            check_has_compile_errors(case.src),
            "check should report compile diagnostics for case `{}`",
            case.name
        );
        let err = run_err(case.src);
        assert!(
            err.contains(case.run_fragment),
            "run should reject case `{}` with fragment `{}`; got: {}",
            case.name,
            case.run_fragment,
            err
        );
    }
}

#[test]
fn run_accepts_compile_valid_let_rebinding_programs() {
    struct Case<'a> {
        name: &'a str,
        src: &'a str,
        expected: Value,
    }

    let cases = [Case {
        name: "sequential let rebinding",
        src: "fn main() -> Int {\n  let x = 1\n  let x = 2\n  x\n}",
        expected: Value::Int(2),
    }];

    for case in cases {
        assert!(
            !check_has_compile_errors(case.src),
            "check should accept compile-valid case `{}`",
            case.name
        );
        let value = run_ok(case.src);
        assert_eq!(
            value, case.expected,
            "run should evaluate case `{}` to {:?}, got {:?}",
            case.name, case.expected, value
        );
    }
}
