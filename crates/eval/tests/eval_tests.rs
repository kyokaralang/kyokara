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
