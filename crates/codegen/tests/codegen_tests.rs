//! End-to-end tests: source → parse → check → KIR → WASM → wasmtime → assert.
#![allow(clippy::unwrap_used)]

use kyokara_hir::check_file;
use kyokara_kir::lower::lower_module;

/// Compile source to WASM, run `main()` via wasmtime, return the i64 result.
fn run_main_i64(source: &str) -> i64 {
    let result = check_file(source);
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

    let wasm_bytes =
        kyokara_codegen::compile(&module, &result.item_tree, &interner).expect("codegen failed");

    let engine = wasmtime::Engine::default();
    let wasm_module = wasmtime::Module::new(&engine, &wasm_bytes).expect("invalid WASM module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &wasm_module, &[]).expect("instantiation failed");

    let main_fn = instance
        .get_typed_func::<(), i64>(&mut store, "main")
        .expect("main function not found");
    main_fn.call(&mut store, ()).expect("main trapped")
}

/// Compile source to WASM, run `main()` via wasmtime, return the f64 result.
fn run_main_f64(source: &str) -> f64 {
    let result = check_file(source);
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

    let wasm_bytes =
        kyokara_codegen::compile(&module, &result.item_tree, &interner).expect("codegen failed");

    let engine = wasmtime::Engine::default();
    let wasm_module = wasmtime::Module::new(&engine, &wasm_bytes).expect("invalid WASM module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &wasm_module, &[]).expect("instantiation failed");

    let main_fn = instance
        .get_typed_func::<(), f64>(&mut store, "main")
        .expect("main function not found");
    main_fn.call(&mut store, ()).expect("main trapped")
}

/// Compile source to WASM, run `main()` via wasmtime, return the i32 result.
fn run_main_i32(source: &str) -> i32 {
    let result = check_file(source);
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

    let wasm_bytes =
        kyokara_codegen::compile(&module, &result.item_tree, &interner).expect("codegen failed");

    let engine = wasmtime::Engine::default();
    let wasm_module = wasmtime::Module::new(&engine, &wasm_bytes).expect("invalid WASM module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &wasm_module, &[]).expect("instantiation failed");

    let main_fn = instance
        .get_typed_func::<(), i32>(&mut store, "main")
        .expect("main function not found");
    main_fn.call(&mut store, ()).expect("main trapped")
}

// ── Constants ─────────────────────────────────────────────────────

#[test]
fn test_int_constant() {
    assert_eq!(run_main_i64("fn main() -> Int { 42 }"), 42);
}

#[test]
fn test_negative_int_constant() {
    assert_eq!(run_main_i64("fn main() -> Int { -7 }"), -7);
}

#[test]
fn test_zero() {
    assert_eq!(run_main_i64("fn main() -> Int { 0 }"), 0);
}

#[test]
fn test_bool_true() {
    assert_eq!(run_main_i32("fn main() -> Bool { true }"), 1);
}

#[test]
fn test_bool_false() {
    assert_eq!(run_main_i32("fn main() -> Bool { false }"), 0);
}

// ── Arithmetic ────────────────────────────────────────────────────

#[test]
fn test_int_add() {
    assert_eq!(run_main_i64("fn main() -> Int { 3 + 4 }"), 7);
}

#[test]
fn test_int_sub() {
    assert_eq!(run_main_i64("fn main() -> Int { 10 - 3 }"), 7);
}

#[test]
fn test_int_mul() {
    assert_eq!(run_main_i64("fn main() -> Int { 6 * 7 }"), 42);
}

#[test]
fn test_int_div() {
    assert_eq!(run_main_i64("fn main() -> Int { 42 / 6 }"), 7);
}

#[test]
fn test_int_complex_expr() {
    assert_eq!(run_main_i64("fn main() -> Int { (3 + 4) * (10 - 8) }"), 14);
}

#[test]
fn test_float_add() {
    let result = run_main_f64("fn main() -> Float { 1.5 + 2.5 }");
    assert!((result - 4.0).abs() < f64::EPSILON);
}

#[test]
fn test_float_mul() {
    let result = run_main_f64("fn main() -> Float { 3.0 * 2.0 }");
    assert!((result - 6.0).abs() < f64::EPSILON);
}

// ── Comparisons ───────────────────────────────────────────────────

#[test]
fn test_int_eq_true() {
    assert_eq!(run_main_i32("fn main() -> Bool { 42 == 42 }"), 1);
}

#[test]
fn test_int_eq_false() {
    assert_eq!(run_main_i32("fn main() -> Bool { 42 == 43 }"), 0);
}

#[test]
fn test_int_lt() {
    assert_eq!(run_main_i32("fn main() -> Bool { 3 < 5 }"), 1);
}

#[test]
fn test_int_gt() {
    assert_eq!(run_main_i32("fn main() -> Bool { 5 > 3 }"), 1);
}

// ── Unary operations ──────────────────────────────────────────────

#[test]
fn test_int_neg() {
    assert_eq!(run_main_i64("fn main() -> Int { -(42) }"), -42);
}

#[test]
fn test_bool_not() {
    assert_eq!(run_main_i32("fn main() -> Bool { !true }"), 0);
}

#[test]
fn test_bool_not_false() {
    assert_eq!(run_main_i32("fn main() -> Bool { !false }"), 1);
}

// ── Let bindings ──────────────────────────────────────────────────

#[test]
fn test_let_binding() {
    assert_eq!(
        run_main_i64("fn main() -> Int { let x = 10\n let y = 20\n x + y }"),
        30
    );
}

#[test]
fn test_let_chain() {
    assert_eq!(
        run_main_i64("fn main() -> Int { let a = 5\n let b = a * 2\n let c = b + 1\n c }"),
        11
    );
}

// ── If/else ───────────────────────────────────────────────────────

#[test]
fn test_if_else_true() {
    assert_eq!(
        run_main_i64("fn main() -> Int { if true { 1 } else { 2 } }"),
        1
    );
}

#[test]
fn test_if_else_false() {
    assert_eq!(
        run_main_i64("fn main() -> Int { if false { 1 } else { 2 } }"),
        2
    );
}

#[test]
fn test_if_else_condition() {
    assert_eq!(
        run_main_i64("fn main() -> Int { let x = 10\n if x > 5 { 100 } else { 0 } }"),
        100
    );
}

#[test]
fn test_nested_if_else() {
    assert_eq!(
        run_main_i64(
            "fn main() -> Int {\
               let x = 3\n\
               if x > 5 { 100 } else { if x > 1 { 50 } else { 0 } }\
             }"
        ),
        50
    );
}

// ── Function calls ────────────────────────────────────────────────

#[test]
fn test_function_call() {
    assert_eq!(
        run_main_i64(
            "fn add(a: Int, b: Int) -> Int { a + b }\n\
             fn main() -> Int { add(3, 4) }"
        ),
        7
    );
}

#[test]
fn test_multi_function() {
    assert_eq!(
        run_main_i64(
            "fn double(x: Int) -> Int { x * 2 }\n\
             fn inc(x: Int) -> Int { x + 1 }\n\
             fn main() -> Int { inc(double(5)) }"
        ),
        11
    );
}

#[test]
fn test_recursive_like_chain() {
    assert_eq!(
        run_main_i64(
            "fn f(x: Int) -> Int { x + 1 }\n\
             fn g(x: Int) -> Int { f(x) * 2 }\n\
             fn main() -> Int { g(10) }"
        ),
        22
    );
}

// ── ADTs ──────────────────────────────────────────────────────────

#[test]
fn test_adt_construct_and_match() {
    assert_eq!(
        run_main_i64(
            "type Opt = | Some(Int) | None\n\
             fn main() -> Int {\n\
               match Some(42) {\n\
                 Some(x) => x\n\
                 None => 0\n\
               }\n\
             }"
        ),
        42
    );
}

#[test]
fn test_adt_match_none() {
    assert_eq!(
        run_main_i64(
            "type Opt = | Some(Int) | None\n\
             fn main() -> Int {\n\
               match None {\n\
                 Some(x) => x\n\
                 None => -1\n\
               }\n\
             }"
        ),
        -1
    );
}

#[test]
fn test_adt_three_variants() {
    assert_eq!(
        run_main_i64(
            "type Color = | Red | Green | Blue\n\
             fn to_int(c: Color) -> Int {\n\
               match c {\n\
                 Red => 1\n\
                 Green => 2\n\
                 Blue => 3\n\
               }\n\
             }\n\
             fn main() -> Int { to_int(Green) }"
        ),
        2
    );
}

// ── Records ───────────────────────────────────────────────────────

#[test]
fn test_record_create_and_field() {
    assert_eq!(
        run_main_i64(
            "type Pair = { x: Int, y: Int }\n\
             fn main() -> Int {\n\
               let r = Pair { x: 10, y: 20 }\n\
               r.x + r.y\n\
             }"
        ),
        30
    );
}

#[test]
fn test_record_single_field() {
    assert_eq!(
        run_main_i64(
            "type Wrap = { val: Int }\n\
             fn main() -> Int {\n\
               let r = Wrap { val: 42 }\n\
               r.val\n\
             }"
        ),
        42
    );
}

// ── Contracts (requires) ──────────────────────────────────────────

/// Compile and run, returning `true` if WASM execution traps.
fn run_main_traps(source: &str) -> bool {
    let result = check_file(source);
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

    let wasm_bytes =
        kyokara_codegen::compile(&module, &result.item_tree, &interner).expect("codegen failed");

    let engine = wasmtime::Engine::default();
    let wasm_module = wasmtime::Module::new(&engine, &wasm_bytes).expect("invalid WASM module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &wasm_module, &[]).expect("instantiation failed");

    let main_fn = instance
        .get_typed_func::<(), i64>(&mut store, "main")
        .expect("main function not found");
    main_fn.call(&mut store, ()).is_err()
}

#[test]
fn test_requires_pass() {
    // Requires clause that passes — should return normally.
    assert_eq!(
        run_main_i64(
            "fn check(x: Int) -> Int requires x > 0 { x * 2 }\n\
             fn main() -> Int { check(5) }"
        ),
        10
    );
}

#[test]
fn test_requires_fail_traps() {
    assert!(run_main_traps(
        "fn check(x: Int) -> Int requires x > 0 { x * 2 }\n\
         fn main() -> Int { check(-5) }"
    ));
}

#[test]
fn test_requires_fail_zero_traps() {
    assert!(run_main_traps(
        "fn check(x: Int) -> Int requires x > 0 { x * 2 }\n\
         fn main() -> Int { check(0) }"
    ));
}

#[test]
fn test_requires_pass_boundary() {
    assert_eq!(
        run_main_i64(
            "fn check(x: Int) -> Int requires x > 0 { x * 2 }\n\
             fn main() -> Int { check(1) }"
        ),
        2
    );
}

#[test]
fn test_requires_multiple_callers() {
    assert_eq!(
        run_main_i64(
            "fn check(x: Int) -> Int requires x > 0 { x * 2 }\n\
             fn main() -> Int { check(3) + check(7) }"
        ),
        20
    );
}

#[test]
fn test_requires_fail_complex_condition_traps() {
    assert!(run_main_traps(
        "fn check(x: Int, y: Int) -> Int requires x > y { x - y }\n\
         fn main() -> Int { check(3, 5) }"
    ));
}

#[test]
fn test_ensures_pass() {
    assert_eq!(
        run_main_i64(
            "fn positive() -> Int ensures result > 0 { 42 }\n\
             fn main() -> Int { positive() }"
        ),
        42
    );
}

#[test]
fn test_ensures_fail_traps() {
    assert!(run_main_traps(
        "fn positive() -> Int ensures result > 0 { -1 }\n\
         fn main() -> Int { positive() }"
    ));
}

// ── Float Edge Cases ──────────────────────────────────────────────

#[test]
fn test_float_sub() {
    let r = run_main_f64("fn main() -> Float { 5.5 - 2.0 }");
    assert!((r - 3.5).abs() < f64::EPSILON);
}

#[test]
fn test_float_div() {
    let r = run_main_f64("fn main() -> Float { 7.0 / 2.0 }");
    assert!((r - 3.5).abs() < f64::EPSILON);
}

#[test]
fn test_float_neg() {
    let r = run_main_f64("fn main() -> Float { -(3.5) }");
    assert!((r - (-3.5)).abs() < f64::EPSILON);
}

#[test]
fn test_float_negative_constant() {
    let r = run_main_f64("fn main() -> Float { -1.5 }");
    assert!((r - (-1.5)).abs() < f64::EPSILON);
}

#[test]
fn test_float_eq_true() {
    assert_eq!(run_main_i32("fn main() -> Bool { 3.0 == 3.0 }"), 1);
}

#[test]
fn test_float_eq_false() {
    assert_eq!(run_main_i32("fn main() -> Bool { 3.0 == 4.0 }"), 0);
}

#[test]
fn test_float_lt() {
    assert_eq!(run_main_i32("fn main() -> Bool { 1.5 < 2.5 }"), 1);
}

#[test]
fn test_float_gt() {
    assert_eq!(run_main_i32("fn main() -> Bool { 2.5 > 1.5 }"), 1);
}

#[test]
fn test_float_complex_expr() {
    let r = run_main_f64("fn main() -> Float { (1.5 + 2.5) * (4.0 - 1.0) }");
    assert!((r - 12.0).abs() < f64::EPSILON);
}

// ── Comparison Edge Cases ─────────────────────────────────────────

#[test]
fn test_int_lteq_true() {
    assert_eq!(run_main_i32("fn main() -> Bool { 3 <= 5 }"), 1);
}

#[test]
fn test_int_lteq_boundary() {
    assert_eq!(run_main_i32("fn main() -> Bool { 5 <= 5 }"), 1);
}

#[test]
fn test_int_gteq_true() {
    assert_eq!(run_main_i32("fn main() -> Bool { 5 >= 3 }"), 1);
}

#[test]
fn test_int_gteq_boundary() {
    assert_eq!(run_main_i32("fn main() -> Bool { 5 >= 5 }"), 1);
}

#[test]
fn test_int_neq_true() {
    assert_eq!(run_main_i32("fn main() -> Bool { 3 != 5 }"), 1);
}

#[test]
fn test_int_neq_false() {
    assert_eq!(run_main_i32("fn main() -> Bool { 5 != 5 }"), 0);
}

#[test]
fn test_bool_eq_true_true() {
    assert_eq!(run_main_i32("fn main() -> Bool { true == true }"), 1);
}

#[test]
fn test_bool_eq_true_false() {
    assert_eq!(run_main_i32("fn main() -> Bool { true == false }"), 0);
}

// ── Complex Control Flow ──────────────────────────────────────────

#[test]
fn test_deeply_nested_if_else() {
    assert_eq!(
        run_main_i64(
            "fn main() -> Int {\n\
               let x = 5\n\
               if x > 10 { 100 } else { if x > 3 { if x > 4 { 50 } else { 30 } } else { 0 } }\n\
             }"
        ),
        50
    );
}

#[test]
fn test_if_else_both_return() {
    assert_eq!(
        run_main_i64(
            "fn abs(x: Int) -> Int {\n\
               if x > 0 { x } else { -(x) }\n\
             }\n\
             fn main() -> Int { abs(-7) }"
        ),
        7
    );
}

#[test]
fn test_if_else_in_let() {
    assert_eq!(
        run_main_i64(
            "fn main() -> Int {\n\
               let x = if true { 10 } else { 20 }\n\
               x + 5\n\
             }"
        ),
        15
    );
}

#[test]
fn test_match_with_complex_arm_body() {
    assert_eq!(
        run_main_i64(
            "type Opt = | Some(Int) | None\n\
             fn main() -> Int {\n\
               match Some(10) {\n\
                 Some(x) => x * 2 + 1\n\
                 None => 0\n\
               }\n\
             }"
        ),
        21
    );
}

#[test]
fn test_if_inside_match_arm() {
    assert_eq!(
        run_main_i64(
            "type Opt = | Some(Int) | None\n\
             fn main() -> Int {\n\
               match Some(5) {\n\
                 Some(x) => if x > 3 { x * 10 } else { x }\n\
                 None => 0\n\
               }\n\
             }"
        ),
        50
    );
}

#[test]
fn test_match_four_variants() {
    assert_eq!(
        run_main_i64(
            "type Dir = | North | South | East | West\n\
             fn to_int(d: Dir) -> Int {\n\
               match d {\n\
                 North => 1\n\
                 South => 2\n\
                 East => 3\n\
                 West => 4\n\
               }\n\
             }\n\
             fn main() -> Int { to_int(West) }"
        ),
        4
    );
}

#[test]
fn test_match_then_computation() {
    assert_eq!(
        run_main_i64(
            "type Opt = | Some(Int) | None\n\
             fn main() -> Int {\n\
               let v = match Some(6) {\n\
                 Some(x) => x\n\
                 None => 0\n\
               }\n\
               v * 3 + 1\n\
             }"
        ),
        19
    );
}

// ── Complex Data Structures ───────────────────────────────────────

#[test]
fn test_adt_two_fields() {
    assert_eq!(
        run_main_i64(
            "type Pair = | Pair(Int, Int)\n\
             fn main() -> Int {\n\
               match Pair(10, 20) {\n\
                 Pair(a, b) => a + b\n\
               }\n\
             }"
        ),
        30
    );
}

#[test]
fn test_adt_three_fields() {
    assert_eq!(
        run_main_i64(
            "type Triple = | Triple(Int, Int, Int)\n\
             fn main() -> Int {\n\
               match Triple(1, 2, 3) {\n\
                 Triple(a, b, c) => a + b + c\n\
               }\n\
             }"
        ),
        6
    );
}

#[test]
fn test_record_three_fields() {
    assert_eq!(
        run_main_i64(
            "type Vec3 = { x: Int, y: Int, z: Int }\n\
             fn main() -> Int {\n\
               let v = Vec3 { x: 1, y: 2, z: 3 }\n\
               v.x + v.y + v.z\n\
             }"
        ),
        6
    );
}

#[test]
fn test_record_field_ordering() {
    // Fields created out of alphabetical order — sorted layout means
    // `a` is at offset 8, `b` at offset 16.
    assert_eq!(
        run_main_i64(
            "type Rec = { b: Int, a: Int }\n\
             fn main() -> Int {\n\
               let r = Rec { b: 100, a: 7 }\n\
               r.a\n\
             }"
        ),
        7
    );
}

#[test]
fn test_multiple_adt_allocations() {
    assert_eq!(
        run_main_i64(
            "type Opt = | Some(Int) | None\n\
             fn main() -> Int {\n\
               let a = Some(10)\n\
               let b = Some(20)\n\
               let x = match a {\n\
                 Some(v) => v\n\
                 None => 0\n\
               }\n\
               let y = match b {\n\
                 Some(v) => v\n\
                 None => 0\n\
               }\n\
               x + y\n\
             }"
        ),
        30
    );
}

#[test]
fn test_adt_field_in_computation() {
    assert_eq!(
        run_main_i64(
            "type Opt = | Some(Int) | None\n\
             fn main() -> Int {\n\
               let v = match Some(5) {\n\
                 Some(x) => x\n\
                 None => 0\n\
               }\n\
               v * 3 + 2\n\
             }"
        ),
        17
    );
}

#[test]
fn test_record_field_in_computation() {
    assert_eq!(
        run_main_i64(
            "type Pair = { x: Int, y: Int }\n\
             fn main() -> Int {\n\
               let r = Pair { x: 4, y: 6 }\n\
               r.x * r.y\n\
             }"
        ),
        24
    );
}

#[test]
fn test_multiple_record_allocations() {
    assert_eq!(
        run_main_i64(
            "type Pair = { x: Int, y: Int }\n\
             fn main() -> Int {\n\
               let a = Pair { x: 1, y: 2 }\n\
               let b = Pair { x: 10, y: 20 }\n\
               a.x + b.y\n\
             }"
        ),
        21
    );
}

// ── Function Edge Cases ───────────────────────────────────────────

#[test]
fn test_function_three_params() {
    assert_eq!(
        run_main_i64(
            "fn sum3(a: Int, b: Int, c: Int) -> Int { a + b + c }\n\
             fn main() -> Int { sum3(1, 2, 3) }"
        ),
        6
    );
}

#[test]
fn test_function_zero_params() {
    assert_eq!(
        run_main_i64(
            "fn forty_two() -> Int { 42 }\n\
             fn main() -> Int { forty_two() }"
        ),
        42
    );
}

#[test]
fn test_function_chain_three() {
    assert_eq!(
        run_main_i64(
            "fn f(x: Int) -> Int { x + 1 }\n\
             fn g(x: Int) -> Int { x * 2 }\n\
             fn h(x: Int) -> Int { x - 3 }\n\
             fn main() -> Int { h(g(f(5))) }"
        ),
        9
    );
}

#[test]
fn test_function_returning_bool() {
    assert_eq!(
        run_main_i64(
            "fn is_positive(x: Int) -> Bool { x > 0 }\n\
             fn main() -> Int {\n\
               if is_positive(5) { 100 } else { 0 }\n\
             }"
        ),
        100
    );
}

#[test]
fn test_function_with_float_params() {
    let r = run_main_f64(
        "fn add_f(a: Float, b: Float) -> Float { a + b }\n\
         fn main() -> Float { add_f(1.5, 2.5) }",
    );
    assert!((r - 4.0).abs() < f64::EPSILON);
}

#[test]
fn test_function_multiple_calls_same_fn() {
    assert_eq!(
        run_main_i64(
            "fn double(x: Int) -> Int { x * 2 }\n\
             fn main() -> Int { double(3) + double(7) }"
        ),
        20
    );
}

// ── Let Binding Edge Cases ────────────────────────────────────────

#[test]
fn test_let_five_bindings() {
    assert_eq!(
        run_main_i64(
            "fn main() -> Int {\n\
               let a = 1\n\
               let b = 2\n\
               let c = 3\n\
               let d = 4\n\
               let e = 5\n\
               a + b + c + d + e\n\
             }"
        ),
        15
    );
}

#[test]
fn test_let_complex_chain() {
    assert_eq!(
        run_main_i64(
            "fn main() -> Int {\n\
               let a = 2\n\
               let b = a * 3\n\
               let c = b + a\n\
               let d = c * b\n\
               d\n\
             }"
        ),
        48
    );
}

#[test]
fn test_let_adt_then_match() {
    assert_eq!(
        run_main_i64(
            "type Opt = | Some(Int) | None\n\
             fn main() -> Int {\n\
               let x = Some(7)\n\
               match x {\n\
                 Some(v) => v\n\
                 None => 0\n\
               }\n\
             }"
        ),
        7
    );
}

#[test]
fn test_let_record_then_field() {
    assert_eq!(
        run_main_i64(
            "type Wrap = { val: Int }\n\
             fn main() -> Int {\n\
               let r = Wrap { val: 99 }\n\
               r.val\n\
             }"
        ),
        99
    );
}

#[test]
fn test_let_if_result() {
    assert_eq!(
        run_main_i64(
            "fn main() -> Int {\n\
               let x = 10\n\
               let y = if x > 5 { x * 2 } else { x }\n\
               y + 1\n\
             }"
        ),
        21
    );
}

// ── Integration / Interaction ─────────────────────────────────────

#[test]
fn test_adt_construction_in_if_arms() {
    assert_eq!(
        run_main_i64(
            "type Opt = | Some(Int) | None\n\
             fn main() -> Int {\n\
               let x = 5\n\
               let opt = if x > 3 { Some(x) } else { None }\n\
               match opt {\n\
                 Some(v) => v\n\
                 None => 0\n\
               }\n\
             }"
        ),
        5
    );
}

#[test]
fn test_record_creation_in_match_arm() {
    assert_eq!(
        run_main_i64(
            "type Opt = | Some(Int) | None\n\
             type Pair = { x: Int, y: Int }\n\
             fn main() -> Int {\n\
               let r = match Some(10) {\n\
                 Some(v) => Pair { x: v, y: v * 2 }\n\
                 None => Pair { x: 0, y: 0 }\n\
               }\n\
               r.x + r.y\n\
             }"
        ),
        30
    );
}

#[test]
fn test_function_call_with_adt_arg() {
    assert_eq!(
        run_main_i64(
            "type Opt = | Some(Int) | None\n\
             fn unwrap_or(o: Opt, default: Int) -> Int {\n\
               match o {\n\
                 Some(v) => v\n\
                 None => default\n\
               }\n\
             }\n\
             fn main() -> Int { unwrap_or(Some(42), 0) }"
        ),
        42
    );
}

#[test]
fn test_function_call_with_adt_arg_none() {
    assert_eq!(
        run_main_i64(
            "type Opt = | Some(Int) | None\n\
             fn unwrap_or(o: Opt, default: Int) -> Int {\n\
               match o {\n\
                 Some(v) => v\n\
                 None => default\n\
               }\n\
             }\n\
             fn main() -> Int { unwrap_or(None, 99) }"
        ),
        99
    );
}

#[test]
fn test_function_call_with_record_arg() {
    assert_eq!(
        run_main_i64(
            "type Pair = { x: Int, y: Int }\n\
             fn sum_pair(p: Pair) -> Int { p.x + p.y }\n\
             fn main() -> Int { sum_pair(Pair { x: 3, y: 7 }) }"
        ),
        10
    );
}

#[test]
fn test_function_returning_adt() {
    assert_eq!(
        run_main_i64(
            "type Opt = | Some(Int) | None\n\
             fn make_some(x: Int) -> Opt { Some(x) }\n\
             fn main() -> Int {\n\
               match make_some(42) {\n\
                 Some(v) => v\n\
                 None => 0\n\
               }\n\
             }"
        ),
        42
    );
}

#[test]
fn test_function_returning_record() {
    assert_eq!(
        run_main_i64(
            "type Pair = { x: Int, y: Int }\n\
             fn make_pair(a: Int, b: Int) -> Pair { Pair { x: a, y: b } }\n\
             fn main() -> Int {\n\
               let p = make_pair(5, 10)\n\
               p.x + p.y\n\
             }"
        ),
        15
    );
}

#[test]
fn test_nested_match_in_match() {
    assert_eq!(
        run_main_i64(
            "type Outer = | A(Int) | B\n\
             type Inner = | X(Int) | Y\n\
             fn main() -> Int {\n\
               let o = A(1)\n\
               match o {\n\
                 A(v) => match X(v * 10) {\n\
                   X(w) => w + 1\n\
                   Y => 0\n\
                 }\n\
                 B => -1\n\
               }\n\
             }"
        ),
        11
    );
}

// ── Arithmetic Edge Cases ─────────────────────────────────────────

#[test]
fn test_int_division_negative() {
    // WASM i64.div_s truncates toward zero: -7 / 2 = -3
    assert_eq!(run_main_i64("fn main() -> Int { -7 / 2 }"), -3);
}

#[test]
fn test_int_division_negative_both() {
    assert_eq!(run_main_i64("fn main() -> Int { -10 / -3 }"), 3);
}

#[test]
fn test_comparison_result_in_computation() {
    assert_eq!(
        run_main_i64(
            "fn classify(x: Int) -> Int {\n\
               if x > 0 { if x > 100 { 3 } else { 2 } } else { 1 }\n\
             }\n\
             fn main() -> Int { classify(50) + classify(-1) + classify(200) }"
        ),
        6
    );
}

#[test]
fn test_double_negation() {
    assert_eq!(run_main_i64("fn main() -> Int { -(-(5)) }"), 5);
}
