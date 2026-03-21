//! End-to-end tests: source → parse → check → KIR → WASM → wasmtime → assert.
#![allow(clippy::unwrap_used)]

use kyokara_hir::check_file;
use kyokara_hir_def::{expr::BinaryOp, item_tree::ItemTree, name::Name};
use kyokara_hir_ty::{effects::EffectSet, ty::Ty};
use kyokara_intern::Interner;
use kyokara_kir::{
    KirModule, build::KirBuilder, function::KirContracts, inst::Constant, lower::lower_module,
};

fn instantiate_main(source: &str) -> kyokara_wasm_runtime::WasmProgram {
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
    kyokara_wasm_runtime::WasmProgram::instantiate(&wasm_bytes).expect("instantiation failed")
}

fn instantiate_manual_module(
    module: &KirModule,
    item_tree: &ItemTree,
    interner: &Interner,
) -> kyokara_wasm_runtime::WasmProgram {
    let wasm_bytes = kyokara_codegen::compile(module, item_tree, interner).expect("codegen failed");
    kyokara_wasm_runtime::WasmProgram::instantiate(&wasm_bytes).expect("instantiation failed")
}

/// Compile source to WASM, run `main()` via wasmtime, return the i64 result.
fn run_main_i64(source: &str) -> i64 {
    instantiate_main(source)
        .call_main_i64()
        .expect("main trapped")
}

/// Compile source to WASM, run `main()` via wasmtime, return the f64 result.
fn run_main_f64(source: &str) -> f64 {
    instantiate_main(source)
        .call_main_f64()
        .expect("main trapped")
}

/// Compile source to WASM, run `main()` via wasmtime, return the i32 result.
fn run_main_i32(source: &str) -> i32 {
    instantiate_main(source)
        .call_main_i32()
        .expect("main trapped")
}

fn run_main_bool(source: &str) -> bool {
    run_main_i32(source) != 0
}

fn run_main_string(source: &str) -> String {
    let mut program = instantiate_main(source);
    let ptr = program.call_main_i32().expect("main trapped") as u32;
    let header = program
        .read_memory(ptr, 8)
        .expect("string header should be readable");
    let byte_len = u32::from_le_bytes(header[0..4].try_into().expect("4-byte len header"));
    let bytes = program
        .read_memory(ptr + 8, byte_len)
        .expect("string bytes should be readable");
    String::from_utf8(bytes).expect("guest string should be valid UTF-8")
}

fn with_imports(header: &str, src: &str) -> String {
    format!("{header}\n{src}")
}

fn with_option_variants(src: &str) -> String {
    with_imports("from Option import Some, None", src)
}

fn with_result_variants(src: &str) -> String {
    with_imports("from Result import Ok, Err", src)
}

fn with_core_variants(src: &str) -> String {
    with_imports(
        "from Result import Ok, Err\nfrom ParseError import InvalidInt, InvalidFloat",
        src,
    )
}

fn assert_i64_cases(cases: &[(&str, i64)]) {
    for (source, expected) in cases {
        assert_eq!(
            run_main_i64(source),
            *expected,
            "unexpected i64 result for source:\n{source}"
        );
    }
}

fn assert_i32_cases(cases: &[(&str, i32)]) {
    for (source, expected) in cases {
        assert_eq!(
            run_main_i32(source),
            *expected,
            "unexpected i32 result for source:\n{source}"
        );
    }
}

fn assert_f64_cases(cases: &[(&str, f64)]) {
    for (source, expected) in cases {
        let result = run_main_f64(source);
        assert!(
            (result - expected).abs() < f64::EPSILON,
            "unexpected f64 result for source:\n{source}\nexpected {expected}, got {result}"
        );
    }
}

// ── Constants ─────────────────────────────────────────────────────

#[test]
fn test_constants() {
    assert_i64_cases(&[
        ("fn main() -> Int { 42 }", 42),
        ("fn main() -> Int { -7 }", -7),
        ("fn main() -> Int { 0 }", 0),
    ]);
    assert_i32_cases(&[
        ("fn main() -> Bool { true }", 1),
        ("fn main() -> Bool { false }", 0),
    ]);
}

#[test]
fn test_string_constant_roundtrips_from_guest_memory() {
    assert_eq!(run_main_string(r#"fn main() -> String { "hi" }"#), "hi");
    assert_eq!(run_main_string(r#"fn main() -> String { "café" }"#), "café");
}

#[test]
fn test_string_len_counts_unicode_scalars() {
    assert_eq!(run_main_i64(r#"fn main() -> Int { "café".len() }"#), 4);
}

#[test]
fn test_string_chars_count_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(r#"fn main() -> Int { "hello".chars().count() }"#),
        5
    );
    assert_eq!(
        run_main_i64(r#"fn main() -> Int { "".chars().count() }"#),
        0
    );
    assert_eq!(
        run_main_i64(r#"fn main() -> Int { "café".chars().count() }"#),
        4
    );
    assert_eq!(
        run_main_i64(r#"fn main() -> Int { "a\nb".chars().count() }"#),
        3
    );
}

#[test]
fn test_string_chars_fold_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"fn main() -> Int { "abc".chars().fold(0, fn(acc: Int, ch: Char) => acc + ch.code()) }"#
        ),
        294
    );
    assert_eq!(
        run_main_i64(
            "fn main() -> Int {\n\
               let base = 1\n\
               \"café\".chars().fold(base, fn(acc: Int, ch: Char) => acc + ch.code())\n\
             }"
        ),
        532
    );
}

#[test]
fn test_string_chars_any_matches_interpreter_semantics() {
    assert!(run_main_bool(
        "fn main() -> Bool {\n\
           let target = 'é'\n\
           \"café\".chars().any(fn(ch: Char) => ch == target)\n\
         }"
    ));
    assert!(!run_main_bool(
        r#"fn main() -> Bool { "abc".chars().any(fn(ch: Char) => ch == 'z') }"#
    ));
    assert!(!run_main_bool(
        r#"fn main() -> Bool { "".chars().any(fn(_ch: Char) => true) }"#
    ));
}

#[test]
fn test_string_chars_all_matches_interpreter_semantics() {
    assert!(run_main_bool(
        r#"fn main() -> Bool { "abc".chars().all(fn(ch: Char) => ch != 'z') }"#
    ));
    assert!(!run_main_bool(
        "fn main() -> Bool {\n\
           let limit = 200\n\
           \"café\".chars().all(fn(ch: Char) => ch.code() < limit)\n\
         }"
    ));
    assert!(run_main_bool(
        r#"fn main() -> Bool { "".chars().all(fn(_ch: Char) => false) }"#
    ));
}

#[test]
fn test_string_chars_find_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            "fn main() -> Int {\n\
               let target = 'é'\n\
               \"café\".chars().find(fn(ch: Char) => ch == target).unwrap_or('!').code()\n\
             }"
        ),
        233
    );
    assert_eq!(
        run_main_i64(
            r#"fn main() -> Int { "abc".chars().find(fn(ch: Char) => ch == 'z').unwrap_or('!').code() }"#
        ),
        33
    );
    assert_eq!(
        run_main_i64(
            r#"fn main() -> Int { "".chars().find(fn(_ch: Char) => true).unwrap_or('?').code() }"#
        ),
        63
    );
}

#[test]
fn test_string_chars_contains_matches_interpreter_semantics() {
    assert!(run_main_bool(
        r#"fn main() -> Bool { "café".chars().contains('é') }"#
    ));
    assert!(!run_main_bool(
        r#"fn main() -> Bool { "abc".chars().contains('z') }"#
    ));
    assert!(!run_main_bool(
        r#"fn main() -> Bool { "".chars().contains('a') }"#
    ));
}

#[test]
fn test_string_chars_count_by_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            "fn main() -> Int {\n\
               let target = 'a'\n\
               \"banana\".chars().count(fn(ch: Char) => ch == target)\n\
             }"
        ),
        3
    );
    assert_eq!(
        run_main_i64(r#"fn main() -> Int { "café".chars().count(fn(ch: Char) => ch == 'é') }"#),
        1
    );
    assert_eq!(
        run_main_i64(r#"fn main() -> Int { "".chars().count(fn(_ch: Char) => true) }"#),
        0
    );
}

#[test]
fn test_string_lines_count_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(r#"fn main() -> Int { "a\nb\nc".lines().count() }"#),
        3
    );
    assert_eq!(
        run_main_i64(r#"fn main() -> Int { "a\nb\n".lines().count() }"#),
        2
    );
    assert_eq!(
        run_main_i64(r#"fn main() -> Int { "".lines().count() }"#),
        0
    );
    assert_eq!(
        run_main_i64(r#"fn main() -> Int { "a\r\nb\r\nc".lines().count() }"#),
        3
    );
    assert_eq!(
        run_main_i64(r#"fn main() -> Int { "\n\n".lines().count() }"#),
        2
    );
}

#[test]
fn test_string_lines_count_by_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"fn main() -> Int { "a\nb\nc".lines().count(fn(line: String) => line == "b") }"#
        ),
        1
    );
    assert_eq!(
        run_main_i64(
            r#"fn main() -> Int { "a\r\n\r\nc".lines().count(fn(line: String) => line == "") }"#
        ),
        1
    );
    assert_eq!(
        run_main_i64(
            r#"fn main() -> Int { "\n\n".lines().count(fn(line: String) => line == "") }"#
        ),
        2
    );
    assert_eq!(
        run_main_i64(r#"fn main() -> Int { "".lines().count(fn(_line: String) => true) }"#),
        0
    );
}

#[test]
fn test_string_lines_any_matches_interpreter_semantics() {
    assert_i32_cases(&[
        (
            r#"fn main() -> Bool { "a\nb\nc".lines().any(fn(line: String) => line == "b") }"#,
            1,
        ),
        (
            r#"fn main() -> Bool { "a\r\n\r\nc".lines().any(fn(line: String) => line == "") }"#,
            1,
        ),
        (
            r#"fn main() -> Bool { "\n\n".lines().any(fn(line: String) => line == "z") }"#,
            0,
        ),
        (
            r#"fn main() -> Bool { "".lines().any(fn(_line: String) => true) }"#,
            0,
        ),
    ]);
}

#[test]
fn test_string_lines_all_matches_interpreter_semantics() {
    assert_i32_cases(&[
        (
            r#"fn main() -> Bool { "a\nb\nc".lines().all(fn(line: String) => line.len() == 1) }"#,
            1,
        ),
        (
            r#"fn main() -> Bool { "a\r\n\r\nc".lines().all(fn(line: String) => line != "") }"#,
            0,
        ),
        (
            r#"fn main() -> Bool { "\n\n".lines().all(fn(line: String) => line == "") }"#,
            1,
        ),
        (
            r#"fn main() -> Bool { "".lines().all(fn(_line: String) => false) }"#,
            1,
        ),
    ]);
}

#[test]
fn test_string_lines_find_matches_interpreter_semantics() {
    assert_eq!(
        run_main_string(
            r#"fn main() -> String { "a\nb\nc".lines().find(fn(line: String) => line == "b").unwrap_or("!") }"#
        ),
        "b"
    );
    assert_eq!(
        run_main_string(
            r#"fn main() -> String { "a\r\n\r\nc".lines().find(fn(line: String) => line == "").unwrap_or("!") }"#
        ),
        ""
    );
    assert_eq!(
        run_main_string(
            r#"fn main() -> String { "\n\n".lines().find(fn(line: String) => line == "z").unwrap_or("!") }"#
        ),
        "!"
    );
    assert_eq!(
        run_main_string(
            r#"fn main() -> String { "".lines().find(fn(_line: String) => true).unwrap_or("!") }"#
        ),
        "!"
    );
}

#[test]
fn test_string_lines_fold_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"fn main() -> Int { "a\r\nbb\n".lines().fold(0, fn(acc: Int, line: String) => acc + line.len()) }"#
        ),
        3
    );
    assert_eq!(
        run_main_i64(
            r#"fn main() -> Int { "\n\n".lines().fold(0, fn(acc: Int, _line: String) => acc + 1) }"#
        ),
        2
    );
}

#[test]
fn test_string_split_fold_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"fn main() -> Int { "a,b,,c".split(",").fold(0, fn(acc: Int, part: String) => acc + part.len()) }"#
        ),
        3
    );
    assert_eq!(
        run_main_i64(
            r#"fn main() -> Int { "ab".split("").fold(0, fn(acc: Int, part: String) => acc + part.len()) }"#
        ),
        2
    );
}

#[test]
fn test_list_new_and_len_match_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let xs: collections.List<Int> = collections.List.new()
  xs.len()
}"#
        ),
        0
    );
}

#[test]
fn test_mutable_list_push_get_and_to_list_match_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new().push(10).push(20)
  let ys = xs.to_list()
  ys.get(0).unwrap_or(7) * 100 + ys.get(1).unwrap_or(7) * 10 + ys.get(9).unwrap_or(7)
}"#
        ),
        1207
    );
}

#[test]
fn test_mutable_list_push_is_alias_visible_in_wasm() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new()
  let ys = xs.push(10)
  xs.push(20)
  ys.len()
}"#
        ),
        2
    );
}

#[test]
fn test_mutable_list_get_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new().push(10).push(20)
  xs.get(0).unwrap_or(7) * 100 + xs.get(1).unwrap_or(7) * 10 + xs.get(9).unwrap_or(7)
}"#
        ),
        1207
    );
}

#[test]
fn test_mutable_list_get_single_element_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new().push(10)
  xs.get(0).unwrap_or(7)
}"#
        ),
        10
    );
}

#[test]
fn test_mutable_list_to_list_is_snapshot_in_wasm() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new().push(1)
  let snap = xs.to_list()
  xs.push(2)
  snap.len() * 10 + xs.len()
}"#
        ),
        12
    );
}

#[test]
fn test_mutable_list_from_list_clones_source_in_wasm() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let base = collections.MutableList.new().push(1).to_list()
  let ys = collections.MutableList.from_list(base)
  ys.push(2)
  base.len() * 10 + ys.len()
}"#
        ),
        12
    );
}

#[test]
fn test_list_index_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new().push(10).push(20).to_list()
  xs[1]
}"#
        ),
        20
    );
}

#[test]
fn test_list_index_matches_interpreter_semantics_with_member_imports() {
    assert_eq!(
        run_main_i64(
            r#"from collections import MutableList
fn main() -> Int {
  let xs = MutableList.new().push(10).push(20).to_list()
  xs[1]
}"#
        ),
        20
    );
}

#[test]
fn test_mutable_list_index_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new().push(10).push(20)
  xs[0] * 100 + xs[1]
}"#
        ),
        1020
    );
}

#[test]
fn test_list_index_out_of_bounds_traps_in_wasm() {
    assert!(run_main_traps(
        r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new().push(10).push(20).to_list()
  xs[2]
}"#
    ));
}

#[test]
fn test_mutable_list_negative_index_traps_in_wasm() {
    assert!(run_main_traps(
        r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new().push(10).push(20)
  xs[-1]
}"#
    ));
}

#[test]
fn test_mutable_list_set_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new().push(10).push(20).set(1, 99)
  xs.get(1).unwrap_or(0)
}"#
        ),
        99
    );
}

#[test]
fn test_mutable_list_set_is_alias_visible_in_wasm() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new().push(10).push(20)
  let alias = xs
  xs.set(0, 77)
  alias.get(0).unwrap_or(0)
}"#
        ),
        77
    );
}

#[test]
fn test_mutable_list_set_out_of_bounds_traps_in_wasm() {
    assert!(run_main_traps(
        r#"import collections
fn main() -> Int {
  collections.MutableList.new().push(10).set(9, 0).len()
}"#
    ));
}

#[test]
fn test_mutable_list_set_negative_index_traps_in_wasm() {
    assert!(run_main_traps(
        r#"import collections
fn main() -> Int {
  collections.MutableList.new().push(10).set(0 - 1, 0).len()
}"#
    ));
}

#[test]
fn test_mutable_list_update_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new().push(10).push(20)
  let ys = xs.update(0, fn(n: Int) => n + 5)
  ys.get(0).unwrap_or(0)
}"#
        ),
        15
    );
}

#[test]
fn test_mutable_list_update_is_alias_visible_in_wasm() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new().push(10).push(20)
  let alias = xs
  xs.update(1, fn(n: Int) => n + 7)
  alias.get(1).unwrap_or(0)
}"#
        ),
        27
    );
}

#[test]
fn test_mutable_list_update_out_of_bounds_traps_in_wasm() {
    assert!(run_main_traps(
        r#"import collections
fn main() -> Int {
  collections.MutableList.new().push(10).update(3, fn(n: Int) => n + 1).len()
}"#
    ));
}

#[test]
fn test_mutable_list_insert_delete_and_remove_match_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new()
  let alias = xs
  let _ = xs.insert(0, 10).insert(1, 30).insert(1, 20).insert(xs.len(), 40)
  let last_removed = xs.remove_at(xs.len() - 1)
  let _ = xs.delete_at(1).insert(1, 25).delete_at(0)
  if (last_removed == 40 && alias.len() == 2 && alias.get(0).unwrap_or(0) == 25 && alias.get(1).unwrap_or(0) == 30) {
    1
  } else {
    0
  }
}"#
        ),
        1
    );
}

#[test]
fn test_mutable_list_remove_at_singleton_leaves_empty_in_wasm() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new().insert(0, 42)
  let removed = xs.remove_at(0)
  if (removed == 42 && xs.is_empty() && xs.len() == 0) { 1 } else { 0 }
}"#
        ),
        1
    );
}

#[test]
fn test_mutable_list_insert_out_of_bounds_traps_in_wasm() {
    assert!(run_main_traps(
        r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new().push(3).push(4)
  xs.insert(3, 0).len()
}"#
    ));
}

#[test]
fn test_mutable_list_delete_at_out_of_bounds_traps_in_wasm() {
    assert!(run_main_traps(
        r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new()
  xs.delete_at(0).len()
}"#
    ));
}

#[test]
fn test_mutable_list_remove_at_out_of_bounds_traps_in_wasm() {
    assert!(run_main_traps(
        r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new()
  xs.remove_at(0)
}"#
    ));
}

#[test]
fn test_list_seq_count_and_count_by_match_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let floor = 1
  let xs = collections.MutableList.new().push(1).push(2).push(3)
  let ys = xs.to_list()
  ys.count() * 100 + xs.count(fn(n: Int) => n > floor) * 10 + ys.count(fn(n: Int) => n % 2 == 1)
}"#
        ),
        322
    );
}

#[test]
fn test_list_seq_any_and_all_match_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new().push(1).push(0).push(2)
  let ys = xs.to_list()
  if (xs.any(fn(n: Int) => n == 0) && ys.all(fn(n: Int) => n <= 2)) { 1 } else { 0 }
}"#
        ),
        1
    );
}

#[test]
fn test_list_seq_find_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new().push(2).push(1).push(3).to_list()
  xs.find(fn(n: Int) => n == 1).unwrap_or(0)
}"#
        ),
        1
    );
}

#[test]
fn test_list_seq_fold_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new().push(1).push(2).push(3)
  let ys = xs.to_list()
  ys.fold(10, fn(acc: Int, n: Int) => acc + n) * 10 + xs.fold(0, fn(acc: Int, n: Int) => acc + n)
}"#
        ),
        166
    );
}

#[test]
fn test_deque_push_front_back_and_pop_front_fifo_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(&with_option_variants(
            r#"import collections
fn main() -> Int {
  let q0 = collections.Deque.new().appended(1).appended(2).prepended(0)
  match (q0.popped_front()) {
    Some(p1) => match (p1.rest.popped_front()) {
      Some(p2) => p1.value * 100 + p2.value * 10 + p2.rest.len()
      None => -1
    }
    None => -1
  }
}"#
        )),
        11
    );
}

#[test]
fn test_deque_push_front_back_and_pop_back_lifo_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(&with_option_variants(
            r#"import collections
fn main() -> Int {
  let q0 = collections.Deque.new().appended(1).appended(2).prepended(0)
  match (q0.popped_back()) {
    Some(p1) => match (p1.rest.popped_back()) {
      Some(p2) => p1.value * 100 + p2.value * 10 + p2.rest.len()
      None => -1
    }
    None => -1
  }
}"#
        )),
        211
    );
}

#[test]
fn test_mutable_deque_push_pop_and_to_deque_match_interpreter_semantics() {
    assert_eq!(
        run_main_i64(&with_option_variants(
            r#"import collections
fn main() -> Int {
  let q = collections.MutableDeque.new().push_back(1).push_front(0).push_back(2)
  let front = match (q.pop_front()) {
    Some(x) => x
    None => -1
  }
  let back = match (q.pop_back()) {
    Some(x) => x
    None => -1
  }
  let frozen = q.to_deque()
  if (front == 0 && back == 2 && q.len() == 1 && frozen.len() == 1) { 1 } else { 0 }
}"#
        )),
        1
    );
}

#[test]
fn test_deque_and_mutable_deque_empty_pops_return_none() {
    assert_eq!(
        run_main_i64(&with_option_variants(
            r#"import collections
fn main() -> Int {
  let dq: collections.Deque<Int> = collections.Deque.new()
  let mdq: collections.MutableDeque<Int> = collections.MutableDeque.new()
  let dq_ok = match (dq.popped_front()) {
    Some(_p) => false
    None => true
  }
  let mdq_ok = match (mdq.pop_back()) {
    Some(_x) => false
    None => true
  }
  if (dq_ok && mdq_ok) { 1 } else { 0 }
}"#
        )),
        1
    );
}

#[test]
fn test_deque_seq_traversal_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let q = collections.Deque.new().appended(1).appended(2).appended(3)
  let m = collections.MutableDeque.from_deque(q)
  if (
    q.count() == 3 &&
    q.count(fn(n: Int) => n >= 2) == 2 &&
    q.any(fn(n: Int) => n == 2) &&
    q.all(fn(n: Int) => n <= 3) &&
    q.find(fn(n: Int) => n == 2).unwrap_or(0) == 2 &&
    q.fold(0, fn(acc: Int, n: Int) => acc + n) == 6 &&
    m.fold(0, fn(acc: Int, n: Int) => acc + n) == 6
  ) { 1 } else { 0 }
}"#
        ),
        1
    );
}

#[test]
fn test_linear_collection_contains_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let a = collections.MutableList.new().push(1).push(2).contains(2)
  let b = collections.MutableList.from_list(collections.MutableList.new().push(1).push(2).to_list()).contains(1)
  let c = collections.Deque.new().appended(1).appended(2).contains(3)
  let d = collections.MutableDeque.from_deque(collections.Deque.new().appended(1).appended(2)).contains(2)

  if (a && b && !c && d) { 1 } else { 0 }
}"#
        ),
        1
    );
}

#[test]
fn test_list_reverse_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new().push(3).push(1).push(4).to_list()
  let ys = xs.reversed()
  if (ys[0] == 4 && ys[2] == 3 && xs[0] == 3) { 1 } else { 0 }
}"#
        ),
        1
    );
}

#[test]
fn test_mutable_list_reverse_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new().push(3).push(1).push(4)
  let alias = xs
  let _r = xs.reverse()
  if (alias[0] == 4 && alias[2] == 3 && xs.len() == 3) { 1 } else { 0 }
}"#
        ),
        1
    );
}

#[test]
fn test_list_sort_and_binary_search_match_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new().push(3).push(1).push(4).push(1).push(5).to_list()
  let asc = xs.sorted()
  let desc = xs.sorted_by(fn(a: Int, b: Int) => b - a)
  if (
    asc[0] == 1 &&
    asc[3] == 4 &&
    desc[0] == 5 &&
    desc[4] == 1 &&
    asc.binary_search(4) == 3 &&
    asc.binary_search(2) == -3
  ) { 1 } else { 0 }
}"#
        ),
        1
    );
}

#[test]
fn test_mutable_list_sort_and_binary_search_match_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new().push(3).push(1).push(4).push(1).push(5)
  let alias = xs
  let _sorted = xs.sort()
  let found = alias.binary_search(4)
  let missing = xs.binary_search(2)
  let _desc = xs.sort_by(fn(a: Int, b: Int) => b - a)
  if (
    found == 3 &&
    missing == -3 &&
    alias[0] == 5 &&
    alias[4] == 1
  ) { 1 } else { 0 }
}"#
        ),
        1
    );
}

#[test]
fn test_mutable_list_sort_and_binary_search_support_derived_ord_records_in_wasm() {
    assert_eq!(
        run_main_i64(
            r#"import collections
type Point derive(Eq, Ord) = { x: Int }

fn main() -> Int {
  let a: Point = Point { x: 3 }
  let b: Point = Point { x: 1 }
  let c: Point = Point { x: 2 }
  let xs = collections.MutableList.new().push(a).push(b).push(c).sort()
  let needle: Point = Point { x: 2 }
  if (
    xs[0].x == 1 &&
    xs[1].x == 2 &&
    xs[2].x == 3 &&
    xs.binary_search(needle) == 1
  ) { 1 } else { 0 }
}"#
        ),
        1
    );
}

#[test]
fn test_mutable_list_sort_supports_lexicographic_nested_lists_in_wasm() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let a = collections.MutableList.new().push(2).to_list()
  let b = collections.MutableList.new().push(1).push(9).to_list()
  let c = collections.MutableList.new().push(2).push(1).to_list()
  let xs = collections.MutableList.new().push(a).push(b).push(c).sort()
  let needle = collections.MutableList.new().push(2).push(1).to_list()
  if (
    xs[0][0] == 1 &&
    xs[1][0] == 2 &&
    xs[1].len() == 1 &&
    xs[2][1] == 1 &&
    xs.binary_search(needle) == 2
  ) { 1 } else { 0 }
}"#
        ),
        1
    );
}

#[test]
fn test_mutable_list_sort_supports_deeply_nested_lists_in_wasm() {
    assert_eq!(
        run_main_i64(
            r#"from collections import List, MutableList
fn wrap(n: Int) -> List<List<List<List<Int>>>> {
  MutableList.new()
    .push(MutableList.new()
      .push(MutableList.new()
        .push(MutableList.new().push(n).to_list())
        .to_list())
      .to_list())
    .to_list()
}
fn main() -> Int {
  let xs = MutableList.new().push(wrap(2)).push(wrap(1)).sort()
  let needle = wrap(2)
  if (
    xs[0][0][0][0][0] == 1 &&
    xs[1][0][0][0][0] == 2 &&
    xs.binary_search(needle) == 1
  ) { 1 } else { 0 }
}"#
        ),
        1
    );
}

#[test]
fn test_mutable_list_sort_supports_very_deeply_nested_lists_in_wasm() {
    assert_eq!(
        run_main_i64(
            r#"from collections import List, MutableList
fn wrap(n: Int) -> List<List<List<List<List<List<List<List<List<List<Int>>>>>>>>>> {
  MutableList.new()
    .push(MutableList.new()
      .push(MutableList.new()
        .push(MutableList.new()
          .push(MutableList.new()
            .push(MutableList.new()
              .push(MutableList.new()
                .push(MutableList.new()
                  .push(MutableList.new()
                    .push(MutableList.new().push(n).to_list())
                    .to_list())
                  .to_list())
                .to_list())
              .to_list())
            .to_list())
          .to_list())
        .to_list())
      .to_list())
    .to_list()
}
fn main() -> Int {
  let xs = MutableList.new().push(wrap(2)).push(wrap(1)).sort()
  let needle = wrap(2)
  if (
    xs[0][0][0][0][0][0][0][0][0][0][0] == 1 &&
    xs[1][0][0][0][0][0][0][0][0][0][0] == 2 &&
    xs.binary_search(needle) == 1
  ) { 1 } else { 0 }
}"#
        ),
        1
    );
}

#[test]
fn test_builtin_trait_qualified_ord_compare_matches_interpreter_semantics() {
    assert_eq!(run_main_i64("fn main() -> Int { Ord.compare(3, 1) }"), 1);
}

#[test]
fn test_builtin_trait_qualified_eq_on_derived_record_matches_interpreter_semantics() {
    assert!(run_main_bool(
        r#"type Point derive(Eq) = { x: Int }
fn main() -> Bool {
  let a: Point = Point { x: 7 }
  let b: Point = Point { x: 7 }
  Eq.eq(a, b)
}"#
    ));
}

#[test]
fn test_builtin_trait_qualified_eq_on_derived_record_with_list_field_matches_interpreter_semantics()
{
    assert!(run_main_bool(
        r#"from collections import List, MutableList
type Box derive(Eq) = { xs: List<Int> }

fn mk(n: Int) -> Box {
  Box { xs: MutableList.new().push(n).to_list() }
}

fn main() -> Bool {
  Eq.eq(mk(1), mk(1))
}"#
    ));
}

#[test]
fn test_user_impl_trait_qualified_show_dispatches_in_wasm() {
    assert_eq!(
        run_main_string(
            r#"trait Show { fn show(self) -> String }
type Point = { x: Int }
impl Show for Point { fn show(self) -> String { "p" } }

fn main() -> String {
  let p: Point = Point { x: 1 }
  Show.show(p)
}"#
        ),
        "p"
    );
}

#[test]
fn test_builtin_trait_qualified_show_on_derived_record_with_list_field_matches_interpreter_semantics()
 {
    assert_eq!(
        run_main_string(
            r#"from collections import List, MutableList
type Box derive(Show) = { xs: List<Int> }

fn mk(n: Int) -> Box {
  Box { xs: MutableList.new().push(n).to_list() }
}

fn main() -> String {
  Show.show(mk(1))
}"#
        ),
        "{ xs: [1] }"
    );
}

#[test]
fn test_builtin_trait_qualified_hash_is_stable_for_primitives_in_wasm() {
    assert!(run_main_bool(
        r#"fn main() -> Bool {
  let a = Hash.hash(42)
  let b = Hash.hash(42)
  let c = Hash.hash(7)
  a == b && a != c
}"#
    ));
}

#[test]
fn test_builtin_trait_qualified_hash_supports_structural_records_in_wasm() {
    assert!(run_main_bool(
        r#"type Pair derive(Hash) = { left: Int, right: Int }
fn main() -> Bool {
  let a: Pair = Pair { left: 1, right: 2 }
  let b: Pair = Pair { left: 1, right: 2 }
  let c: Pair = Pair { left: 2, right: 1 }
  let ha = Hash.hash(a)
  let hb = Hash.hash(b)
  let hc = Hash.hash(c)
  ha == hb && ha != hc
}"#
    ));
}

#[test]
fn test_builtin_trait_qualified_show_supports_structural_values_in_wasm() {
    assert_eq!(
        run_main_string(
            r#"import collections
type Pair derive(Show) = { right: Int, left: Int }

fn main() -> String {
  let pair: Pair = Pair { right: 2, left: 1 }
  let list = collections.MutableList.new().push(4).push(5).to_list()
  let deque = collections.MutableDeque.new().push_back(6).push_back(7).to_deque()
  Show.show(pair)
    .concat("|")
    .concat(Show.show(list))
    .concat("|")
    .concat(Show.show(deque))
}"#
        ),
        "{ left: 1, right: 2 }|[4, 5]|Deque([6, 7])"
    );
}

#[test]
fn test_builtin_trait_qualified_show_and_hash_support_deep_nested_lists_in_wasm() {
    assert_eq!(
        run_main_string(
            r#"from collections import List, MutableList
fn wrap(n: Int) -> List<List<List<List<Int>>>> {
  MutableList.new()
    .push(MutableList.new()
      .push(MutableList.new()
        .push(MutableList.new().push(n).to_list())
        .to_list())
      .to_list())
    .to_list()
}
fn main() -> String { Show.show(wrap(1)) }"#
        ),
        "[[[[1]]]]"
    );
    assert!(run_main_bool(
        r#"from collections import List, MutableList
fn wrap(n: Int) -> List<List<List<List<Int>>>> {
  MutableList.new()
    .push(MutableList.new()
      .push(MutableList.new()
        .push(MutableList.new().push(n).to_list())
        .to_list())
      .to_list())
    .to_list()
}
fn main() -> Bool {
  let a = Hash.hash(wrap(1))
  let b = Hash.hash(wrap(1))
  let c = Hash.hash(wrap(2))
  a == b && a != c
}"#
    ));
}

#[test]
fn test_builtin_trait_qualified_hash_supports_very_deep_nested_lists_in_wasm() {
    assert!(run_main_bool(
        r#"from collections import List, MutableList
fn wrap(n: Int) -> List<List<List<List<List<List<List<List<List<List<Int>>>>>>>>>> {
  MutableList.new()
    .push(MutableList.new()
      .push(MutableList.new()
        .push(MutableList.new()
          .push(MutableList.new()
            .push(MutableList.new()
              .push(MutableList.new()
                .push(MutableList.new()
                  .push(MutableList.new()
                    .push(MutableList.new().push(n).to_list())
                    .to_list())
                  .to_list())
                .to_list())
              .to_list())
            .to_list())
          .to_list())
        .to_list())
      .to_list())
    .to_list()
}
fn main() -> Bool {
  let a = Hash.hash(wrap(1))
  let b = Hash.hash(wrap(1))
  let c = Hash.hash(wrap(2))
  a == b && a != c
}"#
    ));
}

#[test]
fn test_builtin_trait_qualified_show_supports_option_variants_in_wasm() {
    assert_eq!(
        run_main_string(&with_option_variants(
            r#"fn main() -> String { Show.show(Some(3)) }"#
        )),
        "Some(3)"
    );
    assert_eq!(
        run_main_string(&with_option_variants(
            r#"fn main() -> String {
  let value: Option<Int> = None
  Show.show(value)
}"#
        )),
        "None"
    );
}

#[test]
fn test_builtin_trait_qualified_hash_supports_option_variants_in_wasm() {
    assert!(run_main_bool(&with_option_variants(
        r#"fn main() -> Bool {
  let a = Hash.hash(Some(3))
  let b = Hash.hash(Some(3))
  let c = Hash.hash(Some(4))
  let d: Option<Int> = None
  a == b && a != c && a != Hash.hash(d)
}"#
    )));
}

#[test]
fn test_builtin_trait_qualified_show_supports_underconstrained_result_variants_in_wasm() {
    assert_eq!(
        run_main_string(
            r#"from Result import Ok, Err
fn main() -> String { Show.show(Ok(3)) }"#
        ),
        "Ok(3)"
    );
    assert_eq!(
        run_main_string(
            r#"from Result import Ok, Err
fn main() -> String { Show.show(Err("boom")) }"#
        ),
        "Err(boom)"
    );
}

#[test]
fn test_builtin_trait_qualified_hash_supports_underconstrained_result_variants_in_wasm() {
    assert!(run_main_bool(
        r#"from Result import Ok, Err
fn main() -> Bool {
  let a = Hash.hash(Ok(3))
  let b = Hash.hash(Ok(3))
  let c = Hash.hash(Ok(4))
  let d = Hash.hash(Err("boom"))
  a == b && a != c && a != d
}"#
    ));
}

#[test]
fn test_list_concat_head_and_tail_match_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new().push(3).push(1).push(4).to_list()
  let ys = collections.MutableList.new().push(1).push(5).to_list()
  let zs = xs.concat(ys)
  if (
    xs.head().unwrap_or(0) == 3 &&
    xs.tail().len() == 2 &&
    xs.tail()[0] == 1 &&
    zs.len() == 5 &&
    zs[3] == 1 &&
    zs[4] == 5
  ) { 1 } else { 0 }
}"#
        ),
        1
    );
}

#[test]
fn test_mutable_list_last_pop_extend_head_and_tail_match_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new().push(3).push(1)
  let added = collections.MutableList.new().push(4).push(1).push(5).to_list()
  let head = xs.head().unwrap_or(0)
  let tail = xs.tail()
  let last = xs.last().unwrap_or(0)
  let popped = xs.pop().unwrap_or(0)
  let _ = xs.extend(added)
  if (
    head == 3 &&
    tail.len() == 1 &&
    tail[0] == 1 &&
    last == 1 &&
    popped == 1 &&
    xs.len() == 4 &&
    xs[0] == 3 &&
    xs[3] == 5
  ) { 1 } else { 0 }
}"#
        ),
        1
    );
}

#[test]
fn test_long_logical_and_chain_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"fn main() -> Int {
  let x = 1
  if (
    x == 1 &&
    x == 1 &&
    x == 1 &&
    x == 1 &&
    x == 1 &&
    x == 1 &&
    x == 1 &&
    x == 1 &&
    x == 1 &&
    x == 1 &&
    x == 1 &&
    x == 1 &&
    x == 1 &&
    x == 1 &&
    x == 1 &&
    x == 1 &&
    x == 1 &&
    x == 1
  ) { 1 } else { 0 }
}"#
        ),
        1
    );
}

#[test]
fn test_mutable_bitset_long_logical_chain_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let grown = collections.MutableBitSet.from_bitset(collections.BitSet.new(8).with_bit(1).with_bit(3))
  let alias = grown
  let _a = grown.set(0)
  let _b = grown.reset(1)
  let _c = alias.flip(2)
  let values = grown.values().to_list()

  let unioned = collections.MutableBitSet.from_bitset(grown.to_bitset())
    .union_with(collections.MutableBitSet.new(8).set(3).set(4))
  let intersected = collections.MutableBitSet.from_bitset(collections.BitSet.new(8).with_bit(0).with_bit(2).with_bit(4))
    .intersection_with(collections.MutableBitSet.new(8).set(0).set(4).set(7))
  let differenced = collections.MutableBitSet.from_bitset(collections.BitSet.new(8).with_bit(0).with_bit(2).with_bit(4))
    .difference_with(collections.MutableBitSet.new(8).set(2))
  let xored = collections.MutableBitSet.from_bitset(collections.BitSet.new(8).with_bit(0).with_bit(2))
    .xor_with(collections.MutableBitSet.new(8).set(2).set(5))
  let snapshot = unioned.to_bitset()

  if (
    grown.size() == 8 &&
    grown.count() == 3 &&
    grown.test(0) &&
    grown.test(2) &&
    values[0] == 0 &&
    values[2] == 3 &&
    unioned.count() == 4 &&
    unioned.test(4) &&
    intersected.count() == 2 &&
    intersected.test(0) &&
    intersected.test(4) &&
    differenced.count() == 2 &&
    differenced.test(0) &&
    differenced.test(4) &&
    xored.count() == 2 &&
    xored.test(0) &&
    xored.test(5) &&
    snapshot.count() == 4
  ) { 1 } else { 0 }
}"#
        ),
        1
    );
}

#[test]
fn test_bitset_core_methods_match_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let a = collections.BitSet.new(10).with_bit(3).with_bit(1).toggled(3).with_bit(7).without_bit(1)
  let b = collections.BitSet.new(10).with_bit(7).with_bit(2)
  let union = a.union(b)
  let inter = union.intersection(collections.BitSet.new(10).with_bit(7).with_bit(9))
  let diff = union.difference(collections.BitSet.new(10).with_bit(2))
  let xo = diff.xor(collections.BitSet.new(10).with_bit(7))
  if (
    a.test(7) &&
    !a.test(3) &&
    a.count() == 1 &&
    a.size() == 10 &&
    !a.is_empty() &&
    inter.count() == 1 &&
    diff.count() == 1 &&
    xo.is_empty()
  ) { 1 } else { 0 }
}"#
        ),
        1
    );
}

#[test]
fn test_mutable_bitset_aliasing_and_snapshot_match_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let a = collections.MutableBitSet.new(8)
  let b = a
  let snap0 = a.to_bitset()
  b.set(1).set(3)
  let frozen = a.to_bitset()
  let copied = collections.MutableBitSet.from_bitset(frozen)
  copied.reset(1)
  if (
    a.test(1) &&
    b.test(3) &&
    !snap0.test(1) &&
    frozen.test(1) &&
    frozen.test(3) &&
    !copied.test(1) &&
    copied.test(3) &&
    a.count() == 2 &&
    copied.count() == 1
  ) { 1 } else { 0 }
}"#
        ),
        1
    );
}

#[test]
fn test_bitset_out_of_bounds_traps() {
    assert!(run_main_traps(
        r#"import collections
fn main() -> Int {
  collections.BitSet.new(4).with_bit(4).count()
}"#
    ));
    assert!(run_main_traps(
        r#"import collections
fn main() -> Int {
  collections.MutableBitSet.new(4).set(0 - 1).count()
}"#
    ));
}

#[test]
fn test_bitset_size_mismatch_traps() {
    assert!(run_main_traps(
        r#"import collections
fn main() -> Int {
  let a = collections.BitSet.new(4).with_bit(1)
  let b = collections.BitSet.new(8).with_bit(1)
  a.union(b).count()
}"#
    ));
    assert!(run_main_traps(
        r#"import collections
fn main() -> Int {
  let a = collections.MutableBitSet.new(4).set(1)
  let b = collections.MutableBitSet.new(8).set(1)
  a.union_with(b).count()
}"#
    ));
}

#[test]
fn test_bitset_values_to_list_matches_interpreter_semantics() {
    assert!(run_main_bool(
        r#"import collections
fn main() -> Bool {
  let vals = collections.BitSet.new(16).with_bit(9).with_bit(1).with_bit(12).with_bit(3).values().to_list()
  vals.len() == 4
    && vals.get(0).unwrap_or(-1) == 1
    && vals.get(1).unwrap_or(-1) == 3
    && vals.get(2).unwrap_or(-1) == 9
    && vals.get(3).unwrap_or(-1) == 12
}"#
    ));
    assert!(run_main_bool(
        r#"import collections
fn main() -> Bool {
  let vals = collections.MutableBitSet.new(10).set(4).set(1).set(7).values().to_list()
  vals.len() == 3
    && vals.get(0).unwrap_or(-1) == 1
    && vals.get(1).unwrap_or(-1) == 4
    && vals.get(2).unwrap_or(-1) == 7
}"#
    ));
}

#[test]
fn test_bitset_values_seq_consumers_match_interpreter_semantics() {
    assert!(run_main_bool(
        r#"import collections
fn main() -> Bool {
  let vals = collections.MutableBitSet.new(12).set(1).set(4).set(9).values()
  vals.count() == 3
    && vals.count(fn(n: Int) => n >= 4) == 2
    && vals.any(fn(n: Int) => n == 4)
    && vals.all(fn(n: Int) => n < 10)
    && vals.contains(9)
    && vals.find(fn(n: Int) => n > 4).unwrap_or(-1) == 9
    && vals.fold(0, fn(acc: Int, n: Int) => acc + n) == 14
}"#
    ));
    assert!(run_main_bool(
        r#"import collections
fn main() -> Bool {
  let vals = collections.BitSet.new(0).values()
  vals.count() == 0
    && !vals.any(fn(_n: Int) => true)
    && vals.all(fn(_n: Int) => true)
    && !vals.contains(0)
    && vals.find(fn(_n: Int) => true).unwrap_or(-1) == -1
    && vals.fold(5, fn(acc: Int, n: Int) => acc + n) == 5
}"#
    ));
}

#[test]
fn test_string_split_count_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(r#"fn main() -> Int { "a,b,c".split(",").count() }"#),
        3
    );
    assert_eq!(
        run_main_i64(r#"fn main() -> Int { "a,,b".split(",").count() }"#),
        3
    );
    assert_eq!(
        run_main_i64(r#"fn main() -> Int { "".split(",").count() }"#),
        1
    );
    assert_eq!(
        run_main_i64(r#"fn main() -> Int { "abc".split("x").count() }"#),
        1
    );
    assert_eq!(
        run_main_i64(r#"fn main() -> Int { "ab".split("").count() }"#),
        4
    );
}

#[test]
fn test_string_split_any_nonempty_delim_matches_interpreter_semantics() {
    assert_i32_cases(&[
        (
            r#"fn main() -> Bool { "a,b,,c".split(",").any(fn(part: String) => part == "") }"#,
            1,
        ),
        (
            r#"fn main() -> Bool { "a,b,c".split(",").any(fn(part: String) => part == "z") }"#,
            0,
        ),
    ]);
}

#[test]
fn test_string_split_any_empty_delim_matches_interpreter_semantics() {
    assert_i32_cases(&[
        (
            r#"fn main() -> Bool { "ab".split("").any(fn(part: String) => part == "") }"#,
            1,
        ),
        (
            r#"fn main() -> Bool { "éé".split("").any(fn(part: String) => part == "é") }"#,
            1,
        ),
        (
            r#"fn main() -> Bool { "ab".split("").any(fn(part: String) => part == "zz") }"#,
            0,
        ),
    ]);
}

#[test]
fn test_string_split_all_nonempty_delim_matches_interpreter_semantics() {
    assert_i32_cases(&[
        (
            r#"fn main() -> Bool { "a,b,c".split(",").all(fn(part: String) => part != "") }"#,
            1,
        ),
        (
            r#"fn main() -> Bool { "a,b,,c".split(",").all(fn(part: String) => part != "") }"#,
            0,
        ),
    ]);
}

#[test]
fn test_string_split_all_empty_delim_matches_interpreter_semantics() {
    assert_i32_cases(&[
        (
            r#"fn main() -> Bool { "ab".split("").all(fn(part: String) => part.len() <= 1) }"#,
            1,
        ),
        (
            r#"fn main() -> Bool { "éé".split("").all(fn(part: String) => part == "é") }"#,
            0,
        ),
    ]);
}

#[test]
fn test_string_split_find_nonempty_delim_matches_interpreter_semantics() {
    assert_eq!(
        run_main_string(
            r#"fn main() -> String { "a,b,,c".split(",").find(fn(part: String) => part == "").unwrap_or("!") }"#
        ),
        ""
    );
    assert_eq!(
        run_main_string(
            r#"fn main() -> String { "a,b,c".split(",").find(fn(part: String) => part == "b").unwrap_or("!") }"#
        ),
        "b"
    );
    assert_eq!(
        run_main_string(
            r#"fn main() -> String { "a,b,c".split(",").find(fn(part: String) => part == "z").unwrap_or("!") }"#
        ),
        "!"
    );
}

#[test]
fn test_string_split_find_empty_delim_matches_interpreter_semantics() {
    assert_eq!(
        run_main_string(
            r#"fn main() -> String { "ab".split("").find(fn(part: String) => part == "").unwrap_or("!") }"#
        ),
        ""
    );
    assert_eq!(
        run_main_string(
            r#"fn main() -> String { "éé".split("").find(fn(part: String) => part == "é").unwrap_or("!") }"#
        ),
        "é"
    );
    assert_eq!(
        run_main_string(
            r#"fn main() -> String { "ab".split("").find(fn(part: String) => part == "zz").unwrap_or("!") }"#
        ),
        "!"
    );
}

#[test]
fn test_string_split_count_by_nonempty_delim_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"fn main() -> Int { "a,b,,c".split(",").count(fn(part: String) => part != "") }"#
        ),
        3
    );
}

#[test]
fn test_string_split_count_by_empty_delim_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"fn main() -> Int { "ab".split("").count(fn(part: String) => part == "") }"#
        ),
        2
    );
    assert_eq!(
        run_main_i64(
            r#"fn main() -> Int { "ab".split("").count(fn(part: String) => part == "a") }"#
        ),
        1
    );
    assert_eq!(
        run_main_i64(
            r#"fn main() -> Int { "éé".split("").count(fn(part: String) => part == "é") }"#
        ),
        2
    );
}

#[test]
fn test_seq_map_filter_fold_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"fn main() -> Int {
  (0..<10)
    .map(fn(n: Int) => n + 1)
    .filter(fn(n: Int) => n % 2 == 0)
    .fold(0, fn(acc: Int, n: Int) => acc + n)
}"#
        ),
        30
    );
}

#[test]
fn test_list_map_filter_and_materialized_seq_terminals_match_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new().push(3).push(1).push(4).push(1).push(5).to_list()
  let mapped = xs.map(fn(n: Int) => n + 1)
  let filtered = mapped.filter(fn(n: Int) => n > 3)
  filtered.count() * 100 + filtered.to_list()[0] * 10 + filtered.fold(0, fn(acc: Int, n: Int) => acc + n)
}"#
        ),
        355
    );
}

#[test]
fn test_mutable_deque_filter_count_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let q = collections.MutableDeque.new().push_back(1).push_back(2).push_back(3)
  q.filter(fn(n: Int) => n > 1).count()
}"#
        ),
        2
    );
}

#[test]
fn test_list_flat_map_to_list_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new().push(3).push(1).push(4).to_list()
  let flat = xs
    .flat_map(fn(n: Int) => collections.MutableList.new().push(n).push(n + 10).to_list())
    .to_list()
  flat.len() * 1000 + flat[1] * 100 + flat[4] * 10 + flat[5]
}"#
        ),
        7354
    );
}

#[test]
fn test_range_flat_map_to_list_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let xs = (1..<4)
    .flat_map(fn(n: Int) => collections.MutableList.new().push(n).push(n * 10).to_list())
    .to_list()
  xs.len() * 100 + xs[1] * 10 + xs[5]
}"#
        ),
        730
    );
}

#[test]
fn test_list_scan_to_list_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new().push(3).push(1).push(4).push(1).push(5).to_list()
  let scanned = xs.scan(0, fn(acc: Int, n: Int) => acc + n).to_list()
  scanned.len() * 100 + scanned[0] * 10 + scanned[5]
}"#
        ),
        614
    );
}

#[test]
fn test_range_scan_to_list_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"fn main() -> Int {
  let scanned = (1..<4).scan(0, fn(acc: Int, n: Int) => acc + n).to_list()
  scanned.len() * 100 + scanned[0] * 10 + scanned[3]
}"#
        ),
        406
    );
}

#[test]
fn test_seq_unfold_to_list_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"from Option import Some, None
fn main() -> Int {
  let xs = (0).unfold(fn(state: Int) =>
    if (state < 3) {
      Some({ value: state + 1, state: state + 1 })
    } else {
      None
    }
  ).to_list()
  xs.len() * 10 + xs[2]
}"#
        ),
        33
    );
}

#[test]
fn test_list_enumerate_zip_chunks_windows_match_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let xs = collections.MutableList.new().push(3).push(1).push(4).push(1).push(5).to_list()
  let mapped = xs.map(fn(n: Int) => n + 1).to_list()
  let enumerated = xs.enumerate().to_list()
  let zipped = xs.zip(mapped).to_list()
  let chunks = xs.chunks(2).to_list()
  let windows = xs.windows(3).to_list()
  if (
    enumerated[2].index == 2 &&
    enumerated[2].value == 4 &&
    zipped[0].left == 3 &&
    zipped[0].right == 4 &&
    chunks.len() == 3 &&
    chunks[1][0] == 4 &&
    windows.len() == 3 &&
    windows[2][1] == 1
  ) { 1 } else { 0 }
}"#
        ),
        1
    );
}

#[test]
fn test_string_backed_seq_map_and_enumerate_match_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"fn main() -> Int {
  let chars = "ab".chars().map(fn(ch: Char) => ch.code()).to_list()
  let lines = "a\nbb".lines().map(fn(line: String) => line.len()).to_list()
  let parts = "x,yy".split(",").enumerate().to_list()

  if (
    chars[0] == 97 &&
    chars[1] == 98 &&
    lines[0] == 1 &&
    lines[1] == 2 &&
    parts[1].index == 1 &&
    parts[1].value == "yy"
  ) { 1 } else { 0 }
}"#
        ),
        1
    );
}

#[test]
fn test_string_backed_seq_flat_map_and_scan_match_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let expanded = "ab".chars()
    .flat_map(fn(ch: Char) => collections.MutableList.new().push(ch.code()).push(ch.code() + 100).to_list())
    .to_list()
  let scanned = "a\nbb".lines().scan(0, fn(acc: Int, line: String) => acc + line.len()).to_list()

  if (
    expanded.len() == 4 &&
    expanded[0] == 97 &&
    expanded[1] == 197 &&
    expanded[3] == 198 &&
    scanned.len() == 3 &&
    scanned[1] == 1 &&
    scanned[2] == 3
  ) { 1 } else { 0 }
}"#
        ),
        1
    );
}

#[test]
fn test_range_zip_list_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let left = (1..<4)
  let right = collections.MutableList.new().push(10).push(20).push(30).push(40).to_list()
  let zs = left.zip(right).to_list()
  zs.len() * 100 + zs[0].left + zs[0].right + zs[2].right
}"#
        ),
        341
    );
}

#[test]
fn test_range_chunks_and_windows_match_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"fn main() -> Int {
  let base = (1..<7)
  let cs = base.chunks(4).to_list()
  let ws = base.windows(3).to_list()
  cs.len() * 100 + cs[1].len() * 10 + ws.len() + ws[3][0]
}"#
        ),
        228
    );
}

#[test]
fn test_list_frequencies_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let counts = collections.MutableList.new().push(3).push(1).push(3).push(2).frequencies()
  counts.get(1).unwrap_or(0) * 100 + counts.get(2).unwrap_or(0) * 10 + counts.get(3).unwrap_or(0)
}"#
        ),
        112
    );
}

#[test]
fn test_map_index_supports_structural_keys_in_wasm() {
    assert_eq!(
        run_main_i64(
            r#"import collections
type Point derive(Eq, Hash) = { x: Int }
fn main() -> Int {
  let p: Point = Point { x: 3 }
  let counts = collections.MutableList.new().push(p).push(p).frequencies()
  counts[p]
}"#
        ),
        2
    );
}

#[test]
fn test_map_and_mutable_map_example_surface_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let snapshot = collections.MutableMap.new()
    .insert("alpha", 1)
    .insert("beta", 2)
    .insert("gamma", 3)
    .to_map()
  let keys = snapshot.keys().to_list()
  let values_sum = snapshot.values().fold(0, fn(acc: Int, n: Int) => acc + n)

  let calls = collections.MutableList.new().push(0)
  let m = collections.MutableMap.with_capacity(8)
  let first = m.get_or_insert_with("alpha", fn() =>
    if (true) {
      let _ = calls.set(0, calls[0] + 1)
      7
    } else {
      0
    }
  )
  let second = m.get_or_insert_with("alpha", fn() =>
    if (true) {
      let _ = calls.set(0, calls[0] + 1)
      99
    } else {
      0
    }
  )
  let _ = m.insert("beta", 2)
  let _ = m.insert("gamma", 3)
  let had_beta = m.contains("beta")
  let keys_before = m.keys().to_list()
  let values_before = m.values().fold(0, fn(acc: Int, n: Int) => acc + n)
  let _ = m.remove("beta")
  let snapshot2 = m.to_map()
  let round_trip = collections.MutableMap.from_map(snapshot2).insert("delta", 4).to_map()

  if (
    snapshot.len() == 3 &&
    snapshot.get("beta").unwrap_or(0) == 2 &&
    snapshot.contains("gamma") &&
    keys[0] == "alpha" &&
    keys[2] == "gamma" &&
    values_sum == 6 &&
    first == 7 &&
    second == 7 &&
    calls[0] == 1 &&
    had_beta &&
    m.get("gamma").unwrap_or(0) == 3 &&
    m.len() == 2 &&
    keys_before[0] == "alpha" &&
    keys_before[1] == "beta" &&
    keys_before[2] == "gamma" &&
    values_before == 12 &&
    snapshot2.get("beta").unwrap_or(0) == 0 &&
    round_trip.get("delta").unwrap_or(0) == 4 &&
    round_trip.len() == 3
  ) { 1 } else { 0 }
}"#
        ),
        1
    );
}

#[test]
fn test_set_and_mutable_set_example_surface_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
fn main() -> Int {
  let snapshot = collections.MutableSet.new()
    .insert(3)
    .insert(1)
    .insert(3)
    .insert(2)
    .to_set()
  let values = snapshot.values().to_list()

  let s = collections.MutableSet.with_capacity(8)
  let _ = s.insert(5)
  let _ = s.insert(7)
  let _ = s.insert(5)
  let had_five = s.contains(5)
  let values_before = s.values().to_list()
  let _ = s.remove(5)
  let snapshot2 = s.to_set()
  let round_trip = collections.MutableSet.from_set(snapshot2).insert(9).to_set()

  if (
    snapshot.len() == 3 &&
    snapshot.contains(1) &&
    !snapshot.contains(99) &&
    values[0] == 3 &&
    values[1] == 1 &&
    values[2] == 2 &&
    had_five &&
    s.len() == 1 &&
    values_before.len() == 2 &&
    values_before[0] == 5 &&
    values_before[1] == 7 &&
    !s.contains(5) &&
    snapshot2.contains(7) &&
    round_trip.contains(9) &&
    round_trip.len() == 2
  ) { 1 } else { 0 }
}"#
        ),
        1
    );
}

#[test]
fn test_mutable_priority_queue_surface_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64(
            r#"import collections
from Option import Some, None

fn main() -> Int {
  let minq: collections.MutablePriorityQueue<Int, Int> = collections.MutablePriorityQueue.new_min()
  let alias = minq.push(5, 50).push(1, 10)
  let _ = alias.push(1, 11).push(3, 30)

  let peek_min = match (minq.peek()) {
    Some(item) => item.priority * 100 + item.value,
    None => -1,
  }
  let pop_min_1 = match (minq.pop()) {
    Some(item) => item.priority * 100 + item.value,
    None => -1,
  }
  let pop_min_2 = match (minq.pop()) {
    Some(item) => item.priority * 100 + item.value,
    None => -1,
  }
  let pop_min_3 = match (minq.pop()) {
    Some(item) => item.priority * 100 + item.value,
    None => -1,
  }
  let pop_min_4 = match (minq.pop()) {
    Some(item) => item.priority * 100 + item.value,
    None => -1,
  }
  let empty_pop = match (minq.pop()) {
    Some(_item) => 0,
    None => 1,
  }

  let maxq: collections.MutablePriorityQueue<Int, Int> = collections.MutablePriorityQueue.new_max()
  let _ = maxq.push(5, 50).push(7, 70).push(7, 71).push(1, 10)
  let peek_max = match (maxq.peek()) {
    Some(item) => item.priority * 100 + item.value,
    None => -1,
  }
  let pop_max_1 = match (maxq.pop()) {
    Some(item) => item.priority * 100 + item.value,
    None => -1,
  }
  let pop_max_2 = match (maxq.pop()) {
    Some(item) => item.priority * 100 + item.value,
    None => -1,
  }
  let pop_max_3 = match (maxq.pop()) {
    Some(item) => item.priority * 100 + item.value,
    None => -1,
  }
  let pop_max_4 = match (maxq.pop()) {
    Some(item) => item.priority * 100 + item.value,
    None => -1,
  }

  if (
    peek_min == 110 &&
    pop_min_1 == 110 &&
    pop_min_2 == 111 &&
    pop_min_3 == 330 &&
    pop_min_4 == 550 &&
    minq.len() == 0 &&
    minq.is_empty() &&
    empty_pop == 1 &&
    peek_max == 770 &&
    pop_max_1 == 770 &&
    pop_max_2 == 771 &&
    pop_max_3 == 550 &&
    pop_max_4 == 110 &&
    maxq.len() == 0 &&
    maxq.is_empty()
  ) { 1 } else { 0 }
}"#
        ),
        1
    );
}

#[test]
fn test_builtin_trait_qualified_show_supports_collection_families_in_wasm() {
    assert_eq!(
        run_main_string(
            r#"import collections

type Point = { x: Int, y: Int }

fn main() -> String {
  let a = Show.show(collections.BitSet.new(8).with_bit(1).with_bit(3))
  let b = Show.show(collections.MutableBitSet.new(8).set(2).set(4))
  let c = Show.show(collections.MutableMap.new().insert("k", 1).to_map())
  let d = Show.show(collections.MutableSet.new().insert(7).insert(9).to_set())
  let e = Show.show(collections.MutableMap.new().insert({ x: 1, y: 2 }, 3))
  let f = Show.show(collections.MutableSet.new().insert({ x: 1, y: 2 }))
  let pq: collections.MutablePriorityQueue<Int, Int> = collections.MutablePriorityQueue.new_min()
  let g = Show.show(pq.push(3, 30).push(1, 10).push(2, 20))
  a.concat(" | ")
    .concat(b)
    .concat(" | ")
    .concat(c)
    .concat(" | ")
    .concat(d)
    .concat(" | ")
    .concat(e)
    .concat(" | ")
    .concat(f)
    .concat(" | ")
    .concat(g)
}"#
        ),
        "BitSet(size=8, #{1, 3}) | MutableBitSet(size=8, #{2, 4}) | {k: 1} | #{7, 9} | MutableMap({{ x: 1, y: 2 }: 3}) | MutableSet(#{{ x: 1, y: 2 }}) | MutablePriorityQueue(direction=min, len=3)"
    );
}

#[test]
fn test_mutable_map_and_set_support_derived_record_keys_in_wasm() {
    assert_eq!(
        run_main_i64(
            r#"import collections

type Point derive(Eq, Hash) = { x: Int, y: Int }

fn pt(x: Int, y: Int) -> Point {
  Point { x: x, y: y }
}

fn main() -> Int {
  let p1 = pt(1, 2)
  let p2 = pt(3, 4)
  let p1_again = pt(1, 2)

  let m: collections.MutableMap<Point, Int> = collections.MutableMap.new()
  let s: collections.MutableSet<Point> = collections.MutableSet.new()

  let _ = m.insert(p1, 7).insert(p2, 9)
  let _ = s.insert(p1_again).insert(p2)

  let snapshot = m.to_map()
  let values = s.values().to_list()

  if (
    m.get(p1_again).unwrap_or(0) == 7 &&
    snapshot.get(p2).unwrap_or(0) == 9 &&
    s.contains(p1) &&
    values.len() == 2 &&
    values[0].x == 1 &&
    values[1].y == 4
  ) { 1 } else { 0 }
}"#
        ),
        1
    );
}

#[test]
fn test_list_contains_supports_derived_record_values_in_wasm() {
    assert_eq!(
        run_main_i64(
            r#"import collections

type Point derive(Eq, Hash) = { x: Int, y: Int }

fn pt(x: Int, y: Int) -> Point {
  Point { x: x, y: y }
}

fn main() -> Int {
  let xs = collections.MutableList.new()
    .push(pt(1, 2))
    .push(pt(3, 4))
    .to_list()

  if (
    xs.contains(pt(1, 2)) &&
    !xs.contains(pt(9, 9))
  ) { 1 } else { 0 }
}"#
        ),
        1
    );
}

#[test]
fn test_structural_set_values_seq_transformers_match_interpreter_semantics_in_wasm() {
    assert_eq!(
        run_main_i64(
            r#"import collections

type Point derive(Eq, Hash) = { x: Int }

fn main() -> Int {
  let s: collections.MutableSet<Point> = collections.MutableSet.new()
  let _ = s.insert(Point { x: 1 }).insert(Point { x: 2 })
  let mapped = s.values().map(fn(p: Point) => p.x).to_list()
  let enumerated = s.values().enumerate().to_list()
  let flat = s.values().flat_map(fn(p: Point) =>
    collections.MutableList.new().push(p.x).push(p.x + 10).to_list()
  ).to_list()
  let scanned = s.values().scan(0, fn(acc: Int, p: Point) => acc + p.x).to_list()
  mapped.fold(0, fn(acc: Int, n: Int) => acc + n)
    + enumerated.fold(0, fn(acc: Int, pair: { index: Int, value: Point }) =>
        acc + pair.index + pair.value.x
      ) * 10
    + flat.fold(0, fn(acc: Int, n: Int) => acc + n) * 100
    + scanned[2] * 10000
}"#
        ),
        32643
    );
}

#[test]
fn test_structural_set_values_seq_consumers_match_interpreter_semantics_in_wasm() {
    assert_eq!(
        run_main_i64(
            r#"import collections

type Point derive(Eq, Hash) = { x: Int }

fn main() -> Int {
  let p1 = Point { x: 1 }
  let p2 = Point { x: 2 }
  let s: collections.MutableSet<Point> = collections.MutableSet.new()
  let _ = s.insert(p1).insert(p2)
  let counts = collections.MutableList.new().push(p1).push(p2).push(p2).frequencies()
  let found = s.values().find(fn(p: Point) => p.x == 2).map_or(0, fn(p: Point) => p.x)
  let any_two = s.values().any(fn(p: Point) => p.x == 2)
  let all_pos = s.values().all(fn(p: Point) => p.x > 0)
  let count_two = s.values().count(fn(p: Point) => p.x == 2)
  let contains_p1 = s.values().contains(p1)
  let sum = s.values().fold(0, fn(acc: Int, p: Point) => acc + p.x)
  if (any_two && all_pos && contains_p1) {
    found + count_two * 10 + sum * 100 + counts[p2] * 1000
  } else {
    0
  }
}"#
        ),
        2312
    );
}

#[test]
fn test_string_lines_contains_matches_interpreter_semantics() {
    assert_i32_cases(&[
        (
            r#"fn main() -> Bool { "a\nb\nc".lines().contains("b") }"#,
            1,
        ),
        (r#"fn main() -> Bool { "a\r\nb".lines().contains("a") }"#, 1),
        (r#"fn main() -> Bool { "".lines().contains("") }"#, 0),
        (r#"fn main() -> Bool { "\n\n".lines().contains("") }"#, 1),
    ]);
}

#[test]
fn test_string_split_contains_nonempty_delim_matches_interpreter_semantics() {
    assert_i32_cases(&[
        (
            r#"fn main() -> Bool { "a,b,c".split(",").contains("b") }"#,
            1,
        ),
        (r#"fn main() -> Bool { "a,,b".split(",").contains("") }"#, 1),
        (r#"fn main() -> Bool { "".split(",").contains("") }"#, 1),
    ]);
}

#[test]
fn test_string_split_contains_empty_delim_matches_interpreter_semantics() {
    assert_i32_cases(&[
        (r#"fn main() -> Bool { "ab".split("").contains("a") }"#, 1),
        (r#"fn main() -> Bool { "ab".split("").contains("z") }"#, 0),
    ]);
}

#[test]
fn test_string_equality_uses_value_semantics() {
    assert_i32_cases(&[
        (
            r#"fn main() -> Bool { "foo".concat("bar") == "foobar" }"#,
            1,
        ),
        (r#"fn main() -> Bool { "abc" != "abc".concat("d") }"#, 1),
        (r#"fn main() -> Bool { "same" == "same" }"#, 1),
        (
            r#"fn main() -> Bool { "é".concat("clair") == "éclair" }"#,
            1,
        ),
    ]);
}

#[test]
fn test_string_predicates_match_interpreter_semantics() {
    assert_i32_cases(&[
        (r#"fn main() -> Bool { "bananas".contains("ana") }"#, 1),
        (r#"fn main() -> Bool { "bananas".contains("xyz") }"#, 0),
        (r#"fn main() -> Bool { "éclair".starts_with("é") }"#, 1),
        (
            r#"fn main() -> Bool { "hello world".starts_with("lo", start: 3) }"#,
            1,
        ),
        (
            r#"fn main() -> Bool { "hello".starts_with("he", start: 99) }"#,
            0,
        ),
        (
            r#"fn main() -> Bool { "hello".starts_with("he", start: -1) }"#,
            0,
        ),
        (r#"fn main() -> Bool { "éclair".ends_with("clair") }"#, 1),
        (r#"fn main() -> Bool { "éclair".ends_with("é") }"#, 0),
    ]);
}

#[test]
fn test_string_substring_matches_interpreter_semantics() {
    assert_eq!(
        run_main_string(r#"fn main() -> String { "hello world".substring(0, 5) }"#),
        "hello"
    );
    assert_eq!(
        run_main_string(r#"fn main() -> String { "héllo".substring(1, 4) }"#),
        "éll"
    );
    assert_eq!(
        run_main_string(r#"fn main() -> String { "abc".substring(5, 9) }"#),
        ""
    );
    assert_eq!(
        run_main_string(r#"fn main() -> String { "abc".substring(2, 1) }"#),
        ""
    );
}

#[test]
fn test_string_case_conversion_matches_interpreter_semantics() {
    assert_eq!(
        run_main_string(r#"fn main() -> String { "hello".to_upper() }"#),
        "HELLO"
    );
    assert_eq!(
        run_main_string(r#"fn main() -> String { "ÉCOLE".to_lower() }"#),
        "école"
    );
    assert_eq!(
        run_main_string(r#"fn main() -> String { "straße".to_upper() }"#),
        "STRASSE"
    );
}

#[test]
fn test_string_md5_matches_interpreter_semantics() {
    assert_eq!(
        run_main_string(r#"fn main() -> String { "abc".md5() }"#),
        "900150983cd24fb0d6963f7d28e17f72"
    );
    assert_eq!(
        run_main_string(
            r#"import hash
fn main() -> String { hash.md5("abc") }"#,
        ),
        "900150983cd24fb0d6963f7d28e17f72"
    );
    assert_eq!(
        run_main_string(
            r#"import hash as h
fn main() -> String { h.md5("abc") }"#,
        ),
        "900150983cd24fb0d6963f7d28e17f72"
    );
}

#[test]
fn test_string_trim_matches_interpreter_semantics() {
    assert_eq!(
        run_main_string(r#"fn main() -> String { "  hello world  ".trim() }"#),
        "hello world"
    );
    assert_eq!(
        run_main_string(r#"fn main() -> String { " héllo　".trim() }"#),
        "héllo"
    );
    assert_eq!(
        run_main_string(r#"fn main() -> String { "    ".trim() }"#),
        ""
    );
}

#[test]
fn test_string_concat_roundtrips_from_guest_memory() {
    assert_eq!(
        run_main_string(r#"fn main() -> String { "foo".concat("bar") }"#),
        "foobar"
    );
    assert_eq!(
        run_main_string(r#"fn main() -> String { "é".concat("clair") }"#),
        "éclair"
    );
}

#[test]
fn test_parse_int_matches_interpreter_semantics() {
    assert_i64_cases(&[(
        &with_core_variants(
            r#"fn main() -> Int {
            match ("42".parse_int()) {
                Ok(n) => n
                Err(_) => -1
            }
        }"#,
        ),
        42,
    )]);
}

#[test]
fn test_parse_int_error_matches_interpreter_semantics() {
    assert_i32_cases(&[(
        &with_core_variants(
            r#"fn main() -> Bool {
            match ("oops".parse_int()) {
                Ok(_) => false
                Err(e) => match (e) {
                    InvalidInt(msg) => msg.len() > 0
                    InvalidFloat(_) => false
                }
            }
        }"#,
        ),
        1,
    )]);
}

#[test]
fn test_parse_float_matches_interpreter_semantics() {
    assert_f64_cases(&[(
        &with_core_variants(
            r#"fn main() -> Float {
            match ("3.14".parse_float()) {
                Ok(f) => f
                Err(_) => 0.0
            }
        }"#,
        ),
        314.0 / 100.0,
    )]);
}

#[test]
fn test_parse_float_special_values_match_interpreter_semantics() {
    assert_i32_cases(&[(
        &with_core_variants(
            r#"fn main() -> Bool {
            let inf_ok = match ("inf".parse_float()) {
                Ok(f) => f.is_infinite()
                Err(_) => false
            }
            let nan_ok = match ("NaN".parse_float()) {
                Ok(f) => f.is_nan()
                Err(_) => false
            }
            inf_ok && nan_ok
        }"#,
        ),
        1,
    )]);
}

#[test]
fn test_parse_float_error_matches_interpreter_semantics() {
    assert_i32_cases(&[(
        &with_core_variants(
            r#"fn main() -> Bool {
            match ("oops".parse_float()) {
                Ok(_) => false
                Err(e) => match (e) {
                    InvalidInt(_) => false
                    InvalidFloat(msg) => msg.len() > 0
                }
            }
        }"#,
        ),
        1,
    )]);
}

#[test]
fn test_option_unwrap_or_matches_interpreter_semantics() {
    assert_i64_cases(&[
        (
            &with_option_variants(r#"fn main() -> Int { Some(41).unwrap_or(0) }"#),
            41,
        ),
        (
            &with_option_variants(
                r#"fn main() -> Int {
            let o: Option<Int> = None
            o.unwrap_or(7)
        }"#,
            ),
            7,
        ),
    ]);
}

#[test]
fn test_option_map_matches_interpreter_semantics() {
    assert_i64_cases(&[
        (
            &with_option_variants(
                r#"fn inc(n: Int) -> Int { n + 1 }
        fn main() -> Int {
            match (Some(41).map(inc)) {
                Some(n) => n
                None => 0
            }
        }"#,
            ),
            42,
        ),
        (
            &with_option_variants(
                r#"fn inc(n: Int) -> Int { n + 1 }
        fn main() -> Int {
            let o: Option<Int> = None
            match (o.map(inc)) {
                Some(n) => n
                None => 7
            }
        }"#,
            ),
            7,
        ),
    ]);
}

#[test]
fn test_option_and_then_matches_interpreter_semantics() {
    assert_i64_cases(&[
        (
            &with_option_variants(
                r#"fn next(n: Int) -> Option<Int> { Some(n + 1) }
        fn main() -> Int {
            match (Some(41).and_then(next)) {
                Some(n) => n
                None => 0
            }
        }"#,
            ),
            42,
        ),
        (
            &with_option_variants(
                r#"fn next(n: Int) -> Option<Int> { Some(n + 1) }
        fn main() -> Int {
            let o: Option<Int> = None
            match (o.and_then(next)) {
                Some(n) => n
                None => 7
            }
        }"#,
            ),
            7,
        ),
    ]);
}

#[test]
fn test_option_map_or_matches_interpreter_semantics() {
    assert_i64_cases(&[
        (
            &with_option_variants(
                r#"fn inc(n: Int) -> Int { n + 1 }
        fn main() -> Int { Some(41).map_or(0, inc) }"#,
            ),
            42,
        ),
        (
            &with_option_variants(
                r#"fn inc(n: Int) -> Int { n + 1 }
        fn main() -> Int {
            let o: Option<Int> = None
            o.map_or(9, inc)
        }"#,
            ),
            9,
        ),
    ]);
}

#[test]
fn test_result_unwrap_or_matches_interpreter_semantics() {
    assert_i64_cases(&[
        (
            &with_result_variants(r#"fn main() -> Int { Ok(41).unwrap_or(0) }"#),
            41,
        ),
        (
            &with_result_variants(r#"fn main() -> Int { Err("oops").unwrap_or(7) }"#),
            7,
        ),
    ]);
}

#[test]
fn test_result_map_matches_interpreter_semantics() {
    assert_i64_cases(&[
        (
            &with_result_variants(
                r#"fn inc(n: Int) -> Int { n + 1 }
        fn main() -> Int {
            let r: Result<Int, String> = Ok(41)
            let mapped: Result<Int, String> = r.map(inc)
            match (mapped) {
                Ok(n) => n
                Err(_) => 0
            }
        }"#,
            ),
            42,
        ),
        (
            &with_result_variants(
                r#"fn inc(n: Int) -> Int { n + 1 }
        fn main() -> Int {
            let r: Result<Int, String> = Err("oops")
            let mapped: Result<Int, String> = r.map(inc)
            match (mapped) {
                Ok(n) => n
                Err(msg) => msg.len()
            }
        }"#,
            ),
            4,
        ),
    ]);
}

#[test]
fn test_result_and_then_matches_interpreter_semantics() {
    assert_i64_cases(&[
        (
            &with_result_variants(
                r#"fn next(n: Int) -> Result<Int, String> { Ok(n + 1) }
        fn main() -> Int {
            match (Ok(41).and_then(next)) {
                Ok(n) => n
                Err(_) => 0
            }
        }"#,
            ),
            42,
        ),
        (
            &with_result_variants(
                r#"fn next(n: Int) -> Result<Int, String> { Ok(n + 1) }
        fn main() -> Int {
            let r: Result<Int, String> = Err("oops")
            match (r.and_then(next)) {
                Ok(n) => n
                Err(msg) => msg.len()
            }
        }"#,
            ),
            4,
        ),
    ]);
}

#[test]
fn test_result_map_err_matches_interpreter_semantics() {
    assert_i64_cases(&[
        (
            &with_result_variants(
                r#"fn err_len(msg: String) -> Int { msg.len() }
        fn main() -> Int {
            let r: Result<Int, String> = Err("oops")
            match (r.map_err(err_len)) {
                Ok(_) => 0
                Err(n) => n
            }
        }"#,
            ),
            4,
        ),
        (
            &with_result_variants(
                r#"fn err_len(msg: String) -> Int { msg.len() }
        fn main() -> Int {
            match (Ok(41).map_err(err_len)) {
                Ok(n) => n
                Err(_) => 0
            }
        }"#,
            ),
            41,
        ),
    ]);
}

#[test]
fn test_result_map_or_matches_interpreter_semantics() {
    assert_i64_cases(&[
        (
            &with_result_variants(
                r#"fn inc(n: Int) -> Int { n + 1 }
        fn main() -> Int { Ok(41).map_or(0, inc) }"#,
            ),
            42,
        ),
        (
            &with_result_variants(
                r#"fn inc(n: Int) -> Int { n + 1 }
        fn main() -> Int {
            let r: Result<Int, String> = Err("oops")
            r.map_or(9, inc)
        }"#,
            ),
            9,
        ),
    ]);
}

#[test]
fn test_int_to_string_roundtrips_from_guest_memory() {
    assert_eq!(
        run_main_string("fn main() -> String { 42.to_string() }"),
        "42"
    );
    assert_eq!(
        run_main_string("fn main() -> String { (-1200).to_string() }"),
        "-1200"
    );
}

#[test]
fn test_char_to_string_roundtrips_from_guest_memory() {
    assert_eq!(
        run_main_string("fn main() -> String { 'x'.to_string() }"),
        "x"
    );
    assert_eq!(
        run_main_string("fn main() -> String { 'é'.to_string() }"),
        "é"
    );
}

#[test]
fn test_char_constant_roundtrips_as_i32_scalar() {
    assert_eq!(run_main_i32("fn main() -> Char { 'x' }"), 'x' as i32);
}

#[test]
fn test_char_equality_comparisons() {
    assert_i32_cases(&[
        ("fn main() -> Bool { 'a' == 'a' }", 1),
        ("fn main() -> Bool { 'a' != 'b' }", 1),
        ("fn main() -> Bool { 'é' == 'é' }", 1),
    ]);
}

#[test]
fn test_char_code_matches_interpreter_semantics() {
    assert_eq!(run_main_i64("fn main() -> Int { 'A'.code() }"), 65);
    assert_eq!(run_main_i64("fn main() -> Int { '😀'.code() }"), 128512);
}

#[test]
fn test_char_is_decimal_digit_matches_interpreter_semantics() {
    assert_i32_cases(&[
        ("fn main() -> Bool { '7'.is_decimal_digit() }", 1),
        ("fn main() -> Bool { 'x'.is_decimal_digit() }", 0),
        ("fn main() -> Bool { '٤'.is_decimal_digit() }", 0),
    ]);
}

#[test]
fn test_char_to_decimal_digit_matches_interpreter_semantics() {
    assert_i64_cases(&[
        (
            &with_option_variants(
                r#"fn main() -> Int {
    match ('7'.to_decimal_digit()) {
        Some(n) => n
        None => 0 - 1
    }
}"#,
            ),
            7,
        ),
        (
            &with_option_variants(
                r#"fn main() -> Int {
    match ('x'.to_decimal_digit()) {
        Some(n) => n
        None => 0 - 1
    }
}"#,
            ),
            -1,
        ),
    ]);
}

#[test]
fn test_char_to_digit_matches_interpreter_semantics() {
    assert_i64_cases(&[
        (
            &with_option_variants(
                r#"fn main() -> Int {
    match ('F'.to_digit(16)) {
        Some(n) => n
        None => 0 - 1
    }
}"#,
            ),
            15,
        ),
        (
            &with_option_variants(
                r#"fn main() -> Int {
    match ('f'.to_digit(16)) {
        Some(n) => n
        None => 0 - 1
    }
}"#,
            ),
            15,
        ),
        (
            &with_option_variants(
                r#"fn main() -> Int {
    match ('g'.to_digit(16)) {
        Some(n) => n
        None => 0 - 1
    }
}"#,
            ),
            -1,
        ),
    ]);
}

#[test]
fn test_char_to_digit_invalid_radix_traps() {
    assert!(run_main_traps(&with_option_variants(
        "fn main() -> Int { match ('7'.to_digit(1)) { Some(n) => n None => 0 } }",
    )));
}

#[test]
fn test_int_numeric_intrinsics_match_interpreter_semantics() {
    assert_i64_cases(&[
        ("fn main() -> Int { (0 - 5).abs() }", 5),
        ("fn main() -> Int { 2.pow(10) }", 1024),
        ("fn main() -> Int { 5.pow(0) }", 1),
        ("import math\nfn main() -> Int { math.min(3, 7) }", 3),
        ("import math\nfn main() -> Int { math.max(3, 7) }", 7),
        ("import math\nfn main() -> Int { math.gcd(54, 24) }", 6),
        (
            "import math\nfn main() -> Int { math.gcd(-54, 24) + math.gcd(0, 0) }",
            6,
        ),
        ("import math\nfn main() -> Int { math.lcm(6, 8) }", 24),
        (
            "import math\nfn main() -> Int { math.lcm(0, 5) + math.lcm(-3, 4) }",
            12,
        ),
    ]);
}

#[test]
fn test_int_numeric_intrinsic_traps_match_current_wasm_runtime_behavior() {
    assert!(run_main_traps(
        "fn main() -> Int { (-9223372036854775807 - 1).abs() }"
    ));
    assert!(run_main_traps("fn main() -> Int { 2.pow(-1) }"));
    assert!(run_main_traps("fn main() -> Int { 2.pow(63) }"));
    assert!(run_main_traps(
        "import math\nfn main() -> Int { math.lcm(9223372036854775807, 2) }"
    ));
}

#[test]
fn test_float_numeric_intrinsics_match_interpreter_semantics() {
    let to_float = run_main_f64("fn main() -> Float { 42.to_float() }");
    assert!((to_float - 42.0).abs() < f64::EPSILON);

    let abs_val = run_main_f64("fn main() -> Float { (0.0 - 2.5).abs() }");
    assert!((abs_val - 2.5).abs() < f64::EPSILON);

    assert_i64_cases(&[("fn main() -> Int { 42.9.to_int() }", 42)]);
    assert_i32_cases(&[
        ("fn main() -> Bool { (0.0 / 0.0).is_nan() }", 1),
        ("fn main() -> Bool { (1.0 / 0.0).is_infinite() }", 1),
        ("fn main() -> Bool { ((0.0 - 1.0) / 0.0).is_infinite() }", 1),
        ("fn main() -> Bool { 1.5.is_finite() }", 1),
        (
            "fn main() -> Bool { !(0.0 / 0.0).is_finite() && !(1.0 / 0.0).is_finite() }",
            1,
        ),
    ]);
}

#[test]
fn test_float_min_max_match_interpreter_semantics() {
    let fmin = run_main_f64("import math\nfn main() -> Float { math.fmin(3.5, 1.5) }");
    assert!((fmin - 1.5).abs() < f64::EPSILON);

    let fmax = run_main_f64("import math\nfn main() -> Float { math.fmax(3.5, 1.5) }");
    assert!((fmax - 3.5).abs() < f64::EPSILON);

    let nan_left = run_main_f64("import math\nfn main() -> Float { math.fmin(0.0 / 0.0, 2.0) }");
    assert!((nan_left - 2.0).abs() < f64::EPSILON);

    let nan_right = run_main_f64("import math\nfn main() -> Float { math.fmax(2.0, 0.0 / 0.0) }");
    assert!((nan_right - 2.0).abs() < f64::EPSILON);
}

// ── Arithmetic ────────────────────────────────────────────────────

#[test]
fn test_arithmetic() {
    assert_i64_cases(&[
        ("fn main() -> Int { 3 + 4 }", 7),
        ("fn main() -> Int { 10 - 3 }", 7),
        ("fn main() -> Int { 6 * 7 }", 42),
        ("fn main() -> Int { 42 / 6 }", 7),
        ("fn main() -> Int { 42 % 5 }", 2),
        ("fn main() -> Int { (3 + 4) * (10 - 8) }", 14),
    ]);
    assert_f64_cases(&[
        ("fn main() -> Float { 1.5 + 2.5 }", 4.0),
        ("fn main() -> Float { 3.0 * 2.0 }", 6.0),
        ("fn main() -> Float { 5.5 % 2.0 }", 1.5),
    ]);
}

// ── Comparisons ───────────────────────────────────────────────────

#[test]
fn test_int_comparisons() {
    assert_i32_cases(&[
        ("fn main() -> Bool { 42 == 42 }", 1),
        ("fn main() -> Bool { 42 == 43 }", 0),
        ("fn main() -> Bool { 3 < 5 }", 1),
        ("fn main() -> Bool { 5 > 3 }", 1),
    ]);
}

// ── Unary operations ──────────────────────────────────────────────

#[test]
fn test_unary_bitwise_and_short_circuit_ops() {
    assert_i64_cases(&[
        ("fn main() -> Int { -(42) }", -42),
        ("fn main() -> Int { ~42 }", !42_i64),
        ("fn main() -> Int { 12 & 10 }", 8),
        ("fn main() -> Int { 12 | 10 }", 14),
        ("fn main() -> Int { 12 ^ 10 }", 6),
        ("fn main() -> Int { 1 << 5 }", 32),
        ("fn main() -> Int { 128 >> 3 }", 16),
    ]);
    assert_i32_cases(&[
        ("fn main() -> Bool { !true }", 0),
        ("fn main() -> Bool { !false }", 1),
        ("fn main() -> Bool { false && 1 / 0 == 0 }", 0),
        ("fn main() -> Bool { true || 1 / 0 == 0 }", 1),
    ]);
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

#[test]
fn test_mutable_local_reassignment() {
    assert_eq!(
        run_main_i64("fn main() -> Int { var x = 1\n x = x + 4\n x }"),
        5
    );
}

#[test]
fn test_while_loop_accumulates_mutable_state() {
    assert_eq!(
        run_main_i64(
            "fn main() -> Int {\n\
               var i = 0\n\
               var acc = 0\n\
               while (i < 5) {\n\
                 acc = acc + i\n\
                 i = i + 1\n\
               }\n\
               acc\n\
             }"
        ),
        10
    );
}

#[test]
fn test_while_loop_break_and_continue() {
    assert_eq!(
        run_main_i64(
            "fn main() -> Int {\n\
               var i = 0\n\
               var acc = 0\n\
               while (i < 8) {\n\
                 i = i + 1\n\
                 if (i == 7) { break }\n\
                 if ((i % 2) == 0) { continue }\n\
                 acc = acc + i\n\
               }\n\
               acc\n\
             }"
        ),
        9
    );
}

#[test]
fn test_for_range_loop_break_and_continue() {
    assert_eq!(
        run_main_i64(
            "fn main() -> Int {\n\
               var acc = 0\n\
               for (x in 0..<8) {\n\
                 if (x == 6) { break }\n\
                 if ((x % 2) == 0) { continue }\n\
                 acc = acc + x\n\
               }\n\
               acc\n\
             }"
        ),
        9
    );
}

#[test]
fn test_for_range_source_is_evaluated_once() {
    assert_eq!(
        run_main_i64(
            "fn main() -> Int {\n\
               var counter = 0\n\
               for (x in { counter = counter + 1\n 0..<3 }) {\n\
                 x\n\
               }\n\
               counter\n\
             }"
        ),
        1
    );
}

#[test]
fn test_while_loop_match_can_update_mutable_state() {
    assert_eq!(
        run_main_i64(
            "type Step = Keep | Skip\n\
             fn step(i: Int) -> Step {\n\
               if ((i % 2) == 0) { Step.Skip } else { Step.Keep }\n\
             }\n\
             fn main() -> Int {\n\
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
             }"
        ),
        16
    );
}

#[test]
fn test_for_range_loop_match_can_update_mutable_state() {
    assert_eq!(
        run_main_i64(
            "type Step = Keep | Skip\n\
             fn step(i: Int) -> Step {\n\
               if ((i % 2) == 0) { Step.Skip } else { Step.Keep }\n\
             }\n\
             fn main() -> Int {\n\
               var acc = 0\n\
               for (x in 0..<6) {\n\
                 let ignored = match (step(x)) {\n\
                   Step.Keep => { acc = acc + x\n 0 }\n\
                   Step.Skip => { 0 }\n\
                 }\n\
                 ignored\n\
               }\n\
               acc\n\
             }"
        ),
        9
    );
}

#[test]
fn test_while_loop_match_can_break_and_continue() {
    assert_eq!(
        run_main_i64(
            "type Step = Keep | Skip | Stop\n\
             fn step(i: Int) -> Step {\n\
               if (i == 7) { Step.Stop } else { if ((i % 2) == 0) { Step.Skip } else { Step.Keep } }\n\
             }\n\
             fn main() -> Int {\n\
               var i = 0\n\
               var acc = 0\n\
               while (i < 10) {\n\
                 i = i + 1\n\
                 match (step(i)) {\n\
                   Step.Keep => { acc = acc + i }\n\
                   Step.Skip => { continue }\n\
                   Step.Stop => { break }\n\
                 }\n\
               }\n\
               acc\n\
             }"
        ),
        9
    );
}

#[test]
fn test_for_range_loop_match_can_break_and_continue() {
    assert_eq!(
        run_main_i64(
            "type Step = Keep | Skip | Stop\n\
             fn step(i: Int) -> Step {\n\
               if (i == 7) { Step.Stop } else { if ((i % 2) == 0) { Step.Skip } else { Step.Keep } }\n\
             }\n\
             fn main() -> Int {\n\
               var acc = 0\n\
               for (x in 0..<10) {\n\
                 match (step(x)) {\n\
                   Step.Keep => { acc = acc + x }\n\
                   Step.Skip => { continue }\n\
                   Step.Stop => { break }\n\
                 }\n\
               }\n\
               acc\n\
             }"
        ),
        9
    );
}

#[test]
fn test_if_branch_reassignment_updates_mutable_local() {
    assert_eq!(
        run_main_i64(
            "fn main() -> Int {\n\
               var x = 0\n\
               if (true) {\n\
                 x = 5\n\
               } else {\n\
                 x = 9\n\
               }\n\
               x\n\
             }"
        ),
        5
    );
}

// ── If/else ───────────────────────────────────────────────────────

#[test]
fn test_if_else_true() {
    assert_eq!(
        run_main_i64("fn main() -> Int { if (true) { 1 } else { 2 } }"),
        1
    );
}

#[test]
fn test_if_else_false() {
    assert_eq!(
        run_main_i64("fn main() -> Int { if (false) { 1 } else { 2 } }"),
        2
    );
}

#[test]
fn test_if_else_condition() {
    assert_eq!(
        run_main_i64("fn main() -> Int { let x = 10\n if (x > 5) { 100 } else { 0 } }"),
        100
    );
}

#[test]
fn test_nested_if_else() {
    assert_eq!(
        run_main_i64(
            "fn main() -> Int {\
               let x = 3\n\
               if (x > 5) { 100 } else { if (x > 1) { 50 } else { 0 } }\
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
fn test_named_function_ref_local_indirect_call() {
    assert_eq!(
        run_main_i64(
            "fn double(x: Int) -> Int { x * 2 }\n\
             fn main() -> Int {\n\
               let f = double\n\
               f(7)\n\
             }"
        ),
        14
    );
}

#[test]
fn test_named_function_ref_passed_to_higher_order_fn() {
    assert_eq!(
        run_main_i64(
            "fn apply(f: fn(Int) -> Int, x: Int) -> Int { f(x) }\n\
             fn double(x: Int) -> Int { x * 2 }\n\
             fn main() -> Int { apply(double, 7) }"
        ),
        14
    );
}

#[test]
fn test_non_capturing_lambda_local_indirect_call() {
    assert_eq!(
        run_main_i64(
            "fn main() -> Int {\n\
               let f = fn(x: Int) => x + 1\n\
               f(5)\n\
             }"
        ),
        6
    );
}

#[test]
fn test_non_capturing_lambda_passed_to_higher_order_fn() {
    assert_eq!(
        run_main_i64(
            "fn apply(f: fn(Int) -> Int, x: Int) -> Int { f(x) }\n\
             fn main() -> Int { apply(fn(x: Int) => x * 3, 7) }"
        ),
        21
    );
}

#[test]
fn test_capturing_lambda_local_indirect_call() {
    assert_eq!(
        run_main_i64(
            "fn main() -> Int {\n\
               let base = 5\n\
               let f = fn(x: Int) => x + base\n\
               f(7)\n\
             }"
        ),
        12
    );
}

#[test]
fn test_capturing_lambda_passed_to_higher_order_fn() {
    assert_eq!(
        run_main_i64(
            "fn apply(f: fn(Int) -> Int, x: Int) -> Int { f(x) }\n\
             fn main() -> Int {\n\
               let base = 5\n\
               let f = fn(x: Int) => x + base\n\
               apply(f, 7)\n\
             }"
        ),
        12
    );
}

#[test]
fn test_capturing_lambda_returned_from_function() {
    assert_eq!(
        run_main_i64(
            "fn make_adder(base: Int) -> fn(Int) -> Int {\n\
               fn(x: Int) => x + base\n\
             }\n\
             fn main() -> Int {\n\
               let add5 = make_adder(5)\n\
               add5(7)\n\
             }"
        ),
        12
    );
}

#[test]
fn test_zero_arg_lambda_local_call() {
    assert_eq!(
        run_main_i64(
            "fn main() -> Int {\n\
               let f = fn() => 9\n\
               f()\n\
             }"
        ),
        9
    );
}

#[test]
fn test_named_args_reordered_on_direct_lambda_call() {
    assert_eq!(
        run_main_i64(
            "fn main() -> Int {\n\
               (fn(x: Int, y: Int) => x - y)(y: 10, x: 3)\n\
             }"
        ),
        -7
    );
}

#[test]
fn test_named_args_reordered_on_local_named_function_value() {
    assert_eq!(
        run_main_i64(
            "fn sub(x: Int, y: Int) -> Int { x - y }\n\
             fn main() -> Int {\n\
               let f = sub\n\
               f(y: 10, x: 3)\n\
             }"
        ),
        -7
    );
}

#[test]
fn test_named_args_reordered_on_local_lambda_value() {
    assert_eq!(
        run_main_i64(
            "fn main() -> Int {\n\
               let f = fn(x: Int, y: Int) => x - y\n\
               f(y: 10, x: 3)\n\
             }"
        ),
        -7
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

#[test]
fn test_range_count_matches_interpreter_semantics() {
    assert_eq!(run_main_i64("fn main() -> Int { (0..<10).count() }"), 10);
}

#[test]
fn test_range_count_is_zero_when_start_is_not_less_than_end() {
    assert_eq!(run_main_i64("fn main() -> Int { (10..<0).count() }"), 0);
}

#[test]
fn test_range_count_by_with_capturing_predicate() {
    assert_eq!(
        run_main_i64(
            "fn main() -> Int {\n\
               let floor = 3\n\
               (0..<10).count(fn(x: Int) => x > floor)\n\
             }"
        ),
        6
    );
}

#[test]
fn test_range_fold_matches_interpreter_semantics() {
    assert_eq!(
        run_main_i64("fn main() -> Int { (0..<5).fold(0, fn(acc: Int, x: Int) => acc + x) }"),
        10
    );
}

#[test]
fn test_range_fold_with_capturing_closure() {
    assert_eq!(
        run_main_i64(
            "fn main() -> Int {\n\
               let base = 10\n\
               (0..<4).fold(0, fn(acc: Int, x: Int) => acc + x + base)\n\
             }"
        ),
        46
    );
}

#[test]
fn test_range_any_empty_is_false() {
    assert!(!run_main_bool(
        "fn main() -> Bool { ((0)..<0).any(fn(_n: Int) => true) }"
    ));
}

#[test]
fn test_range_all_empty_is_true() {
    assert!(run_main_bool(
        "fn main() -> Bool { ((0)..<0).all(fn(_n: Int) => false) }"
    ));
}

#[test]
fn test_range_any_and_all_with_capturing_predicate() {
    assert!(run_main_bool(
        "fn main() -> Bool {\n\
           let pivot = 3\n\
           let xs = ((0)..<5)\n\
           xs.any(fn(n: Int) => n == pivot) && xs.all(fn(n: Int) => n < pivot + 2)\n\
         }"
    ));
}

#[test]
fn test_range_find_returns_first_match() {
    assert_eq!(
        run_main_i64(
            "fn main() -> Int {\n\
               ((0)..<6).find(fn(n: Int) => n % 2 == 0 && n > 0).unwrap_or(-1)\n\
             }"
        ),
        2
    );
}

#[test]
fn test_range_contains_returns_true_for_present_value() {
    assert!(run_main_bool("fn main() -> Bool { ((0)..<6).contains(4) }"));
}

#[test]
fn test_range_contains_returns_false_for_missing_value() {
    assert!(!run_main_bool(
        "fn main() -> Bool { ((0)..<6).contains(9) }"
    ));
}

#[test]
fn test_range_contains_empty_is_false() {
    assert!(!run_main_bool(
        "fn main() -> Bool { ((0)..<0).contains(0) }"
    ));
}

// ── ADTs ──────────────────────────────────────────────────────────

#[test]
fn test_adt_construct_and_match() {
    assert_eq!(
        run_main_i64(
            "type Opt = Just(Int) | Nothing\n\
             fn main() -> Int {\n\
               match (Opt.Just(42)) {\n\
                 Opt.Just(x) => x\n\
                 Opt.Nothing => 0\n\
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
            "type Opt = Just(Int) | Nothing\n\
             fn main() -> Int {\n\
               match (Opt.Nothing) {\n\
                 Opt.Just(x) => x\n\
                 Opt.Nothing => -1\n\
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
            "type Color = Red | Green | Blue\n\
             fn to_int(c: Color) -> Int {\n\
               match (c) {\n\
                 Color.Red => 1\n\
                 Color.Green => 2\n\
                 Color.Blue => 3\n\
               }\n\
             }\n\
             fn main() -> Int { to_int(Color.Green) }"
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

#[test]
fn test_record_update_preserves_unchanged_fields() {
    let mut interner = Interner::default();
    let main_name = Name::new(&mut interner, "main");
    let entry_name = Name::new(&mut interner, "entry");
    let x_field = Name::new(&mut interner, "x");
    let y_field = Name::new(&mut interner, "y");

    let record_ty = Ty::Record {
        fields: vec![(x_field, Ty::Int), (y_field, Ty::Int)],
    };

    let mut builder = KirBuilder::new();
    let entry = builder.new_block(Some(entry_name));
    builder.switch_to(entry);

    let one = builder.push_const(Constant::Int(1), Ty::Int);
    let two = builder.push_const(Constant::Int(2), Ty::Int);
    let ten = builder.push_const(Constant::Int(10), Ty::Int);
    let hundred = builder.push_const(Constant::Int(100), Ty::Int);
    let ten_scale = builder.push_const(Constant::Int(10), Ty::Int);

    let base = builder.push_record_create(vec![(x_field, one), (y_field, two)], record_ty.clone());
    let updated = builder.push_record_update(base, vec![(x_field, ten)], record_ty.clone());

    let updated_x = builder.push_field_get(updated, x_field, Ty::Int);
    let updated_y = builder.push_field_get(updated, y_field, Ty::Int);
    let base_x = builder.push_field_get(base, x_field, Ty::Int);
    let updated_x_scaled = builder.push_binary(BinaryOp::Mul, updated_x, hundred, Ty::Int);
    let updated_y_scaled = builder.push_binary(BinaryOp::Mul, updated_y, ten_scale, Ty::Int);
    let partial = builder.push_binary(BinaryOp::Add, updated_x_scaled, updated_y_scaled, Ty::Int);
    let result = builder.push_binary(BinaryOp::Add, partial, base_x, Ty::Int);
    builder.set_return(result);

    let function = builder.build(
        main_name,
        vec![],
        Ty::Int,
        EffectSet::default(),
        entry,
        KirContracts::default(),
    );

    let mut module = KirModule::default();
    let fn_id = module.functions.alloc(function);
    module.entry = Some(fn_id);

    let mut program = instantiate_manual_module(&module, &ItemTree::default(), &interner);
    assert_eq!(program.call_main_i64().expect("main trapped"), 1021);
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
            "fn check(x: Int) -> Int contract requires (x > 0) { x * 2 }\n\
             fn main() -> Int { check(5) }"
        ),
        10
    );
}

#[test]
fn test_requires_fail_traps() {
    assert!(run_main_traps(
        "fn check(x: Int) -> Int contract requires (x > 0) { x * 2 }\n\
         fn main() -> Int { check(-5) }"
    ));
}

#[test]
fn test_requires_fail_zero_traps() {
    assert!(run_main_traps(
        "fn check(x: Int) -> Int contract requires (x > 0) { x * 2 }\n\
         fn main() -> Int { check(0) }"
    ));
}

#[test]
fn test_requires_pass_boundary() {
    assert_eq!(
        run_main_i64(
            "fn check(x: Int) -> Int contract requires (x > 0) { x * 2 }\n\
             fn main() -> Int { check(1) }"
        ),
        2
    );
}

#[test]
fn test_requires_multiple_callers() {
    assert_eq!(
        run_main_i64(
            "fn check(x: Int) -> Int contract requires (x > 0) { x * 2 }\n\
             fn main() -> Int { check(3) + check(7) }"
        ),
        20
    );
}

#[test]
fn test_requires_fail_complex_condition_traps() {
    assert!(run_main_traps(
        "fn check(x: Int, y: Int) -> Int contract requires (x > y) { x - y }\n\
         fn main() -> Int { check(3, 5) }"
    ));
}

#[test]
fn test_ensures_pass() {
    assert_eq!(
        run_main_i64(
            "fn positive() -> Int contract ensures (result > 0) { 42 }\n\
             fn main() -> Int { positive() }"
        ),
        42
    );
}

#[test]
fn test_ensures_fail_traps() {
    assert!(run_main_traps(
        "fn positive() -> Int contract ensures (result > 0) { -1 }\n\
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
               if (x > 10) { 100 } else { if (x > 3) { if (x > 4) { 50 } else { 30 } } else { 0 } }\n\
             }"
        ),
        50
    );
}

#[test]
fn test_very_deeply_nested_if_else_exceeds_old_follow_chain_cap() {
    let mut expr = String::from("0");
    for i in (0..80).rev() {
        expr = format!("if (x == {i}) {{ {i} }} else {{ {expr} }}");
    }
    let source = format!(
        "fn main() -> Int {{\n\
           let x = 79\n\
           {expr}\n\
         }}"
    );
    assert_eq!(run_main_i64(&source), 79);
}

#[test]
fn test_if_else_both_return() {
    assert_eq!(
        run_main_i64(
            "fn abs(x: Int) -> Int {\n\
               if (x > 0) { x } else { -(x) }\n\
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
               let x = if (true) { 10 } else { 20 }\n\
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
            "type Opt = Just(Int) | Nothing\n\
             fn main() -> Int {\n\
               match (Opt.Just(10)) {\n\
                 Opt.Just(x) => x * 2 + 1\n\
                 Opt.Nothing => 0\n\
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
            "type Opt = Just(Int) | Nothing\n\
             fn main() -> Int {\n\
               match (Opt.Just(5)) {\n\
                 Opt.Just(x) => if (x > 3) { x * 10 } else { x }\n\
                 Opt.Nothing => 0\n\
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
            "type Dir = North | South | East | West\n\
             fn to_int(d: Dir) -> Int {\n\
               match (d) {\n\
                 Dir.North => 1\n\
                 Dir.South => 2\n\
                 Dir.East => 3\n\
                 Dir.West => 4\n\
               }\n\
             }\n\
             fn main() -> Int { to_int(Dir.West) }"
        ),
        4
    );
}

#[test]
fn test_match_then_computation() {
    assert_eq!(
        run_main_i64(
            "type Opt = Just(Int) | Nothing\n\
             fn main() -> Int {\n\
               let v = match (Opt.Just(6)) {\n\
                 Opt.Just(x) => x\n\
                 Opt.Nothing => 0\n\
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
            "type Pair = Pair(Int, Int)\n\
             fn main() -> Int {\n\
               match (Pair.Pair(10, 20)) {\n\
                 Pair.Pair(a, b) => a + b\n\
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
            "type Triple = Triple(Int, Int, Int)\n\
             fn main() -> Int {\n\
               match (Triple.Triple(1, 2, 3)) {\n\
                 Triple.Triple(a, b, c) => a + b + c\n\
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
            "type Opt = Just(Int) | Nothing\n\
             fn main() -> Int {\n\
               let a = Opt.Just(10)\n\
               let b = Opt.Just(20)\n\
               let x = match (a) {\n\
                 Opt.Just(v) => v\n\
                 Opt.Nothing => 0\n\
               }\n\
               let y = match (b) {\n\
                 Opt.Just(v) => v\n\
                 Opt.Nothing => 0\n\
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
            "type Opt = Just(Int) | Nothing\n\
             fn main() -> Int {\n\
               let v = match (Opt.Just(5)) {\n\
                 Opt.Just(x) => x\n\
                 Opt.Nothing => 0\n\
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
               if (is_positive(5)) { 100 } else { 0 }\n\
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
            "type Opt = Just(Int) | Nothing\n\
             fn main() -> Int {\n\
               let x = Opt.Just(7)\n\
               match (x) {\n\
                 Opt.Just(v) => v\n\
                 Opt.Nothing => 0\n\
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
               let y = if (x > 5) { x * 2 } else { x }\n\
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
            "type Opt = Just(Int) | Nothing\n\
             fn main() -> Int {\n\
               let x = 5\n\
               let opt = if (x > 3) { Opt.Just(x) } else { Opt.Nothing }\n\
               match (opt) {\n\
                 Opt.Just(v) => v\n\
                 Opt.Nothing => 0\n\
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
            "type Opt = Just(Int) | Nothing\n\
             type Pair = { x: Int, y: Int }\n\
             fn main() -> Int {\n\
               let r = match (Opt.Just(10)) {\n\
                 Opt.Just(v) => Pair { x: v, y: v * 2 }\n\
                 Opt.Nothing => Pair { x: 0, y: 0 }\n\
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
            "type Opt = Just(Int) | Nothing\n\
             fn unwrap_or(o: Opt, default: Int) -> Int {\n\
               match (o) {\n\
                 Opt.Just(v) => v\n\
                 Opt.Nothing => default\n\
               }\n\
             }\n\
             fn main() -> Int { unwrap_or(Opt.Just(42), 0) }"
        ),
        42
    );
}

#[test]
fn test_function_call_with_adt_arg_none() {
    assert_eq!(
        run_main_i64(
            "type Opt = Just(Int) | Nothing\n\
             fn unwrap_or(o: Opt, default: Int) -> Int {\n\
               match (o) {\n\
                 Opt.Just(v) => v\n\
                 Opt.Nothing => default\n\
               }\n\
             }\n\
             fn main() -> Int { unwrap_or(Opt.Nothing, 99) }"
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
            "type Opt = Just(Int) | Nothing\n\
             fn make_some(x: Int) -> Opt { Opt.Just(x) }\n\
             fn main() -> Int {\n\
               match (make_some(42)) {\n\
                 Opt.Just(v) => v\n\
                 Opt.Nothing => 0\n\
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
            "type Outer = A(Int) | B\n\
             type Inner = X(Int) | Y\n\
             fn main() -> Int {\n\
               let o = Outer.A(1)\n\
               match (o) {\n\
                 Outer.A(v) => match (Inner.X(v * 10)) {\n\
                   Inner.X(w) => w + 1\n\
                   Inner.Y => 0\n\
                 }\n\
                 Outer.B => -1\n\
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
               if (x > 0) { if (x > 100) { 3 } else { 2 } } else { 1 }\n\
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

// ══════════════════════════════════════════════════════════════════
// Phase 1: Negative & Edge-Case Tests
// ══════════════════════════════════════════════════════════════════

// ── Contract Edge Cases ───────────────────────────────────────────

#[test]
fn test_ensures_gteq_pass() {
    // ensures with >= comparison on result — passes (15 >= 10).
    assert_eq!(
        run_main_i64(
            "fn tripled(x: Int) -> Int contract ensures (result >= 10) { x * 3 }\n\
             fn main() -> Int { tripled(5) }"
        ),
        15
    );
}

#[test]
fn test_ensures_gteq_traps() {
    // 3 >= 10 is false → trap.
    assert!(run_main_traps(
        "fn tripled(x: Int) -> Int contract ensures (result >= 10) { x }\n\
         fn main() -> Int { tripled(3) }"
    ));
}

#[test]
fn test_ensures_equality_pass() {
    assert_eq!(
        run_main_i64(
            "fn get_42() -> Int contract ensures (result == 42) { 42 }\n\
             fn main() -> Int { get_42() }"
        ),
        42
    );
}

#[test]
fn test_ensures_equality_traps() {
    assert!(run_main_traps(
        "fn get_42() -> Int contract ensures (result == 42) { 41 }\n\
         fn main() -> Int { get_42() }"
    ));
}

#[test]
fn test_requires_and_ensures_both_pass() {
    assert_eq!(
        run_main_i64(
            "fn safe(x: Int) -> Int contract requires (x > 0) ensures (result > x) { x + 1 }\n\
             fn main() -> Int { safe(5) }"
        ),
        6
    );
}

#[test]
fn test_requires_pass_ensures_fail_traps() {
    // requires passes (5 > 0), but ensures fails (4 > 5 is false).
    assert!(run_main_traps(
        "fn safe(x: Int) -> Int contract requires (x > 0) ensures (result > x) { x - 1 }\n\
         fn main() -> Int { safe(5) }"
    ));
}

#[test]
fn test_requires_fail_with_ensures_traps() {
    // requires fails first (-1 > 0 is false) — traps before body runs.
    assert!(run_main_traps(
        "fn safe(x: Int) -> Int contract requires (x > 0) ensures (result > x) { x + 1 }\n\
         fn main() -> Int { safe(-1) }"
    ));
}

#[test]
fn test_contract_on_bool_returning_fn() {
    // ensures on a Bool-returning function, condition uses result.
    assert_eq!(
        run_main_i32(
            "fn check(x: Int) -> Bool contract ensures (result == true) { x > 10 }\n\
             fn main() -> Bool { check(42) }"
        ),
        1
    );
}

// ── Stack Hygiene (regression guards for alloc fix) ───────────────

#[test]
fn test_multiple_adt_in_same_if_arm() {
    // Two ADT constructions in the same then-arm — both allocs must
    // leave the value stack clean.
    assert_eq!(
        run_main_i64(
            "type Opt = Just(Int) | Nothing\n\
             fn main() -> Int {\n\
               let x = 5\n\
               let pair = if (x > 0) {\n\
                 let a = Opt.Just(x)\n\
                 let b = Opt.Just(x * 2)\n\
                 let va = match (a) { Opt.Just(v) => v\n Opt.Nothing => 0 }\n\
                 let vb = match (b) { Opt.Just(v) => v\n Opt.Nothing => 0 }\n\
                 va + vb\n\
               } else { 0 }\n\
               pair\n\
             }"
        ),
        15
    );
}

#[test]
fn test_adt_in_deeply_nested_if() {
    // ADT construction at the bottom of 3 levels of if/else nesting.
    assert_eq!(
        run_main_i64(
            "type Opt = Just(Int) | Nothing\n\
             fn main() -> Int {\n\
               let x = 5\n\
               let opt = if (x > 0) {\n\
                 if (x > 3) {\n\
                   if (x > 4) { Opt.Just(x) } else { Opt.Nothing }\n\
                 } else { Opt.Nothing }\n\
               } else { Opt.Nothing }\n\
               match (opt) { Opt.Just(v) => v\n Opt.Nothing => -1 }\n\
             }"
        ),
        5
    );
}

#[test]
fn test_adt_in_nested_match_arms() {
    // ADT construction in both arms of a match that's inside another match.
    assert_eq!(
        run_main_i64(
            "type AB = A | B\n\
             type Opt = Just(Int) | Nothing\n\
             fn main() -> Int {\n\
               let x = AB.A\n\
               let opt = match (x) {\n\
                 AB.A => match (AB.B) {\n\
                   AB.A => Opt.Just(1)\n\
                   AB.B => Opt.Just(2)\n\
                 }\n\
                 AB.B => Opt.Nothing\n\
               }\n\
               match (opt) { Opt.Just(v) => v\n Opt.Nothing => 0 }\n\
             }"
        ),
        2
    );
}

#[test]
fn test_record_in_if_arms() {
    // Record construction in both if/else arms.
    assert_eq!(
        run_main_i64(
            "type Pair = { x: Int, y: Int }\n\
             fn main() -> Int {\n\
               let cond = true\n\
               let r = if (cond) { Pair { x: 10, y: 20 } } else { Pair { x: 1, y: 2 } }\n\
               r.x + r.y\n\
             }"
        ),
        30
    );
}

#[test]
fn test_mixed_adt_and_record_in_scope() {
    // ADT and record allocations interleaved in the same block.
    assert_eq!(
        run_main_i64(
            "type Opt = Just(Int) | Nothing\n\
             type Pair = { x: Int, y: Int }\n\
             fn main() -> Int {\n\
               let a = Opt.Just(10)\n\
               let r = Pair { x: 20, y: 30 }\n\
               let b = Opt.Just(40)\n\
               let va = match (a) { Opt.Just(v) => v\n Opt.Nothing => 0 }\n\
               let vb = match (b) { Opt.Just(v) => v\n Opt.Nothing => 0 }\n\
               va + r.x + r.y + vb\n\
             }"
        ),
        100
    );
}

#[test]
fn test_record_in_match_arm_with_if() {
    // Record construction inside an if/else that's inside a match arm.
    assert_eq!(
        run_main_i64(
            "type Opt = Just(Int) | Nothing\n\
             type Pair = { x: Int, y: Int }\n\
             fn main() -> Int {\n\
               let r = match (Opt.Just(5)) {\n\
                 Opt.Just(v) => if (v > 3) {\n\
                   Pair { x: v, y: v * 10 }\n\
                 } else {\n\
                   Pair { x: 0, y: 0 }\n\
                 }\n\
                 Opt.Nothing => Pair { x: -1, y: -1 }\n\
               }\n\
               r.x + r.y\n\
             }"
        ),
        55
    );
}

// ── Control Flow Edge Cases ───────────────────────────────────────

#[test]
fn test_match_all_arms_return() {
    // Every arm uses explicit return — no merge block exists.
    assert_eq!(
        run_main_i64(
            "type Opt = Just(Int) | Nothing\n\
             fn extract(o: Opt) -> Int {\n\
               match (o) {\n\
                 Opt.Just(x) => return x\n\
                 Opt.Nothing => return -1\n\
               }\n\
             }\n\
             fn main() -> Int { extract(Opt.Just(42)) }"
        ),
        42
    );
}

#[test]
fn test_match_all_arms_return_default() {
    // Same but hits the Nothing arm.
    assert_eq!(
        run_main_i64(
            "type Opt = Just(Int) | Nothing\n\
             fn extract(o: Opt) -> Int {\n\
               match (o) {\n\
                 Opt.Just(x) => return x\n\
                 Opt.Nothing => return -1\n\
               }\n\
             }\n\
             fn main() -> Int { extract(Opt.Nothing) }"
        ),
        -1
    );
}

#[test]
fn test_if_both_arms_explicit_return() {
    // Both if/else branches use explicit return — no merge block.
    assert_eq!(
        run_main_i64(
            "fn abs_ret(x: Int) -> Int {\n\
               if (x > 0) { return x } else { return -(x) }\n\
             }\n\
             fn main() -> Int { abs_ret(-7) }"
        ),
        7
    );
}

#[test]
fn test_match_mixed_branch_and_switch_arms() {
    // One arm has if/else (Branch), another has nested match (Switch).
    assert_eq!(
        run_main_i64(
            "type Outer = A(Int) | B(Int)\n\
             type Inner = X(Int) | Y\n\
             fn main() -> Int {\n\
               let o = Outer.A(5)\n\
               match (o) {\n\
                 Outer.A(v) => if (v > 3) { v * 10 } else { v }\n\
                 Outer.B(v) => match (Inner.X(v)) {\n\
                   Inner.X(w) => w + 100\n\
                   Inner.Y => 0\n\
                 }\n\
               }\n\
             }"
        ),
        50
    );
}

#[test]
fn test_match_mixed_branch_and_switch_arms_other() {
    // Same structure but hits the B arm (nested Switch path).
    assert_eq!(
        run_main_i64(
            "type Outer = A(Int) | B(Int)\n\
             type Inner = X(Int) | Y\n\
             fn main() -> Int {\n\
               let o = Outer.B(7)\n\
               match (o) {\n\
                 Outer.A(v) => if (v > 3) { v * 10 } else { v }\n\
                 Outer.B(v) => match (Inner.X(v)) {\n\
                   Inner.X(w) => w + 100\n\
                   Inner.Y => 0\n\
                 }\n\
               }\n\
             }"
        ),
        107
    );
}

#[test]
fn test_single_variant_match() {
    // Single-variant ADT — br_table with one entry.
    assert_eq!(
        run_main_i64(
            "type Wrap = Wrap(Int)\n\
             fn main() -> Int {\n\
               match (Wrap.Wrap(42)) {\n\
                 Wrap.Wrap(x) => x\n\
               }\n\
             }"
        ),
        42
    );
}

// ── Arithmetic Boundaries ─────────────────────────────────────────

#[test]
fn test_int_division_by_zero_traps() {
    // WASM i64.div_s traps on division by zero.
    assert!(run_main_traps("fn main() -> Int { 10 / 0 }"));
}

#[test]
fn test_int_mod_by_zero_traps() {
    // WASM i64.rem_s traps on division by zero.
    assert!(run_main_traps("fn main() -> Int { 10 % 0 }"));
}

#[test]
fn test_int_overflow_add_traps() {
    assert!(run_main_traps(
        "fn main() -> Int { 9223372036854775807 + 1 }"
    ));
}

#[test]
fn test_int_overflow_sub_traps() {
    assert!(run_main_traps(
        "fn main() -> Int {\n\
           let min = -(9223372036854775807) - 1\n\
           min - 1\n\
         }"
    ));
}

#[test]
fn test_int_overflow_mul_traps() {
    assert!(run_main_traps(
        "fn main() -> Int {\n\
           let a = 4611686018427387904\n\
           a * 4\n\
         }"
    ));
}

#[test]
fn test_int_min_div_neg_one_traps() {
    // i64::MIN / -1 overflows (result would be i64::MAX+1) — WASM traps.
    assert!(run_main_traps(
        "fn main() -> Int {\n\
           let min = -(9223372036854775807) - 1\n\
           min / -1\n\
         }"
    ));
}

#[test]
fn test_int_min_mod_neg_one_traps() {
    assert!(run_main_traps(
        "fn main() -> Int {\n\
           let min = -(9223372036854775807) - 1\n\
           min % -1\n\
         }"
    ));
}

#[test]
fn test_unary_neg_min_traps() {
    assert!(run_main_traps(
        "fn main() -> Int { -(-(9223372036854775807) - 1) }"
    ));
}

#[test]
fn test_shift_amount_out_of_range_traps() {
    assert!(run_main_traps("fn main() -> Int { 1 << 64 }"));
    assert!(run_main_traps("fn main() -> Int { 1 >> -1 }"));
}

#[test]
fn test_int_max_literal() {
    assert_eq!(
        run_main_i64("fn main() -> Int { 9223372036854775807 }"),
        i64::MAX
    );
}

#[test]
fn test_float_nan_eq_self_false() {
    // IEEE 754: NaN == NaN is false.
    assert_eq!(
        run_main_i32(
            "fn main() -> Bool {\n\
               let nan = 0.0 / 0.0\n\
               nan == nan\n\
             }"
        ),
        0
    );
}

#[test]
fn test_float_nan_neq_self_true() {
    // IEEE 754: NaN != NaN is true.
    assert_eq!(
        run_main_i32(
            "fn main() -> Bool {\n\
               let nan = 0.0 / 0.0\n\
               nan != nan\n\
             }"
        ),
        1
    );
}

// ══════════════════════════════════════════════════════════════════
// Bug #196: `result` in ensures gets Ty::Error — TDD tests
// These test arithmetic/unary operations on `result` inside ensures.
// They all fail before the fix with:
//   UnsupportedInstruction("binary ... on Error")
// ══════════════════════════════════════════════════════════════════

#[test]
fn test_ensures_result_mul_pass() {
    // result * 2 = 30, 30 > 10 → ensures passes.
    assert_eq!(
        run_main_i64(
            "fn tripled(x: Int) -> Int contract ensures (result * 2 > 10) { x * 3 }\n\
             fn main() -> Int { tripled(5) }"
        ),
        15
    );
}

#[test]
fn test_ensures_result_mul_traps() {
    // result = 3, 3 * 2 = 6, 6 > 10 is false → trap.
    assert!(run_main_traps(
        "fn identity(x: Int) -> Int contract ensures (result * 2 > 10) { x }\n\
         fn main() -> Int { identity(3) }"
    ));
}

#[test]
fn test_ensures_result_add_pass() {
    // result = 10, 10 + 5 = 15, 15 > 10 → passes.
    assert_eq!(
        run_main_i64(
            "fn double(x: Int) -> Int contract ensures (result + 5 > 10) { x * 2 }\n\
             fn main() -> Int { double(5) }"
        ),
        10
    );
}

#[test]
fn test_ensures_result_sub_pass() {
    // result = 10, 10 - 1 = 9, 9 >= 0 → passes.
    assert_eq!(
        run_main_i64(
            "fn inc(x: Int) -> Int contract ensures (result - 1 >= 0) { x + 1 }\n\
             fn main() -> Int { inc(9) }"
        ),
        10
    );
}

#[test]
fn test_ensures_result_div_pass() {
    // result = 20, 20 / 2 = 10, 10 > 0 → passes.
    assert_eq!(
        run_main_i64(
            "fn dbl(x: Int) -> Int contract ensures (result / 2 > 0) { x * 2 }\n\
             fn main() -> Int { dbl(10) }"
        ),
        20
    );
}

#[test]
fn test_ensures_result_neg_pass() {
    // result = -5, -(result) = 5, 5 > 0 → passes.
    assert_eq!(
        run_main_i64(
            "fn negate(x: Int) -> Int contract ensures (-(result) > 0) { -(x) }\n\
             fn main() -> Int { negate(5) }"
        ),
        -5
    );
}

#[test]
fn test_ensures_result_squared_pass() {
    // result = 4, 4 * 4 = 16, 16 > 0 → passes.
    assert_eq!(
        run_main_i64(
            "fn square(x: Int) -> Int contract ensures (result * result > 0) { x * x }\n\
             fn main() -> Int { square(2) }"
        ),
        4
    );
}

#[test]
fn test_ensures_result_with_param_arithmetic() {
    // result = 6, x = 5, 6 - 5 = 1, 1 > 0 → passes.
    assert_eq!(
        run_main_i64(
            "fn inc(x: Int) -> Int contract ensures (result - x > 0) { x + 1 }\n\
             fn main() -> Int { inc(5) }"
        ),
        6
    );
}

#[test]
fn test_ensures_result_chained_arithmetic() {
    // result = 7, 7 * 2 + 1 = 15, 15 > 10 → passes.
    assert_eq!(
        run_main_i64(
            "fn compute(x: Int) -> Int contract ensures (result * 2 + 1 > 10) { x }\n\
             fn main() -> Int { compute(7) }"
        ),
        7
    );
}

#[test]
fn test_ensures_result_chained_arithmetic_traps() {
    // result = 3, 3 * 2 + 1 = 7, 7 > 10 is false → trap.
    assert!(run_main_traps(
        "fn compute(x: Int) -> Int contract ensures (result * 2 + 1 > 10) { x }\n\
         fn main() -> Int { compute(3) }"
    ));
}

#[test]
fn test_ensures_result_self_eq() {
    // result == result is always true — both sides reference result.
    assert_eq!(
        run_main_i64(
            "fn identity(x: Int) -> Int contract ensures (result == result) { x }\n\
             fn main() -> Int { identity(42) }"
        ),
        42
    );
}

#[test]
fn test_ensures_float_result_arithmetic() {
    // Float return: result + 1.0 = 5.0, 5.0 > 3.0 → passes.
    let r = run_main_f64(
        "fn half(x: Float) -> Float contract ensures (result + 1.0 > 3.0) { x / 2.0 }\n\
         fn main() -> Float { half(8.0) }",
    );
    assert!((r - 4.0).abs() < f64::EPSILON);
}

#[test]
fn test_ensures_float_result_mul() {
    // Float return: result * 2.0 = 10.0, 10.0 > 5.0 → passes.
    let r = run_main_f64(
        "fn half(x: Float) -> Float contract ensures (result * 2.0 > 5.0) { x / 2.0 }\n\
         fn main() -> Float { half(10.0) }",
    );
    assert!((r - 5.0).abs() < f64::EPSILON);
}

#[test]
fn test_requires_param_arithmetic() {
    // requires with arithmetic on params: x * 2 > 5.
    // Verifies requires expressions also get proper type inference.
    assert_eq!(
        run_main_i64(
            "fn check(x: Int) -> Int contract requires (x * 2 > 5) { x }\n\
             fn main() -> Int { check(4) }"
        ),
        4
    );
}

#[test]
fn test_requires_param_arithmetic_traps() {
    // x = 2, 2 * 2 = 4, 4 > 5 is false → trap.
    assert!(run_main_traps(
        "fn check(x: Int) -> Int contract requires (x * 2 > 5) { x }\n\
         fn main() -> Int { check(2) }"
    ));
}

#[test]
fn test_requires_complex_and_ensures_complex() {
    // Both requires and ensures have arithmetic.
    // requires: x * 2 > 0 → 10 * 2 = 20 > 0 ✓
    // result = 30, ensures: result - x > 0 → 30 - 10 = 20 > 0 ✓
    assert_eq!(
        run_main_i64(
            "fn triple(x: Int) -> Int contract requires (x * 2 > 0) ensures (result - x > 0) { x * 3 }\n\
             fn main() -> Int { triple(10) }"
        ),
        30
    );
}

// ── Float field extraction (#335) ───────────────────────────────

#[test]
fn test_adt_float_field_roundtrip() {
    // Float stored in ADT field, then extracted via match — must use
    // type-aware f64.store/f64.load (not i64 reinterpret hack).
    let val = run_main_f64(
        "type Wrap = Wrap(Float)\n\
         fn main() -> Float {\n\
           match (Wrap.Wrap(3.14)) {\n\
             Wrap.Wrap(x) => x\n\
           }\n\
         }",
    );
    let expected = 314.0_f64 / 100.0_f64;
    assert!(
        (val - expected).abs() < 1e-10,
        "expected {expected}, got {val}"
    );
}

#[test]
fn test_adt_float_field_arithmetic() {
    // Extract float from ADT and use it in arithmetic.
    let val = run_main_f64(
        "type Wrap = Wrap(Float)\n\
         fn main() -> Float {\n\
           match (Wrap.Wrap(2.5)) {\n\
             Wrap.Wrap(x) => x * 2.0\n\
           }\n\
         }",
    );
    assert!((val - 5.0).abs() < 1e-10, "expected 5.0, got {val}");
}

#[test]
fn test_adt_mixed_int_float_fields() {
    // ADT with both Int and Float fields — Int extraction must still work.
    assert_eq!(
        run_main_i64(
            "type Pair = Pair(Int, Float)\n\
             fn main() -> Int {\n\
               match (Pair.Pair(42, 3.14)) {\n\
                 Pair.Pair(n, _) => n\n\
               }\n\
             }"
        ),
        42
    );
}

#[test]
fn test_adt_mixed_int_float_fields_get_float() {
    // Same mixed ADT but extract the Float field.
    let val = run_main_f64(
        "type Pair = Pair(Int, Float)\n\
         fn main() -> Float {\n\
           match (Pair.Pair(42, 3.14)) {\n\
             Pair.Pair(_, f) => f\n\
           }\n\
         }",
    );
    let expected = 314.0_f64 / 100.0_f64;
    assert!(
        (val - expected).abs() < 1e-10,
        "expected {expected}, got {val}"
    );
}

#[test]
fn test_record_float_field_roundtrip() {
    // Float stored in record field, then extracted via field access.
    let val = run_main_f64(
        "type Pt = { x: Float, y: Float }\n\
         fn main() -> Float {\n\
           let p = Pt { x: 1.5, y: 2.5 }\n\
           p.x\n\
         }",
    );
    assert!((val - 1.5).abs() < 1e-10, "expected 1.5, got {val}");
}

#[test]
fn test_record_float_field_addition() {
    // Extract two float fields from a record and add them.
    let val = run_main_f64(
        "type Pt = { x: Float, y: Float }\n\
         fn main() -> Float {\n\
           let p = Pt { x: 1.5, y: 2.5 }\n\
           p.x + p.y\n\
         }",
    );
    assert!((val - 4.0).abs() < 1e-10, "expected 4.0, got {val}");
}

#[test]
fn test_record_mixed_int_float_fields() {
    // Record with Int and Float fields — guard that Int extraction is unaffected.
    assert_eq!(
        run_main_i64(
            "type Rec = { count: Int, value: Float }\n\
             fn main() -> Int {\n\
               let r = Rec { count: 7, value: 3.14 }\n\
               r.count\n\
             }"
        ),
        7
    );
}

#[test]
fn test_record_mixed_int_float_get_float() {
    // Same mixed record but extract the Float field.
    let val = run_main_f64(
        "type Rec = { count: Int, value: Float }\n\
         fn main() -> Float {\n\
           let r = Rec { count: 7, value: 3.14 }\n\
           r.value\n\
         }",
    );
    let expected = 314.0_f64 / 100.0_f64;
    assert!(
        (val - expected).abs() < 1e-10,
        "expected {expected}, got {val}"
    );
}
