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
