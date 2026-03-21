#![allow(clippy::unwrap_used)]

use kyokara_eval::value::Value;
use kyokara_hir::check_file;
use kyokara_kir::lower::lower_module;

fn compile_source(source: &str) -> Vec<u8> {
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

    kyokara_codegen::compile(&module, &result.item_tree, &interner).expect("codegen failed")
}

fn assert_matches_interpreter(source: &str) {
    let interp = kyokara_eval::run(source).expect("interpreter run failed");
    let wasm_bytes = compile_source(source);
    let mut program = kyokara_wasm_runtime::WasmProgram::instantiate(&wasm_bytes)
        .expect("wasm instantiation failed");

    match interp.value {
        Value::Int(expected) => {
            assert_eq!(
                program.call_main_i64().expect("wasm int call failed"),
                expected
            );
        }
        Value::Float(expected) => {
            let actual = program.call_main_f64().expect("wasm float call failed");
            assert!(
                (actual - expected).abs() < f64::EPSILON,
                "expected {expected}, got {actual}"
            );
        }
        Value::Bool(expected) => {
            let actual = program.call_main_i32().expect("wasm bool call failed");
            assert_eq!(actual, i32::from(expected));
        }
        Value::Unit => {
            let actual = program.call_main_i32().expect("wasm unit call failed");
            assert_eq!(actual, 0);
        }
        other => panic!("unexpected MVP parity value: {other:?}"),
    }
}

#[test]
fn instantiate_and_call_main_for_scalar_signatures() {
    let int_bytes = compile_source("fn main() -> Int { 42 }");
    let float_bytes = compile_source("fn main() -> Float { 1.5 + 2.5 }");
    let bool_bytes = compile_source("fn main() -> Bool { 3 < 5 }");
    let unit_bytes = compile_source("fn main() -> Unit {}");

    let mut int_program =
        kyokara_wasm_runtime::WasmProgram::instantiate(&int_bytes).expect("int instantiate");
    assert_eq!(int_program.call_main_i64().expect("int main"), 42);

    let mut float_program =
        kyokara_wasm_runtime::WasmProgram::instantiate(&float_bytes).expect("float instantiate");
    let float_result = float_program.call_main_f64().expect("float main");
    assert!((float_result - 4.0).abs() < f64::EPSILON);

    let mut bool_program =
        kyokara_wasm_runtime::WasmProgram::instantiate(&bool_bytes).expect("bool instantiate");
    assert_eq!(bool_program.call_main_i32().expect("bool main"), 1);

    let mut unit_program =
        kyokara_wasm_runtime::WasmProgram::instantiate(&unit_bytes).expect("unit instantiate");
    assert_eq!(unit_program.call_main_i32().expect("unit main"), 0);
}

#[test]
fn scalar_mvp_programs_match_interpreter_results() {
    let cases = [
        "fn main() -> Int { let x = 10\n let y = 20\n x + y }",
        "fn main() -> Int { if (true) { 1 } else { 2 } }",
        "fn main() -> Float { (1.5 + 2.5) * (4.0 - 1.0) }",
        "fn main() -> Bool { false && 1 / 0 == 0 }",
        "fn main() -> Unit {}",
    ];

    for source in cases {
        assert_matches_interpreter(source);
    }
}

#[test]
fn deep_recursive_programs_do_not_trap_from_default_wasm_stack_limit() {
    let wasm_bytes = compile_source(
        "fn dive(n: Int) -> Int { if (n == 0) { 0 } else { dive(n - 1) } }\nfn main() -> Int { dive(35000) }",
    );
    let mut program =
        kyokara_wasm_runtime::WasmProgram::instantiate(&wasm_bytes).expect("wasm instantiate");
    assert_eq!(
        program.call_main_i64().expect("wasm recursion call failed"),
        0
    );
}
