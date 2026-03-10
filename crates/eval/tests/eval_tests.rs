//! Integration tests for the tree-walking interpreter.
#![allow(clippy::unwrap_used)]

use std::borrow::Cow;
use std::sync::{Mutex, OnceLock};

use kyokara_eval::manifest::CapabilityManifest;
use kyokara_eval::value::Value;

fn normalize_immutable_collection_constructor_import(source: &str) -> Cow<'_, str> {
    // RFC 0009 makes immutable collection constructors module-qualified.
    // Most behavior tests do not care about import mechanics, so keep them
    // focused on behavior once they migrate to `collections.*.new()`.
    let uses_canonical_immutable_constructor = source.contains("collections.List.new(")
        || source.contains("collections.Map.new(")
        || source.contains("collections.Set.new(");
    if uses_canonical_immutable_constructor && !source.contains("import collections") {
        Cow::Owned(format!("import collections\n{source}"))
    } else {
        Cow::Borrowed(source)
    }
}

fn run_ok(source: &str) -> Value {
    let source = normalize_immutable_collection_constructor_import(source);
    match kyokara_eval::run(source.as_ref()) {
        Ok(result) => result.value,
        Err(e) => panic!("runtime error: {e}"),
    }
}

fn run_err(source: &str) -> String {
    let source = normalize_immutable_collection_constructor_import(source);
    match kyokara_eval::run(source.as_ref()) {
        Ok(result) => panic!("expected error, got {:?}", result.value),
        Err(e) => e.to_string(),
    }
}

fn recursion_limit_env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct RecursionLimitEnvGuard {
    previous: Option<String>,
}

impl Drop for RecursionLimitEnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => unsafe {
                std::env::set_var("KYOKARA_INTERPRETER_MAX_CALL_DEPTH", value)
            },
            None => unsafe { std::env::remove_var("KYOKARA_INTERPRETER_MAX_CALL_DEPTH") },
        }
    }
}

fn run_err_with_call_depth_limit(source: &str, limit: usize) -> String {
    let _lock = recursion_limit_env_lock().lock().unwrap();
    let previous = std::env::var("KYOKARA_INTERPRETER_MAX_CALL_DEPTH").ok();
    let _guard = RecursionLimitEnvGuard { previous };
    unsafe { std::env::set_var("KYOKARA_INTERPRETER_MAX_CALL_DEPTH", limit.to_string()) };
    run_err(source)
}

fn run_ok_with_call_depth_limit(source: &str, limit: usize) -> Value {
    let _lock = recursion_limit_env_lock().lock().unwrap();
    let previous = std::env::var("KYOKARA_INTERPRETER_MAX_CALL_DEPTH").ok();
    let _guard = RecursionLimitEnvGuard { previous };
    unsafe { std::env::set_var("KYOKARA_INTERPRETER_MAX_CALL_DEPTH", limit.to_string()) };
    run_ok(source)
}

fn check_has_compile_errors(source: &str) -> bool {
    let source = normalize_immutable_collection_constructor_import(source);
    let result = kyokara_hir::check_file(source.as_ref());
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

fn run_project_with_files_err(files: &[(&str, &str)]) -> String {
    let dir = tempfile::tempdir().unwrap();
    for (name, source) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, source).unwrap();
    }
    let main_path = dir.path().join("main.ky");
    match kyokara_eval::run_project(&main_path) {
        Ok(result) => panic!("expected error, got {:?}", result.value),
        Err(e) => e.to_string(),
    }
}

fn run_project_with_files_manifest_ok(
    files: &[(&str, &str)],
    manifest: Option<CapabilityManifest>,
) -> Value {
    let dir = tempfile::tempdir().unwrap();
    for (name, source) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, source).unwrap();
    }
    let main_path = dir.path().join("main.ky");
    match kyokara_eval::run_project_with_manifest(&main_path, manifest) {
        Ok(result) => result.value,
        Err(e) => panic!("runtime error: {e}"),
    }
}

fn run_project_with_files_manifest_err(
    files: &[(&str, &str)],
    manifest: Option<CapabilityManifest>,
) -> String {
    let dir = tempfile::tempdir().unwrap();
    for (name, source) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, source).unwrap();
    }
    let main_path = dir.path().join("main.ky");
    match kyokara_eval::run_project_with_manifest(&main_path, manifest) {
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
fn eval_pipe_identifier_is_allowed_and_pipeline_still_runs() {
    let val = run_ok("fn id(x: Int) -> Int { x }\nfn main() -> Int { let pipe = 1\n pipe |> id }");
    assert!(matches!(val, Value::Int(1)));
}

#[test]
fn eval_top_level_let_constant_is_available_in_main() {
    let val = run_ok("let off = 1\nfn main() -> Int { off }");
    assert_eq!(val, Value::Int(1));
}

#[test]
fn eval_later_top_level_let_can_use_earlier_top_level_let() {
    let val = run_ok("let off = 1\nlet off2 = off + 1\nfn main() -> Int { off2 }");
    assert_eq!(val, Value::Int(2));
}

#[test]
fn eval_function_body_can_read_top_level_let() {
    let val = run_ok("let off = 1\nfn add(x: Int) -> Int { x + off }\nfn main() -> Int { add(1) }");
    assert_eq!(val, Value::Int(2));
}

#[test]
fn eval_imported_module_function_can_read_its_top_level_let() {
    let val = run_project_with_files_manifest_ok(
        &[
            ("main.ky", "import util\nfn main() -> Int { util() }"),
            ("util.ky", "let off = 1\npub fn util() -> Int { off }"),
        ],
        None,
    );
    assert_eq!(val, Value::Int(1));
}

#[test]
fn eval_builtin_trait_qualified_call() {
    let val = run_ok("fn main() -> Int { Ord.compare(3, 1) }");
    assert!(matches!(val, Value::Int(1)));
}

#[test]
fn eval_user_impl_trait_qualified_call() {
    let val = run_ok(
        "trait Show { fn show(self) -> String }\n\
         type Point = { x: Int }\n\
         impl Show for Point { fn show(self) -> String { \"p\" } }\n\
         fn main() -> String {\n\
             let p: Point = Point { x: 1 }\n\
             Show.show(p)\n\
         }",
    );
    assert!(matches!(val, Value::String(ref s) if s == "p"));
}

#[test]
fn eval_map_supports_derived_key_type() {
    let val = run_ok(
        "import collections\n\
         type Point derive(Eq, Hash) = { x: Int, y: Int }\n\
         fn main() -> Int {\n\
             let p: Point = Point { x: 1, y: 2 }\n\
             let m = collections.Map.new().insert(p, 7)\n\
             if (m.contains(p)) { m[p] } else { 0 }\n\
         }",
    );
    assert_eq!(val, Value::Int(7));
}

#[test]
fn eval_mutable_map_supports_derived_key_type() {
    let val = run_ok(
        "import collections\n\
         type Point derive(Eq, Hash) = { x: Int, y: Int }\n\
         fn main() -> Int {\n\
             let p: Point = Point { x: 1, y: 2 }\n\
             let m = collections.MutableMap.new().insert(p, 11)\n\
             if (m.contains(p)) { m[p] } else { 0 }\n\
         }",
    );
    assert_eq!(val, Value::Int(11));
}

#[test]
fn eval_set_and_frequencies_support_derived_hash_eq_types() {
    let val = run_ok(
        "import collections\n\
         type Point derive(Eq, Hash) = { x: Int }\n\
         fn main() -> Int {\n\
             let p: Point = Point { x: 3 }\n\
             let s = collections.Set.new().insert(p)\n\
             let counts = collections.List.new().push(p).push(p).frequencies()\n\
             if (s.contains(p)) { counts[p] } else { 0 }\n\
         }",
    );
    assert_eq!(val, Value::Int(2));
}

#[test]
fn eval_mutable_set_supports_derived_hash_eq_type() {
    let val = run_ok(
        "import collections\n\
         type Point derive(Eq, Hash) = { x: Int }\n\
         fn main() -> Int {\n\
             let p: Point = Point { x: 3 }\n\
             let s = collections.MutableSet.new().insert(p)\n\
             if (s.contains(p)) { s.len() } else { 0 }\n\
         }",
    );
    assert_eq!(val, Value::Int(1));
}

#[test]
fn eval_manual_eq_hash_impls_drive_collection_behavior() {
    let val = run_ok(
        "import collections\n\
         type Key = { raw: Int }\n\
         impl Eq for Key {\n\
             fn eq(self, other: Key) -> Bool { self.raw % 2 == other.raw % 2 }\n\
         }\n\
         impl Hash for Key {\n\
             fn hash(self) -> Int { self.raw % 2 }\n\
         }\n\
         fn main() -> Int {\n\
             let a: Key = Key { raw: 1 }\n\
             let b: Key = Key { raw: 3 }\n\
             let m = collections.Map.new().insert(a, 10).insert(b, 20)\n\
             m.len() * 100 + m[a]\n\
         }",
    );
    assert_eq!(val, Value::Int(120));
}

#[test]
fn eval_list_sort_and_binary_search_support_user_ord() {
    let val = run_ok(
        "import collections\n\
         type Point derive(Eq, Ord) = { x: Int }\n\
         fn main() -> Int {\n\
             let a: Point = Point { x: 3 }\n\
             let b: Point = Point { x: 1 }\n\
             let c: Point = Point { x: 2 }\n\
             let xs = collections.List.new().push(a).push(b).push(c).sort()\n\
             let needle: Point = Point { x: 2 }\n\
             xs[0].x * 100 + xs[1].x * 10 + xs.binary_search(needle)\n\
         }",
    );
    assert_eq!(val, Value::Int(121));
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
fn eval_function_wrong_arity_is_rejected_at_compile_time() {
    let err = run_err(
        "fn add(x: Int, y: Int) -> Int { x + y }
         fn main() -> Int { add(1) }",
    );
    assert_eq!(
        err,
        "type error at runtime: compile type errors: expected 2 argument(s), found 1"
    );
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

#[test]
fn eval_newline_parenthesized_range_after_let() {
    let val = run_ok(
        "fn main() -> Int {
           (0..<1).fold(0, fn(acc: Int, i: Int) => {
             let base = i
             ((i + 1)..<4).fold(acc, fn(a: Int, j: Int) => a + j + base)
           })
         }",
    );
    assert!(matches!(val, Value::Int(6)));
}

// ── If/else tests ────────────────────────────────────────────────────

#[test]
fn eval_if_true() {
    let val = run_ok("fn main() -> Int { if (true) { 1 } else { 2 } }");
    assert!(matches!(val, Value::Int(1)));
}

#[test]
fn eval_if_false() {
    let val = run_ok("fn main() -> Int { if (false) { 1 } else { 2 } }");
    assert!(matches!(val, Value::Int(2)));
}

// ── Loop tests ───────────────────────────────────────────────────────

#[test]
fn eval_while_loop_breaks_correctly() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let box = collections.MutableList.new().push(0)
           while (box[0] < 5) {
             box.set(0, box[0] + 1)
           }
           box[0]
         }",
    );
    assert!(matches!(val, Value::Int(5)));
}

#[test]
fn eval_var_reassignment_in_loop_accumulates_state() {
    let val = run_ok(
        "fn main() -> Int {
           var acc = 0
           var cur = 1
           while (cur <= 4) {
             acc = acc + cur
             cur = cur + 1
           }
           acc
         }",
    );
    assert!(matches!(val, Value::Int(10)));
}

#[test]
fn eval_typed_var_reassignment_works() {
    let val = run_ok(
        "fn main() -> Int {
           var acc: Int = 1
           acc = acc + 4
           acc
         }",
    );
    assert!(matches!(val, Value::Int(5)));
}

#[test]
fn eval_for_loop_over_range_with_continue_and_break() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let acc = collections.MutableList.new().push(0)
           for (x in 0..<10) {
             if (x == 7) { break }
             if ((x % 2) == 0) { continue }
             acc.set(0, acc[0] + x)
           }
           acc[0]
         }",
    );
    assert!(matches!(val, Value::Int(9)));
}

#[test]
fn eval_return_inside_loop_exits_function() {
    let val = run_ok("fn main() -> Int { while (true) { return 7 }\n 0 }");
    assert!(matches!(val, Value::Int(7)));
}

#[test]
fn eval_for_source_is_evaluated_once() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let counter = collections.MutableList.new().push(0)
           for (x in { counter.set(0, counter[0] + 1)\n 0..<3 }) { x }
           counter[0]
         }",
    );
    assert!(matches!(val, Value::Int(1)));
}

#[test]
fn eval_break_continue_outside_loop_are_compile_errors() {
    assert!(check_has_compile_errors("fn main() { break }"));
    assert!(check_has_compile_errors("fn main() { continue }"));
}

#[test]
fn eval_if_with_comparison() {
    let val = run_ok(
        "fn main() -> Int {
           let x = 5
           if (x > 3) { 100 } else { 0 }
         }",
    );
    assert!(matches!(val, Value::Int(100)));
}

#[test]
fn eval_else_if_true_path() {
    let val = run_ok("fn main() -> Int { if (true) { 1 } else if (true) { 2 } else { 3 } }");
    assert!(matches!(val, Value::Int(1)));
}

#[test]
fn eval_else_if_middle_path() {
    let val = run_ok("fn main() -> Int { if (false) { 1 } else if (true) { 2 } else { 3 } }");
    assert!(matches!(val, Value::Int(2)));
}

#[test]
fn eval_else_if_matches_nested_form() {
    let else_if = run_ok("fn main() -> Int { if (false) { 1 } else if (true) { 2 } else { 3 } }");
    let nested =
        run_ok("fn main() -> Int { if (false) { 1 } else { if (true) { 2 } else { 3 } } }");
    assert_eq!(else_if, nested);
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
fn eval_equality_on_list_is_compile_error() {
    let err = run_err("fn main() -> Bool { collections.List.new() == collections.List.new() }");
    assert!(err.contains("equality operator requires"), "got: {err}");
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

// ── Char equality tests ──────────────────────────────────────────────

#[test]
fn eval_char_eq_same() {
    let val = run_ok("fn main() -> Bool { 'a' == 'a' }");
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_char_eq_different() {
    let val = run_ok("fn main() -> Bool { 'a' == 'b' }");
    assert!(matches!(val, Value::Bool(false)));
}

#[test]
fn eval_char_neq() {
    let val = run_ok("fn main() -> Bool { 'a' != 'b' }");
    assert!(matches!(val, Value::Bool(true)));
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
        "type Color = Red | Green | Blue
         fn main() -> Color { Red }",
    );
    assert!(matches!(val, Value::Adt { variant: 0, .. }));
}

#[test]
fn eval_adt_constructor_call() {
    let val = run_ok(
        "type Maybe<T> = Just(T) | Nothing
         fn main() -> Maybe<Int> { Just(42) }",
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
        "type Bool2 = True | False
         fn to_int(x: Bool2) -> Int {
           match (x) {
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
        "type Maybe<T> = Just(T) | Nothing
         fn unwrap(x: Maybe<Int>) -> Int {
           match (x) {
             Just(v) => v
             Nothing => 0
           }
         }
         fn main() -> Int { unwrap(Just(42)) }",
    );
    assert!(matches!(val, Value::Int(42)));
}

#[test]
fn eval_pattern_match_wildcard() {
    let val = run_ok(
        "type Color = Red | Green | Blue
         fn is_red(c: Color) -> Int {
           match (c) {
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

#[test]
fn eval_record_literal_not_confused_with_adt_constructor() {
    // Regression test for issue #127: when a record type alias and an ADT
    // constructor share the same name, `Point { x: 1 }` must produce a
    // record value (not an ADT), and field access on it must work.
    let val = run_ok(
        "type Point = { x: Int }
         type Wrap = Point(Int)
         fn main() -> Int {
           let p = Point { x: 1 }
           p.x
         }",
    );
    assert!(matches!(val, Value::Int(1)));
}

// ── Recursion tests ──────────────────────────────────────────────────

#[test]
fn eval_factorial() {
    let val = run_ok(
        "fn fact(n: Int) -> Int {
           if (n <= 1) { 1 } else { n * fact(n - 1) }
         }
         fn main() -> Int { fact(5) }",
    );
    assert!(matches!(val, Value::Int(120)));
}

#[test]
fn eval_fibonacci() {
    let val = run_ok(
        "fn fib(n: Int) -> Int {
           if (n < 2) { n } else { fib(n - 1) + fib(n - 2) }
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

#[test]
fn eval_lambda_shadowing_and_nested_capture() {
    let val = run_ok(
        "fn make(x: Int) -> fn(Int) -> Int {
           let y = x + 1
           fn(x: Int) => {
             let z = y + x
             z
           }
         }
         fn main() -> Int { make(10)(5) }",
    );
    assert!(matches!(val, Value::Int(16)));
}

// ── String intrinsic tests ───────────────────────────────────────────

#[test]
fn eval_int_to_string() {
    let val = run_ok("fn main() -> String { 42.to_string() }");
    match val {
        Value::String(s) => assert_eq!(s, "42"),
        other => panic!("expected String, got {other:?}"),
    }
}

#[test]
fn eval_string_concat() {
    let val = run_ok(r#"fn main() -> String { "foo".concat("bar") }"#);
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
        "type Maybe<T> = Just(T) | Nothing

         fn unwrap_or(opt: Maybe<Int>, default: Int) -> Int {
           match (opt) {
             Just(x) => x
             Nothing => default
           }
         }

         fn main() -> Int {
           let x = Just(42)
           let y = Nothing
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
           match (Some(42)) {
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
           match (None) {
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
           match (Ok(1)) {
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
           match (Err(99)) {
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
           contract
           requires (x > 0)
         { x }
         fn main() -> Int { check(5) }",
    );
    assert!(matches!(val, Value::Int(5)));
}

#[test]
fn eval_requires_fails() {
    let err = run_err(
        "fn check(x: Int) -> Int
           contract
           requires (x > 0)
         { x }
         fn main() -> Int { check(-1) }",
    );
    assert!(err.contains("precondition failed"));
}

#[test]
fn eval_ensures_passes() {
    let val = run_ok(
        "fn get() -> Int
           contract
           ensures (result > 0)
         { 42 }
         fn main() -> Int { get() }",
    );
    assert!(matches!(val, Value::Int(42)));
}

#[test]
fn eval_ensures_fails() {
    let err = run_err(
        "fn get() -> Int
           contract
           ensures (result > 100)
         { 42 }
         fn main() -> Int { get() }",
    );
    assert!(err.contains("postcondition failed"));
}

#[test]
fn eval_ensures_result_binding() {
    let val = run_ok(
        "fn ten() -> Int
           contract
           ensures (result == 10)
         { 10 }
         fn main() -> Int { ten() }",
    );
    assert!(matches!(val, Value::Int(10)));
}

#[test]
fn eval_old_in_ensures() {
    let val = run_ok(
        "fn inc(x: Int) -> Int
           contract
           ensures (result == old(x) + 1)
         { x + 1 }
         fn main() -> Int { inc(5) }",
    );
    assert!(matches!(val, Value::Int(6)));
}

#[test]
fn eval_old_in_ensures_fails() {
    let err = run_err(
        "fn inc(x: Int) -> Int
           contract
           ensures (result == old(x))
         { x + 1 }
         fn main() -> Int { inc(5) }",
    );
    assert!(err.contains("postcondition failed"));
}

#[test]
fn eval_invariant_passes() {
    let val = run_ok(
        "fn check(x: Int) -> Int
           contract
           invariant (x > 0)
         { x }
         fn main() -> Int { check(5) }",
    );
    assert!(matches!(val, Value::Int(5)));
}

#[test]
fn eval_invariant_fails() {
    let err = run_err(
        "fn check(x: Int) -> Int
           contract
           invariant (x > 100)
         { x }
         fn main() -> Int { check(5) }",
    );
    assert!(err.contains("invariant violated"));
}

#[test]
fn eval_requires_and_ensures_combined() {
    let val = run_ok(
        "fn safe_inc(x: Int) -> Int
           contract
           requires (x > 0)
           ensures (result > x)
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
           contract
           requires (x > 0)
         { x }
         fn main() -> Int { positive(0) }",
    );
    assert!(err.contains("precondition failed"));
}

#[test]
fn eval_requires_fails_with_equality_check() {
    let err = run_err(
        "fn expect_ten(x: Int) -> Int
           contract
           requires (x == 10)
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
           contract
           requires (b > 0)
         { a / b }
         fn main() -> Int { safe_div(10, 0) }",
    );
    assert!(err.contains("precondition failed"));
}

#[test]
fn eval_requires_fails_negative_bound() {
    let err = run_err(
        "fn clamp_low(x: Int) -> Int
           contract
           requires (x >= 0)
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
           contract
           requires (x > 0)
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
           contract
           ensures (result > 0)
         { 0 }
         fn main() -> Int { bad() }",
    );
    assert!(err.contains("postcondition failed"));
}

#[test]
fn eval_ensures_fails_negative_return() {
    let err = run_err(
        "fn negate(x: Int) -> Int
           contract
           ensures (result >= 0)
         { 0 - x }
         fn main() -> Int { negate(5) }",
    );
    assert!(err.contains("postcondition failed"));
}

#[test]
fn eval_ensures_fails_equality_mismatch() {
    let err = run_err(
        "fn double(x: Int) -> Int
           contract
           ensures (result == x + x)
         { x * 3 }
         fn main() -> Int { double(4) }",
    );
    assert!(err.contains("postcondition failed"));
}

#[test]
fn eval_ensures_passes_with_computation() {
    let val = run_ok(
        "fn double(x: Int) -> Int
           contract
           ensures (result == x + x)
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
           contract
           invariant (x > 10)
         { x }
         fn main() -> Int { process(5) }",
    );
    assert!(err.contains("invariant violated"));
}

#[test]
fn eval_invariant_fails_at_zero() {
    let err = run_err(
        "fn nonzero(x: Int) -> Int
           contract
           invariant (x != 0)
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
           contract
           ensures (result == old(x) + 5)
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
           contract
           ensures (result == old(x) + 1)
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
           contract
           requires (x > 0)
           ensures (result > 0)
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
           contract
           requires (x > 0)
           ensures (result > x)
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
           contract
           requires (x > 0)
           ensures (result == old(x) + 1)
           invariant (x > 0)
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
           contract
           requires (x > 0)
           ensures (result > 0)
           invariant (x > 100)
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
           contract
           requires (n >= 0)
           ensures (result >= 1)
         {
           if (n <= 1) { 1 } else { n * fact(n - 1) }
         }
         fn main() -> Int { fact(5) }",
    );
    assert!(matches!(val, Value::Int(120)));
}

#[test]
fn eval_direct_recursion_hits_interpreter_recursion_limit() {
    let err = run_err_with_call_depth_limit(
        "fn dive(n: Int) -> Int {
           if (n == 0) { 0 } else { dive(n - 1) }
         }
         fn main() -> Int { dive(32) }",
        16,
    );
    assert!(
        err.contains("recursion limit exceeded"),
        "unexpected error: {err}"
    );
}

#[test]
fn eval_finite_recursion_below_limit_still_succeeds() {
    let val = run_ok_with_call_depth_limit(
        "fn dive(n: Int) -> Int {
           if (n == 0) { 0 } else { dive(n - 1) }
         }
         fn main() -> Int { dive(8) }",
        16,
    );
    assert!(matches!(val, Value::Int(0)));
}

#[test]
fn eval_mutual_recursion_hits_interpreter_recursion_limit() {
    let err = run_err_with_call_depth_limit(
        "fn ping(n: Int) -> Int {
           if (n == 0) { 0 } else { pong(n - 1) }
         }
         fn pong(n: Int) -> Int {
           if (n == 0) { 0 } else { ping(n - 1) }
         }
         fn main() -> Int { ping(32) }",
        16,
    );
    assert!(
        err.contains("recursion limit exceeded"),
        "unexpected error: {err}"
    );
}

#[test]
fn eval_contract_on_called_fn_not_main() {
    // Contract on a helper, main has none.
    let err = run_err(
        "fn helper(x: Int) -> Int
           contract
           requires (x > 0)
         { x }
         fn main() -> Int { helper(-1) }",
    );
    assert!(err.contains("precondition failed"));
}

#[test]
fn eval_ensures_with_bool_return() {
    let val = run_ok(
        "fn is_positive(x: Int) -> Bool
           contract
           ensures (result == true)
         { x > 0 }
         fn main() -> Bool { is_positive(5) }",
    );
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_ensures_with_bool_return_fails() {
    let err = run_err(
        "fn is_positive(x: Int) -> Bool
           contract
           ensures (result == true)
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
           contract
           requires (x > 0)
         { x }
         fn main() -> Int { bounded(-10) }",
    );
    assert!(err.contains("precondition failed"));
}

#[test]
fn eval_ensures_result_is_zero() {
    let val = run_ok(
        "fn zero() -> Int
           contract
           ensures (result == 0)
         { 0 }
         fn main() -> Int { zero() }",
    );
    assert!(matches!(val, Value::Int(0)));
}

#[test]
fn eval_ensures_result_is_zero_fails() {
    let err = run_err(
        "fn not_zero() -> Int
           contract
           ensures (result == 0)
         { 1 }
         fn main() -> Int { not_zero() }",
    );
    assert!(err.contains("postcondition failed"));
}

#[test]
fn eval_old_with_multiple_params() {
    let val = run_ok(
        "fn sum_inc(a: Int, b: Int) -> Int
           contract
           ensures (result == old(a) + old(b) + 1)
         { a + b + 1 }
         fn main() -> Int { sum_inc(3, 4) }",
    );
    assert!(matches!(val, Value::Int(8)));
}

#[test]
fn eval_old_with_multiple_params_fails() {
    let err = run_err(
        "fn sum_inc(a: Int, b: Int) -> Int
           contract
           ensures (result == old(a) + old(b))
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
           contract
           requires (x > 100)
         { x }
         fn main() -> Int { my_special_fn(1) }",
    );
    assert!(err.contains("my_special_fn"));
}

#[test]
fn eval_precondition_error_message_is_stable() {
    let err = run_err(
        "fn my_special_fn(x: Int) -> Int
           contract
           requires (x > 100)
         { x }
         fn main() -> Int { my_special_fn(1) }",
    );
    assert_eq!(err, "precondition failed: my_special_fn");
}

#[test]
fn eval_postcondition_error_names_function() {
    let err = run_err(
        "fn another_fn() -> Int
           contract
           ensures (result > 999)
         { 1 }
         fn main() -> Int { another_fn() }",
    );
    assert!(err.contains("another_fn"));
}

#[test]
fn eval_postcondition_error_message_is_stable() {
    let err = run_err(
        "fn another_fn() -> Int
           contract
           ensures (result > 999)
         { 1 }
         fn main() -> Int { another_fn() }",
    );
    assert_eq!(err, "postcondition failed: another_fn");
}

#[test]
fn eval_invariant_error_names_function() {
    let err = run_err(
        "fn inv_fn(x: Int) -> Int
           contract
           invariant (x > 999)
         { x }
         fn main() -> Int { inv_fn(1) }",
    );
    assert!(err.contains("inv_fn"));
}

#[test]
fn eval_invariant_error_message_is_stable() {
    let err = run_err(
        "fn inv_fn(x: Int) -> Int
           contract
           invariant (x > 999)
         { x }
         fn main() -> Int { inv_fn(1) }",
    );
    assert_eq!(err, "invariant violated: inv_fn");
}

// ── Reserved core constructor names ──────────────────────────────────

#[test]
fn eval_user_option_with_reserved_constructors_is_compile_error() {
    let err = run_err(
        "type Option<T> = Some(T) | None
         fn main() -> Int {
           match (Some(7)) {
             Some(x) => x
             None => 0
           }
         }",
    );
    assert!(
        err.contains("constructor `Some` is reserved for core stdlib"),
        "unexpected error: {err}"
    );
}

// ── List tests ──────────────────────────────────────────────────────

#[test]
fn eval_list_new_and_push() {
    let val = run_ok(
        "fn main() -> Int {
           let xs = collections.List.new()
           let ys = xs.push(1)
           let zs = ys.push(2)
           zs.len()
         }",
    );
    assert!(matches!(val, Value::Int(2)));
}

#[test]
fn eval_list_len_empty() {
    let val = run_ok(
        "fn main() -> Int {
           collections.List.new().len()
         }",
    );
    assert!(matches!(val, Value::Int(0)));
}

#[test]
fn eval_list_get_some() {
    let val = run_ok(
        "fn main() -> Int {
           let xs = collections.List.new().push(10).push(20)
           match (xs.get(1)) {
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
           let xs = collections.List.new().push(10)
           match (xs.get(5)) {
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
           let xs = collections.List.new().push(10).push(20)
           match (xs.head()) {
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
           match (collections.List.new().head()) {
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
           let xs = collections.List.new().push(1).push(2).push(3)
           xs.tail().len()
         }",
    );
    assert!(matches!(val, Value::Int(2)));
}

#[test]
fn eval_list_tail_empty() {
    let val = run_ok(
        "fn main() -> Int {
           collections.List.new().tail().len()
         }",
    );
    assert!(matches!(val, Value::Int(0)));
}

#[test]
fn eval_list_is_empty() {
    let val = run_ok(
        "fn main() -> Bool {
           collections.List.new().is_empty()
         }",
    );
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_list_is_empty_false() {
    let val = run_ok(
        "fn main() -> Bool {
           collections.List.new().push(1).is_empty()
         }",
    );
    assert!(matches!(val, Value::Bool(false)));
}

#[test]
fn eval_list_reverse() {
    let val = run_ok(
        "fn main() -> Int {
           let xs = collections.List.new().push(1).push(2).push(3)
           let rev = xs.reverse()
           match (rev.head()) {
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
           let a = collections.List.new().push(1).push(2)
           let b = collections.List.new().push(3).push(4)
           a.concat(b).len()
         }",
    );
    assert!(matches!(val, Value::Int(4)));
}

#[test]
fn eval_list_map_lambda() {
    let val = run_ok(
        "fn main() -> Int {
           let xs = collections.List.new().push(1).push(2).push(3)
           let ys = xs.map(fn(x: Int) => x * 2).to_list()
           match (ys.get(2)) {
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
           let xs = collections.List.new().push(5).push(10)
           let ys = xs.map(double).to_list()
           match (ys.head()) {
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
           let xs = collections.List.new().push(1).push(2).push(3).push(4)
           xs.filter(fn(x: Int) => x > 2).count()
         }",
    );
    assert!(matches!(val, Value::Int(2)));
}

#[test]
fn eval_list_fold_sum() {
    let val = run_ok(
        "fn main() -> Int {
           let xs = collections.List.new().push(1).push(2).push(3)
           xs.fold(0, fn(acc: Int, x: Int) => acc + x)
         }",
    );
    assert!(matches!(val, Value::Int(6)));
}

#[test]
fn eval_list_set_basic() {
    let val = run_ok(
        "fn main() -> Int {
           let xs = collections.List.new().push(10).push(20).set(1, 99)
           xs.get(1).unwrap_or(0)
         }",
    );
    assert!(matches!(val, Value::Int(99)));
}

#[test]
fn eval_list_update_basic() {
    let val = run_ok(
        "fn main() -> Int {
           let xs = collections.List.new().push(10).push(20)
           let ys = xs.update(0, fn(n: Int) => n + 5)
           ys.get(0).unwrap_or(0)
         }",
    );
    assert!(matches!(val, Value::Int(15)));
}

#[test]
fn eval_list_set_out_of_bounds_runtime_error() {
    let err = run_err(
        "fn main() -> Int {
           collections.List.new().push(10).set(9, 0).len()
         }",
    );
    assert!(err.contains("index out of bounds"), "got: {err}");
}

#[test]
fn eval_list_update_out_of_bounds_runtime_error() {
    let err = run_err(
        "fn main() -> Int {
           collections.List.new().push(10).update(3, fn(n: Int) => n + 1).len()
         }",
    );
    assert!(err.contains("index out of bounds"), "got: {err}");
}

#[test]
fn eval_list_set_negative_index_runtime_error() {
    let err = run_err(
        "fn main() -> Int {
           collections.List.new().push(10).set(0 - 1, 0).len()
         }",
    );
    assert!(err.contains("index out of bounds"), "got: {err}");
}

// ── Deque tests ─────────────────────────────────────────────────────

#[test]
fn eval_deque_push_front_back_and_pop_front_fifo() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let q0 = collections.Deque.new().push_back(1).push_back(2).push_front(0)
           match (q0.pop_front()) {
             Some(p1) => match (p1.rest.pop_front()) {
               Some(p2) => p1.value * 100 + p2.value * 10 + p2.rest.len()
               None => -1
             }
             None => -1
           }
         }",
    );
    assert!(matches!(val, Value::Int(11)));
}

#[test]
fn eval_deque_pop_front_non_empty_returns_value_and_rest() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let q0 = collections.Deque.new().push_back(10).push_back(20)
           match (q0.pop_front()) {
             Some(p) =>
               if (p.value == 10 && p.rest.len() == 1) {
                 match (p.rest.pop_front()) {
                   Some(p2) => p2.value
                   None => -1
                 }
               } else {
                 -1
               }
             None => -1
           }
         }",
    );
    assert!(matches!(val, Value::Int(20)));
}

#[test]
fn eval_deque_pop_front_empty_returns_none() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           match (collections.Deque.new().pop_front()) {
             Some(_p) => 0
             None => 1
           }
         }",
    );
    assert!(matches!(val, Value::Int(1)));
}

#[test]
fn eval_deque_push_front_back_and_pop_back_lifo() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let q0 = collections.Deque.new().push_back(1).push_back(2).push_front(0)
           match (q0.pop_back()) {
             Some(p1) => match (p1.rest.pop_back()) {
               Some(p2) => p1.value * 100 + p2.value * 10 + p2.rest.len()
               None => -1
             }
             None => -1
           }
         }",
    );
    assert!(matches!(val, Value::Int(211)));
}

#[test]
fn eval_deque_pop_back_non_empty_returns_value_and_rest() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let q0 = collections.Deque.new().push_back(10).push_back(20)
           match (q0.pop_back()) {
             Some(p) =>
               if (p.value == 20 && p.rest.len() == 1) {
                 match (p.rest.pop_back()) {
                   Some(p2) => p2.value
                   None => -1
                 }
               } else {
                 -1
               }
             None => -1
           }
         }",
    );
    assert!(matches!(val, Value::Int(10)));
}

#[test]
fn eval_deque_pop_back_empty_returns_none() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           match (collections.Deque.new().pop_back()) {
             Some(_p) => 0
             None => 1
           }
         }",
    );
    assert!(matches!(val, Value::Int(1)));
}

#[test]
fn eval_deque_empty_pop_operations_are_stable() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let q0 = collections.Deque.new()
           let q1 = match (q0.pop_front()) {
             Some(p) => p.rest
             None => q0
           }
           let q2 = match (q1.pop_back()) {
             Some(p) => p.rest
             None => q1
           }
           if (q2.is_empty() && q2.len() == 0) { 1 } else { 0 }
         }",
    );
    assert!(matches!(val, Value::Int(1)));
}

#[test]
fn eval_deque_pop_back_singleton_rest_is_empty() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let q0 = collections.Deque.new().push_back(7)
           match (q0.pop_back()) {
             Some(p) =>
               if (p.value == 7 && p.rest.is_empty() && p.rest.len() == 0) {
                 1
               } else {
                 0
               }
             None => 0
           }
         }",
    );
    assert!(matches!(val, Value::Int(1)));
}

#[test]
fn eval_deque_is_empty_and_len() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let q0: Deque<Int> = collections.Deque.new()
           let q1 = q0.push_back(42)
           if (q0.is_empty() && q1.is_empty() == false && q1.len() == 1) { 1 } else { 0 }
         }",
    );
    assert!(matches!(val, Value::Int(1)));
}

#[test]
fn eval_collections_deque_constructor_surface_rfc_0004() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let q0 = collections.Deque.new().push_back(1).push_back(2).push_front(0)
           match (q0.pop_front()) {
             Some(p1) => match (p1.rest.pop_front()) {
               Some(p2) => p1.value * 100 + p2.value * 10 + p2.rest.len()
               None => -1
             }
             None => -1
           }
         }",
    );
    assert!(matches!(val, Value::Int(11)));
}

#[test]
fn eval_collections_alias_constructor_surface_rfc_0004() {
    let val = run_ok(
        "import collections as c
         fn main() -> Int {
           c.Deque.new().push_back(1).push_back(2).len()
         }",
    );
    assert!(matches!(val, Value::Int(2)));
}

#[test]
fn eval_global_deque_constructor_surface_is_removed_rfc_0004() {
    let err = run_err("fn main() -> Int { Deque.new().len() }");
    assert!(
        err.contains("no method `new`")
            || err.contains("unresolved name")
            || err.contains("import name"),
        "unexpected error: {err}"
    );
}

#[test]
fn eval_collections_list_map_set_constructor_surface_rfc_0009() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let xs = collections.List.new().push(1).push(2)
           let m = collections.Map.new().insert(\"a\", 10).insert(\"b\", 20)
           let s = collections.Set.new().insert(\"x\").insert(\"y\")
           xs.len() * 100 + m.len() * 10 + s.len()
         }",
    );
    assert!(matches!(val, Value::Int(222)));
}

#[test]
fn eval_collections_alias_list_map_set_constructor_surface_rfc_0009() {
    let val = run_ok(
        "import collections as c
         fn main() -> Int {
           let xs: List<Int> = c.List.new().push(7)
           let m: Map<String, Int> = c.Map.new().insert(\"k\", 3)
           let s: Set<String> = c.Set.new().insert(\"k\")
           xs.head().unwrap_or(0) + m.get(\"k\").unwrap_or(0) + if (s.contains(\"k\")) { 1 } else { 0 }
         }",
    );
    assert!(matches!(val, Value::Int(11)));
}

#[test]
fn eval_global_immutable_list_map_set_constructor_surface_is_removed_rfc_0009() {
    for src in [
        "fn main() -> Int { List.new().len() }",
        "fn main() -> Int { Map.new().len() }",
        "fn main() -> Int { Set.new().len() }",
    ] {
        let err = run_err(src);
        assert!(
            err.contains("no method `new`")
                || err.contains("unresolved name")
                || err.contains("import name"),
            "unexpected error for `{src}`: {err}"
        );
    }
}

#[test]
fn eval_effect_module_alias_call_works_rfc_0004() {
    let val = run_ok(
        "import io as i
         fn main() -> Int
         {
           i.println(\"ok\")
           1
         }",
    );
    assert!(matches!(val, Value::Int(1)));
}

#[test]
fn eval_collections_mutable_list_constructor_surface_rfc_0005() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let xs = collections.MutableList.new().push(1).push(2)
           let ys = collections.MutableList.from_list(collections.List.new().push(3).push(4))
           xs.len() * 10 + ys.get(1).unwrap_or(0)
         }",
    );
    assert!(matches!(val, Value::Int(24)));
}

#[test]
fn eval_collections_mutable_list_alias_constructor_surface_rfc_0005() {
    let val = run_ok(
        "import collections as c
         fn main() -> Int {
           let xs: MutableList<Int> = c.MutableList.new().push(7).push(8)
           xs.len()
         }",
    );
    assert!(matches!(val, Value::Int(2)));
}

#[test]
fn eval_global_mutable_list_constructor_surface_is_removed_rfc_0005() {
    let err = run_err("fn main() -> Int { MutableList.new().len() }");
    assert!(
        err.contains("no method `new`")
            || err.contains("unresolved name")
            || err.contains("import name"),
        "unexpected error: {err}"
    );
}

#[test]
fn eval_collections_mutable_map_constructor_surface_rfc_0008() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let m = collections.MutableMap.new().insert(\"a\", 1).insert(\"b\", 2)
           m.len()
         }",
    );
    assert!(matches!(val, Value::Int(2)));
}

#[test]
fn eval_collections_mutable_map_alias_constructor_surface_rfc_0008() {
    let val = run_ok(
        "import collections as c
         fn main() -> Int {
           let m: MutableMap<String, Int> = c.MutableMap.new().insert(\"x\", 5)
           match (m.get(\"x\")) {
             Some(v) => v
             None => 0
           }
         }",
    );
    assert!(matches!(val, Value::Int(5)));
}

#[test]
fn eval_collections_mutable_map_with_capacity_surface() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let m = collections.MutableMap.with_capacity(8).insert(\"a\", 1).insert(\"b\", 2)
           m.get(\"b\").unwrap_or(0) * 10 + m.len()
         }",
    );
    assert!(matches!(val, Value::Int(22)));
}

#[test]
fn eval_collections_mutable_map_alias_with_capacity_surface() {
    let val = run_ok(
        "import collections as c
         fn main() -> Int {
           let m: MutableMap<String, Int> = c.MutableMap.with_capacity(4).insert(\"x\", 5)
           match (m.get(\"x\")) {
             Some(v) => v
             None => 0
           }
         }",
    );
    assert!(matches!(val, Value::Int(5)));
}

#[test]
fn eval_global_mutable_map_constructor_surface_is_removed_rfc_0008() {
    let err = run_err("fn main() -> Int { MutableMap.new().len() }");
    assert!(
        err.contains("no method `new`")
            || err.contains("unresolved name")
            || err.contains("import name"),
        "unexpected error: {err}"
    );
}

#[test]
fn eval_global_mutable_map_with_capacity_surface_is_removed() {
    let err = run_err("fn main() -> Int { MutableMap.with_capacity(4).len() }");
    assert!(
        err.contains("no method `with_capacity`")
            || err.contains("unresolved name")
            || err.contains("import name"),
        "unexpected error: {err}"
    );
}

#[test]
fn eval_mutable_map_with_capacity_negative_capacity_is_runtime_error() {
    let err = run_err(
        "import collections
         fn main() -> Int {
           collections.MutableMap.with_capacity(-1).len()
         }",
    );
    assert!(
        err.contains("mutable_map_with_capacity: capacity must be >= 0"),
        "unexpected error: {err}"
    );
}

#[test]
fn eval_mutable_map_with_capacity_zero_capacity_succeeds() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let m: MutableMap<Int, Int> = collections.MutableMap.with_capacity(0)
           if (m.is_empty()) { 1 } else { 0 }
         }",
    );
    assert!(matches!(val, Value::Int(1)));
}

#[test]
fn eval_mutable_map_aliases_observe_in_place_mutation() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let m = collections.MutableMap.new().insert(\"a\", 1).insert(\"b\", 2)
           let alias = m
           m.insert(\"a\", 9)
           alias.get(\"a\").unwrap_or(0) * 10 + alias.len()
         }",
    );
    assert!(matches!(val, Value::Int(92)));
}

#[test]
fn eval_mutable_map_key_order_is_deterministic() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let m = collections.MutableMap.new()
             .insert(\"b\", 1)
             .insert(\"a\", 2)
             .remove(\"b\")
             .insert(\"b\", 3)
           let ks = m.keys().to_list()
           if (ks[0] == \"a\" && ks[1] == \"b\") { 1 } else { 0 }
         }",
    );
    assert!(matches!(val, Value::Int(1)));
}

#[test]
fn eval_mutable_map_updates_existing_key_without_moving_order() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let m = collections.MutableMap.with_capacity(4)
             .insert(\"a\", 1)
             .insert(\"b\", 2)
             .insert(\"a\", 9)
           let ks = m.keys().to_list()
           if (ks[0] == \"a\" && ks[1] == \"b\") { m.get(\"a\").unwrap_or(0) } else { 0 }
         }",
    );
    assert!(matches!(val, Value::Int(9)));
}

#[test]
fn eval_mutable_map_tombstone_churn_preserves_lookups() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let m = collections.MutableMap.with_capacity(1)
           let _m1 = m.insert(1, 10)
           let _m2 = m.insert(2, 20)
           let _m3 = m.insert(3, 30)
           let _m4 = m.remove(2)
           let _m5 = m.insert(4, 40)
           let _m6 = m.insert(5, 50)
           m.get(1).unwrap_or(0) + m.get(3).unwrap_or(0) + m.get(4).unwrap_or(0) + m.get(5).unwrap_or(0)
         }",
    );
    assert!(matches!(val, Value::Int(130)));
}

#[test]
fn eval_mutable_map_supports_all_primitive_key_kinds_with_fast_path() {
    let val = run_ok(
        "import collections
         fn unit_value() -> Unit {}
         fn main() -> Int {
           let ints = collections.MutableMap.with_capacity(2).insert(1, 10)
           let strings = collections.MutableMap.with_capacity(2).insert(\"x\", 20)
           let chars = collections.MutableMap.with_capacity(2).insert('z', 30)
           let bools = collections.MutableMap.with_capacity(2).insert(true, 40)
           let u = unit_value()
           let units = collections.MutableMap.with_capacity(2).insert(u, 50)
           ints.get(1).unwrap_or(0)
             + strings.get(\"x\").unwrap_or(0)
             + chars.get('z').unwrap_or(0)
             + bools.get(true).unwrap_or(0)
             + units.get(u).unwrap_or(0)
         }",
    );
    assert!(matches!(val, Value::Int(150)));
}

#[test]
fn eval_mutable_map_invalid_key_type_is_compile_error() {
    assert!(check_has_compile_errors(
        "import collections
         fn main() -> Int {
           let m = collections.MutableMap.new().insert(3.14, 1)
           m.len()
         }"
    ));
}

#[test]
fn eval_mutable_map_get_or_insert_with_skips_closure_on_hit_and_inserts_on_miss() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let calls = collections.MutableList.new().push(0)
           let memo = collections.MutableMap.new().insert(\"a\", 7)
           let first = memo.get_or_insert_with(\"a\", fn() =>
             if (true) {
               let _c = calls.set(0, calls[0] + 1)
               99
             } else {
               0
             }
           )
           let second = memo.get_or_insert_with(\"b\", fn() =>
             if (true) {
               let _c = calls.set(0, calls[0] + 1)
               11
             } else {
               0
             }
           )
           first * 100 + second * 10 + calls[0]
         }",
    );
    assert!(matches!(val, Value::Int(811)));
}

#[test]
fn eval_mutable_map_get_or_insert_with_skips_thunk_expression_evaluation_on_hit() {
    let val = run_ok(
        "import collections
         fn make(calls: MutableList<Int>) -> fn() -> Int {
           let _c = calls.set(0, calls[0] + 1)
           fn() => 99
         }
         fn main() -> Int {
           let calls = collections.MutableList.new().push(0)
           let memo = collections.MutableMap.new().insert(\"a\", 7)
           let value = memo.get_or_insert_with(\"a\", make(calls))
           value * 10 + calls[0]
         }",
    );
    assert!(matches!(val, Value::Int(70)));
}

#[test]
fn eval_mutable_map_get_or_insert_with_evaluates_thunk_expression_once_per_miss() {
    let val = run_ok(
        "import collections
         fn make(calls: MutableList<Int>) -> fn() -> Int {
           let _c = calls.set(0, calls[0] + 1)
           fn() => 7
         }
         fn main() -> Int {
           let calls = collections.MutableList.new().push(0)
           let memo = collections.MutableMap.new()
           let first = memo.get_or_insert_with(\"a\", make(calls))
           let second = memo.get_or_insert_with(\"a\", make(calls))
           first * 100 + second * 10 + calls[0]
         }",
    );
    assert!(matches!(val, Value::Int(771)));
}

#[test]
fn eval_mutable_map_get_or_insert_with_supports_recursive_memo_use() {
    let val = run_ok(
        "import collections
         fn fib(n: Int, memo: MutableMap<Int, Int>) -> Int {
           memo.get_or_insert_with(n, fn() =>
             if (n < 2) {
               n
             } else {
               fib(n - 1, memo) + fib(n - 2, memo)
             }
           )
         }
         fn main() -> Int {
           let memo = collections.MutableMap.new()
           fib(10, memo) * 100 + memo.len()
         }",
    );
    assert!(matches!(val, Value::Int(5511)));
}

#[test]
fn eval_collections_mutable_set_constructor_surface_rfc_0008() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let s = collections.MutableSet.new().insert(\"a\").insert(\"b\")
           s.len()
         }",
    );
    assert!(matches!(val, Value::Int(2)));
}

#[test]
fn eval_collections_mutable_set_alias_constructor_surface_rfc_0008() {
    let val = run_ok(
        "import collections as c
         fn main() -> Int {
           let s: MutableSet<String> = c.MutableSet.new().insert(\"x\")
           if (s.contains(\"x\")) { 1 } else { 0 }
         }",
    );
    assert!(matches!(val, Value::Int(1)));
}

#[test]
fn eval_collections_mutable_set_with_capacity_surface() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let s = collections.MutableSet.with_capacity(8).insert(\"a\").insert(\"b\")
           s.len()
         }",
    );
    assert!(matches!(val, Value::Int(2)));
}

#[test]
fn eval_collections_mutable_set_alias_with_capacity_surface() {
    let val = run_ok(
        "import collections as c
         fn main() -> Int {
           let s: MutableSet<String> = c.MutableSet.with_capacity(4).insert(\"x\")
           if (s.contains(\"x\")) { 1 } else { 0 }
         }",
    );
    assert!(matches!(val, Value::Int(1)));
}

#[test]
fn eval_global_mutable_set_constructor_surface_is_removed_rfc_0008() {
    let err = run_err("fn main() -> Int { MutableSet.new().len() }");
    assert!(
        err.contains("no method `new`")
            || err.contains("unresolved name")
            || err.contains("import name"),
        "unexpected error: {err}"
    );
}

#[test]
fn eval_global_mutable_set_with_capacity_surface_is_removed() {
    let err = run_err("fn main() -> Int { MutableSet.with_capacity(4).len() }");
    assert!(
        err.contains("no method `with_capacity`")
            || err.contains("unresolved name")
            || err.contains("import name"),
        "unexpected error: {err}"
    );
}

#[test]
fn eval_mutable_set_with_capacity_negative_capacity_is_runtime_error() {
    let err = run_err(
        "import collections
         fn main() -> Int {
           collections.MutableSet.with_capacity(-1).len()
         }",
    );
    assert!(
        err.contains("mutable_set_with_capacity: capacity must be >= 0"),
        "unexpected error: {err}"
    );
}

#[test]
fn eval_mutable_set_with_capacity_zero_capacity_succeeds() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let s: MutableSet<Int> = collections.MutableSet.with_capacity(0)
           if (s.is_empty()) { 1 } else { 0 }
         }",
    );
    assert!(matches!(val, Value::Int(1)));
}

#[test]
fn eval_mutable_set_aliases_observe_in_place_mutation() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let s = collections.MutableSet.new().insert(\"a\").insert(\"b\")
           let alias = s
           s.remove(\"a\")
           (if (alias.contains(\"a\")) { 10 } else { 0 }) + alias.len()
         }",
    );
    assert!(matches!(val, Value::Int(1)));
}

#[test]
fn eval_mutable_set_tombstone_churn_preserves_membership() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let s = collections.MutableSet.with_capacity(1)
           let _s1 = s.insert(1)
           let _s2 = s.insert(2)
           let _s3 = s.insert(3)
           let _s4 = s.remove(2)
           let _s5 = s.insert(4)
           let _s6 = s.insert(5)
           (if (s.contains(1)) { 100 } else { 0 })
             + (if (s.contains(3)) { 10 } else { 0 })
             + (if (s.contains(4)) { 1 } else { 0 })
             + (if (s.contains(5)) { 1000 } else { 0 })
         }",
    );
    assert!(matches!(val, Value::Int(1111)));
}

#[test]
fn eval_mutable_set_supports_all_primitive_key_kinds_with_fast_path() {
    let val = run_ok(
        "import collections
         fn unit_value() -> Unit {}
         fn main() -> Int {
           let ints = collections.MutableSet.with_capacity(2).insert(1)
           let strings = collections.MutableSet.with_capacity(2).insert(\"x\")
           let chars = collections.MutableSet.with_capacity(2).insert('z')
           let bools = collections.MutableSet.with_capacity(2).insert(true)
           let u = unit_value()
           let units = collections.MutableSet.with_capacity(2).insert(u)
           (if (ints.contains(1)) { 10000 } else { 0 })
             + (if (strings.contains(\"x\")) { 1000 } else { 0 })
             + (if (chars.contains('z')) { 100 } else { 0 })
             + (if (bools.contains(true)) { 10 } else { 0 })
             + (if (units.contains(u)) { 1 } else { 0 })
         }",
    );
    assert!(matches!(val, Value::Int(11111)));
}

#[test]
fn eval_mutable_set_values_order_is_deterministic() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let s = collections.MutableSet.new()
             .insert(\"b\")
             .insert(\"a\")
             .remove(\"b\")
             .insert(\"b\")
           let values = s.values().to_list()
           if (values[0] == \"a\" && values[1] == \"b\") { 1 } else { 0 }
         }",
    );
    assert!(matches!(val, Value::Int(1)));
}

#[test]
fn eval_mutable_set_invalid_element_type_is_compile_error() {
    assert!(check_has_compile_errors(
        "import collections
         fn main() -> Int {
           let s = collections.MutableSet.new().insert(3.14)
           s.len()
         }"
    ));
}

#[test]
fn eval_mutable_list_aliases_observe_in_place_mutation() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let xs = collections.MutableList.from_list(collections.List.new().push(10).push(20))
           let alias = xs
           xs.set(1, 99)
           xs.update(0, fn(n: Int) => n + 1)
           alias.get(0).unwrap_or(0) * 100 + alias.get(1).unwrap_or(0)
         }",
    );
    assert!(matches!(val, Value::Int(1199)));
}

#[test]
fn eval_mutable_list_last_pop_and_extend_semantics() {
    let val = run_ok(
        r#"import collections

fn main() -> Int {
    let xs = collections.MutableList.from_list(collections.List.new().push(10).push(20).push(30))
    let alias = xs
    let last = xs.last().unwrap_or(0)
    let popped = xs.pop().unwrap_or(0)
    let _ = xs.extend(collections.List.new().push(40).push(50))
    if (last == 30 && popped == 30 && alias.len() == 4 && alias.get(2).unwrap_or(0) == 40 && alias.get(3).unwrap_or(0) == 50) {
        1
    } else {
        0
    }
}"#,
    );
    assert_eq!(val, Value::Int(1));
}

#[test]
fn eval_mutable_list_insert_delete_at_and_remove_at_semantics() {
    let val = run_ok(
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
}"#,
    );
    assert_eq!(val, Value::Int(1));
}

#[test]
fn eval_mutable_list_remove_at_singleton_leaves_empty() {
    let val = run_ok(
        r#"import collections

fn main() -> Int {
    let xs = collections.MutableList.new().insert(0, 42)
    let removed = xs.remove_at(0)
    if (removed == 42 && xs.is_empty() && xs.len() == 0) { 1 } else { 0 }
}"#,
    );
    assert_eq!(val, Value::Int(1));
}

#[test]
fn eval_mutable_list_last_and_pop_return_none_on_empty() {
    let val = run_ok(
        r#"import collections

fn main() -> Int {
    let xs = collections.MutableList.new()
    let last_missing = match (xs.last()) {
        Some(_n) => 0
        None => 1
    }
    let pop_missing = match (xs.pop()) {
        Some(_n) => 0
        None => 1
    }
    if (last_missing == 1 && pop_missing == 1 && xs.is_empty()) { 1 } else { 0 }
}"#,
    );
    assert_eq!(val, Value::Int(1));
}

#[test]
fn eval_mutable_list_indexing_and_bounds_behavior() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let xs = collections.MutableList.from_list(collections.List.new().push(3).push(4))
           xs[1]
         }",
    );
    assert!(matches!(val, Value::Int(4)));

    let val = run_ok(
        "import collections
         fn main() -> Int {
           let xs = collections.MutableList.from_list(collections.List.new().push(3).push(4))
           match (xs.get(9)) {
             Some(_n) => 0
             None => 1
           }
         }",
    );
    assert!(matches!(val, Value::Int(1)));

    let err = run_err(
        "import collections
         fn main() -> Int {
           let xs = collections.MutableList.new()
           xs.delete_at(0).len()
         }",
    );
    assert!(err.contains("index out of bounds"), "got: {err}");

    let err = run_err(
        "import collections
         fn main() -> Int {
           let xs = collections.MutableList.new()
           xs.remove_at(0)
         }",
    );
    assert!(err.contains("index out of bounds"), "got: {err}");

    let err = run_err(
        "import collections
         fn main() -> Int {
           let xs = collections.MutableList.from_list(collections.List.new().push(3).push(4))
           xs.insert(3, 0).len()
         }",
    );
    assert!(err.contains("index out of bounds"), "got: {err}");

    let err = run_err(
        "import collections
         fn main() -> Int {
           let xs = collections.MutableList.from_list(collections.List.new().push(3).push(4))
           xs.delete_at(-1).len()
         }",
    );
    assert!(err.contains("index out of bounds"), "got: {err}");

    let err = run_err(
        "import collections
         fn main() -> Int {
           let xs = collections.MutableList.from_list(collections.List.new().push(3).push(4))
           xs.remove_at(2)
         }",
    );
    assert!(err.contains("index out of bounds"), "got: {err}");

    let err = run_err(
        "import collections
         fn main() -> Int {
           let xs = collections.MutableList.from_list(collections.List.new().push(3).push(4))
           xs.set(-1, 0).len()
         }",
    );
    assert!(err.contains("index out of bounds"), "got: {err}");

    let err = run_err(
        "import collections
         fn main() -> Int {
           let xs = collections.MutableList.from_list(collections.List.new().push(3).push(4))
           xs[2]
         }",
    );
    assert!(err.contains("index out of bounds"), "got: {err}");
}

#[test]
fn eval_mutable_list_collection_first_traversal_semantics() {
    let val = run_ok(
        r#"import collections

fn main() -> Int {
    let xs = collections.MutableList.from_list(collections.List.new().push(1).push(2).push(3))
    let count = xs.map(fn(n: Int) => n + 1).filter(fn(n: Int) => n > 2).count()
    let zip_count = xs.zip(collections.List.new().push(10).push(20)).count()
    count * 10 + zip_count
}"#,
    );
    assert_eq!(val, Value::Int(22));
}

#[test]
fn eval_mutable_list_any_all_find_short_circuit() {
    let val = run_ok(
        r#"import collections

fn main() -> Int {
    let any_hit = collections.MutableList.from_list(collections.List.new().push(1).push(0)).any(fn(n: Int) =>
        if (n == 1) { true } else { 10 / n > 0 }
    )
    let all_ok = collections.MutableList.from_list(collections.List.new().push(0).push(1)).all(fn(n: Int) =>
        if (n == 0) { false } else { 10 / (n - 1) > 0 }
    ) == false
    let found = collections.MutableList.from_list(collections.List.new().push(2).push(1)).find(fn(n: Int) =>
        if (n == 2) { true } else { 10 / (n - 1) > 0 }
    ).unwrap_or(-1)
    if (any_hit && all_ok && found == 2) { 1 } else { 0 }
}"#,
    );
    assert_eq!(val, Value::Int(1));
}

#[test]
fn eval_mutable_list_seq_snapshot_excludes_push_after_pipeline_creation() {
    let val = run_ok(
        r#"import collections

fn main() -> Int {
    let xs = collections.MutableList.from_list(collections.List.new().push(1).push(2))
    let seq = xs.map(fn(n: Int) => n)
    xs.push(99)
    seq.count()
}"#,
    );
    assert_eq!(val, Value::Int(2));
}

#[test]
fn eval_mutable_list_seq_snapshot_preserves_values_before_in_place_set() {
    let val = run_ok(
        r#"import collections

fn main() -> Int {
    let xs = collections.MutableList.from_list(collections.List.new().push(10).push(20))
    let seq = xs.map(fn(n: Int) => n)
    xs.set(0, 99)
    xs.set(1, 77)
    seq.fold(0, fn(acc: Int, n: Int) => acc * 100 + n)
}"#,
    );
    assert_eq!(val, Value::Int(1020));
}

#[test]
fn eval_mutable_list_seq_snapshot_excludes_indexed_edits_after_pipeline_creation() {
    let val = run_ok(
        r#"import collections

fn main() -> Int {
    let xs = collections.MutableList.from_list(collections.List.new().push(10).push(20).push(30))
    let seq = xs.map(fn(n: Int) => n)
    let _ = xs.insert(1, 99).delete_at(0)
    seq.fold(0, fn(acc: Int, n: Int) => acc * 100 + n)
}"#,
    );
    assert_eq!(val, Value::Int(102030));
}

#[test]
fn eval_mutable_list_loop_if_tail_does_not_consume_outer_binding() {
    let val = run_ok(
        r#"import collections

fn main() -> Int {
    let xs = collections.MutableList.new().push(0)
    for (_i in 0..<3) {
        if (true) {
            xs.push(1)
        }
    }
    xs.len()
}"#,
    );
    assert_eq!(val, Value::Int(4));
}

#[test]
fn eval_nested_scope_shadow_rebind_preserves_outer_binding() {
    let val = run_ok(
        r#"import collections

fn main() -> Int {
    let xs = collections.List.new().push(1)
    {
        let xs = xs.push(2)
        xs.len()
    }
    xs.len()
}"#,
    );
    assert_eq!(val, Value::Int(1));
}

// ── Map tests ───────────────────────────────────────────────────────

#[test]
fn eval_map_insert_and_get() {
    let val = run_ok(
        r#"fn main() -> Int {
           let m = collections.Map.new().insert("a", 1)
           match (m.get("a")) {
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
           let m = collections.Map.new().insert("a", 1)
           match (m.get("b")) {
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
           let m = collections.Map.new().insert("key", 42)
           m.contains("key")
         }"#,
    );
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_map_remove() {
    let val = run_ok(
        r#"fn main() -> Int {
           let m = collections.Map.new().insert("a", 1).insert("b", 2)
           let m2 = m.remove("a")
           m2.len()
         }"#,
    );
    assert!(matches!(val, Value::Int(1)));
}

#[test]
fn eval_map_len() {
    let val = run_ok(
        r#"fn main() -> Int {
           let m = collections.Map.new().insert("a", 1).insert("b", 2)
           m.len()
         }"#,
    );
    assert!(matches!(val, Value::Int(2)));
}

#[test]
fn eval_map_keys() {
    let val = run_ok(
        r#"fn main() -> Int {
           let m = collections.Map.new().insert("a", 1).insert("b", 2)
           m.keys().count()
         }"#,
    );
    assert!(matches!(val, Value::Int(2)));
}

#[test]
fn eval_map_values() {
    let val = run_ok(
        r#"fn main() -> Int {
           let m = collections.Map.new().insert("a", 10).insert("b", 20)
           let vals = m.values()
           vals.fold(0, fn(acc: Int, x: Int) => acc + x)
         }"#,
    );
    assert!(matches!(val, Value::Int(30)));
}

#[test]
fn eval_map_is_empty() {
    let val = run_ok(
        "fn main() -> Bool {
           collections.Map.new().is_empty()
         }",
    );
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_map_insert_overwrite() {
    let val = run_ok(
        r#"fn main() -> Int {
           let m0 = collections.Map.new().insert("a", 1)
           let m1 = m0.insert("a", 99)
           match (m1.get("a")) {
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
    let val = run_ok(r#"fn main() -> Int { "hello".len() }"#);
    assert!(matches!(val, Value::Int(5)));
}

#[test]
fn eval_string_contains() {
    let val = run_ok(r#"fn main() -> Bool { "hello world".contains("world") }"#);
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_string_starts_with() {
    let val = run_ok(r#"fn main() -> Bool { "hello".starts_with("hel") }"#);
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_string_ends_with() {
    let val = run_ok(r#"fn main() -> Bool { "hello".ends_with("llo") }"#);
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_string_trim() {
    let val = run_ok(r#"fn main() -> Int { "  hi  ".trim().len() }"#);
    assert!(matches!(val, Value::Int(2)));
}

#[test]
fn eval_string_split() {
    let val = run_ok(
        r#"fn main() -> Int {
           let parts = "a,b,c".split(",")
           parts.count()
         }"#,
    );
    assert!(matches!(val, Value::Int(3)));
}

#[test]
fn eval_string_substring() {
    let val = run_ok(r#"fn main() -> String { "hello world".substring(0, 5) }"#);
    match val {
        Value::String(s) => assert_eq!(s, "hello"),
        other => panic!("expected String, got {other:?}"),
    }
}

#[test]
fn eval_string_to_upper() {
    let val = run_ok(r#"fn main() -> String { "hello".to_upper() }"#);
    match val {
        Value::String(s) => assert_eq!(s, "HELLO"),
        other => panic!("expected String, got {other:?}"),
    }
}

#[test]
fn eval_string_to_lower() {
    let val = run_ok(r#"fn main() -> String { "HELLO".to_lower() }"#);
    match val {
        Value::String(s) => assert_eq!(s, "hello"),
        other => panic!("expected String, got {other:?}"),
    }
}

#[test]
fn eval_char_to_string() {
    let val = run_ok("fn main() -> String { 'A'.to_string() }");
    match val {
        Value::String(s) => assert_eq!(s, "A"),
        other => panic!("expected String, got {other:?}"),
    }
}

#[test]
fn eval_char_code_ascii_and_unicode() {
    let ascii = run_ok("fn main() -> Int { 'A'.code() }");
    assert_eq!(ascii, Value::Int(65));

    let latin = run_ok("fn main() -> Int { 'é'.code() }");
    assert_eq!(latin, Value::Int(233));

    let emoji = run_ok("fn main() -> Int { '😀'.code() }");
    assert_eq!(emoji, Value::Int(128512));
}

// ── Int/Float math tests ────────────────────────────────────────────

#[test]
fn eval_abs() {
    let val = run_ok("fn main() -> Int { (-5).abs() }");
    assert!(matches!(val, Value::Int(5)));
}

#[test]
fn eval_min() {
    let val = run_ok("import math\nfn main() -> Int { math.min(3, 7) }");
    assert!(matches!(val, Value::Int(3)));
}

#[test]
fn eval_max() {
    let val = run_ok("import math\nfn main() -> Int { math.max(3, 7) }");
    assert!(matches!(val, Value::Int(7)));
}

#[test]
fn eval_gcd() {
    let val = run_ok("import math\nfn main() -> Int { math.gcd(54, 24) }");
    assert!(matches!(val, Value::Int(6)));
}

#[test]
fn eval_gcd_zero_and_negative_inputs() {
    let val = run_ok("import math\nfn main() -> Int { math.gcd(-54, 24) + math.gcd(0, 0) }");
    assert!(matches!(val, Value::Int(6)));
}

#[test]
fn eval_lcm() {
    let val = run_ok("import math\nfn main() -> Int { math.lcm(6, 8) }");
    assert!(matches!(val, Value::Int(24)));
}

#[test]
fn eval_lcm_zero_and_negative_inputs() {
    let val = run_ok("import math\nfn main() -> Int { math.lcm(0, 5) + math.lcm(-3, 4) }");
    assert!(matches!(val, Value::Int(12)));
}

#[test]
fn eval_lcm_overflow_is_runtime_error() {
    let err = run_err("import math\nfn main() -> Int { math.lcm(9223372036854775807, 2) }");
    assert!(err.contains("integer overflow"), "got: {err}");
}

// ── import enforcement tests ────────────────────────────────────────
// Synthetic modules require explicit `import` even in single-file mode.

#[test]
fn eval_io_without_import_fails() {
    let err = run_err(r#"fn main() -> Unit { io.println("hi") }"#);
    assert!(
        err.contains("unresolved name"),
        "expected unresolved name error, got: {err}"
    );
}

#[test]
fn eval_math_without_import_fails() {
    let err = run_err("fn main() -> Int { math.min(1, 2) }");
    assert!(
        err.contains("unresolved name"),
        "expected unresolved name error, got: {err}"
    );
}

#[test]
fn eval_fs_without_import_fails() {
    let err = run_err(r#"fn main() -> String { fs.read_file("x") }"#);
    assert!(
        err.contains("unresolved name"),
        "expected unresolved name error, got: {err}"
    );
}

#[test]
fn eval_io_with_import_works() {
    let val = run_ok("import io\nfn main() -> Unit { io.println(\"ok\") }");
    assert!(matches!(val, Value::Unit));
}

#[test]
fn eval_math_with_import_works() {
    let val = run_ok("import math\nfn main() -> Int { math.min(1, 2) }");
    assert!(matches!(val, Value::Int(1)));
}

#[test]
fn eval_int_to_float() {
    let val = run_ok("fn main() -> Float { 42.to_float() }");
    match val {
        Value::Float(f) => assert!((f - 42.0).abs() < f64::EPSILON),
        other => panic!("expected Float, got {other:?}"),
    }
}

#[test]
fn eval_float_to_int() {
    let val = run_ok("fn main() -> Int { 3.7.to_int() }");
    assert!(matches!(val, Value::Int(3)));
}

#[test]
fn eval_float_abs() {
    let val = run_ok("fn main() -> Float { (0.0 - 2.5).abs() }");
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
           let xs = collections.List.new().push(1).push(2).push(3)
           let doubled = xs.map(fn(x: Int) => x * 2)
           doubled.fold(0, fn(acc: Int, x: Int) => acc + x)
         }",
    );
    assert!(matches!(val, Value::Int(12)));
}

#[test]
fn eval_map_list_interop() {
    let val = run_ok(
        r#"fn main() -> Int {
           let m = collections.Map.new().insert("x", 10).insert("y", 20)
           let keys = m.keys()
           let vals = m.values()
           keys.count() + vals.fold(0, fn(acc: Int, x: Int) => acc + x)
         }"#,
    );
    assert!(matches!(val, Value::Int(32)));
}

// ── Capability manifest enforcement ─────────────────────────────────

#[test]
fn no_manifest_print_works() {
    // No manifest = allow all (backward compat).
    let val = run_with_manifest_ok(
        r#"import io
        fn main() -> Unit {
            io.println("hello")
        }"#,
        None,
    );
    assert!(matches!(val, Value::Unit));
}

#[test]
fn manifest_with_io_print_works() {
    let manifest = manifest_from_json(r#"{"caps": {"io": {}}}"#);
    let val = run_with_manifest_ok(
        r#"import io
        fn main() -> Unit {
            io.println("hello")
        }"#,
        Some(manifest),
    );
    assert!(matches!(val, Value::Unit));
}

#[test]
fn manifest_without_io_print_denied() {
    let manifest = manifest_from_json(r#"{"caps": {"Net": {}}}"#);
    let err = run_with_manifest_err(
        r#"import io
        fn main() -> Unit {
            io.print("hello")
        }"#,
        Some(manifest),
    );
    assert!(err.contains("capability denied"));
    assert!(err.contains("io"));
}

#[test]
fn manifest_without_io_println_denied() {
    let manifest = manifest_from_json(r#"{"caps": {"Net": {}}}"#);
    let err = run_with_manifest_err(
        r#"import io
        fn main() -> Unit {
            io.println("hello")
        }"#,
        Some(manifest),
    );
    assert!(err.contains("capability denied"));
    assert!(err.contains("io"));
}

#[test]
fn manifest_with_io_pure_intrinsics_work() {
    let manifest = manifest_from_json(r#"{"caps": {"io": {}}}"#);
    let val = run_with_manifest_ok(
        r#"fn main() -> String {
            42.to_string()
        }"#,
        Some(manifest),
    );
    assert!(matches!(val, Value::String(s) if s == "42"));
}

#[test]
fn empty_manifest_denies_io() {
    let manifest = manifest_from_json(r#"{"caps": {}}"#);
    let err = run_with_manifest_err(
        r#"import io
        fn main() -> Unit {
            io.println("hello")
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
        effect Console
        fn greet(name: String) -> String with Console {
            "hi ".concat(name)
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
    let manifest = manifest_from_json(r#"{"caps": {"io": {}}}"#);
    let err = run_with_manifest_err(
        r#"
        effect Console
        fn greet(name: String) -> String with Console {
            "hi ".concat(name)
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
        effect Console
        effect Logger
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
        effect Console
        effect Logger
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
    let manifest = manifest_from_json(r#"{"caps": {"Net": {}, "io": {}}}"#);
    let val = run_with_manifest_ok(
        r#"import io
        fn main() -> Unit {
            io.println("hello")
        }"#,
        Some(manifest),
    );
    assert!(matches!(val, Value::Unit));
}

#[test]
fn capability_denied_error_message_format() {
    let manifest = manifest_from_json(r#"{"caps": {}}"#);
    let err = run_with_manifest_err(
        r#"import io
        fn main() -> Unit { io.println("x") }"#,
        Some(manifest),
    );
    // Should contain both the capability name and the function name.
    assert!(err.contains("io"));
    assert!(err.contains("Println"));
}

#[test]
fn run_with_manifest_none_allows_all() {
    let val = run_with_manifest_ok("import io\nfn main() -> Unit { io.println(\"ok\") }", None);
    assert!(matches!(val, Value::Unit));
}

#[test]
fn manifest_allow_domains_constraint_rejected_until_enforced_issue_186() {
    let manifest = manifest_from_json(r#"{"caps": {"Net": {"allow_domains": ["rates.example"]}}}"#);
    let err = run_with_manifest_err("fn main() -> Int { 1 }", Some(manifest));
    assert!(
        err.contains("allow_domains"),
        "expected field name in error, got: {err}"
    );
    assert!(
        err.contains("Net"),
        "expected capability name in error, got: {err}"
    );
}

#[test]
fn manifest_allow_tables_constraint_rejected_until_enforced_issue_186() {
    let manifest = manifest_from_json(r#"{"caps": {"Db": {"allow_tables": ["payments"]}}}"#);
    let err = run_with_manifest_err("fn main() -> Int { 1 }", Some(manifest));
    assert!(
        err.contains("allow_tables"),
        "expected field name in error, got: {err}"
    );
    assert!(
        err.contains("Db"),
        "expected capability name in error, got: {err}"
    );
}

#[test]
fn manifest_allow_keys_constraint_rejected_until_enforced_issue_186() {
    let manifest =
        manifest_from_json(r#"{"caps": {"Secrets": {"allow_keys": ["PAYMENTS_API_KEY"]}}}"#);
    let err = run_with_manifest_err("fn main() -> Int { 1 }", Some(manifest));
    assert!(
        err.contains("allow_keys"),
        "expected field name in error, got: {err}"
    );
    assert!(
        err.contains("Secrets"),
        "expected capability name in error, got: {err}"
    );
}

#[test]
fn project_manifest_allow_domains_constraint_rejected_until_enforced_issue_186() {
    let manifest = manifest_from_json(r#"{"caps": {"Net": {"allow_domains": ["rates.example"]}}}"#);
    let err = run_project_with_files_manifest_err(
        &[("main.ky", "fn main() -> Int { 1 }\n")],
        Some(manifest),
    );
    assert!(
        err.contains("allow_domains"),
        "expected field name in error, got: {err}"
    );
}

#[test]
fn project_manifest_without_fine_grained_constraints_still_runs_issue_186_guard() {
    let manifest = manifest_from_json(r#"{"caps": {"IO": {}, "Net": {}}}"#);
    let val = run_project_with_files_manifest_ok(
        &[("main.ky", "fn main() -> Int { 1 }\n")],
        Some(manifest),
    );
    assert!(matches!(val, Value::Int(1)));
}

// ── Multi-file project diagnostics ──────────────────────────────────

#[test]
fn run_project_rejects_type_error_in_imported_module() {
    let err = run_project_with_files_err(&[
        ("util.ky", "pub fn util() -> Int { true }\n"),
        ("main.ky", "import util\nfn main() -> Int { util() }\n"),
    ]);
    assert!(
        err.contains("type error"),
        "expected 'type error' in message, got: {err}"
    );
}

#[test]
fn run_project_rejects_parse_error_in_sibling_module() {
    let err = run_project_with_files_err(&[
        ("bad.ky", "pub fn bad( -> Int { 42 }\n"),
        ("main.ky", "fn main() -> Int { 42 }\n"),
    ]);
    assert!(
        err.contains("parse error"),
        "expected 'parse error' in message, got: {err}"
    );
}

#[test]
fn run_project_rejects_lowering_error_in_sibling_module() {
    let err = run_project_with_files_err(&[
        (
            "dup.ky",
            "pub fn foo() -> Int { 1 }\npub fn foo() -> Int { 2 }\n",
        ),
        ("main.ky", "fn main() -> Int { 42 }\n"),
    ]);
    assert!(
        err.contains("lowering error") || err.contains("duplicate"),
        "expected lowering/duplicate error in message, got: {err}"
    );
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
    let val =
        run_ok("fn get() -> Int contract ensures (result > 0) { 42 }\nfn main() -> Int { get() }");
    assert!(matches!(val, Value::Int(42)));
}

#[test]
fn run_ensures_result_and_user_result_coexist() {
    // Edge case: one function has ensures (with implicit `result`),
    // another function references an undefined `result` — the second
    // should still be caught at compile time.
    let src = "fn get() -> Int contract ensures (result > 0) { 42 }\nfn main() -> Int { result }";
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
    let err = run_project_with_files_err(&[
        ("bad.ky", "pub fn oops() -> Int { unknown_name }\n"),
        ("main.ky", "fn main() -> Int { 42 }\n"),
    ]);
    assert!(
        err.contains("unresolved") || err.contains("lowering"),
        "expected unresolved/lowering error in message, got: {err}"
    );
}

// ── Duplicate binding rejection (#161) ─────────────────────────────

#[test]
fn run_rejects_duplicate_binding_in_pattern() {
    // Bug test: duplicate binding in constructor pattern must be rejected.
    let src = r#"
type Pair = Pair(Int, Int)
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
type Pair = Pair(Int, Int)
fn main() -> Int {
  let Pair(a, b) = Pair(1, 2)
  a + b
}
"#;
    let result = run_ok(src);
    assert_eq!(result, Value::Int(3));
}

#[test]
fn run_recursive_packet_adt_nested_matches() {
    let src = r#"
import collections

type Packet = Num(Int) | Lst(List<Packet>)

fn cmp(a: Packet, b: Packet) -> Int {
  match (a) {
    Num(av) => match (b) {
      Num(bv) => av - bv,
      Lst(_bs) => -1,
    },
    Lst(items) => if (items.len() == 0) {
      match (b) {
        Num(_bv) => 1,
        Lst(other_items) => if (other_items.len() == 0) { 0 } else { -1 },
      }
    } else {
      match (items[0]) {
        Num(head) => head,
        Lst(inner) => inner.len(),
      }
    },
  }
}

fn main() -> Int {
  let nested = Lst(collections.List.new().push(Lst(collections.List.new().push(Num(7)))))
  cmp(nested, Num(0))
}
"#;
    let result = run_ok(src);
    assert_eq!(result, Value::Int(1));
}

#[test]
fn run_rejects_duplicate_binding_in_match_arm() {
    // Edge case: duplicate binding in match arm pattern.
    let src = r#"
type Pair = Pair(Int, Int)
fn main() -> Int {
  match (Pair(1, 2)) {
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
            name: "legacy leading-pipe type variant syntax",
            src: "type Option = | Some(Int) | None\nfn main() -> Int { 1 }",
            run_fragment: "leading `|` is not allowed in type variants",
        },
        Case {
            name: "legacy leading-pipe match arm syntax",
            src: "fn main() -> Int { match (1) { | _ => 0 } }",
            run_fragment: "match arms do not use a leading `|`",
        },
        Case {
            name: "pub property item is invalid",
            src: "pub property p(x: Int <- Gen.int()) { true }\nfn main() -> Int { 1 }",
            run_fragment: "expected item",
        },
        Case {
            name: "pub let item is invalid",
            src: "pub let x = 1\nfn main() -> Int { 1 }",
            run_fragment: "expected item",
        },
        Case {
            name: "unresolved name",
            src: "fn main() -> Int { unknown_name }",
            run_fragment: "lowering errors:",
        },
        Case {
            name: "top-level bodyless function declaration",
            src: "fn foo() -> Int\nfn main() -> Int { foo() }",
            run_fragment: "parse errors:",
        },
        Case {
            name: "misordered contract clauses",
            src: "fn inc(x: Int) -> Int contract ensures (result > x) requires (x >= 0) { x + 1 }\nfn main() -> Int { inc(1) }",
            run_fragment: "requires cannot appear after ensures",
        },
        Case {
            name: "duplicate pattern binding",
            src: "type Pair = Pair(Int, Int)\nfn main() -> Int {\n  let Pair(x, x) = Pair(1, 2)\n  x\n}",
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
        Case {
            name: "unknown named argument",
            src: "fn add(x: Int, y: Int) -> Int { x + y }\nfn main() -> Int { add(z: 1, y: 2) }",
            run_fragment: "unknown named argument",
        },
        Case {
            name: "duplicate named argument",
            src: "fn add(x: Int, y: Int) -> Int { x + y }\nfn main() -> Int { add(x: 1, x: 2) }",
            run_fragment: "duplicate named argument",
        },
        Case {
            name: "unknown named argument on local fn value",
            src: "fn add(x: Int, y: Int) -> Int { x + y }\nfn main() -> Int { let f = add\n f(z: 1, y: 2) }",
            run_fragment: "unknown named argument",
        },
        Case {
            name: "positional argument after named argument",
            src: "fn add(x: Int, y: Int) -> Int { x + y }\nfn main() -> Int { add(x: 1, 2) }",
            run_fragment: "positional argument cannot appear after named argument",
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
fn run_allows_label_only_effect_declaration() {
    let src = r#"
effect IO
fn main() -> Int { 1 }
"#;
    let value = run_ok(src);
    assert_eq!(value, Value::Int(1));
}

#[test]
fn run_compile_gating_uses_structured_error_classes() {
    struct Case<'a> {
        name: &'a str,
        src: &'a str,
        class_prefix: &'a str,
    }

    let cases = [
        Case {
            name: "parse",
            src: "fn main( -> Int { 1 }",
            class_prefix: "compile parse errors:",
        },
        Case {
            name: "lowering item",
            src: "fn foo() -> Int { 1 }\nfn foo() -> Int { 2 }\nfn main() -> Int { foo() }",
            class_prefix: "compile lowering errors:",
        },
        Case {
            name: "lowering body",
            src: "fn main() -> Int { unknown_name }",
            class_prefix: "compile lowering errors:",
        },
        Case {
            name: "type",
            src: "fn main() -> Int { \"x\" }",
            class_prefix: "compile type errors:",
        },
        Case {
            name: "named arg validation",
            src: "fn add(x: Int, y: Int) -> Int { x + y }\nfn main() -> Int { add(z: 1, y: 2) }",
            class_prefix: "compile type errors:",
        },
    ];

    for case in cases {
        let err = run_err(case.src);
        assert!(
            err.contains(case.class_prefix),
            "run should classify `{}` as `{}`; got: {}",
            case.name,
            case.class_prefix,
            err
        );
    }
}

#[test]
fn run_project_compile_gating_uses_structured_error_classes() {
    use std::io::Write;

    struct Case<'a> {
        name: &'a str,
        files: Vec<(&'a str, &'a str)>,
        class_prefix: &'a str,
    }

    let cases = [
        Case {
            name: "parse",
            files: vec![
                ("main.ky", "fn main() -> Int { 42 }\n"),
                ("bad.ky", "pub fn bad( -> Int { 42 }\n"),
            ],
            class_prefix: "compile parse errors:",
        },
        Case {
            name: "lowering item",
            files: vec![
                ("main.ky", "fn main() -> Int { 42 }\n"),
                (
                    "dup.ky",
                    "pub fn foo() -> Int { 1 }\npub fn foo() -> Int { 2 }\n",
                ),
            ],
            class_prefix: "compile lowering errors:",
        },
        Case {
            name: "lowering body",
            files: vec![
                ("main.ky", "fn main() -> Int { 42 }\n"),
                ("bad.ky", "pub fn oops() -> Int { unknown_name }\n"),
            ],
            class_prefix: "compile lowering errors:",
        },
        Case {
            name: "type",
            files: vec![
                ("main.ky", "import util\nfn main() -> Int { util() }\n"),
                ("util.ky", "pub fn util() -> Int { true }\n"),
            ],
            class_prefix: "compile type errors:",
        },
    ];

    for case in cases {
        let dir = tempfile::tempdir().unwrap();
        for (rel, src) in &case.files {
            let path = dir.path().join(rel);
            let mut file = std::fs::File::create(&path).unwrap();
            write!(file, "{}", src).unwrap();
        }
        let main_path = dir.path().join("main.ky");
        let err = match kyokara_eval::run_project(&main_path) {
            Ok(result) => panic!(
                "expected compile-time rejection for `{}`, got {:?}",
                case.name, result.value
            ),
            Err(e) => e.to_string(),
        };
        assert!(
            err.contains(case.class_prefix),
            "run_project should classify `{}` as `{}`; got: {}",
            case.name,
            case.class_prefix,
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

// ── User-defined functions shadow intrinsics (#70) ──────────────────

#[test]
fn user_fn_shadows_intrinsic_abs() {
    // User defines `abs` that adds 100 instead of returning the absolute value.
    // The user version should take precedence over the builtin intrinsic.
    let val = run_ok(
        r#"
        fn abs(x: Int) -> Int { x + 100 }
        fn main() -> Int { abs(5) }
        "#,
    );
    assert_eq!(val, Value::Int(105));
}

#[test]
fn user_fn_shadows_intrinsic_min() {
    // User defines `min` that always returns the first argument.
    let val = run_ok(
        r#"
        fn min(a: Int, b: Int) -> Int { a }
        fn main() -> Int { min(10, 3) }
        "#,
    );
    assert_eq!(val, Value::Int(10));
}

#[test]
fn user_fn_shadows_intrinsic_max() {
    // User defines `max` that returns the sum instead of the max.
    let val = run_ok(
        r#"
        fn max(a: Int, b: Int) -> Int { a + b }
        fn main() -> Int { max(2, 3) }
        "#,
    );
    assert_eq!(val, Value::Int(5));
}

// ── Integer overflow tests (#71) ────────────────────────────────────

#[test]
fn overflow_add_max_plus_one() {
    let err = run_err("fn main() -> Int { 9223372036854775807 + 1 }");
    assert!(
        err.contains("integer overflow"),
        "expected overflow error, got: {err}"
    );
}

#[test]
fn overflow_sub_min_minus_one() {
    let err = run_err(
        // i64::MIN is -9223372036854775808; we express it as 0 - 9223372036854775807 - 1 - 1
        // Actually simpler: -9223372036854775807 is the negation of MAX which is fine,
        // then subtract 1 to get MIN, then subtract 1 more to overflow.
        "fn main() -> Int { -9223372036854775807 - 2 }",
    );
    assert!(
        err.contains("integer overflow"),
        "expected overflow error, got: {err}"
    );
}

#[test]
fn overflow_mul_max_times_two() {
    let err = run_err("fn main() -> Int { 9223372036854775807 * 2 }");
    assert!(
        err.contains("integer overflow"),
        "expected overflow error, got: {err}"
    );
}

#[test]
fn overflow_unary_neg_of_min() {
    // -(i64::MIN) overflows because i64::MAX is 9223372036854775807 but |i64::MIN| is 9223372036854775808.
    // We build i64::MIN as -9223372036854775807 - 1, then negate it.
    let err = run_err("fn main() -> Int { -(-9223372036854775807 - 1) }");
    assert!(
        err.contains("integer overflow"),
        "expected overflow error, got: {err}"
    );
}

#[test]
fn overflow_abs_of_min() {
    // abs(i64::MIN) overflows for the same reason as unary neg.
    let err = run_err("fn main() -> Int { (-9223372036854775807 - 1).abs() }");
    assert!(
        err.contains("integer overflow"),
        "expected overflow error, got: {err}"
    );
}

#[test]
fn overflow_div_min_by_neg_one() {
    // i64::MIN / -1 overflows (result would be i64::MAX + 1).
    let err = run_err("fn main() -> Int { (-9223372036854775807 - 1) / -1 }");
    assert!(
        err.contains("integer overflow"),
        "expected overflow error, got: {err}"
    );
}

#[test]
fn normal_arithmetic_still_works() {
    let val = run_ok("fn main() -> Int { 100 + 200 }");
    assert_eq!(val, Value::Int(300));
    let val = run_ok("fn main() -> Int { 100 - 200 }");
    assert_eq!(val, Value::Int(-100));
    let val = run_ok("fn main() -> Int { 100 * 200 }");
    assert_eq!(val, Value::Int(20000));
    let val = run_ok("fn main() -> Int { -42 }");
    assert_eq!(val, Value::Int(-42));
    let val = run_ok("fn main() -> Int { (-42).abs() }");
    assert_eq!(val, Value::Int(42));
}

// ── Named argument tests ────────────────────────────────────────────

#[test]
fn eval_named_args_basic() {
    // Named args in order should work.
    let val = run_ok(
        "fn add(x: Int, y: Int) -> Int { x + y }
         fn main() -> Int { add(x: 1, y: 2) }",
    );
    assert_eq!(val, Value::Int(3));
}

#[test]
fn eval_named_args_reordered() {
    // Reordered named args: sub(y: 10, x: 3) should bind x=3, y=10 → 3 - 10 = -7.
    let val = run_ok(
        "fn sub(x: Int, y: Int) -> Int { x - y }
         fn main() -> Int { sub(y: 10, x: 3) }",
    );
    assert_eq!(val, Value::Int(-7));
}

#[test]
fn eval_positional_args_still_work() {
    // Guard: positional args should remain correct.
    let val = run_ok(
        "fn sub(x: Int, y: Int) -> Int { x - y }
         fn main() -> Int { sub(3, 10) }",
    );
    assert_eq!(val, Value::Int(-7));
}

#[test]
fn eval_named_args_reordered_direct_lambda() {
    let val = run_ok(
        "fn main() -> Int {
           (fn(x: Int, y: Int) => x - y)(y: 10, x: 3)
         }",
    );
    assert_eq!(val, Value::Int(-7));
}

#[test]
fn eval_named_args_reordered_local_fn_value() {
    let val = run_ok(
        "fn sub(x: Int, y: Int) -> Int { x - y }
         fn main() -> Int {
           let f = sub
           f(y: 10, x: 3)
         }",
    );
    assert_eq!(val, Value::Int(-7));
}

#[test]
fn eval_named_args_reordered_local_lambda_value() {
    let val = run_ok(
        "fn main() -> Int {
           let f = fn(x: Int, y: Int) => x - y
           f(y: 10, x: 3)
         }",
    );
    assert_eq!(val, Value::Int(-7));
}

#[test]
fn eval_named_args_reordered_module_call() {
    let val = run_ok(
        "import math
         fn main() -> Int { math.min(b: 10, a: 3) }",
    );
    assert_eq!(val, Value::Int(3));
}

#[test]
fn eval_named_args_reordered_method_call() {
    let val = run_ok(
        "fn main() -> String {
           \"abcd\".substring(end: 3, start: 1)
         }",
    );
    assert_eq!(val, Value::String("bc".to_string()));
}

#[test]
fn eval_named_args_preserve_source_order_evaluation() {
    let val = run_ok(
        "import io
         fn tap(n: Int) -> Int {
           io.println(n.to_string())
           n
         }
         fn sub(x: Int, y: Int) -> Int { x - y }
         fn main() -> Int {
           sub(y: tap(10), x: tap(3))
         }",
    );
    assert_eq!(val, Value::Int(-7));
}

#[test]
fn eval_positional_args_preserve_source_order_evaluation() {
    let val = run_ok(
        "import collections
         fn tap(log: MutableList<Int>, n: Int) -> Int {
           log.push(n)
           n
         }
         fn pair(x: Int, y: Int) -> Int { x * 10 + y }
         fn main() -> Int {
           let log = collections.MutableList.new()
           let result = pair(tap(log, 1), tap(log, 2))
           if (log.len() == 2 && log[0] == 1 && log[1] == 2) { result } else { 0 }
         }",
    );
    assert_eq!(val, Value::Int(12));
}

#[test]
fn eval_module_call_positional_args_preserve_source_order_evaluation() {
    let val = run_ok(
        "import collections
         import math
         fn tap(log: MutableList<Int>, n: Int) -> Int {
           log.push(n)
           n
         }
         fn main() -> Int {
           let log = collections.MutableList.new()
           let result = math.min(tap(log, 3), tap(log, 10))
           if (log.len() == 2 && log[0] == 3 && log[1] == 10) { result } else { 99 }
         }",
    );
    assert_eq!(val, Value::Int(3));
}

#[test]
fn eval_method_call_evaluates_receiver_before_args() {
    let val = run_ok(
        "import collections
         type Box = { value: Int }
         fn Box.sub(self, n: Int) -> Int { self.value - n }
         fn build(log: MutableList<Int>) -> Box {
           log.push(1)
           Box { value: 10 }
         }
         fn tap(log: MutableList<Int>, n: Int) -> Int {
           log.push(n)
           n
         }
         fn main() -> Int {
           let log = collections.MutableList.new()
           let result = build(log).sub(tap(log, 2))
           if (log.len() == 2 && log[0] == 1 && log[1] == 2) { result } else { 0 }
         }",
    );
    assert_eq!(val, Value::Int(8));
}

#[test]
fn eval_local_callable_shadows_top_level_function_on_positional_call() {
    let val = run_ok(
        "fn calc(x: Int) -> Int { x + 100 }
         fn main() -> Int {
           let calc = fn(x: Int) => x + 1
           calc(3)
         }",
    );
    assert_eq!(val, Value::Int(4));
}

#[test]
fn eval_param_callable_shadows_top_level_function_on_positional_call() {
    let val = run_ok(
        "fn apply(x: Int) -> Int { x + 100 }
         fn run(apply: fn(Int) -> Int) -> Int {
           apply(3)
         }
         fn main() -> Int {
           run(fn(x: Int) => x + 1)
         }",
    );
    assert_eq!(val, Value::Int(4));
}

#[test]
fn eval_local_callable_shadows_top_level_function_on_named_call() {
    let val = run_ok(
        "fn sub(x: Int, y: Int) -> Int { x - y }
         fn main() -> Int {
           let sub = fn(x: Int, y: Int) => y - x
           sub(y: 10, x: 3)
         }",
    );
    assert_eq!(val, Value::Int(7));
}

#[test]
fn eval_named_args_matrix_happy_paths() {
    struct Case<'a> {
        name: &'a str,
        src: &'a str,
        expected: Value,
    }

    let cases = [
        Case {
            name: "direct function",
            src: "fn sub(x: Int, y: Int) -> Int { x - y }\nfn main() -> Int { sub(y: 10, x: 3) }",
            expected: Value::Int(-7),
        },
        Case {
            name: "local function value",
            src: "fn sub(x: Int, y: Int) -> Int { x - y }\nfn main() -> Int { let f = sub\n f(y: 10, x: 3) }",
            expected: Value::Int(-7),
        },
        Case {
            name: "direct lambda",
            src: "fn main() -> Int { (fn(x: Int, y: Int) => x - y)(y: 10, x: 3) }",
            expected: Value::Int(-7),
        },
        Case {
            name: "local lambda value",
            src: "fn main() -> Int { let f = fn(x: Int, y: Int) => x - y\n f(y: 10, x: 3) }",
            expected: Value::Int(-7),
        },
        Case {
            name: "module-qualified call",
            src: "import math\nfn main() -> Int { math.min(b: 10, a: 3) }",
            expected: Value::Int(3),
        },
        Case {
            name: "method call",
            src: "fn main() -> String { \"abcd\".substring(end: 3, start: 1) }",
            expected: Value::String("bc".to_string()),
        },
    ];

    for case in cases {
        let val = run_ok(case.src);
        assert_eq!(val, case.expected, "case `{}` failed", case.name);
    }
}

#[test]
fn eval_named_args_matrix_unhappy_paths_compile_errors() {
    struct Case<'a> {
        name: &'a str,
        src: &'a str,
        run_fragment: &'a str,
    }

    let cases = [
        Case {
            name: "direct unknown",
            src: "fn add(x: Int, y: Int) -> Int { x + y }\nfn main() -> Int { add(z: 1, y: 2) }",
            run_fragment: "unknown named argument",
        },
        Case {
            name: "direct duplicate",
            src: "fn add(x: Int, y: Int) -> Int { x + y }\nfn main() -> Int { add(x: 1, x: 2) }",
            run_fragment: "duplicate named argument",
        },
        Case {
            name: "direct missing",
            src: "fn add(x: Int, y: Int) -> Int { x + y }\nfn main() -> Int { add(x: 1, x: 2) }",
            run_fragment: "missing argument for parameter `y`",
        },
        Case {
            name: "direct positional-after-named",
            src: "fn add(x: Int, y: Int) -> Int { x + y }\nfn main() -> Int { add(x: 1, 2) }",
            run_fragment: "positional argument cannot appear after named argument",
        },
        Case {
            name: "local-fn unknown",
            src: "fn add(x: Int, y: Int) -> Int { x + y }\nfn main() -> Int { let f = add\n f(z: 1, y: 2) }",
            run_fragment: "unknown named argument",
        },
        Case {
            name: "local-fn duplicate",
            src: "fn add(x: Int, y: Int) -> Int { x + y }\nfn main() -> Int { let f = add\n f(x: 1, x: 2) }",
            run_fragment: "duplicate named argument",
        },
        Case {
            name: "local-fn missing",
            src: "fn add(x: Int, y: Int) -> Int { x + y }\nfn main() -> Int { let f = add\n f(x: 1, x: 2) }",
            run_fragment: "missing argument for parameter `y`",
        },
        Case {
            name: "local-fn positional-after-named",
            src: "fn add(x: Int, y: Int) -> Int { x + y }\nfn main() -> Int { let f = add\n f(x: 1, 2) }",
            run_fragment: "positional argument cannot appear after named argument",
        },
        Case {
            name: "direct-lambda unknown",
            src: "fn main() -> Int { (fn(x: Int, y: Int) => x + y)(z: 1, y: 2) }",
            run_fragment: "unknown named argument",
        },
        Case {
            name: "direct-lambda duplicate",
            src: "fn main() -> Int { (fn(x: Int, y: Int) => x + y)(x: 1, x: 2) }",
            run_fragment: "duplicate named argument",
        },
        Case {
            name: "direct-lambda missing",
            src: "fn main() -> Int { (fn(x: Int, y: Int) => x + y)(x: 1, x: 2) }",
            run_fragment: "missing argument for parameter `y`",
        },
        Case {
            name: "direct-lambda positional-after-named",
            src: "fn main() -> Int { (fn(x: Int, y: Int) => x + y)(x: 1, 2) }",
            run_fragment: "positional argument cannot appear after named argument",
        },
        Case {
            name: "local-lambda unknown",
            src: "fn main() -> Int { let f = fn(x: Int, y: Int) => x + y\n f(z: 1, y: 2) }",
            run_fragment: "unknown named argument",
        },
        Case {
            name: "local-lambda duplicate",
            src: "fn main() -> Int { let f = fn(x: Int, y: Int) => x + y\n f(x: 1, x: 2) }",
            run_fragment: "duplicate named argument",
        },
        Case {
            name: "local-lambda missing",
            src: "fn main() -> Int { let f = fn(x: Int, y: Int) => x + y\n f(x: 1, x: 2) }",
            run_fragment: "missing argument for parameter `y`",
        },
        Case {
            name: "local-lambda positional-after-named",
            src: "fn main() -> Int { let f = fn(x: Int, y: Int) => x + y\n f(x: 1, 2) }",
            run_fragment: "positional argument cannot appear after named argument",
        },
        Case {
            name: "module unknown",
            src: "import math\nfn main() -> Int { math.min(c: 1, b: 2) }",
            run_fragment: "unknown named argument",
        },
        Case {
            name: "module duplicate",
            src: "import math\nfn main() -> Int { math.min(a: 1, a: 2) }",
            run_fragment: "duplicate named argument",
        },
        Case {
            name: "module missing",
            src: "import math\nfn main() -> Int { math.min(a: 1, a: 2) }",
            run_fragment: "missing argument for parameter `b`",
        },
        Case {
            name: "module positional-after-named",
            src: "import math\nfn main() -> Int { math.min(a: 1, 2) }",
            run_fragment: "positional argument cannot appear after named argument",
        },
        Case {
            name: "method unknown",
            src: "fn main() -> String { \"abcd\".substring(foo: 1, end: 3) }",
            run_fragment: "unknown named argument",
        },
        Case {
            name: "method duplicate",
            src: "fn main() -> String { \"abcd\".substring(start: 1, start: 2) }",
            run_fragment: "duplicate named argument",
        },
        Case {
            name: "method missing",
            src: "fn main() -> String { \"abcd\".substring(start: 1, start: 2) }",
            run_fragment: "missing argument for parameter `end`",
        },
        Case {
            name: "method positional-after-named",
            src: "fn main() -> String { \"abcd\".substring(start: 1, 3) }",
            run_fragment: "positional argument cannot appear after named argument",
        },
    ];

    for case in cases {
        assert!(
            check_has_compile_errors(case.src),
            "expected compile errors for case `{}`",
            case.name
        );
        let err = run_err(case.src);
        assert!(
            err.contains(case.run_fragment),
            "case `{}` expected fragment `{}`; got: {}",
            case.name,
            case.run_fragment,
            err
        );
    }
}

// ── Issue #68: wrong imported function body when sibling modules export same name ──

#[test]
fn run_project_uses_imported_module_body_not_sibling() {
    // When main.ky imports util, and both util.ky and other.ky export
    // `pub fn foo() -> Int`, only util's body should be used.
    use std::io::Write;

    let dir = tempfile::tempdir().unwrap();

    let util_path = dir.path().join("util.ky");
    let mut util_file = std::fs::File::create(&util_path).unwrap();
    writeln!(util_file, "pub fn foo() -> Int {{ 42 }}").unwrap();

    let other_path = dir.path().join("other.ky");
    let mut other_file = std::fs::File::create(&other_path).unwrap();
    writeln!(other_file, "pub fn foo() -> Int {{ 999 }}").unwrap();

    let main_path = dir.path().join("main.ky");
    let mut main_file = std::fs::File::create(&main_path).unwrap();
    writeln!(main_file, "import util").unwrap();
    writeln!(main_file, "fn main() -> Int {{ foo() }}").unwrap();

    let result = kyokara_eval::run_project(&main_path).expect("should succeed");
    assert_eq!(
        result.value,
        Value::Int(42),
        "foo() should resolve to util::foo() (42), not other::foo() (999)"
    );
}

#[test]
fn run_project_imported_overload_family_by_arity_dispatches() {
    use std::io::Write;

    let dir = tempfile::tempdir().unwrap();

    let util_path = dir.path().join("util.ky");
    let mut util_file = std::fs::File::create(&util_path).unwrap();
    writeln!(util_file, "pub fn foo() -> Int {{ 1 }}").unwrap();
    writeln!(util_file, "pub fn foo(x: Int) -> Int {{ x + 1 }}").unwrap();

    let main_path = dir.path().join("main.ky");
    let mut main_file = std::fs::File::create(&main_path).unwrap();
    writeln!(main_file, "import util").unwrap();
    writeln!(main_file, "fn main() -> Int {{ foo() + foo(2) }}").unwrap();

    let result = kyokara_eval::run_project(&main_path).expect("should succeed");
    assert_eq!(
        result.value,
        Value::Int(4),
        "imported overload family should dispatch by arity"
    );
}

#[test]
fn run_project_dual_import_same_name_is_conflict() {
    // When main.ky imports both util and other, and both export `foo`,
    // the second import should produce a conflicting-import error.
    use std::io::Write;

    let dir = tempfile::tempdir().unwrap();

    let util_path = dir.path().join("util.ky");
    let mut util_file = std::fs::File::create(&util_path).unwrap();
    writeln!(util_file, "pub fn foo() -> Int {{ 42 }}").unwrap();

    let other_path = dir.path().join("other.ky");
    let mut other_file = std::fs::File::create(&other_path).unwrap();
    writeln!(other_file, "pub fn foo() -> Int {{ 999 }}").unwrap();

    let main_path = dir.path().join("main.ky");
    let mut main_file = std::fs::File::create(&main_path).unwrap();
    writeln!(main_file, "import util").unwrap();
    writeln!(main_file, "import other").unwrap();
    writeln!(main_file, "fn main() -> Int {{ foo() }}").unwrap();

    let result = kyokara_eval::run_project(&main_path);
    match result {
        Ok(_) => panic!("expected conflicting import error"),
        Err(e) => {
            let err = e.to_string();
            assert!(
                err.contains("conflicting import"),
                "expected 'conflicting import' in message, got: {err}"
            );
        }
    }
}

#[test]
fn run_project_rejects_ambiguous_import_last_segment() {
    // import math is ambiguous when both a/math.ky and b/math.ky exist.
    use std::io::Write;

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("a")).unwrap();
    std::fs::create_dir_all(dir.path().join("b")).unwrap();

    let a_math_path = dir.path().join("a").join("math.ky");
    let mut a_math_file = std::fs::File::create(&a_math_path).unwrap();
    writeln!(a_math_file, "pub fn value() -> Int {{ 1 }}").unwrap();

    let b_math_path = dir.path().join("b").join("math.ky");
    let mut b_math_file = std::fs::File::create(&b_math_path).unwrap();
    writeln!(b_math_file, "pub fn value() -> Int {{ 2 }}").unwrap();

    let main_path = dir.path().join("main.ky");
    let mut main_file = std::fs::File::create(&main_path).unwrap();
    writeln!(main_file, "import math").unwrap();
    writeln!(main_file, "fn main() -> Int {{ value() }}").unwrap();

    let result = kyokara_eval::run_project(&main_path);
    match result {
        Ok(_) => panic!("expected ambiguous import error"),
        Err(e) => {
            let err = e.to_string();
            assert!(
                err.contains("ambiguous import"),
                "expected 'ambiguous import' in message, got: {err}"
            );
        }
    }
}

#[test]
fn run_project_import_math_module_does_not_activate_synthetic_math() {
    // If project import resolution picks a real `math` module, synthetic `math`
    // must not be activated from the same token.
    use std::io::Write;

    let dir = tempfile::tempdir().unwrap();

    let math_path = dir.path().join("math.ky");
    let mut math_file = std::fs::File::create(&math_path).unwrap();
    writeln!(math_file, "pub fn add(a: Int, b: Int) -> Int {{ a + b }}").unwrap();

    let main_path = dir.path().join("main.ky");
    let mut main_file = std::fs::File::create(&main_path).unwrap();
    writeln!(main_file, "import math").unwrap();
    writeln!(main_file, "fn main() -> Int {{ math.min(1, 2) }}").unwrap();

    let result = kyokara_eval::run_project(&main_path);
    match result {
        Ok(_) => panic!(
            "expected unresolved name: synthetic math should not activate when project module `math` resolves"
        ),
        Err(e) => {
            let err = e.to_string();
            assert!(
                err.contains("import name `math` used as value") || err.contains("unresolved name"),
                "expected import-value or unresolved-name diagnostic, got: {err}"
            );
        }
    }
}

#[test]
fn run_project_import_math_activates_synthetic_when_no_project_module_exists() {
    // Regression guard: when no project module named `math` exists, `import math`
    // should still activate the synthetic math module.
    use std::io::Write;

    let dir = tempfile::tempdir().unwrap();

    let main_path = dir.path().join("main.ky");
    let mut main_file = std::fs::File::create(&main_path).unwrap();
    writeln!(main_file, "import math").unwrap();
    writeln!(main_file, "fn main() -> Int {{ math.min(1, 2) }}").unwrap();

    let result = kyokara_eval::run_project(&main_path).expect("synthetic math import should work");
    assert_eq!(result.value, Value::Int(1));
}

#[test]
fn run_project_aliased_collections_import_activates_alias() {
    use std::io::Write;

    let dir = tempfile::tempdir().unwrap();

    let main_path = dir.path().join("main.ky");
    let mut main_file = std::fs::File::create(&main_path).unwrap();
    writeln!(main_file, "import collections as c").unwrap();
    writeln!(
        main_file,
        "fn main() -> Int {{ c.Deque.new().push_back(1).push_back(2).len() }}"
    )
    .unwrap();

    let result = kyokara_eval::run_project(&main_path)
        .expect("aliased synthetic collections import should work");
    assert_eq!(result.value, Value::Int(2));
}

#[test]
fn run_project_qualified_import_resolves_duplicate_leaf_modules() {
    // import a.math should resolve to a/math.ky even when b/math.ky exists.
    use std::io::Write;

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("a")).unwrap();
    std::fs::create_dir_all(dir.path().join("b")).unwrap();

    let a_math_path = dir.path().join("a").join("math.ky");
    let mut a_math_file = std::fs::File::create(&a_math_path).unwrap();
    writeln!(a_math_file, "pub fn value() -> Int {{ 1 }}").unwrap();

    let b_math_path = dir.path().join("b").join("math.ky");
    let mut b_math_file = std::fs::File::create(&b_math_path).unwrap();
    writeln!(b_math_file, "pub fn value() -> Int {{ 2 }}").unwrap();

    let main_path = dir.path().join("main.ky");
    let mut main_file = std::fs::File::create(&main_path).unwrap();
    writeln!(main_file, "import a.math").unwrap();
    writeln!(main_file, "fn main() -> Int {{ value() }}").unwrap();

    let result = kyokara_eval::run_project(&main_path).expect("should succeed");
    assert_eq!(result.value, Value::Int(1));
}

#[test]
fn run_project_qualified_import_missing_path_reports_unresolved() {
    // import c.math should not match a/math.ky or b/math.ky by leaf name.
    use std::io::Write;

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("a")).unwrap();
    std::fs::create_dir_all(dir.path().join("b")).unwrap();

    let a_math_path = dir.path().join("a").join("math.ky");
    let mut a_math_file = std::fs::File::create(&a_math_path).unwrap();
    writeln!(a_math_file, "pub fn value() -> Int {{ 1 }}").unwrap();

    let b_math_path = dir.path().join("b").join("math.ky");
    let mut b_math_file = std::fs::File::create(&b_math_path).unwrap();
    writeln!(b_math_file, "pub fn value() -> Int {{ 2 }}").unwrap();

    let main_path = dir.path().join("main.ky");
    let mut main_file = std::fs::File::create(&main_path).unwrap();
    writeln!(main_file, "import c.math").unwrap();
    writeln!(main_file, "fn main() -> Int {{ value() }}").unwrap();

    let result = kyokara_eval::run_project(&main_path);
    match result {
        Ok(_) => panic!("expected unresolved import error"),
        Err(e) => {
            let err = e.to_string();
            assert!(
                err.contains("unresolved import"),
                "expected unresolved import error, got: {err}"
            );
        }
    }
}

// ── Issue #69: imported pub fn calling private helper ────────────────

#[test]
fn run_project_imported_pub_fn_calls_private_helper() {
    // pub fn foo() in util.ky calls private helper() in the same module.
    // The interpreter must resolve helper() in util's scope, not main's.
    use std::io::Write;

    let dir = tempfile::tempdir().unwrap();

    let util_path = dir.path().join("util.ky");
    let mut util_file = std::fs::File::create(&util_path).unwrap();
    writeln!(util_file, "fn helper() -> Int {{ 41 }}").unwrap();
    writeln!(util_file, "pub fn foo() -> Int {{ helper() + 1 }}").unwrap();

    let main_path = dir.path().join("main.ky");
    let mut main_file = std::fs::File::create(&main_path).unwrap();
    writeln!(main_file, "import util").unwrap();
    writeln!(main_file, "fn main() -> Int {{ foo() }}").unwrap();

    let result = kyokara_eval::run_project(&main_path).expect("should succeed");
    assert_eq!(
        result.value,
        Value::Int(42),
        "foo() should call util's private helper() and return 42"
    );
}

#[test]
fn run_project_imported_private_helper_name_does_not_capture_entry_fn() {
    // Regression guard: if entry module also defines `helper`, util::foo must
    // still resolve util::helper, not main::helper.
    use std::io::Write;

    let dir = tempfile::tempdir().unwrap();

    let util_path = dir.path().join("util.ky");
    let mut util_file = std::fs::File::create(&util_path).unwrap();
    writeln!(util_file, "fn helper() -> Int {{ 41 }}").unwrap();
    writeln!(util_file, "pub fn foo() -> Int {{ helper() + 1 }}").unwrap();

    let main_path = dir.path().join("main.ky");
    let mut main_file = std::fs::File::create(&main_path).unwrap();
    writeln!(main_file, "import util").unwrap();
    writeln!(main_file, "fn helper() -> Int {{ 100 }}").unwrap();
    writeln!(main_file, "fn main() -> Int {{ foo() }}").unwrap();

    let result = kyokara_eval::run_project(&main_path).expect("should succeed");
    assert_eq!(
        result.value,
        Value::Int(42),
        "foo() should resolve helper() in util module scope, not entry module scope"
    );
}

#[test]
fn run_project_imported_deep_private_call_chains_across_modules() {
    // Both imported modules have private `base`/`mid` chains; public entry points
    // must resolve private calls within each source module.
    use std::io::Write;

    let dir = tempfile::tempdir().unwrap();

    let util_a_path = dir.path().join("util_a.ky");
    let mut util_a_file = std::fs::File::create(&util_a_path).unwrap();
    writeln!(util_a_file, "fn base() -> Int {{ 20 }}").unwrap();
    writeln!(util_a_file, "fn mid() -> Int {{ base() + 1 }}").unwrap();
    writeln!(util_a_file, "pub fn foo() -> Int {{ mid() + 1 }}").unwrap();

    let util_b_path = dir.path().join("util_b.ky");
    let mut util_b_file = std::fs::File::create(&util_b_path).unwrap();
    writeln!(util_b_file, "fn base() -> Int {{ 30 }}").unwrap();
    writeln!(util_b_file, "fn mid() -> Int {{ base() + 1 }}").unwrap();
    writeln!(util_b_file, "pub fn bar() -> Int {{ mid() + 1 }}").unwrap();

    let main_path = dir.path().join("main.ky");
    let mut main_file = std::fs::File::create(&main_path).unwrap();
    writeln!(main_file, "import util_a").unwrap();
    writeln!(main_file, "import util_b").unwrap();
    writeln!(main_file, "fn main() -> Int {{ foo() + bar() }}").unwrap();

    let result = kyokara_eval::run_project(&main_path).expect("should succeed");
    assert_eq!(
        result.value,
        Value::Int(54),
        "each imported module should keep private call-chain resolution module-local"
    );
}

#[test]
fn run_project_private_helper_not_directly_callable_from_main() {
    // Private functions in util.ky should NOT be callable directly from main.ky.
    // Only pub functions are imported.
    use std::io::Write;

    let dir = tempfile::tempdir().unwrap();

    let util_path = dir.path().join("util.ky");
    let mut util_file = std::fs::File::create(&util_path).unwrap();
    writeln!(util_file, "fn helper() -> Int {{ 42 }}").unwrap();
    writeln!(util_file, "pub fn foo() -> Int {{ helper() }}").unwrap();

    let main_path = dir.path().join("main.ky");
    let mut main_file = std::fs::File::create(&main_path).unwrap();
    writeln!(main_file, "import util").unwrap();
    // Calling helper() directly should fail — it's private.
    writeln!(main_file, "fn main() -> Int {{ helper() }}").unwrap();

    let result = kyokara_eval::run_project(&main_path);
    assert!(
        result.is_err(),
        "calling private helper() directly from main should fail"
    );
}

// ── Escape sequence tests (#74) ─────────────────────────────────────

#[test]
fn eval_string_escape_newline() {
    let val = run_ok(r#"fn main() -> String { "\n" }"#);
    assert_eq!(val, Value::String("\n".to_owned()));
}

#[test]
fn eval_string_escape_tab() {
    let val = run_ok(r#"fn main() -> String { "\t" }"#);
    assert_eq!(val, Value::String("\t".to_owned()));
}

#[test]
fn eval_string_escape_backslash() {
    let val = run_ok(r#"fn main() -> String { "\\" }"#);
    assert_eq!(val, Value::String("\\".to_owned()));
}

#[test]
fn eval_string_escape_double_quote() {
    let val = run_ok(r#"fn main() -> String { "he said \"hi\"" }"#);
    assert_eq!(val, Value::String("he said \"hi\"".to_owned()));
}

#[test]
fn eval_char_escape_newline() {
    let val = run_ok(r"fn main() -> Char { '\n' }");
    assert_eq!(val, Value::Char('\n'));
}

#[test]
fn eval_char_escape_backslash() {
    let val = run_ok(r"fn main() -> Char { '\\' }");
    assert_eq!(val, Value::Char('\\'));
}

#[test]
fn eval_char_newline_neq_backslash() {
    // The repro from issue #74
    let val = run_ok(r"fn main() -> Bool { '\n'.to_string() == '\\'.to_string() }");
    assert_eq!(val, Value::Bool(false));
}

#[test]
fn eval_match_escaped_char_literal() {
    let val = run_ok(
        r"fn main() -> Int {
            let c = '\n'
            match (c) {
                '\n' => 1
                _ => 0
            }
        }",
    );
    assert_eq!(val, Value::Int(1));
}

#[test]
fn eval_match_escaped_string_literal() {
    let val = run_ok(
        r#"fn main() -> Int {
            let s = "\t"
            match (s) {
                "\t" => 1
                _ => 0
            }
        }"#,
    );
    assert_eq!(val, Value::Int(1));
}

// ── Path-qualified record literal validation (#126) ─────────────────

#[test]
fn eval_path_record_lit_non_record_type_rejected() {
    // The exact repro from issue #126: Option is not a record type.
    assert!(check_has_compile_errors(
        "fn main() -> Int { let r = Option { x: 1 }\n0 }"
    ));
}

#[test]
fn eval_path_record_lit_user_adt_rejected() {
    assert!(check_has_compile_errors(
        "type Foo = A | B\nfn main() -> Int { let r = Foo { x: 1 }\n0 }"
    ));
}

#[test]
fn eval_path_record_lit_valid_record_works() {
    // Guard: legitimate named record literals still work end-to-end.
    let val = run_ok(
        "type Point = { x: Int, y: Int }\nfn main() -> Int { let p = Point { x: 1, y: 2 }\np.x + p.y }",
    );
    assert_eq!(val, Value::Int(3));
}

#[test]
fn eval_string_no_escapes_unchanged() {
    // Guard: plain strings without escapes still work.
    let val = run_ok(r#"fn main() -> String { "hello world" }"#);
    assert_eq!(val, Value::String("hello world".to_owned()));
}

#[test]
fn eval_invalid_escape_produces_diagnostic() {
    // Guard: invalid escape sequence is flagged at compile time.
    assert!(check_has_compile_errors(r#"fn main() -> String { "\q" }"#));
}

// ── Modulo operator tests ───────────────────────────────────────────

#[test]
fn eval_modulo_basic() {
    let val = run_ok("fn main() -> Int { 10 % 3 }");
    assert_eq!(val, Value::Int(1));
}

#[test]
fn eval_modulo_zero() {
    let val = run_ok("fn main() -> Int { 7 % 7 }");
    assert_eq!(val, Value::Int(0));
}

#[test]
fn eval_modulo_larger_divisor() {
    let val = run_ok("fn main() -> Int { 3 % 10 }");
    assert_eq!(val, Value::Int(3));
}

#[test]
fn eval_modulo_negative() {
    // Rust's checked_rem preserves sign of dividend.
    // This parses as 0 - (7 % 3) = 0 - 1 = -1 due to precedence.
    let val = run_ok("fn main() -> Int { 0 - 7 % 3 }");
    assert_eq!(val, Value::Int(-1));
}

#[test]
fn eval_modulo_division_by_zero() {
    let err = run_err("fn main() -> Int { 10 % 0 }");
    assert!(err.contains("division by zero"), "got: {err}");
}

#[test]
fn eval_modulo_float() {
    let val = run_ok("fn main() -> Float { 5.5 % 2.0 }");
    assert!(matches!(val, Value::Float(f) if (f - 1.5).abs() < 1e-10));
}

#[test]
fn eval_modulo_precedence_same_as_mul() {
    // % has same precedence as * and /, left-associative.
    // 10 % 3 * 2 should be (10 % 3) * 2 = 1 * 2 = 2
    let val = run_ok("fn main() -> Int { 10 % 3 * 2 }");
    assert_eq!(val, Value::Int(2));
}

#[test]
fn eval_modulo_precedence_lower_than_nothing_higher_than_add() {
    // 1 + 10 % 3 should be 1 + (10 % 3) = 1 + 1 = 2
    let val = run_ok("fn main() -> Int { 1 + 10 % 3 }");
    assert_eq!(val, Value::Int(2));
}

#[test]
fn eval_modulo_type_error() {
    assert!(check_has_compile_errors(
        r#"fn main() -> Int { "hello" % 2 }"#
    ));
}

// ── Logical AND operator tests ──────────────────────────────────────

#[test]
fn eval_and_true_true() {
    let val = run_ok("fn main() -> Bool { true && true }");
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn eval_and_true_false() {
    let val = run_ok("fn main() -> Bool { true && false }");
    assert_eq!(val, Value::Bool(false));
}

#[test]
fn eval_and_false_true() {
    let val = run_ok("fn main() -> Bool { false && true }");
    assert_eq!(val, Value::Bool(false));
}

#[test]
fn eval_and_false_false() {
    let val = run_ok("fn main() -> Bool { false && false }");
    assert_eq!(val, Value::Bool(false));
}

#[test]
fn eval_and_short_circuit() {
    // If LHS is false, RHS should not be evaluated.
    // 1 / 0 would cause a runtime error, but && should short-circuit.
    let val = run_ok("fn main() -> Bool { false && 1 / 0 == 0 }");
    assert_eq!(val, Value::Bool(false));
}

#[test]
fn eval_and_with_comparisons() {
    let val = run_ok("fn main() -> Bool { 1 > 0 && 2 > 1 }");
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn eval_and_with_comparisons_false() {
    let val = run_ok("fn main() -> Bool { 1 > 0 && 2 < 1 }");
    assert_eq!(val, Value::Bool(false));
}

#[test]
fn eval_and_type_error() {
    assert!(check_has_compile_errors("fn main() -> Bool { 1 && true }"));
}

// ── Logical OR operator tests ───────────────────────────────────────

#[test]
fn eval_or_true_true() {
    let val = run_ok("fn main() -> Bool { true || true }");
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn eval_or_true_false() {
    let val = run_ok("fn main() -> Bool { true || false }");
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn eval_or_false_true() {
    let val = run_ok("fn main() -> Bool { false || true }");
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn eval_or_false_false() {
    let val = run_ok("fn main() -> Bool { false || false }");
    assert_eq!(val, Value::Bool(false));
}

#[test]
fn eval_or_short_circuit() {
    // If LHS is true, RHS should not be evaluated.
    // 1 / 0 would cause a runtime error, but || should short-circuit.
    let val = run_ok("fn main() -> Bool { true || 1 / 0 == 0 }");
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn eval_and_short_circuit_inside_lambda() {
    let src = r#"
        fn main() -> Bool {
            let f = fn() => false && 1 / 0 == 0
            f()
        }
    "#;
    let val = run_ok(src);
    assert_eq!(val, Value::Bool(false));
}

#[test]
fn eval_or_short_circuit_inside_lambda() {
    let src = r#"
        fn main() -> Bool {
            let f = fn() => true || 1 / 0 == 0
            f()
        }
    "#;
    let val = run_ok(src);
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn eval_or_with_comparisons() {
    let val = run_ok("fn main() -> Bool { 1 < 0 || 2 > 1 }");
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn eval_or_type_error() {
    assert!(check_has_compile_errors("fn main() -> Bool { 1 || true }"));
}

// ── Precedence: && binds tighter than || ────────────────────────────

#[test]
fn eval_or_and_precedence() {
    // true || false && false should be true || (false && false) = true || false = true
    let val = run_ok("fn main() -> Bool { true || false && false }");
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn eval_and_or_precedence() {
    // false && true || true should be (false && true) || true = false || true = true
    let val = run_ok("fn main() -> Bool { false && true || true }");
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn eval_and_or_precedence_2() {
    // false || false && true should be false || (false && true) = false || false = false
    let val = run_ok("fn main() -> Bool { false || false && true }");
    assert_eq!(val, Value::Bool(false));
}

// ── Combined operator tests ────────────────────────────────────────

#[test]
fn eval_modulo_with_logical() {
    // 10 % 2 == 0 && 9 % 3 == 0
    let val = run_ok("fn main() -> Bool { 10 % 2 == 0 && 9 % 3 == 0 }");
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn eval_modulo_with_logical_false() {
    let val = run_ok("fn main() -> Bool { 10 % 3 == 0 || 9 % 2 == 0 }");
    assert_eq!(val, Value::Bool(false));
}

#[test]
fn eval_chained_and() {
    let val = run_ok("fn main() -> Bool { true && true && true && true }");
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn eval_chained_or() {
    let val = run_ok("fn main() -> Bool { false || false || false || true }");
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn eval_not_and_or() {
    // !false && true should be true
    let val = run_ok("fn main() -> Bool { !false && true }");
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn eval_not_or() {
    // !true || false should be false
    let val = run_ok("fn main() -> Bool { !true || false }");
    assert_eq!(val, Value::Bool(false));
}

// ── Bitwise AND (&) ────────────────────────────────────────────────

#[test]
fn eval_bitwise_and_basic() {
    // 12 & 10 = 8 (binary: 1100 & 1010 = 1000)
    let val = run_ok("fn main() -> Int { 12 & 10 }");
    assert_eq!(val, Value::Int(8));
}

#[test]
fn eval_bitwise_and_identity() {
    // x & -1 == x (all bits set)
    let val = run_ok("fn main() -> Int { 42 & -1 }");
    assert_eq!(val, Value::Int(42));
}

#[test]
fn eval_bitwise_and_zero() {
    let val = run_ok("fn main() -> Int { 255 & 0 }");
    assert_eq!(val, Value::Int(0));
}

// ── Bitwise OR (|) ─────────────────────────────────────────────────

#[test]
fn eval_bitwise_or_basic() {
    // 12 | 10 = 14 (binary: 1100 | 1010 = 1110)
    let val = run_ok("fn main() -> Int { 12 | 10 }");
    assert_eq!(val, Value::Int(14));
}

#[test]
fn eval_bitwise_or_zero() {
    let val = run_ok("fn main() -> Int { 42 | 0 }");
    assert_eq!(val, Value::Int(42));
}

// ── Bitwise XOR (^) ────────────────────────────────────────────────

#[test]
fn eval_bitwise_xor_basic() {
    // 12 ^ 10 = 6 (binary: 1100 ^ 1010 = 0110)
    let val = run_ok("fn main() -> Int { 12 ^ 10 }");
    assert_eq!(val, Value::Int(6));
}

#[test]
fn eval_bitwise_xor_self_is_zero() {
    let val = run_ok("fn main() -> Int { 42 ^ 42 }");
    assert_eq!(val, Value::Int(0));
}

#[test]
fn eval_bitwise_xor_zero_identity() {
    let val = run_ok("fn main() -> Int { 42 ^ 0 }");
    assert_eq!(val, Value::Int(42));
}

// ── Left shift (<<) ────────────────────────────────────────────────

#[test]
fn eval_shl_basic() {
    let val = run_ok("fn main() -> Int { 1 << 3 }");
    assert_eq!(val, Value::Int(8));
}

#[test]
fn eval_shl_zero() {
    let val = run_ok("fn main() -> Int { 42 << 0 }");
    assert_eq!(val, Value::Int(42));
}

#[test]
fn eval_shl_out_of_range() {
    let err = run_err("fn main() -> Int { 1 << 64 }");
    assert!(err.contains("out of range"), "got: {err}");
}

#[test]
fn eval_shl_negative_shift() {
    let err = run_err("fn main() -> Int { 1 << -1 }");
    assert!(err.contains("out of range"), "got: {err}");
}

// ── Right shift (>>) ───────────────────────────────────────────────

#[test]
fn eval_shr_basic() {
    let val = run_ok("fn main() -> Int { 16 >> 2 }");
    assert_eq!(val, Value::Int(4));
}

#[test]
fn eval_shr_arithmetic() {
    // Arithmetic right shift: -8 >> 1 should be -4 (sign-extending)
    let val = run_ok("fn main() -> Int { -8 >> 1 }");
    assert_eq!(val, Value::Int(-4));
}

#[test]
fn eval_shr_zero() {
    let val = run_ok("fn main() -> Int { 42 >> 0 }");
    assert_eq!(val, Value::Int(42));
}

#[test]
fn eval_shr_out_of_range() {
    let err = run_err("fn main() -> Int { 1 >> 64 }");
    assert!(err.contains("out of range"), "got: {err}");
}

// ── Bitwise NOT (~) ────────────────────────────────────────────────

#[test]
fn eval_bitwise_not_zero() {
    let val = run_ok("fn main() -> Int { ~0 }");
    assert_eq!(val, Value::Int(-1));
}

#[test]
fn eval_bitwise_not_neg_one() {
    let val = run_ok("fn main() -> Int { ~-1 }");
    assert_eq!(val, Value::Int(0));
}

#[test]
fn eval_bitwise_not_positive() {
    let val = run_ok("fn main() -> Int { ~42 }");
    assert_eq!(val, Value::Int(!42_i64));
}

#[test]
fn eval_bitwise_not_double() {
    // ~~x == x
    let val = run_ok("fn main() -> Int { ~~100 }");
    assert_eq!(val, Value::Int(100));
}

// ── Precedence tests ───────────────────────────────────────────────

#[test]
fn eval_bitwise_precedence_and_before_or() {
    // a | b & c should parse as a | (b & c)
    // 1 | (3 & 2) = 1 | 2 = 3
    let val = run_ok("fn main() -> Int { 1 | 3 & 2 }");
    assert_eq!(val, Value::Int(3));
}

#[test]
fn eval_bitwise_precedence_xor_between_and_or() {
    // a | b ^ c & d should parse as a | (b ^ (c & d))
    // 8 | (4 ^ (6 & 3)) = 8 | (4 ^ 2) = 8 | 6 = 14
    let val = run_ok("fn main() -> Int { 8 | 4 ^ 6 & 3 }");
    assert_eq!(val, Value::Int(14));
}

#[test]
fn eval_bitwise_precedence_add_before_shift() {
    // a + b << c should parse as (a + b) << c — add binds tighter than shift
    let val = run_ok("fn main() -> Int { 1 + 1 << 3 }");
    assert_eq!(val, Value::Int(16)); // (1 + 1) << 3 = 2 << 3 = 16
}

#[test]
fn eval_bitwise_above_comparison() {
    // Bitwise ops bind tighter than comparison (fixed precedence, unlike C)
    // a == b & c should parse as a == (b & c)
    let val = run_ok("fn main() -> Bool { 2 == 3 & 2 }");
    assert_eq!(val, Value::Bool(true)); // 2 == (3 & 2) = 2 == 2 = true
}

#[test]
fn eval_tilde_higher_than_shift() {
    // ~a << b should parse as (~a) << b
    let val = run_ok("fn main() -> Int { ~0 << 8 }");
    assert_eq!(val, Value::Int(-1_i64 << 8)); // (~0) << 8 = -1 << 8 = -256
}

// ── Interaction with logical operators ─────────────────────────────

#[test]
fn eval_bitwise_and_vs_logical_and() {
    // & is bitwise (Int), && is logical (Bool)
    let val = run_ok("fn main() -> Bool { (3 & 1) == 1 && true }");
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn eval_bitwise_or_vs_logical_or() {
    // | is bitwise (Int), || is logical (Bool)
    let val = run_ok("fn main() -> Bool { (0 | 0) == 0 || false }");
    assert_eq!(val, Value::Bool(true));
}

// ── Type error tests ───────────────────────────────────────────────

#[test]
fn eval_bitwise_and_rejects_float() {
    assert!(check_has_compile_errors("fn main() -> Float { 1.0 & 2.0 }"));
}

#[test]
fn eval_bitwise_or_rejects_bool() {
    assert!(check_has_compile_errors(
        "fn main() -> Bool { true | false }"
    ));
}

#[test]
fn eval_bitwise_not_rejects_bool() {
    assert!(check_has_compile_errors("fn main() -> Bool { ~true }"));
}

#[test]
fn eval_shl_rejects_float() {
    assert!(check_has_compile_errors("fn main() -> Float { 1.0 << 2 }"));
}

// ── Combined bitwise expressions ───────────────────────────────────

#[test]
fn eval_bitwise_mask_and_shift() {
    // Extract bits [4:7] from 0xFF: (0xFF >> 4) & 0xF = 0xF = 15
    let val = run_ok("fn main() -> Int { (255 >> 4) & 15 }");
    assert_eq!(val, Value::Int(15));
}

#[test]
fn eval_bitwise_set_bit() {
    // Set bit 3: 0 | (1 << 3) = 8
    let val = run_ok("fn main() -> Int { 0 | (1 << 3) }");
    assert_eq!(val, Value::Int(8));
}

#[test]
fn eval_bitwise_clear_bit() {
    // Clear bit 1 from 0b111: 7 & ~(1 << 1) = 7 & ~2 = 7 & (-3) = 5
    let val = run_ok("fn main() -> Int { 7 & ~(1 << 1) }");
    assert_eq!(val, Value::Int(5));
}

#[test]
fn eval_bitwise_toggle_bit() {
    // Toggle bit 2: 0b101 ^ (1 << 2) = 5 ^ 4 = 1
    let val = run_ok("fn main() -> Int { 5 ^ (1 << 2) }");
    assert_eq!(val, Value::Int(1));
}

// ── Associativity chains ───────────────────────────────────────────

#[test]
fn eval_bitwise_and_left_assoc() {
    // Left-associative: (14 & 7) & 3 = 6 & 3 = 2
    let val = run_ok("fn main() -> Int { 14 & 7 & 3 }");
    assert_eq!(val, Value::Int(2));
}

#[test]
fn eval_bitwise_or_left_assoc() {
    // (1 | 2) | 4 = 3 | 4 = 7
    let val = run_ok("fn main() -> Int { 1 | 2 | 4 }");
    assert_eq!(val, Value::Int(7));
}

#[test]
fn eval_bitwise_xor_left_assoc() {
    // (5 ^ 3) ^ 6 = 6 ^ 6 = 0
    let val = run_ok("fn main() -> Int { 5 ^ 3 ^ 6 }");
    assert_eq!(val, Value::Int(0));
}

#[test]
fn eval_shl_left_assoc() {
    // (1 << 2) << 3 = 4 << 3 = 32
    let val = run_ok("fn main() -> Int { 1 << 2 << 3 }");
    assert_eq!(val, Value::Int(32));
}

#[test]
fn eval_shr_left_assoc() {
    // (64 >> 2) >> 1 = 16 >> 1 = 8
    let val = run_ok("fn main() -> Int { 64 >> 2 >> 1 }");
    assert_eq!(val, Value::Int(8));
}

// ── More mixed operator expressions ────────────────────────────────

#[test]
fn eval_bitwise_in_let_binding() {
    let val = run_ok(
        "fn main() -> Int {
            let flags = 1 | 2 | 4
            let mask = 3
            flags & mask
        }",
    );
    assert_eq!(val, Value::Int(3));
}

#[test]
fn eval_shift_in_arithmetic() {
    // (1 << 4) + (1 << 2) = 16 + 4 = 20
    let val = run_ok("fn main() -> Int { (1 << 4) + (1 << 2) }");
    assert_eq!(val, Value::Int(20));
}

#[test]
fn eval_xor_swap_pattern() {
    let val = run_ok(
        "fn main() -> Int {
            let a = 5
            let b = 3
            let a2 = a ^ b
            let b2 = b ^ a2
            let a3 = a2 ^ b2
            a3 + b2 * 100
        }",
    );
    // a3 = 3 (original b), b2 = 5 (original a)
    assert_eq!(val, Value::Int(3 + 5 * 100));
}

#[test]
fn eval_all_bitwise_ops_combined() {
    let val = run_ok(
        "fn main() -> Int {
            let a = 255
            let c = (a & 15) | (a >> 4)
            let d = c ^ (1 << 3)
            d
        }",
    );
    // a & 15 = 15, a >> 4 = 15, c = 15 | 15 = 15, d = 15 ^ 8 = 7
    assert_eq!(val, Value::Int(7));
}

#[test]
fn eval_modulo_with_bitwise() {
    // % has higher BP than &, so: (17 % 10) & 3 = 7 & 3 = 3
    let val = run_ok("fn main() -> Int { 17 % 10 & 3 }");
    assert_eq!(val, Value::Int(3));
}

#[test]
fn eval_logical_with_bitwise_comparison() {
    let val = run_ok(
        "fn main() -> Bool {
            let flags = 7
            let bit0 = flags & 1
            let bit2 = flags & 4
            bit0 != 0 && bit2 != 0
        }",
    );
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn eval_nested_tilde() {
    // ~~~x == ~x (triple negation = single negation)
    let val = run_ok("fn main() -> Int { ~~~42 }");
    assert_eq!(val, Value::Int(!42_i64));
}

#[test]
fn eval_tilde_and_logical_not_distinct() {
    let val = run_ok("fn main() -> Bool { ~0 == -1 && !false }");
    assert_eq!(val, Value::Bool(true));
}

// ── parse_int tests ─────────────────────────────────────────────────
// parse_int returns Result<Int, ParseError>.

#[test]
fn eval_parse_int_basic() {
    let val = run_ok(
        r#"fn main() -> Int {
            match ("42".parse_int()) {
                Ok(n) => n
                Err(_) => -1
            }
        }"#,
    );
    assert_eq!(val, Value::Int(42));
}

#[test]
fn eval_parse_int_negative() {
    let val = run_ok(
        r#"fn main() -> Int {
            match ("-7".parse_int()) {
                Ok(n) => n
                Err(_) => 0
            }
        }"#,
    );
    assert_eq!(val, Value::Int(-7));
}

#[test]
fn eval_parse_int_zero() {
    let val = run_ok(
        r#"fn main() -> Int {
            match ("0".parse_int()) {
                Ok(n) => n
                Err(_) => -1
            }
        }"#,
    );
    assert_eq!(val, Value::Int(0));
}

#[test]
fn eval_parse_int_with_plus() {
    let val = run_ok(
        r#"fn main() -> Int {
            match ("+42".parse_int()) {
                Ok(n) => n
                Err(_) => -1
            }
        }"#,
    );
    assert_eq!(val, Value::Int(42));
}

#[test]
fn eval_parse_int_max() {
    let val = run_ok(
        r#"fn main() -> Int {
            match ("9223372036854775807".parse_int()) {
                Ok(n) => n
                Err(_) => 0
            }
        }"#,
    );
    assert_eq!(val, Value::Int(i64::MAX));
}

#[test]
fn eval_parse_int_min() {
    let val = run_ok(
        r#"fn main() -> Int {
            match ("-9223372036854775808".parse_int()) {
                Ok(n) => n
                Err(_) => 0
            }
        }"#,
    );
    assert_eq!(val, Value::Int(i64::MIN));
}

#[test]
fn eval_parse_int_empty_fails() {
    let val = run_ok(
        r#"fn main() -> Bool {
            match ("".parse_int()) {
                Ok(_) => false
                Err(_) => true
            }
        }"#,
    );
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn eval_parse_int_non_numeric_fails() {
    let val = run_ok(
        r#"fn main() -> Bool {
            match ("abc".parse_int()) {
                Ok(_) => false
                Err(_) => true
            }
        }"#,
    );
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn eval_parse_int_float_string_fails() {
    let val = run_ok(
        r#"fn main() -> Bool {
            match ("3.14".parse_int()) {
                Ok(_) => false
                Err(_) => true
            }
        }"#,
    );
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn eval_parse_int_whitespace_fails() {
    let val = run_ok(
        r#"fn main() -> Bool {
            match (" 42".parse_int()) {
                Ok(_) => false
                Err(_) => true
            }
        }"#,
    );
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn eval_parse_int_overflow_fails() {
    let val = run_ok(
        r#"fn main() -> Bool {
            match ("9223372036854775808".parse_int()) {
                Ok(_) => false
                Err(_) => true
            }
        }"#,
    );
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn eval_parse_int_unwrap_or_ok_returns_inner_value() {
    let val = run_ok(r#"fn main() -> Int { "42".parse_int().unwrap_or(0) }"#);
    assert_eq!(val, Value::Int(42));
}

#[test]
fn eval_parse_int_unwrap_or_err_returns_default() {
    let val = run_ok(r#"fn main() -> Int { "oops".parse_int().unwrap_or(7) }"#);
    assert_eq!(val, Value::Int(7));
}

#[test]
fn eval_parse_int_map_or_ok_applies_mapper() {
    let val = run_ok(
        r#"fn main() -> Int {
            "41".parse_int().map_or(0, fn(n: Int) => n + 1)
        }"#,
    );
    assert_eq!(val, Value::Int(42));
}

#[test]
fn eval_parse_int_map_or_err_returns_default() {
    let val = run_ok(
        r#"fn main() -> Int {
            "oops".parse_int().map_or(9, fn(n: Int) => n + 1)
        }"#,
    );
    assert_eq!(val, Value::Int(9));
}

#[test]
fn eval_option_unwrap_or_some_returns_inner_value() {
    let val = run_ok(r#"fn main() -> Int { collections.List.new().push(42).head().unwrap_or(0) }"#);
    assert_eq!(val, Value::Int(42));
}

#[test]
fn eval_option_unwrap_or_none_returns_default() {
    let val = run_ok(r#"fn main() -> Int { collections.List.new().head().unwrap_or(7) }"#);
    assert_eq!(val, Value::Int(7));
}

#[test]
fn eval_option_map_some_applies_mapper() {
    let val = run_ok(
        r#"fn main() -> Int {
            collections.List.new().push(41).head().map(fn(n: Int) => n + 1).unwrap_or(0)
        }"#,
    );
    assert_eq!(val, Value::Int(42));
}

#[test]
fn eval_option_map_or_none_returns_default() {
    let val = run_ok(
        r#"fn main() -> Int {
            collections.List.new().head().map_or(9, fn(n: Int) => n + 1)
        }"#,
    );
    assert_eq!(val, Value::Int(9));
}

#[test]
fn eval_option_and_then_some_chains() {
    let val = run_ok(
        r#"fn main() -> Int {
            collections.List.new().push(41).head().and_then(fn(n: Int) => Some(n + 1)).unwrap_or(0)
        }"#,
    );
    assert_eq!(val, Value::Int(42));
}

#[test]
fn eval_option_and_then_none_short_circuits() {
    let val = run_ok(
        r#"fn main() -> Int {
            collections.List.new().head().and_then(fn(n: Int) => Some(n + 1)).unwrap_or(9)
        }"#,
    );
    assert_eq!(val, Value::Int(9));
}

#[test]
fn eval_result_map_ok_applies_mapper() {
    let val = run_ok(
        r#"fn main() -> Int {
            "41".parse_int().map(fn(n: Int) => n + 1).unwrap_or(0)
        }"#,
    );
    assert_eq!(val, Value::Int(42));
}

#[test]
fn eval_result_map_err_preserves_error_branch() {
    let val = run_ok(
        r#"fn main() -> Int {
            match ("oops".parse_int().map(fn(n: Int) => n + 1)) {
                Ok(n) => n
                Err(_) => 9
            }
        }"#,
    );
    assert_eq!(val, Value::Int(9));
}

#[test]
fn eval_result_and_then_ok_applies_mapper() {
    let val = run_ok(
        r#"fn main() -> Int {
            "41".parse_int().and_then(fn(n: Int) => Ok(n + 1)).unwrap_or(0)
        }"#,
    );
    assert_eq!(val, Value::Int(42));
}

#[test]
fn eval_result_and_then_err_short_circuits() {
    let val = run_ok(
        r#"fn main() -> Int {
            match ("oops".parse_int().and_then(fn(n: Int) => Ok(n + 1))) {
                Ok(n) => n
                Err(_) => 9
            }
        }"#,
    );
    assert_eq!(val, Value::Int(9));
}

#[test]
fn eval_result_map_err_err_applies_mapper() {
    let val = run_ok(
        r#"fn main() -> Int {
            match ("oops".parse_int().map_err(fn(_e: ParseError) => 7)) {
                Ok(n) => n
                Err(e) => e
            }
        }"#,
    );
    assert_eq!(val, Value::Int(7));
}

#[test]
fn eval_result_map_err_ok_preserves_ok_branch() {
    let val = run_ok(
        r#"fn main() -> Int {
            match ("41".parse_int().map_err(fn(_e: ParseError) => 7)) {
                Ok(n) => n
                Err(e) => e
            }
        }"#,
    );
    assert_eq!(val, Value::Int(41));
}

// ── parse_float tests ───────────────────────────────────────────────
// parse_float returns Result<Float, ParseError>.

#[test]
fn eval_parse_float_basic() {
    let val = run_ok(
        r#"fn main() -> Float {
            match ("3.14".parse_float()) {
                Ok(f) => f
                Err(_) => 0.0
            }
        }"#,
    );
    assert_eq!(val, Value::Float(314.0 / 100.0));
}

#[test]
fn eval_parse_float_integer_string() {
    let val = run_ok(
        r#"fn main() -> Float {
            match ("42".parse_float()) {
                Ok(f) => f
                Err(_) => 0.0
            }
        }"#,
    );
    assert_eq!(val, Value::Float(42.0));
}

#[test]
fn eval_parse_float_negative() {
    let val = run_ok(
        r#"fn main() -> Float {
            match ("-2.5".parse_float()) {
                Ok(f) => f
                Err(_) => 0.0
            }
        }"#,
    );
    assert_eq!(val, Value::Float(-2.5));
}

#[test]
fn eval_parse_float_zero() {
    let val = run_ok(
        r#"fn main() -> Float {
            match ("0.0".parse_float()) {
                Ok(f) => f
                Err(_) => 1.0
            }
        }"#,
    );
    assert_eq!(val, Value::Float(0.0));
}

#[test]
fn eval_parse_float_scientific() {
    let val = run_ok(
        r#"fn main() -> Float {
            match ("1.5e10".parse_float()) {
                Ok(f) => f
                Err(_) => 0.0
            }
        }"#,
    );
    assert_eq!(val, Value::Float(1.5e10));
}

#[test]
fn eval_parse_float_infinity() {
    let val = run_ok(
        r#"fn main() -> Float {
            match ("inf".parse_float()) {
                Ok(f) => f
                Err(_) => 0.0
            }
        }"#,
    );
    assert_eq!(val, Value::Float(f64::INFINITY));
}

#[test]
fn eval_parse_float_neg_infinity() {
    let val = run_ok(
        r#"fn main() -> Float {
            match ("-inf".parse_float()) {
                Ok(f) => f
                Err(_) => 0.0
            }
        }"#,
    );
    assert_eq!(val, Value::Float(f64::NEG_INFINITY));
}

#[test]
fn eval_parse_float_nan() {
    let val = run_ok(
        r#"fn main() -> Float {
            match ("NaN".parse_float()) {
                Ok(f) => f
                Err(_) => 0.0
            }
        }"#,
    );
    match val {
        Value::Float(f) => assert!(f.is_nan(), "expected NaN, got {f}"),
        other => panic!("expected Float, got {other:?}"),
    }
}

#[test]
fn eval_parse_float_empty_fails() {
    let val = run_ok(
        r#"fn main() -> Bool {
            match ("".parse_float()) {
                Ok(_) => false
                Err(_) => true
            }
        }"#,
    );
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn eval_parse_float_non_numeric_fails() {
    let val = run_ok(
        r#"fn main() -> Bool {
            match ("abc".parse_float()) {
                Ok(_) => false
                Err(_) => true
            }
        }"#,
    );
    assert_eq!(val, Value::Bool(true));
}

// ── ParseError variant matching tests ────────────────────────────────

#[test]
fn eval_parse_int_error_is_invalid_int() {
    let val = run_ok(
        r#"fn main() -> Bool {
            match ("abc".parse_int()) {
                Ok(_) => false
                Err(e) => match (e) {
                    InvalidInt(_) => true
                    InvalidFloat(_) => false
                }
            }
        }"#,
    );
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn eval_parse_float_error_is_invalid_float() {
    let val = run_ok(
        r#"fn main() -> Bool {
            match ("xyz".parse_float()) {
                Ok(_) => false
                Err(e) => match (e) {
                    InvalidInt(_) => false
                    InvalidFloat(_) => true
                }
            }
        }"#,
    );
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn eval_parse_int_error_carries_message() {
    let val = run_ok(
        r#"fn main() -> Bool {
            match ("not_a_number".parse_int()) {
                Ok(_) => false
                Err(e) => match (e) {
                    InvalidInt(msg) => msg.len() > 0
                    InvalidFloat(_) => false
                }
            }
        }"#,
    );
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn eval_parse_float_error_carries_message() {
    let val = run_ok(
        r#"fn main() -> Bool {
            match ("not_a_float".parse_float()) {
                Ok(_) => false
                Err(e) => match (e) {
                    InvalidInt(_) => false
                    InvalidFloat(msg) => msg.len() > 0
                }
            }
        }"#,
    );
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn eval_parse_int_user_defined_parse_error_does_not_override_core_behavior() {
    let val = run_ok(
        r#"type ParseError = Oops
           fn main() -> Bool {
             match ("abc".parse_int()) {
               Ok(_) => false
               Err(_) => true
             }
           }"#,
    );
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn eval_parse_float_user_defined_parse_error_does_not_override_core_behavior() {
    let val = run_ok(
        r#"type ParseError = Oops
           fn main() -> Bool {
             match ("abc".parse_float()) {
               Ok(_) => false
               Err(_) => true
             }
           }"#,
    );
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn eval_parse_int_user_defined_parse_error_reserved_variants_are_compile_error() {
    let err = run_err(
        r#"type ParseError = InvalidInt(Int) | InvalidFloat(Int)
           fn main() -> Bool {
             match ("abc".parse_int()) {
               Ok(_) => false
               Err(_) => true
             }
           }"#,
    );
    assert!(
        err.contains("constructor `InvalidInt` is reserved for core stdlib"),
        "unexpected error: {err}"
    );
}

#[test]
fn eval_collections_mutable_priority_queue_constructor_surface_rfc_0012() {
    let val = run_ok(
        "import collections
         fn main() -> Int {
           let pq: MutablePriorityQueue<Int, String> = collections.MutablePriorityQueue.new_min()
               .push(5, \"far\")
               .push(1, \"near\")
           match (pq.peek()) {
             Some(item) => item.priority + pq.len()
             None => 0
           }
         }",
    );
    assert!(matches!(val, Value::Int(3)));
}

#[test]
fn eval_collections_mutable_priority_queue_alias_constructor_surface_rfc_0012() {
    let val = run_ok(
        "import collections as c
         fn main() -> Int {
           let pq: MutablePriorityQueue<Int, String> = c.MutablePriorityQueue.new_max()
               .push(1, \"low\")
               .push(9, \"high\")
           match (pq.pop()) {
             Some(item) => item.priority
             None => 0
           }
         }",
    );
    assert!(matches!(val, Value::Int(9)));
}

#[test]
fn eval_global_mutable_priority_queue_constructor_surface_is_removed_rfc_0012() {
    let err = run_err(
        "fn main() -> Int { let pq: MutablePriorityQueue<Int, Int> = MutablePriorityQueue.new_min() pq.len() }",
    );
    assert!(
        err.contains("no method `new_min`")
            || err.contains("unresolved name")
            || err.contains("import name"),
        "unexpected error: {err}"
    );
}

#[test]
fn eval_mutable_priority_queue_min_max_and_stable_ties() {
    let val = run_ok(
        r#"import collections
         fn main() -> Int {
           let minq: MutablePriorityQueue<Int, Int> = collections.MutablePriorityQueue.new_min()
               .push(5, 50)
               .push(1, 10)
               .push(1, 11)
           let maxq: MutablePriorityQueue<Int, Int> = collections.MutablePriorityQueue.new_max()
               .push(5, 50)
               .push(1, 10)
               .push(5, 51)

           let a = match (minq.pop()) { Some(item) => item.value None => 0 }
           let b = match (minq.pop()) { Some(item) => item.value None => 0 }
           let c = match (maxq.pop()) { Some(item) => item.value None => 0 }
           let d = match (maxq.pop()) { Some(item) => item.value None => 0 }
           a * 1000 + b * 100 + c * 10 + d
         }"#,
    );
    assert!(matches!(val, Value::Int(11651)));
}

#[test]
fn eval_mutable_priority_queue_aliases_observe_in_place_mutation() {
    let val = run_ok(
        r#"import collections
         fn main() -> Int {
           let pq: MutablePriorityQueue<Int, Int> = collections.MutablePriorityQueue.new_min()
           let alias = pq
           pq.push(3, 30).push(1, 10)
           let top = match (alias.peek()) { Some(item) => item.value None => 0 }
           top * 10 + alias.len()
         }"#,
    );
    assert!(matches!(val, Value::Int(102)));
}

#[test]
fn eval_mutable_priority_queue_empty_peek_and_pop_return_none() {
    let val = run_ok(
        r#"import collections
         fn main() -> Int {
           let pq: MutablePriorityQueue<Int, Int> = collections.MutablePriorityQueue.new_min()
           let a = match (pq.peek()) { Some(_) => 1 None => 0 }
           let b = match (pq.pop()) { Some(_) => 1 None => 0 }
           a * 10 + b
         }"#,
    );
    assert!(matches!(val, Value::Int(0)));
}

#[test]
fn eval_mutable_priority_queue_non_ord_priority_is_compile_error() {
    let err = run_err(
        r#"import collections
         fn main() -> Int {
           let pq: MutablePriorityQueue<Float, Int> = collections.MutablePriorityQueue.new_min()
           pq.len()
         }"#,
    );
    assert!(
        err.contains("Ord") || err.contains("trait"),
        "unexpected error: {err}"
    );
}

#[test]
fn eval_parse_float_user_defined_parse_error_reserved_variants_are_compile_error() {
    let err = run_err(
        r#"type ParseError = InvalidInt(Int) | InvalidFloat(Int)
           fn main() -> Bool {
             match ("abc".parse_float()) {
               Ok(_) => false
               Err(_) => true
             }
           }"#,
    );
    assert!(
        err.contains("constructor `InvalidInt` is reserved for core stdlib"),
        "unexpected error: {err}"
    );
}

// ── string_lines tests ─────────────────────────────────────────────

#[test]
fn eval_string_lines_basic() {
    let val = run_ok(r#"fn main() -> Int { "a\nb\nc".lines().count() }"#);
    assert_eq!(val, Value::Int(3));
}

#[test]
fn eval_string_lines_trailing_newline() {
    let val = run_ok(r#"fn main() -> Int { "a\nb\n".lines().count() }"#);
    assert_eq!(val, Value::Int(2));
}

#[test]
fn eval_string_lines_empty() {
    let val = run_ok(r#"fn main() -> Int { "".lines().count() }"#);
    assert_eq!(val, Value::Int(0));
}

#[test]
fn eval_string_lines_single() {
    let val = run_ok(
        r#"fn main() -> String {
            match ("hello".lines().to_list().head()) {
                Some(s) => s
                None => "fail"
            }
        }"#,
    );
    assert_eq!(val, Value::String("hello".to_string()));
}

#[test]
fn eval_string_lines_crlf() {
    let val = run_ok(r#"fn main() -> Int { "a\r\nb\r\nc".lines().count() }"#);
    assert_eq!(val, Value::Int(3));
}

#[test]
fn eval_string_lines_blank_lines() {
    // Two newlines = two empty lines (lines() strips trailing but keeps interior)
    let val = run_ok(r#"fn main() -> Int { "\n\n".lines().count() }"#);
    assert_eq!(val, Value::Int(2));
}

#[test]
fn eval_string_lines_content_check() {
    let val = run_ok(
        r#"fn main() -> String {
            let lines = "first\nsecond\nthird".lines().to_list()
            match (lines.get(1)) {
                Some(s) => s
                None => "fail"
            }
        }"#,
    );
    assert_eq!(val, Value::String("second".to_string()));
}

// ── string_chars tests ──────────────────────────────────────────────

#[test]
fn eval_string_chars_basic() {
    let val = run_ok(r#"fn main() -> Int { "hello".chars().count() }"#);
    assert_eq!(val, Value::Int(5));
}

#[test]
fn eval_string_chars_empty() {
    let val = run_ok(r#"fn main() -> Int { "".chars().count() }"#);
    assert_eq!(val, Value::Int(0));
}

#[test]
fn eval_string_chars_single() {
    let val = run_ok(r#"fn main() -> Int { "x".chars().count() }"#);
    assert_eq!(val, Value::Int(1));
}

#[test]
fn eval_string_chars_unicode() {
    // "café" has 4 chars (é is a single codepoint U+00E9)
    let val = run_ok(r#"fn main() -> Int { "café".chars().count() }"#);
    assert_eq!(val, Value::Int(4));
}

#[test]
fn eval_string_chars_roundtrip() {
    // Convert string to chars, map each char back to string, concat them
    let val = run_ok(
        r#"fn main() -> String {
            let chars = "abc".chars()
            let strings = chars.map(fn(c: Char) => c.to_string())
            strings.fold("", fn(acc: String, s: String) => acc.concat(s))
        }"#,
    );
    assert_eq!(val, Value::String("abc".to_string()));
}

#[test]
fn eval_string_chars_with_newlines() {
    // "a\nb" has 3 chars: 'a', '\n', 'b'
    let val = run_ok(r#"fn main() -> Int { "a\nb".chars().count() }"#);
    assert_eq!(val, Value::Int(3));
}

// ── read_file tests ─────────────────────────────────────────────────

#[test]
fn eval_read_file_basic() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.txt");
    std::fs::write(&file_path, "hello world").unwrap();
    let path_str = file_path.to_str().unwrap();
    let source = format!("import fs\nfn main() -> String {{ fs.read_file(\"{path_str}\") }}");
    let manifest = manifest_from_json(r#"{"caps": {"fs": {}}}"#);
    let val = run_with_manifest_ok(&source, Some(manifest));
    assert_eq!(val, Value::String("hello world".to_string()));
}

#[test]
fn eval_read_file_multiline() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("multi.txt");
    std::fs::write(&file_path, "line1\nline2\nline3\n").unwrap();
    let path_str = file_path.to_str().unwrap();
    let source =
        format!("import fs\nfn main() -> Int {{ fs.read_file(\"{path_str}\").lines().count() }}");
    let manifest = manifest_from_json(r#"{"caps": {"fs": {}}}"#);
    let val = run_with_manifest_ok(&source, Some(manifest));
    assert_eq!(val, Value::Int(3));
}

#[test]
fn eval_read_file_empty() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("empty.txt");
    std::fs::write(&file_path, "").unwrap();
    let path_str = file_path.to_str().unwrap();
    let source = format!("import fs\nfn main() -> String {{ fs.read_file(\"{path_str}\") }}");
    let manifest = manifest_from_json(r#"{"caps": {"fs": {}}}"#);
    let val = run_with_manifest_ok(&source, Some(manifest));
    assert_eq!(val, Value::String(String::new()));
}

#[test]
fn eval_read_file_no_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.txt");
    std::fs::write(&file_path, "allowed").unwrap();
    let path_str = file_path.to_str().unwrap();
    let source = format!("import fs\nfn main() -> String {{ fs.read_file(\"{path_str}\") }}");
    let val = run_with_manifest_ok(&source, None);
    assert_eq!(val, Value::String("allowed".to_string()));
}

#[test]
fn eval_read_file_not_found() {
    let source =
        "import fs\nfn main() -> String { fs.read_file(\"/nonexistent/path/to/file.txt\") }";
    let manifest = manifest_from_json(r#"{"caps": {"fs": {}}}"#);
    let err = run_with_manifest_err(source, Some(manifest));
    assert!(err.contains("read_file"), "got: {err}");
}

#[test]
fn eval_read_file_denied_no_cap() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.txt");
    std::fs::write(&file_path, "secret").unwrap();
    let path_str = file_path.to_str().unwrap();
    let source = format!("import fs\nfn main() -> String {{ fs.read_file(\"{path_str}\") }}");
    let manifest = manifest_from_json(r#"{"caps": {}}"#);
    let err = run_with_manifest_err(&source, Some(manifest));
    assert!(err.contains("capability denied"), "got: {err}");
    assert!(err.contains("fs"), "got: {err}");
}

#[test]
fn eval_read_file_denied_wrong_cap() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.txt");
    std::fs::write(&file_path, "secret").unwrap();
    let path_str = file_path.to_str().unwrap();
    let source = format!("import fs\nfn main() -> String {{ fs.read_file(\"{path_str}\") }}");
    let manifest = manifest_from_json(r#"{"caps": {"io": {}}}"#);
    let err = run_with_manifest_err(&source, Some(manifest));
    assert!(err.contains("capability denied"), "got: {err}");
}

#[test]
fn eval_read_file_with_both_caps() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.txt");
    std::fs::write(&file_path, "both caps").unwrap();
    let path_str = file_path.to_str().unwrap();
    let source = format!("import fs\nfn main() -> String {{ fs.read_file(\"{path_str}\") }}");
    let manifest = manifest_from_json(r#"{"caps": {"io": {}, "fs": {}}}"#);
    let val = run_with_manifest_ok(&source, Some(manifest));
    assert_eq!(val, Value::String("both caps".to_string()));
}

// ── list_sort tests ─────────────────────────────────────────────────

#[test]
fn eval_list_sort_ints() {
    let val = run_ok(
        "fn main() -> Int {
            let xs = collections.List.new().push(3).push(1).push(2)
            let sorted = xs.sort()
            match (sorted.get(0)) {
                Some(x) => x
                None => -1
            }
        }",
    );
    assert_eq!(val, Value::Int(1));
}

#[test]
fn eval_list_sort_ints_reverse() {
    let val = run_ok(
        "fn main() -> Int {
            let xs = collections.List.new().push(5).push(4).push(3).push(2).push(1)
            let sorted = xs.sort()
            match (sorted.get(4)) {
                Some(x) => x
                None => -1
            }
        }",
    );
    assert_eq!(val, Value::Int(5));
}

#[test]
fn eval_list_sort_ints_already_sorted() {
    let val = run_ok(
        "fn main() -> Int {
            let xs = collections.List.new().push(1).push(2).push(3)
            let sorted = xs.sort()
            match (sorted.get(2)) {
                Some(x) => x
                None => -1
            }
        }",
    );
    assert_eq!(val, Value::Int(3));
}

#[test]
fn eval_list_sort_ints_duplicates() {
    let val = run_ok(
        "fn main() -> Int {
            let xs = collections.List.new().push(3).push(1).push(3).push(2)
            let sorted = xs.sort()
            // sorted should be [1, 2, 3, 3], check index 2
            match (sorted.get(2)) {
                Some(x) => x
                None => -1
            }
        }",
    );
    assert_eq!(val, Value::Int(3));
}

#[test]
fn eval_list_sort_ints_negative() {
    let val = run_ok(
        "fn main() -> Int {
            let xs = collections.List.new().push(3).push(-1).push(0).push(-5)
            let sorted = xs.sort()
            match (sorted.get(0)) {
                Some(x) => x
                None => 0
            }
        }",
    );
    assert_eq!(val, Value::Int(-5));
}

#[test]
fn eval_list_sort_ints_single() {
    let val = run_ok(
        "fn main() -> Int {
            let xs = collections.List.new().push(42)
            let sorted = xs.sort()
            match (sorted.get(0)) {
                Some(x) => x
                None => -1
            }
        }",
    );
    assert_eq!(val, Value::Int(42));
}

#[test]
fn eval_list_sort_empty() {
    let val = run_ok(
        "fn main() -> Int {
            let xs: List<Int> = collections.List.new()
            let sorted = xs.sort()
            sorted.len()
        }",
    );
    assert_eq!(val, Value::Int(0));
}

#[test]
fn eval_list_sort_strings() {
    let val = run_ok(
        r#"fn main() -> String {
            let xs = collections.List.new().push("banana").push("apple").push("cherry")
            let sorted = xs.sort()
            match (sorted.get(0)) {
                Some(s) => s
                None => "fail"
            }
        }"#,
    );
    assert_eq!(val, Value::String("apple".to_string()));
}

#[test]
fn eval_list_sort_bools() {
    let val = run_ok(
        "fn main() -> Bool {
            let xs = collections.List.new().push(true).push(false).push(true)
            let sorted = xs.sort()
            match (sorted.get(0)) {
                Some(b) => b
                None => true
            }
        }",
    );
    assert_eq!(val, Value::Bool(false));
}

#[test]
fn eval_list_sort_floats_rejected() {
    let err = run_err(
        "fn main() -> Int {
            let xs = collections.List.new().push(3.0).push(1.0).push(2.0)
            let sorted = xs.sort()
            sorted.len()
        }",
    );
    assert!(
        err.contains("cannot be sorted"),
        "expected compile-time sort rejection, got: {err}"
    );
}

#[test]
fn eval_list_sort_floats_with_nan_rejected() {
    let err = run_err(
        r#"fn main() -> Int {
            let nan = match ("NaN".parse_float()) {
                Ok(f) => f
                Err(_) => 0.0
            }
            let xs = collections.List.new().push(nan).push(1.0).push(2.0)
            let sorted = xs.sort()
            sorted.len()
        }"#,
    );
    assert!(
        err.contains("cannot be sorted"),
        "expected compile-time sort rejection, got: {err}"
    );
}

#[test]
fn eval_list_sort_chars() {
    let val = run_ok(
        "fn main() -> Char {
            let xs = collections.List.new().push('c').push('a').push('b')
            let sorted = xs.sort()
            match (sorted.get(0)) {
                Some(c) => c
                None => 'z'
            }
        }",
    );
    assert_eq!(val, Value::Char('a'));
}

#[test]
fn eval_list_sort_lists_lexicographically() {
    let val = run_ok(
        "fn main() -> Int {
            let a = collections.List.new().push(1)
            let b = collections.List.new().push(1).push(2)
            let xs = collections.List.new().push(b).push(a)
            let sorted = xs.sort()
            match (sorted.get(0)) {
                Some(inner) => inner.len()
                None => -1
            }
        }",
    );
    assert_eq!(val, Value::Int(1));
}

#[test]
fn eval_list_binary_search_found_and_insertion_point() {
    let val = run_ok(
        "fn main() -> Int {
            let xs = collections.List.new().push(1).push(3).push(5).push(7)
            let found = xs.binary_search(5)
            let missing = xs.binary_search(6)
            found * 100 + missing
        }",
    );
    assert_eq!(val, Value::Int(196));
}

#[test]
fn eval_list_binary_search_empty_returns_negative_one() {
    let val = run_ok(
        "fn main() -> Int {
            let xs: List<Int> = collections.List.new()
            xs.binary_search(10)
        }",
    );
    assert_eq!(val, Value::Int(-1));
}

#[test]
fn eval_list_binary_search_lists() {
    let val = run_ok(
        "fn main() -> Int {
            let a = collections.List.new().push(1)
            let b = collections.List.new().push(1).push(2)
            let xs = collections.List.new().push(a).push(b)
            let needle = collections.List.new().push(1).push(2)
            xs.binary_search(needle)
        }",
    );
    assert_eq!(val, Value::Int(1));
}

// ── list_sort_by tests ──────────────────────────────────────────────

#[test]
fn eval_list_sort_by_ascending() {
    let val = run_ok(
        "fn main() -> Int {
            let xs = collections.List.new().push(3).push(1).push(2)
            let sorted = xs.sort_by(fn(a: Int, b: Int) => a - b)
            match (sorted.get(0)) {
                Some(x) => x
                None => -1
            }
        }",
    );
    assert_eq!(val, Value::Int(1));
}

#[test]
fn eval_list_sort_by_descending() {
    let val = run_ok(
        "fn main() -> Int {
            let xs = collections.List.new().push(3).push(1).push(2)
            let sorted = xs.sort_by(fn(a: Int, b: Int) => b - a)
            match (sorted.get(0)) {
                Some(x) => x
                None => -1
            }
        }",
    );
    assert_eq!(val, Value::Int(3));
}

#[test]
fn eval_list_sort_by_named_fn() {
    let val = run_ok(
        "fn cmp(a: Int, b: Int) -> Int { a - b }
        fn main() -> Int {
            let xs = collections.List.new().push(3).push(1).push(2)
            let sorted = xs.sort_by(cmp)
            match (sorted.get(0)) {
                Some(x) => x
                None => -1
            }
        }",
    );
    assert_eq!(val, Value::Int(1));
}

#[test]
fn eval_list_sort_by_empty() {
    let val = run_ok(
        "fn main() -> Int {
            let xs: List<Int> = collections.List.new()
            let sorted = xs.sort_by(fn(a: Int, b: Int) => a - b)
            sorted.len()
        }",
    );
    assert_eq!(val, Value::Int(0));
}

#[test]
fn eval_list_sort_by_single() {
    let val = run_ok(
        "fn main() -> Int {
            let xs = collections.List.new().push(42)
            let sorted = xs.sort_by(fn(a: Int, b: Int) => a - b)
            match (sorted.get(0)) {
                Some(x) => x
                None => -1
            }
        }",
    );
    assert_eq!(val, Value::Int(42));
}

#[test]
fn eval_list_sort_by_strings_by_len() {
    let val = run_ok(
        r#"fn main() -> String {
            let xs = collections.List.new().push("bb").push("a").push("ccc")
            let sorted = xs.sort_by(fn(a: String, b: String) => a.len() - b.len())
            match (sorted.get(0)) {
                Some(s) => s
                None => "fail"
            }
        }"#,
    );
    assert_eq!(val, Value::String("a".to_string()));
}

#[test]
fn eval_list_sort_by_stable() {
    // Sort by tens digit only — elements with same tens digit should keep original order.
    // Input: [21, 12, 11, 22] — sort by (x / 10): [12, 11, 21, 22]
    // 12 before 11 (both have tens=1, original order: 12 at index 1, 11 at index 2)
    let val = run_ok(
        "fn main() -> Int {
            let xs = collections.List.new().push(21).push(12).push(11).push(22)
            let sorted = xs.sort_by(fn(a: Int, b: Int) => a / 10 - b / 10)
            match (sorted.get(0)) {
                Some(x) => x
                None => -1
            }
        }",
    );
    // First element with tens digit 1 should be 12 (appeared before 11 in original)
    assert_eq!(val, Value::Int(12));
}

#[test]
fn eval_list_sort_by_already_sorted() {
    let val = run_ok(
        "fn main() -> Int {
            let xs = collections.List.new().push(1).push(2).push(3)
            let sorted = xs.sort_by(fn(a: Int, b: Int) => a - b)
            match (sorted.get(1)) {
                Some(x) => x
                None => -1
            }
        }",
    );
    assert_eq!(val, Value::Int(2));
}

#[test]
fn eval_list_sort_by_all_equal() {
    let val = run_ok(
        "fn main() -> Int {
            let xs = collections.List.new().push(5).push(5).push(5)
            let sorted = xs.sort_by(fn(a: Int, b: Int) => a - b)
            sorted.len()
        }",
    );
    assert_eq!(val, Value::Int(3));
}

#[test]
fn eval_list_sort_by_comparator_error() {
    // Comparator returns Bool instead of Int
    let err = run_err(
        "fn main() -> Int {
            let xs = collections.List.new().push(2).push(1)
            let sorted = xs.sort_by(fn(a: Int, b: Int) => a < b)
            sorted.len()
        }",
    );
    assert!(
        err.contains("list_sort_by") || err.contains("Int"),
        "got: {err}"
    );
}

#[test]
fn eval_list_sort_by_runtime_error() {
    // Comparator divides by zero — error should propagate
    let err = run_err(
        "fn main() -> Int {
            let xs = collections.List.new().push(2).push(1)
            let sorted = xs.sort_by(fn(a: Int, b: Int) => a / 0)
            sorted.len()
        }",
    );
    assert!(err.contains("division by zero"), "got: {err}");
}

// ── Iteration ergonomics tests (#259) ──────────────────────────────

#[test]
fn eval_list_iteration_ergonomics_matrix() {
    struct Case<'a> {
        name: &'a str,
        src: &'a str,
        expected: Value,
    }

    let cases = [
        Case {
            name: "range half-open",
            src: "fn main() -> Int {
                let xs = ((0)..<5).to_list()
                xs.len() * 100 + xs[0] * 10 + xs[4]
            }",
            expected: Value::Int(504),
        },
        Case {
            name: "range equal bounds empty",
            src: "fn main() -> Int { ((3)..<3).count() }",
            expected: Value::Int(0),
        },
        Case {
            name: "range descending empty",
            src: "fn main() -> Int { ((5)..<2).count() }",
            expected: Value::Int(0),
        },
        Case {
            name: "enumerate fields",
            src: "fn main() -> Int {
                let pairs = collections.List.new().push(10).push(20).enumerate().to_list()
                pairs[0].index + pairs[0].value + pairs[1].index + pairs[1].value
            }",
            expected: Value::Int(31),
        },
        Case {
            name: "zip truncates",
            src: "fn main() -> Int {
                let pairs = collections.List.new().push(1).push(2).push(3)
                    .zip(collections.List.new().push(10).push(20))
                    .to_list()
                pairs.len() * 100 + pairs[0].left * 10 + pairs[1].right
            }",
            expected: Value::Int(230),
        },
        Case {
            name: "chunks keeps tail",
            src: "fn main() -> Int {
                let cs = collections.List.new().push(1).push(2).push(3).push(4).push(5).chunks(2).to_list()
                cs.len() * 100 + cs[0].len() * 10 + cs[2].len()
            }",
            expected: Value::Int(321),
        },
        Case {
            name: "windows overlap",
            src: "fn main() -> Int {
                let ws = collections.List.new().push(1).push(2).push(3).push(4).push(5).windows(3).to_list()
                ws.len() * 100 + ws[0][0] * 10 + ws[2][2]
            }",
            expected: Value::Int(315),
        },
        Case {
            name: "windows larger than len empty",
            src: "fn main() -> Int {
                collections.List.new().push(1).push(2).windows(6).count()
            }",
            expected: Value::Int(0),
        },
        Case {
            name: "chaining from map set string",
            src: "fn main() -> Bool {
                let m = collections.Map.new().insert(\"x\", 1).insert(\"y\", 2)
                let km = m.keys().enumerate().to_list()
                let map_ok = km.len() == 2 && km[0].index == 0 && km[0].value == \"x\"

                let s = collections.Set.new().insert(\"a\").insert(\"b\").insert(\"c\")
                let sc = s.values().chunks(2).to_list()
                let set_ok = sc.len() == 2 && sc[0].len() == 2 && sc[1].len() == 1

                let p = \"abc\".chars().zip(collections.List.new().push(10).push(20)).to_list()
                let string_ok = p.len() == 2 && p[0].left == 'a' && p[1].right == 20

                map_ok && set_ok && string_ok
            }",
            expected: Value::Bool(true),
        },
    ];

    for case in cases {
        let got = run_ok(case.src);
        assert_eq!(got, case.expected, "case `{}` failed", case.name);
    }
}

#[test]
fn eval_list_chunks_windows_invalid_sizes_report_direct_errors() {
    struct Case<'a> {
        name: &'a str,
        src: &'a str,
        expected_fragment: &'a str,
    }

    let cases = [
        Case {
            name: "chunks zero",
            src: "fn main() -> Int { ((0)..<3).chunks(0).count() }",
            expected_fragment: "seq_chunks: chunk size must be > 0",
        },
        Case {
            name: "chunks negative",
            src: "fn main() -> Int { ((0)..<3).chunks(-1).count() }",
            expected_fragment: "seq_chunks: chunk size must be > 0",
        },
        Case {
            name: "windows zero",
            src: "fn main() -> Int { ((0)..<3).windows(0).count() }",
            expected_fragment: "seq_windows: window size must be > 0",
        },
        Case {
            name: "windows negative",
            src: "fn main() -> Int { ((0)..<3).windows(-3).count() }",
            expected_fragment: "seq_windows: window size must be > 0",
        },
    ];

    for case in cases {
        let err = run_err(case.src);
        assert!(
            err.contains(case.expected_fragment),
            "case `{}` expected error containing `{}`, got: {}",
            case.name,
            case.expected_fragment,
            err
        );
    }
}

#[test]
fn eval_seq_surface_happy_paths() {
    struct Case<'a> {
        name: &'a str,
        src: &'a str,
        expected: Value,
    }

    let cases = [
        Case {
            name: "seq range fold without list materialization",
            src: r#"fn main() -> Int {
                ((0)..<5).fold(0, fn(acc: Int, n: Int) => acc + n)
            }"#,
            expected: Value::Int(10),
        },
        Case {
            name: "list to seq map filter count",
            src: r#"fn main() -> Int {
                collections.List.new().push(1).push(2).push(3)
                    .map(fn(n: Int) => n + 1)
                    .filter(fn(n: Int) => n > 2)
                    .count()
            }"#,
            expected: Value::Int(2),
        },
        Case {
            name: "producer methods return seq",
            src: r#"fn main() -> Int {
                let a = "a,b,c".split(",").count()
                let b = "x\ny".lines().count()
                let c = "abc".chars().count()
                a + b + c
            }"#,
            expected: Value::Int(8),
        },
        Case {
            name: "seq is re-iterable",
            src: r#"fn main() -> Int {
                let xs = ((0)..<3).map(fn(n: Int) => n + 1)
                xs.count() * 10 + xs.to_list().len()
            }"#,
            expected: Value::Int(33),
        },
    ];

    for case in cases {
        let got = run_ok(case.src);
        assert_eq!(got, case.expected, "case `{}` failed", case.name);
    }
}

#[test]
fn eval_seq_flat_map_surface_happy_paths() {
    struct Case<'a> {
        name: &'a str,
        src: &'a str,
        expected: Value,
    }

    let cases = [
        Case {
            name: "builtin traversal sources flatten lazily",
            src: r#"import collections

fn main() -> Int {
    let a = collections.List.new().push(1).push(2)
        .flat_map(fn(n: Int) => collections.List.new().push(n).push(n + 10))
        .count()
    let b = collections.MutableList.from_list(collections.List.new().push(1).push(2))
        .flat_map(fn(n: Int) => collections.MutableList.from_list(collections.List.new().push(n).push(n + 1)))
        .count()
    let c = collections.Deque.new().push_back(1).push_back(2)
        .flat_map(fn(n: Int) => collections.Deque.new().push_back(n).push_back(n + 1))
        .count()
    let d = "a,b\nc,d".lines().flat_map(fn(line: String) => line.split(",")).count()
    a * 1000 + b * 100 + c * 10 + d
}"#,
            expected: Value::Int(4444),
        },
        Case {
            name: "user IntoTraversal impl participates and explicit qualified call works",
            src: r#"import collections

type Tokens = { items: List<String> }

impl IntoTraversal<String> for Tokens {
    fn into_seq(self) -> Seq<String> {
        self.items.map(fn(x: String) => x)
    }
}

fn tokenize(line: String) -> Tokens {
    Tokens { items: line.split(",").to_list() }
}

fn main() -> Int {
    let a = "a,b\nc,d".lines().flat_map(fn(line: String) => tokenize(line)).count()
    let b = IntoTraversal.into_seq(Tokens {
        items: collections.List.new().push("x").push("y")
    }).count()
    a * 10 + b
}"#,
            expected: Value::Int(42),
        },
        Case {
            name: "flat_map preserves short-circuit through inner traversals",
            src: r#"import collections

fn main() -> Int {
    let ok = collections.List.new().push(0).push(1)
        .flat_map(fn(n: Int) =>
            if (n == 0) {
                collections.List.new().push(0)
            } else {
                collections.List.new().push(10 / (n - 1))
            }
        )
        .any(fn(n: Int) => n == 0)
    if (ok) { 1 } else { 0 }
}"#,
            expected: Value::Int(1),
        },
    ];

    for case in cases {
        let got = run_ok(case.src);
        assert_eq!(got, case.expected, "case `{}` failed", case.name);
    }
}

#[test]
fn eval_seq_callable_values_work_when_reused_across_iterations() {
    struct Case<'a> {
        name: &'a str,
        src: &'a str,
        expected: Value,
    }

    let cases = [
        Case {
            name: "count predicate uses stored capturing lambda",
            src: r#"fn greater_than(limit: Int) -> fn(Int) -> Bool {
    fn(n: Int) => n > limit
}

fn main() -> Int {
    let pred = greater_than(2)
    (0..<6).count(pred)
}"#,
            expected: Value::Int(3),
        },
        Case {
            name: "flat_map uses stored capturing lambda",
            src: r#"import collections

fn spread(offset: Int) -> fn(Int) -> List<Int> {
    fn(n: Int) => collections.List.new().push(n).push(n + offset)
}

fn main() -> Int {
    let f = spread(10)
    (0..<3).flat_map(f).count()
}"#,
            expected: Value::Int(6),
        },
    ];

    for case in cases {
        let got = run_ok(case.src);
        assert_eq!(got, case.expected, "case `{}` failed", case.name);
    }
}

#[test]
fn eval_seq_flat_map_requires_into_traversal() {
    let err = run_err("fn main() -> Int { (0..<3).flat_map(fn(n: Int) => n).count() }");
    assert!(
        err.contains("IntoTraversal") || err.contains("trait"),
        "expected IntoTraversal/trait error, got: {err}"
    );
}

#[test]
fn eval_collection_first_traversal_surface_semantics_rfc_0002() {
    let val = run_ok(
        r#"import collections

fn main() -> Int {
            let list_count = collections.List.new().push(1).push(2).push(3)
                .map(fn(n: Int) => n + 1)
                .filter(fn(n: Int) => n > 2)
                .count()
            let deque_count = collections.Deque.new().push_back(1).push_back(2).push_back(3)
                .map(fn(n: Int) => n * 2)
                .count()
            let z1 = collections.List.new().push(1).push(2).zip(((10)..<13)).count()
            let z2 = ((0)..<3).zip(collections.List.new().push(7).push(8)).count()
            let z3 = collections.Deque.new().push_back(1).push_back(2).zip(collections.List.new().push(9)).count()
            list_count * 100 + deque_count * 10 + z1 + z2 + z3
        }"#,
    );
    assert_eq!(val, Value::Int(235));
}

#[test]
fn eval_collection_first_any_all_find_short_circuit_rfc_0002() {
    let val = run_ok(
        r#"import collections

fn main() -> Int {
            let any_hit = collections.List.new().push(1).push(0).any(fn(n: Int) =>
                if (n == 1) { true } else { 10 / n > 0 }
            )
            let all_ok = collections.Deque.new().push_back(0).push_back(1).all(fn(n: Int) =>
                if (n == 0) { false } else { 10 / (n - 1) > 0 }
            ) == false
            let found = collections.List.new().push(2).push(1).find(fn(n: Int) =>
                if (n == 2) { true } else { 10 / (n - 1) > 0 }
            ).unwrap_or(-1)
            if (any_hit && all_ok && found == 2) { 1 } else { 0 }
        }"#,
    );
    assert_eq!(val, Value::Int(1));
}

#[test]
fn eval_collection_first_frequencies_semantics() {
    let val = run_ok(
        r#"import collections

fn main() -> Int {
    let a = collections.List.new().push(3).push(1).push(3).push(2).frequencies()
    let b = "a,b,a,c".split(",").frequencies()
    let c = collections.MutableList.from_list(collections.List.new().push(1).push(2).push(1))
        .frequencies()
    let d = collections.Deque.new().push_back(1).push_back(2).push_back(1).frequencies()
    let e = (0..<4).frequencies()
    let empty_src: List<Int> = collections.List.new()
    let empty = empty_src.frequencies()
    let ordered = a.keys().to_list()
    if (
        a.get(3).unwrap_or(0) == 2
            && a.get(1).unwrap_or(0) == 1
            && a.len() == 3
            && ordered[0] == 3
            && ordered[1] == 1
            && ordered[2] == 2
            && b.get("a").unwrap_or(0) == 2
            && b.get("c").unwrap_or(0) == 1
            && c.get(1).unwrap_or(0) == 2
            && d.get(2).unwrap_or(0) == 1
            && e.len() == 4
            && empty.is_empty()
    ) {
        1
    } else {
        0
    }
}"#,
    );
    assert_eq!(val, Value::Int(1));
}

#[test]
fn eval_collection_first_contains_semantics() {
    let val = run_ok(
        r#"import collections

fn main() -> Int {
    let a = collections.List.new().push(1).push(2).contains(2)
    let b = collections.MutableList.from_list(collections.List.new().push(1).push(2)).contains(1)
    let c = collections.Deque.new().push_back(1).push_back(2).contains(3)
    let d = (0..<4).contains(3)
    let e = "a,b,c".split(",").contains("b")
    let f = collections.Map.new().insert("x", 1).contains("x")
    let g = collections.MutableSet.new().insert("x").contains("x")

    if (a && b && !c && d && e && f && g) {
        1
    } else {
        0
    }
}"#,
    );
    assert_eq!(val, Value::Int(1));
}

#[test]
fn eval_contains_requires_eq() {
    let err =
        run_err("fn main() -> Bool { (0..<3).map(fn(n: Int) => n.to_float()).contains(1.0) }");
    assert!(
        err.contains("Eq") || err.contains("trait"),
        "expected Eq/trait error, got: {err}"
    );
}

#[test]
fn eval_collection_first_count_predicate_semantics_rfc_0002() {
    let val = run_ok(
        r#"import collections

fn main() -> Int {
    let a = ((0)..<6).count(fn(n: Int) => n % 2 == 0)
    let b = collections.List.new().push(1).push(2).push(3).count(fn(n: Int) => n >= 2)
    let c = "a,b,,c".split(",").count(fn(part: String) => part != "")
    let d = collections.MutableList.from_list(collections.List.new().push(1).push(2).push(3))
        .count(fn(n: Int) => n != 2)
    let e = collections.Deque.new().push_back(1).push_back(2).push_back(3)
        .count(fn(n: Int) => n < 3)
    a * 10000 + b * 1000 + c * 100 + d * 10 + e
}"#,
    );
    assert_eq!(val, Value::Int(32322));
}

#[test]
fn eval_count_predicate_does_not_short_circuit() {
    let err = run_err(
        r#"fn main() -> Int {
    (0..<3).count(fn(n: Int) =>
        if (n == 0) { true } else { 10 / (n - 1) > 0 }
    )
}"#,
    );
    assert!(
        err.contains("division by zero"),
        "expected division-by-zero from full predicate evaluation, got: {err}"
    );
}

#[test]
fn eval_opaque_traversal_surface_semantics_rfc_0003() {
    let val = run_ok(
        r#"type Seed = { x: Int }

fn main() -> Int {
    let a = (0..<5).count()
    let b = (5..<5).count()
    let c = (5..<2).count()
    let d = (0).unfold(fn(state: Int) =>
        if (state < 3) {
            Some({ value: state + 1, state: state + 1 })
        } else {
            None
        }
    ).count()
    let e = Seed { x: 0 }.unfold(fn(state: Seed) =>
        if (state.x < 2) {
            Some({ value: state.x, state: Seed { x: state.x + 1 } })
        } else {
            None
        }
    ).count()
    a * 100 + b * 10 + c + d + e
}"#,
    );
    assert_eq!(val, Value::Int(505));
}

#[test]
fn eval_seq_surface_is_rejected_rfc_0003() {
    let err_range = run_err("fn main() -> Int { Seq.range(0, 3).count() }");
    assert!(
        err_range.contains("no method") || err_range.contains("unresolved"),
        "expected Seq.range rejection, got: {err_range}"
    );

    let err_unfold = run_err("fn main() -> Int { Seq.unfold(0, fn(state: Int) => None).count() }");
    assert!(
        err_unfold.contains("no method") || err_unfold.contains("unresolved"),
        "expected Seq.unfold rejection, got: {err_unfold}"
    );
}

#[test]
fn eval_list_seq_bridge_is_rejected_rfc_0002() {
    let err = run_err(
        r#"fn main() -> Int {
            collections.List.new().push(1).seq().count()
        }"#,
    );
    assert!(
        err.contains("no method") || err.contains("unresolved"),
        "expected removed .seq() diagnostic, got: {err}"
    );
}

#[test]
fn eval_seq_type_annotation_and_param_mismatch_behavior() {
    assert_eq!(
        run_ok(
            "fn takes_seq(xs: Seq<Int>) -> Int { xs.count() }\nfn main() -> Int { takes_seq(0..<4) }"
        ),
        Value::Int(4)
    );

    let err = run_err(
        r#"fn takes_seq(xs: Seq<Int>) -> Int { xs.count() }
fn main() -> Int {
    let xs = collections.List.new().push(1).push(2)
    takes_seq(xs)
}"#,
    );
    assert!(
        err.contains("type mismatch") || err.contains("expected `Seq<Int>`"),
        "expected Seq/List mismatch diagnostic, got: {err}"
    );
}

#[test]
fn eval_seq_any_all_find_semantics_matrix() {
    struct Case<'a> {
        name: &'a str,
        src: &'a str,
        expected: Value,
    }

    let cases = [
        Case {
            name: "any empty is false",
            src: r#"fn main() -> Bool {
                ((0)..<0).any(fn(_n: Int) => true)
            }"#,
            expected: Value::Bool(false),
        },
        Case {
            name: "all empty is true",
            src: r#"fn main() -> Bool {
                ((0)..<0).all(fn(_n: Int) => false)
            }"#,
            expected: Value::Bool(true),
        },
        Case {
            name: "find empty is none",
            src: r#"fn main() -> Int {
                ((0)..<0).find(fn(_n: Int) => true).unwrap_or(-1)
            }"#,
            expected: Value::Int(-1),
        },
        Case {
            name: "any hit and all pass",
            src: r#"fn main() -> Bool {
                let xs = ((0)..<5)
                xs.any(fn(n: Int) => n == 3) && xs.all(fn(n: Int) => n < 5)
            }"#,
            expected: Value::Bool(true),
        },
        Case {
            name: "find first match",
            src: r#"fn main() -> Int {
                ((0)..<6).find(fn(n: Int) => n % 2 == 0 && n > 0).unwrap_or(-1)
            }"#,
            expected: Value::Int(2),
        },
        Case {
            name: "find miss returns none",
            src: r#"fn main() -> Int {
                ((0)..<5).find(fn(n: Int) => n == 9).unwrap_or(-1)
            }"#,
            expected: Value::Int(-1),
        },
    ];

    for case in cases {
        let got = run_ok(case.src);
        assert_eq!(got, case.expected, "case `{}` failed", case.name);
    }
}

#[test]
fn eval_seq_any_all_find_short_circuit_regression() {
    struct Case<'a> {
        name: &'a str,
        src: &'a str,
        expected: Value,
    }

    let cases = [
        Case {
            name: "any short-circuits after first true",
            src: r#"fn main() -> Bool {
                collections.List.new().push(1).push(0).any(fn(n: Int) =>
                    if (n == 1) { true } else { 10 / n > 0 }
                )
            }"#,
            expected: Value::Bool(true),
        },
        Case {
            name: "all short-circuits after first false",
            src: r#"fn main() -> Bool {
                collections.List.new().push(0).push(1).all(fn(n: Int) =>
                    if (n == 0) { false } else { 10 / (n - 1) > 0 }
                ) == false
            }"#,
            expected: Value::Bool(true),
        },
        Case {
            name: "find short-circuits after first match",
            src: r#"fn main() -> Int {
                collections.List.new().push(2).push(1)
                    .find(fn(n: Int) =>
                        if (n == 2) { true } else { 10 / (n - 1) > 0 }
                    )
                    .unwrap_or(-1)
            }"#,
            expected: Value::Int(2),
        },
    ];

    for case in cases {
        let got = run_ok(case.src);
        assert_eq!(got, case.expected, "case `{}` failed", case.name);
    }
}

#[test]
fn eval_seq_scan_include_init_semantics() {
    let val = run_ok(
        r#"fn main() -> Int {
            let xs = ((1)..<4).scan(0, fn(acc: Int, n: Int) => acc + n).to_list()
            xs.len() * 1000 + xs[0] * 100 + xs[1] * 10 + xs[3]
        }"#,
    );
    assert_eq!(val, Value::Int(4016));
}

#[test]
fn eval_seq_scan_empty_input_emits_init_once() {
    let val = run_ok(
        r#"fn main() -> Int {
            let xs = ((0)..<0).scan(42, fn(acc: Int, _n: Int) => acc + 1).to_list()
            xs.len() * 100 + xs[0]
        }"#,
    );
    assert_eq!(val, Value::Int(142));
}

#[test]
fn eval_seq_unfold_semantics_matrix() {
    struct Case<'a> {
        name: &'a str,
        src: &'a str,
        expected: Value,
    }

    let cases = [
        Case {
            name: "unfold bounded emits values",
            src: r#"fn main() -> Int {
                let xs = (0).unfold(fn(state: Int) =>
                    if (state < 4) {
                        Some({ value: state * 2, state: state + 1 })
                    } else {
                        None
                    }
                ).to_list()
                xs.len() * 100 + xs[0] * 10 + xs[3]
            }"#,
            expected: Value::Int(406),
        },
        Case {
            name: "unfold immediate none is empty",
            src: r#"fn main() -> Int {
                (0).unfold(fn(_state: Int) => None).count()
            }"#,
            expected: Value::Int(0),
        },
    ];

    for case in cases {
        let got = run_ok(case.src);
        assert_eq!(got, case.expected, "case `{}` failed", case.name);
    }
}

#[test]
fn eval_seq_unfold_accepts_named_record_alias_payload() {
    let val = run_ok(
        r#"type PickStep = { value: Int, state: Int }

        fn main() -> Int {
            let xs = (0).unfold(fn(state: Int) =>
                if (state < 4) {
                    Some(PickStep { value: state * 2, state: state + 1 })
                } else {
                    None
                }
            ).to_list()
            xs.len() * 100 + xs[0] * 10 + xs[3]
        }"#,
    );
    assert_eq!(val, Value::Int(406));
}

#[test]
fn eval_int_pow_semantics_matrix() {
    struct Case<'a> {
        name: &'a str,
        src: &'a str,
        expected: Value,
    }

    let cases = [
        Case {
            name: "pow positive exponent",
            src: "fn main() -> Int { 2.pow(10) }",
            expected: Value::Int(1024),
        },
        Case {
            name: "pow zero exponent",
            src: "fn main() -> Int { 5.pow(0) }",
            expected: Value::Int(1),
        },
        Case {
            name: "pow zero to zero",
            src: "fn main() -> Int { 0.pow(0) }",
            expected: Value::Int(1),
        },
    ];

    for case in cases {
        let got = run_ok(case.src);
        assert_eq!(got, case.expected, "case `{}` failed", case.name);
    }
}

#[test]
fn eval_int_pow_errors_negative_exponent_and_overflow() {
    let neg = run_err("fn main() -> Int { 2.pow(-1) }");
    assert!(
        neg.contains("int_pow: exponent must be >= 0"),
        "unexpected negative-exponent error: {neg}"
    );

    let overflow = run_err("fn main() -> Int { 2.pow(63) }");
    assert!(
        overflow.contains("integer overflow"),
        "unexpected overflow error: {overflow}"
    );
}

#[test]
fn eval_seq_invalid_sizes_and_removed_list_surface() {
    let err_chunks = run_err(
        r#"fn main() -> Int {
            ((0)..<3).chunks(0).count()
        }"#,
    );
    assert!(err_chunks.contains("chunk size"), "got: {err_chunks}");

    let err_windows = run_err(
        r#"fn main() -> Int {
            ((0)..<3).windows(0).count()
        }"#,
    );
    assert!(err_windows.contains("window size"), "got: {err_windows}");

    let removed = run_err(
        r#"fn main() -> Int {
            List.range(0, 3).len()
        }"#,
    );
    assert!(
        removed.contains("no method") || removed.contains("unresolved"),
        "got: {removed}"
    );
}

// ── Index syntax tests ──────────────────────────────────────────────

#[test]
fn eval_index_list_basic() {
    let val = run_ok(
        "fn main() -> Int {
            let xs = collections.List.new().push(10).push(20)
            xs[0]
        }",
    );
    assert!(matches!(val, Value::Int(10)));
}

#[test]
fn eval_index_list_last() {
    let val = run_ok(
        "fn main() -> Int {
            let xs = collections.List.new().push(10).push(20)
            xs[1]
        }",
    );
    assert!(matches!(val, Value::Int(20)));
}

#[test]
fn eval_index_list_out_of_bounds() {
    let err = run_err(
        "fn main() -> Int {
            let xs = collections.List.new().push(10).push(20)
            xs[5]
        }",
    );
    assert!(err.contains("index out of bounds"), "got: {err}");
}

#[test]
fn eval_index_list_negative() {
    let err = run_err(
        "fn main() -> Int {
            let xs = collections.List.new().push(10).push(20)
            xs[0 - 1]
        }",
    );
    assert!(err.contains("index out of bounds"), "got: {err}");
}

#[test]
fn eval_index_list_empty() {
    let err = run_err(
        "fn main() -> Int {
            let xs: List<Int> = collections.List.new()
            xs[0]
        }",
    );
    assert!(err.contains("index out of bounds"), "got: {err}");
}

#[test]
fn eval_index_string_basic() {
    let val = run_ok(
        "fn main() -> Char {
            \"hello\"[1]
        }",
    );
    assert!(matches!(val, Value::Char('e')));
}

#[test]
fn eval_index_string_first() {
    let val = run_ok(
        "fn main() -> Char {
            \"hello\"[0]
        }",
    );
    assert!(matches!(val, Value::Char('h')));
}

#[test]
fn eval_index_string_last() {
    let val = run_ok(
        "fn main() -> Char {
            \"hello\"[4]
        }",
    );
    assert!(matches!(val, Value::Char('o')));
}

#[test]
fn eval_index_string_out_of_bounds() {
    let err = run_err(
        "fn main() -> Char {
            \"hello\"[10]
        }",
    );
    assert!(err.contains("index out of bounds"), "got: {err}");
}

#[test]
fn eval_index_string_empty() {
    let err = run_err(
        "fn main() -> Char {
            \"\"[0]
        }",
    );
    assert!(err.contains("index out of bounds"), "got: {err}");
}

#[test]
fn eval_index_map_basic() {
    let val = run_ok(
        "fn main() -> Int {
            let m = collections.Map.new().insert(\"a\", 42)
            m[\"a\"]
        }",
    );
    assert!(matches!(val, Value::Int(42)));
}

#[test]
fn eval_index_map_missing_key() {
    let err = run_err(
        "fn main() -> Int {
            let m = collections.Map.new().insert(\"a\", 42)
            m[\"b\"]
        }",
    );
    assert!(err.contains("key not found"), "got: {err}");
}

#[test]
fn eval_index_chained_list() {
    // Nested list indexing: list of lists
    let val = run_ok(
        "fn main() -> Int {
            let inner = collections.List.new().push(10).push(20)
            let outer = collections.List.new().push(inner)
            outer[0][1]
        }",
    );
    assert!(matches!(val, Value::Int(20)));
}

#[test]
fn eval_index_with_expression() {
    let val = run_ok(
        "fn main() -> Int {
            let xs = collections.List.new().push(10).push(20).push(30)
            xs[1 + 1]
        }",
    );
    assert!(matches!(val, Value::Int(30)));
}

#[test]
fn eval_index_string_unicode() {
    // Multi-byte Unicode chars: indexing by char position, not byte position
    // "héllo" has 5 chars; 'é' is multi-byte in UTF-8 but still 1 char
    let source = format!(
        "fn main() -> Char {{
            let s = \"{}\"
            s[1]
        }}",
        "h\u{00e9}llo"
    );
    let val = run_ok(&source);
    assert!(matches!(val, Value::Char('\u{00e9}')));
}

#[test]
fn eval_index_list_then_field() {
    // Index a list of records, then access a field
    let val = run_ok(
        "type Point = { x: Int, y: Int }
        fn main() -> Int {
            let p = Point { x: 3, y: 4 }
            let xs = collections.List.new().push(p)
            xs[0].x
        }",
    );
    assert!(matches!(val, Value::Int(3)));
}

#[test]
fn eval_index_on_wrong_type() {
    // Indexing an Int should be a compile error
    assert!(check_has_compile_errors("fn main() -> Int { 42[0] }"));
}

// ── Method call syntax tests ────────────────────────────────────────

// String methods
#[test]
fn eval_method_string_len() {
    let val = run_ok(r#"fn main() -> Int { "hello".len() }"#);
    assert!(matches!(val, Value::Int(5)));
}

#[test]
fn eval_method_string_contains() {
    let val = run_ok(r#"fn main() -> Bool { "hello world".contains("world") }"#);
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_method_string_trim() {
    let val = run_ok(r#"fn main() -> String { "  hello  ".trim() }"#);
    match val {
        Value::String(s) => assert_eq!(s, "hello"),
        other => panic!("expected String, got {other:?}"),
    }
}

#[test]
fn eval_method_string_to_upper() {
    let val = run_ok(r#"fn main() -> String { "hello".to_upper() }"#);
    match val {
        Value::String(s) => assert_eq!(s, "HELLO"),
        other => panic!("expected String, got {other:?}"),
    }
}

#[test]
fn eval_method_string_to_lower() {
    let val = run_ok(r#"fn main() -> String { "HELLO".to_lower() }"#);
    match val {
        Value::String(s) => assert_eq!(s, "hello"),
        other => panic!("expected String, got {other:?}"),
    }
}

#[test]
fn eval_method_string_md5() {
    let val = run_ok(r#"fn main() -> String { "abc".md5() }"#);
    match val {
        Value::String(s) => assert_eq!(s, "900150983cd24fb0d6963f7d28e17f72"),
        other => panic!("expected String, got {other:?}"),
    }
}

#[test]
fn eval_hash_module_md5() {
    let val = run_ok(
        r#"import hash
fn main() -> String { hash.md5("abc") }"#,
    );
    match val {
        Value::String(s) => assert_eq!(s, "900150983cd24fb0d6963f7d28e17f72"),
        other => panic!("expected String, got {other:?}"),
    }
}

#[test]
fn eval_hash_module_alias_md5() {
    let val = run_ok(
        r#"import hash as h
fn main() -> String { h.md5("abc") }"#,
    );
    match val {
        Value::String(s) => assert_eq!(s, "900150983cd24fb0d6963f7d28e17f72"),
        other => panic!("expected String, got {other:?}"),
    }
}

#[test]
fn eval_method_string_starts_with() {
    let val = run_ok(r#"fn main() -> Bool { "hello world".starts_with("hello") }"#);
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_method_string_starts_with_named_offset() {
    let val = run_ok(r#"fn main() -> Bool { "hello world".starts_with("lo", start: 3) }"#);
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_method_string_starts_with_named_offset_out_of_range_is_false() {
    let val = run_ok(r#"fn main() -> Bool { "hello".starts_with("he", start: 99) }"#);
    assert!(matches!(val, Value::Bool(false)));
}

#[test]
fn eval_method_string_starts_with_named_offset_negative_is_false() {
    let val = run_ok(r#"fn main() -> Bool { "hello".starts_with("he", start: -1) }"#);
    assert!(matches!(val, Value::Bool(false)));
}

#[test]
fn eval_user_defined_function_overload_family_by_arity_dispatches() {
    let val = run_ok(
        r#"fn foo() -> Int { 1 }
fn foo(x: Int) -> Int { x + 1 }
fn main() -> Int { foo() + foo(2) }"#,
    );
    assert_eq!(val, Value::Int(4));
}

#[test]
fn eval_user_defined_function_overload_family_wrong_arity_is_compile_error() {
    let err = run_err(
        r#"fn foo() -> Int { 1 }
fn foo(x: Int) -> Int { x + 1 }
fn foo(x: Int, y: Int, z: Int) -> Int { x + y + z }
fn main() -> Int { foo(1, 2) }"#,
    );
    assert!(
        err.contains("expected 0, 1, or 3 argument(s), found 2"),
        "expected constrained-family wrong-arity error, got: {err}"
    );
}

#[test]
fn eval_user_defined_method_overload_family_by_arity_dispatches() {
    let val = run_ok(
        r#"type Box = { value: Int }
fn Box.size(self) -> Int { self.value }
fn Box.size(self, extra: Int) -> Int { self.value + extra }
fn main() -> Int {
    let b = Box { value: 2 }
    b.size() + b.size(3)
}"#,
    );
    assert_eq!(val, Value::Int(7));
}

#[test]
fn eval_user_defined_method_overload_family_wrong_arity_is_compile_error() {
    let err = run_err(
        r#"type Box = { value: Int }
fn Box.size(self) -> Int { self.value }
fn Box.size(self, extra: Int) -> Int { self.value + extra }
fn Box.size(self, extra: Int, extra2: Int) -> Int { self.value + extra + extra2 }
fn main() -> Int {
    let b = Box { value: 2 }
    b.size(1, 2, 3)
}"#,
    );
    assert!(
        err.contains("expected 0, 1, or 2 argument(s), found 3"),
        "expected constrained-family wrong-arity error, got: {err}"
    );
}

#[test]
fn eval_overloaded_function_family_as_value_is_compile_error() {
    let err = run_err(
        r#"fn foo() -> Int { 1 }
fn foo(x: Int) -> Int { x }
fn main() -> Int {
    let f = foo
    0
}"#,
    );
    assert!(
        err.contains("E0039") || err.contains("overloaded function family `foo`"),
        "expected overloaded-family-as-value error, got: {err}"
    );
}

#[test]
fn run_project_imported_and_local_overload_family_dispatches() {
    let val = run_project_with_files_manifest_ok(
        &[
            (
                "main.ky",
                "import util\nfn foo() -> Int { 1 }\nfn main() -> Int { foo() + foo(2) }",
            ),
            ("util.ky", "pub fn foo(x: Int) -> Int { x + 1 }"),
        ],
        None,
    );
    assert_eq!(val, Value::Int(4));
}

#[test]
fn run_project_import_overlap_same_shape_is_compile_error() {
    let err = run_project_with_files_err(&[
        (
            "main.ky",
            "import util\nfn foo(x: Int) -> Int { x }\nfn main() -> Int { 0 }",
        ),
        ("util.ky", "pub fn foo(x: Int) -> Int { x + 1 }"),
    ]);
    assert!(
        err.contains("E0102") || err.contains("overlaps an existing definition"),
        "expected conflicting imported overlap diagnostic, got: {err}"
    );
}

#[test]
fn run_project_imported_overloaded_function_family_as_value_is_compile_error() {
    let err = run_project_with_files_err(&[
        (
            "main.ky",
            "import util\nfn main() -> Int {\n  let f = foo\n  0\n}",
        ),
        (
            "util.ky",
            "pub fn foo() -> Int { 1 }\npub fn foo(x: Int) -> Int { x }",
        ),
    ]);
    assert!(
        err.contains("E0039") || err.contains("overloaded function family `foo`"),
        "expected imported overloaded-family-as-value error, got: {err}"
    );
}

#[test]
fn eval_method_string_ends_with() {
    let val = run_ok(r#"fn main() -> Bool { "hello world".ends_with("world") }"#);
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_method_string_split() {
    let val = run_ok(
        r#"fn main() -> Int {
            let parts = "a,b,c".split(",")
            parts.count()
        }"#,
    );
    assert!(matches!(val, Value::Int(3)));
}

#[test]
fn eval_method_string_lines() {
    let val = run_ok(
        r#"fn main() -> Int {
            let ls = "a\nb\nc".lines()
            ls.count()
        }"#,
    );
    assert!(matches!(val, Value::Int(3)));
}

#[test]
fn eval_method_string_chars() {
    let val = run_ok(
        r#"fn main() -> Int {
            let cs = "hello".chars()
            cs.count()
        }"#,
    );
    assert!(matches!(val, Value::Int(5)));
}

// List methods
#[test]
fn eval_method_list_len() {
    let val = run_ok(
        "fn main() -> Int {
            let xs = collections.List.new().push(1).push(2)
            xs.len()
        }",
    );
    assert!(matches!(val, Value::Int(2)));
}

#[test]
fn eval_method_list_push() {
    let val = run_ok(
        "fn main() -> Int {
            let xs = collections.List.new().push(1)
            let ys = xs.push(2)
            ys.len()
        }",
    );
    assert!(matches!(val, Value::Int(2)));
}

#[test]
fn eval_method_list_get() {
    let val = run_ok(
        "fn main() -> Int {
            let xs = collections.List.new().push(10).push(20)
            match (xs.get(1)) {
                Some(v) => v
                None => 0
            }
        }",
    );
    assert!(matches!(val, Value::Int(20)));
}

#[test]
fn eval_method_list_is_empty() {
    let val = run_ok(
        "fn main() -> Bool {
            let xs: List<Int> = collections.List.new()
            xs.is_empty()
        }",
    );
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_method_list_reverse() {
    let val = run_ok(
        "fn main() -> Int {
            let xs = collections.List.new().push(1).push(2)
            let ys = xs.reverse()
            ys[0]
        }",
    );
    assert!(matches!(val, Value::Int(2)));
}

#[test]
fn eval_method_list_map() {
    let val = run_ok(
        "fn main() -> Int {
            let xs = collections.List.new().push(1).push(2)
            let ys = xs.map(fn(x: Int) => x * 10).to_list()
            ys[0]
        }",
    );
    assert!(matches!(val, Value::Int(10)));
}

#[test]
fn eval_method_list_filter() {
    let val = run_ok(
        "fn main() -> Int {
            let xs = collections.List.new().push(1).push(2).push(3)
            xs.filter(fn(x: Int) => x > 1).count()
        }",
    );
    assert!(matches!(val, Value::Int(2)));
}

#[test]
fn eval_method_list_fold() {
    let val = run_ok(
        "fn main() -> Int {
            let xs = collections.List.new().push(1).push(2).push(3)
            xs.fold(0, fn(acc: Int, x: Int) => acc + x)
        }",
    );
    assert!(matches!(val, Value::Int(6)));
}

#[test]
fn eval_method_list_sort() {
    let val = run_ok(
        "fn main() -> Int {
            let xs = collections.List.new().push(3).push(1).push(2)
            let sorted = xs.sort()
            sorted[0]
        }",
    );
    assert!(matches!(val, Value::Int(1)));
}

#[test]
fn eval_method_list_sort_by() {
    let val = run_ok(
        "fn main() -> Int {
            let xs = collections.List.new().push(3).push(1).push(2)
            let sorted = xs.sort_by(fn(a: Int, b: Int) => a - b)
            sorted[2]
        }",
    );
    assert!(matches!(val, Value::Int(3)));
}

// Map methods
#[test]
fn eval_method_map_insert_and_get() {
    let val = run_ok(
        r#"fn main() -> Int {
            let m = collections.Map.new()
            let m2 = m.insert("a", 42)
            match (m2.get("a")) {
                Some(v) => v
                None => 0
            }
        }"#,
    );
    assert!(matches!(val, Value::Int(42)));
}

#[test]
fn eval_method_map_contains() {
    let val = run_ok(
        r#"fn main() -> Bool {
            let m = collections.Map.new().insert("key", 1)
            m.contains("key")
        }"#,
    );
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_method_map_len() {
    let val = run_ok(
        r#"fn main() -> Int {
            let m = collections.Map.new().insert("a", 1).insert("b", 2)
            m.len()
        }"#,
    );
    assert!(matches!(val, Value::Int(2)));
}

#[test]
fn eval_method_map_keys() {
    let val = run_ok(
        r#"fn main() -> Int {
            let m = collections.Map.new().insert("a", 1)
            let ks = m.keys()
            ks.count()
        }"#,
    );
    assert!(matches!(val, Value::Int(1)));
}

// Conversion methods
#[test]
fn eval_method_int_to_string() {
    let val = run_ok(r#"fn main() -> String { 42.to_string() }"#);
    match val {
        Value::String(s) => assert_eq!(s, "42"),
        other => panic!("expected String, got {other:?}"),
    }
}

#[test]
fn eval_method_int_to_float() {
    let val = run_ok("fn main() -> Float { 42.to_float() }");
    match val {
        Value::Float(f) => assert!((f - 42.0).abs() < f64::EPSILON),
        other => panic!("expected Float, got {other:?}"),
    }
}

#[test]
fn eval_method_float_to_int() {
    let val = run_ok("fn main() -> Int { 3.14.to_int() }");
    assert!(matches!(val, Value::Int(3)));
}

#[test]
fn eval_method_char_to_string() {
    let val = run_ok("fn main() -> String { 'a'.to_string() }");
    match val {
        Value::String(s) => assert_eq!(s, "a"),
        other => panic!("expected String, got {other:?}"),
    }
}

#[test]
fn eval_method_char_code_on_indexed_string_chars() {
    let ascii = run_ok(r#"fn main() -> Int { "A"[0].code() }"#);
    assert_eq!(ascii, Value::Int(65));

    let emoji = run_ok(r#"fn main() -> Int { "😀"[0].code() }"#);
    assert_eq!(emoji, Value::Int(128512));
}

#[test]
fn eval_method_char_is_decimal_digit() {
    let ascii_digit = run_ok("fn main() -> Bool { '7'.is_decimal_digit() }");
    assert_eq!(ascii_digit, Value::Bool(true));

    let letter = run_ok("fn main() -> Bool { 'x'.is_decimal_digit() }");
    assert_eq!(letter, Value::Bool(false));
}

#[test]
fn eval_method_char_to_decimal_digit() {
    let digit = run_ok(
        r#"fn main() -> Int {
    match ('7'.to_decimal_digit()) {
        Some(n) => n
        None => 0 - 1
    }
}"#,
    );
    assert_eq!(digit, Value::Int(7));

    let nondigit = run_ok(
        r#"fn main() -> Int {
    match ('x'.to_decimal_digit()) {
        Some(n) => n
        None => 0 - 1
    }
}"#,
    );
    assert_eq!(nondigit, Value::Int(-1));
}

#[test]
fn eval_method_char_to_digit_with_radix() {
    let hex_upper = run_ok(
        r#"fn main() -> Int {
    match ('F'.to_digit(16)) {
        Some(n) => n
        None => 0 - 1
    }
}"#,
    );
    assert_eq!(hex_upper, Value::Int(15));

    let hex_lower = run_ok(
        r#"fn main() -> Int {
    match ('f'.to_digit(16)) {
        Some(n) => n
        None => 0 - 1
    }
}"#,
    );
    assert_eq!(hex_lower, Value::Int(15));

    let missing = run_ok(
        r#"fn main() -> Int {
    match ('g'.to_digit(16)) {
        Some(n) => n
        None => 0 - 1
    }
}"#,
    );
    assert_eq!(missing, Value::Int(-1));
}

#[test]
fn eval_method_char_to_digit_rejects_invalid_radix() {
    let err = run_err("fn main() -> Int { match ('7'.to_digit(1)) { Some(n) => n None => 0 } }");
    assert!(err.contains("radix"), "got: {err}");
}

#[test]
fn eval_method_int_abs() {
    let val = run_ok("fn main() -> Int { (0 - 5).abs() }");
    assert!(matches!(val, Value::Int(5)));
}

// Chaining
#[test]
fn eval_method_chaining_trim_len() {
    let val = run_ok(r#"fn main() -> Int { "  hello  ".trim().len() }"#);
    assert!(matches!(val, Value::Int(5)));
}

#[test]
fn eval_method_chaining_push_push_len() {
    let val = run_ok(
        "fn main() -> Int {
            let xs: List<Int> = collections.List.new()
            xs.push(1).push(2).push(3).len()
        }",
    );
    assert!(matches!(val, Value::Int(3)));
}

#[test]
fn eval_method_chaining_split_len() {
    let val = run_ok(
        r#"fn main() -> Int {
            "a,b,c".split(",").count()
        }"#,
    );
    assert!(matches!(val, Value::Int(3)));
}

// Flat function still works alongside method syntax
#[test]
fn eval_method_flat_function_still_works() {
    let val = run_ok(r#"fn main() -> Int { "hello".len() }"#);
    assert!(matches!(val, Value::Int(5)));
}

// Index + method chaining
#[test]
fn eval_method_index_then_method() {
    let val = run_ok(
        r#"fn main() -> Int {
            let xs = collections.List.new().push("hello")
            xs[0].len()
        }"#,
    );
    assert!(matches!(val, Value::Int(5)));
}

// ── User-defined method tests ───────────────────────────────────────

#[test]
fn eval_user_method_on_record() {
    let val = run_ok(
        r#"
        type Point = { x: Int, y: Int }

        fn Point.sum(self) -> Int {
            self.x + self.y
        }

        fn main() -> Int {
            let p = Point { x: 3, y: 4 }
            p.sum()
        }
        "#,
    );
    assert!(matches!(val, Value::Int(7)));
}

#[test]
fn eval_user_method_with_extra_args() {
    let val = run_ok(
        r#"
        type Counter = { value: Int }

        fn Counter.add(self, n: Int) -> Counter {
            Counter { value: self.value + n }
        }

        fn main() -> Int {
            let c = Counter { value: 10 }
            let c2 = c.add(5)
            c2.value
        }
        "#,
    );
    assert!(matches!(val, Value::Int(15)));
}

#[test]
fn eval_user_method_chaining() {
    let val = run_ok(
        r#"
        type Counter = { value: Int }

        fn Counter.inc(self) -> Counter {
            Counter { value: self.value + 1 }
        }

        fn main() -> Int {
            let c = Counter { value: 0 }
            c.inc().inc().inc().value
        }
        "#,
    );
    assert!(matches!(val, Value::Int(3)));
}

#[test]
fn eval_user_method_on_adt() {
    let val = run_ok(
        r#"
        type Shape = Circle(Int) | Rect(Int, Int)

        fn Shape.area(self) -> Int {
            match (self) {
                Circle(r) => r * r * 3
                Rect(w, h) => w * h
            }
        }

        fn main() -> Int {
            let s = Rect(4, 5)
            s.area()
        }
        "#,
    );
    assert!(matches!(val, Value::Int(20)));
}

#[test]
fn eval_user_method_field_and_method_coexist() {
    // A record with a field named `x` and a method named `len`.
    // Accessing `p.x` should be field access, `p.len()` should be method call.
    let val = run_ok(
        r#"
        type Vec2 = { x: Int, y: Int }

        fn Vec2.len(self) -> Int {
            self.x + self.y
        }

        fn main() -> Int {
            let v = Vec2 { x: 3, y: 4 }
            v.x + v.len()
        }
        "#,
    );
    assert!(matches!(val, Value::Int(10)));
}

#[test]
fn eval_user_method_returns_self_type() {
    let val = run_ok(
        r#"
        type Pair = { a: Int, b: Int }

        fn Pair.swap(self) -> Pair {
            Pair { a: self.b, b: self.a }
        }

        fn main() -> Int {
            let p = Pair { a: 1, b: 2 }
            let swapped = p.swap()
            swapped.a
        }
        "#,
    );
    assert!(matches!(val, Value::Int(2)));
}

#[test]
fn eval_user_method_multiple_methods_same_type() {
    let val = run_ok(
        r#"
        type Box = { val: Int }

        fn Box.get(self) -> Int {
            self.val
        }

        fn Box.double(self) -> Box {
            Box { val: self.val * 2 }
        }

        fn main() -> Int {
            let b = Box { val: 5 }
            b.double().get()
        }
        "#,
    );
    assert!(matches!(val, Value::Int(10)));
}

#[test]
fn eval_user_method_with_self_and_typed_params() {
    let val = run_ok(
        r#"
        type Acc = { total: Int }

        fn Acc.add_all(self, a: Int, b: Int, c: Int) -> Acc {
            Acc { total: self.total + a + b + c }
        }

        fn main() -> Int {
            let acc = Acc { total: 0 }
            acc.add_all(1, 2, 3).total
        }
        "#,
    );
    assert!(matches!(val, Value::Int(6)));
}

#[test]
fn eval_user_method_and_free_fn_coexist() {
    // A free function and a method with different names on the same type.
    let val = run_ok(
        r#"
        type Num = { n: Int }

        fn make_num(x: Int) -> Num {
            Num { n: x }
        }

        fn Num.value(self) -> Int {
            self.n
        }

        fn main() -> Int {
            let x = make_num(42)
            x.value()
        }
        "#,
    );
    assert!(matches!(val, Value::Int(42)));
}

#[test]
fn eval_user_method_self_explicit_type_annotation() {
    // User can also write `self: Point` explicitly.
    let val = run_ok(
        r#"
        type Point = { x: Int, y: Int }

        fn Point.sum(self: Point) -> Int {
            self.x + self.y
        }

        fn main() -> Int {
            let p = Point { x: 10, y: 20 }
            p.sum()
        }
        "#,
    );
    assert!(matches!(val, Value::Int(30)));
}

#[test]
fn eval_user_method_on_adt_circle() {
    let val = run_ok(
        r#"
        type Shape = Circle(Int) | Rect(Int, Int)

        fn Shape.describe(self) -> String {
            match (self) {
                Circle(_) => "circle"
                Rect(_, _) => "rect"
            }
        }

        fn main() -> String {
            let s = Circle(5)
            s.describe()
        }
        "#,
    );
    assert_eq!(val, Value::String("circle".into()));
}

#[test]
fn eval_user_method_using_other_methods() {
    // Method that calls another method on the same type.
    let val = run_ok(
        r#"
        type Wrapper = { inner: Int }

        fn Wrapper.get(self) -> Int {
            self.inner
        }

        fn Wrapper.get_plus(self, n: Int) -> Int {
            self.get() + n
        }

        fn main() -> Int {
            let w = Wrapper { inner: 10 }
            w.get_plus(5)
        }
        "#,
    );
    assert!(matches!(val, Value::Int(15)));
}

// ── Chaining edge cases ────────────────────────────────────────

#[test]
fn eval_index_then_method() {
    // xs[0].len() — index into a list of strings, then call method
    let val = run_ok(
        r#"
        fn main() -> Int {
            let xs = collections.List.new().push("hello").push("world")
            xs[0].len()
        }
        "#,
    );
    assert!(matches!(val, Value::Int(5)));
}

#[test]
fn eval_method_then_index() {
    // "hello".chars().to_list()[1] — method returning seq, then list indexing
    let val = run_ok(
        r#"
        fn main() -> Char {
            "hello".chars().to_list()[1]
        }
        "#,
    );
    assert!(matches!(val, Value::Char('e')));
}

#[test]
fn eval_method_chain_then_index() {
    // "a,b,c".split(",").to_list()[2] — method returning seq, then list indexing
    let val = run_ok(
        r#"
        fn main() -> String {
            "a,b,c".split(",").to_list()[2]
        }
        "#,
    );
    assert!(matches!(val, Value::String(s) if s == "c"));
}

#[test]
fn eval_index_then_field() {
    // xs[0].x — index into list of records, then field access
    let val = run_ok(
        r#"
        type Point = { x: Int, y: Int }
        fn main() -> Int {
            let xs = collections.List.new().push(Point { x: 42, y: 7 })
            xs[0].x
        }
        "#,
    );
    assert!(matches!(val, Value::Int(42)));
}

#[test]
fn eval_no_method_on_int() {
    // 42.len() should produce a type error about no method
    let err = run_err(
        r#"
        fn main() -> Int {
            42.len()
        }
        "#,
    );
    assert!(
        err.contains("no method"),
        "expected 'no method' error, got: {err}"
    );
}

#[test]
fn eval_no_method_on_string() {
    // "hello".nonexistent() should produce a method error
    let err = run_err(
        r#"
        fn main() -> Int {
            "hello".nonexistent()
        }
        "#,
    );
    assert!(
        err.contains("no method"),
        "expected 'no method' error, got: {err}"
    );
}

#[test]
fn eval_flat_fn_and_method_both_work() {
    // "hello".len() and "hello".len() both return 5
    let val = run_ok(
        r#"
        fn main() -> Int {
            let a = "hello".len()
            let b = "hello".len()
            a + b
        }
        "#,
    );
    assert!(matches!(val, Value::Int(10)));
}

// ── Map (IndexMap backing store) tests ─────────────────────────────

#[test]
fn eval_map_int_keys() {
    let val = run_ok(
        "fn main() -> Int {
            let m = collections.Map.new().insert(1, 100).insert(2, 200).insert(3, 300)
            m[2]
        }",
    );
    assert!(matches!(val, Value::Int(200)));
}

#[test]
fn eval_map_bool_keys() {
    let val = run_ok(
        "fn main() -> Int {
            let m = collections.Map.new().insert(true, 1).insert(false, 0)
            m[true] + m[false]
        }",
    );
    assert!(matches!(val, Value::Int(1)));
}

#[test]
fn eval_map_char_keys() {
    let val = run_ok(
        "fn main() -> Int {
            let m = collections.Map.new().insert('a', 1).insert('b', 2)
            m['a'] + m['b']
        }",
    );
    assert!(matches!(val, Value::Int(3)));
}

#[test]
fn eval_map_mixed_operations() {
    // Insert, overwrite, remove, check contains on missing
    let val = run_ok(
        r#"fn main() -> Bool {
            let m = collections.Map.new()
                .insert("x", 1)
                .insert("y", 2)
                .insert("x", 99)
                .remove("y")
            let has_x = m.contains("x")
            let has_y = m.contains("y")
            let len_ok = m.len() == 1
            has_x && !has_y && len_ok
        }"#,
    );
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_map_insertion_order_preserved() {
    // Keys should come back in insertion order (IndexMap guarantee)
    let val = run_ok(
        r#"fn main() -> String {
            let m = collections.Map.new()
                .insert("c", 3)
                .insert("a", 1)
                .insert("b", 2)
            let ks = m.keys().to_list()
            match (ks.head()) {
                Some(k) => k
                None => "fail"
            }
        }"#,
    );
    match val {
        Value::String(s) => assert_eq!(s, "c", "first key should be 'c' (insertion order)"),
        other => panic!("expected String, got {other:?}"),
    }
}

#[test]
fn eval_map_overwrite_preserves_position() {
    // Overwriting a key should keep its original insertion position
    let val = run_ok(
        r#"fn main() -> Int {
            let m = collections.Map.new()
                .insert("a", 1)
                .insert("b", 2)
                .insert("a", 99)
            match (m.get("a")) {
                Some(v) => v
                None => 0
            }
        }"#,
    );
    assert!(matches!(val, Value::Int(99)));
}

#[test]
fn eval_map_values_after_overwrite() {
    // After overwrite, values() should reflect the update
    let val = run_ok(
        r#"fn main() -> Int {
            let m = collections.Map.new()
                .insert("a", 1)
                .insert("b", 2)
                .insert("a", 100)
            m.values().fold(0, fn(acc: Int, x: Int) => acc + x)
        }"#,
    );
    assert!(matches!(val, Value::Int(102)));
}

#[test]
fn eval_map_remove_nonexistent_key() {
    // Removing a key that doesn't exist should be a no-op
    let val = run_ok(
        r#"fn main() -> Int {
            let m = collections.Map.new().insert("a", 1)
            let m2 = m.remove("zzz")
            m2.len()
        }"#,
    );
    assert!(matches!(val, Value::Int(1)));
}

#[test]
fn eval_map_get_after_remove() {
    let val = run_ok(
        r#"fn main() -> Bool {
            let m = collections.Map.new().insert("a", 1).insert("b", 2)
            let m2 = m.remove("a")
            match (m2.get("a")) {
                Some(_) => false
                None => true
            }
        }"#,
    );
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_map_many_inserts() {
    // Verify O(1) behavior doesn't break with more entries
    let val = run_ok(
        "fn main() -> Int {
            let m = collections.Map.new()
                .insert(1, 10)
                .insert(2, 20)
                .insert(3, 30)
                .insert(4, 40)
                .insert(5, 50)
                .insert(6, 60)
                .insert(7, 70)
                .insert(8, 80)
                .insert(9, 90)
                .insert(10, 100)
            m[5] + m[10]
        }",
    );
    assert!(matches!(val, Value::Int(150)));
}

#[test]
fn eval_map_immutable_semantics() {
    // Original map should be unchanged after insert on a copy
    let val = run_ok(
        r#"fn main() -> Bool {
            let m1 = collections.Map.new().insert("a", 1)
            let m2 = m1.insert("b", 2)
            m1.len() == 1 && m2.len() == 2
        }"#,
    );
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_map_index_with_int_key() {
    let val = run_ok(
        "fn main() -> Int {
            let m = collections.Map.new().insert(42, 999)
            m[42]
        }",
    );
    assert!(matches!(val, Value::Int(999)));
}

#[test]
fn eval_map_contains_missing() {
    let val = run_ok(
        r#"fn main() -> Bool {
            let m = collections.Map.new().insert("a", 1)
            m.contains("z")
        }"#,
    );
    assert!(matches!(val, Value::Bool(false)));
}

#[test]
fn eval_map_empty_keys_and_values() {
    let val = run_ok(
        "fn main() -> Bool {
            let m = collections.Map.new()
            m.keys().count() == 0 && m.values().count() == 0
        }",
    );
    assert!(matches!(val, Value::Bool(true)));
}

// ── Map key type compile-time rejection ────────────────────────────

#[test]
fn eval_map_float_key_rejected_at_compile_time() {
    let err = run_err(
        "fn main() -> Int {
            let m = collections.Map.new().insert(3.14, 1)
            0
        }",
    );
    assert!(
        err.contains("cannot be used as a map key"),
        "expected compile-time map key rejection, got: {err}"
    );
}

#[test]
fn eval_map_list_key_allowed_when_elements_are_hashable() {
    let val = run_ok(
        "fn main() -> Int {
            let xs = collections.List.new().push(1)
            let m = collections.Map.new().insert(xs, 1)
            m.get(xs).unwrap_or(0)
        }",
    );
    assert_eq!(val, Value::Int(1));
}

#[test]
fn eval_map_fn_key_rejected_at_compile_time() {
    let err = run_err(
        "fn helper() -> Int { 0 }
         fn main() -> Int {
            let m = collections.Map.new().insert(helper, 1)
            0
        }",
    );
    assert!(
        err.contains("cannot be used as a map key"),
        "expected compile-time map key rejection, got: {err}"
    );
}

#[test]
fn eval_map_valid_keys_no_rejection() {
    // Guard test: valid key types (Int, String, Char, Bool) should NOT trigger rejection
    run_ok(
        r#"fn main() -> Bool {
            let m1 = collections.Map.new().insert(1, "int key")
            let m2 = collections.Map.new().insert("str", "string key")
            let m3 = collections.Map.new().insert('c', "char key")
            let m4 = collections.Map.new().insert(true, "bool key")
            m1.len() == 1 && m2.len() == 1 && m3.len() == 1 && m4.len() == 1
        }"#,
    );
}

// ── Set<T> behavior tests ───────────────────────────────────────────────

#[test]
fn eval_set_new_is_empty() {
    let val = run_ok(
        r#"fn main() -> Bool {
            collections.Set.new().is_empty()
        }"#,
    );
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_set_insert_contains_and_len() {
    let val = run_ok(
        r#"fn main() -> Bool {
            let s = collections.Set.new().insert(1).insert(2).insert(3)
            s.contains(2) && s.len() == 3
        }"#,
    );
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_set_insert_deduplicates_values() {
    let val = run_ok(
        r#"fn main() -> Bool {
            let s = collections.Set.new().insert("x").insert("x").insert("x")
            s.len() == 1 && s.contains("x")
        }"#,
    );
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_set_remove_existing_and_missing() {
    let val = run_ok(
        r#"fn main() -> Bool {
            let s1 = collections.Set.new().insert("a").insert("b")
            let s2 = s1.remove("a")
            let s3 = s2.remove("zzz")
            !s2.contains("a") && s2.len() == 1 && s3.len() == 1
        }"#,
    );
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_set_immutable_semantics() {
    let val = run_ok(
        r#"fn main() -> Bool {
            let s1 = collections.Set.new().insert(1)
            let s2 = s1.insert(2)
            s1.len() == 1 && s2.len() == 2
        }"#,
    );
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_set_values_preserve_insertion_order() {
    let val = run_ok(
        r#"fn main() -> String {
            let s = collections.Set.new().insert("c").insert("a").insert("b")
            let vals = s.values().to_list()
            match (vals.head()) {
                Some(v) => v
                None => "fail"
            }
        }"#,
    );
    match val {
        Value::String(s) => assert_eq!(s, "c"),
        other => panic!("expected String, got {other:?}"),
    }
}

#[test]
fn eval_set_values_remove_and_reinsert_goes_to_end() {
    let val = run_ok(
        r#"fn main() -> Bool {
            let s = collections.Set.new().insert("a").insert("b").remove("a").insert("a")
            let vals = s.values().to_list()
            match (vals.get(0)) {
                Some(first) =>
                    match (vals.get(1)) {
                        Some(second) => first == "b" && second == "a"
                        None => false
                    }
                None => false
            }
        }"#,
    );
    assert!(matches!(val, Value::Bool(true)));
}

// ── Set element type compile-time rejection ─────────────────────────

#[test]
fn eval_set_float_element_rejected_at_compile_time() {
    let err = run_err(
        r#"fn main() -> Int {
            let s = collections.Set.new().insert(3.14)
            s.len()
        }"#,
    );
    assert!(
        err.contains("cannot be used as a set element"),
        "expected compile-time set element rejection, got: {err}"
    );
}

#[test]
fn eval_set_list_element_allowed_when_elements_are_hashable() {
    let val = run_ok(
        r#"fn main() -> Int {
            let xs = collections.List.new().push(1)
            let s = collections.Set.new().insert(xs)
            if (s.contains(xs)) { s.len() } else { 0 }
        }"#,
    );
    assert_eq!(val, Value::Int(1));
}

#[test]
fn eval_set_fn_element_rejected_at_compile_time() {
    let err = run_err(
        r#"fn helper() -> Int { 0 }
           fn main() -> Int {
             let s = collections.Set.new().insert(helper)
             s.len()
           }"#,
    );
    assert!(
        err.contains("cannot be used as a set element"),
        "expected compile-time set element rejection, got: {err}"
    );
}

#[test]
fn eval_set_valid_elements_no_rejection() {
    run_ok(
        r#"fn main() -> Bool {
            let s1 = collections.Set.new().insert(1)
            let s2 = collections.Set.new().insert("str")
            let s3 = collections.Set.new().insert('c')
            let s4 = collections.Set.new().insert(true)
            s1.len() == 1 && s2.len() == 1 && s3.len() == 1 && s4.len() == 1
        }"#,
    );
}

#[test]
fn eval_list_rebinding_chain_preserves_prior_versions() {
    let val = run_ok(
        r#"fn main() -> Bool {
            let xs0 = collections.List.new()
            let xs1 = xs0.push(1)
            let xs2 = xs1.push(2)
            let xs3 = xs2.push(3)
            let xs4 = xs3.push(4)
            let xs5 = xs4.push(5)
            xs0.len() == 0 && xs3.len() == 3 && xs5.len() == 5
        }"#,
    );
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_map_rebinding_chain_preserves_prior_versions() {
    let val = run_ok(
        r#"fn main() -> Bool {
            let m0 = collections.Map.new()
            let m1 = m0.insert(1, 10)
            let m2 = m1.insert(2, 20)
            let m3 = m2.insert(3, 30)
            let m4 = m3.insert(4, 40)
            m0.len() == 0 && m2.len() == 2 && m4.len() == 4
        }"#,
    );
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_set_rebinding_chain_preserves_prior_versions() {
    let val = run_ok(
        r#"fn main() -> Bool {
            let s0 = collections.Set.new()
            let s1 = s0.insert(1)
            let s2 = s1.insert(2)
            let s3 = s2.insert(3)
            let s4 = s3.insert(4)
            s0.len() == 0 && s2.len() == 2 && s4.len() == 4
        }"#,
    );
    assert!(matches!(val, Value::Bool(true)));
}

// ── BitSet / MutableBitSet behavior tests ─────────────────────────────

#[test]
fn eval_bitset_new_is_empty_and_has_fixed_size() {
    let val = run_ok(
        r#"import collections
fn main() -> Bool {
    let bs = collections.BitSet.new(8)
    bs.is_empty() && bs.count() == 0 && bs.size() == 8
}"#,
    );
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_bitset_set_reset_flip_test_and_values() {
    let val = run_ok(
        r#"import collections
fn main() -> Bool {
    let bs = collections.BitSet.new(10).set(3).set(1).flip(3).set(7).reset(1)
    let vals = bs.values().to_list()
    bs.count() == 1
        && bs.test(7)
        && !bs.test(1)
        && !bs.test(3)
        && vals.len() == 1
        && vals.get(0).unwrap_or(-1) == 7
}"#,
    );
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_bitset_values_are_ascending() {
    let val = run_ok(
        r#"import collections
fn main() -> Bool {
    let vals = collections.BitSet.new(16).set(9).set(1).set(12).set(3).values().to_list()
    vals.get(0).unwrap_or(-1) == 1
        && vals.get(1).unwrap_or(-1) == 3
        && vals.get(2).unwrap_or(-1) == 9
        && vals.get(3).unwrap_or(-1) == 12
}"#,
    );
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_bitset_algebra_is_correct() {
    let val = run_ok(
        r#"import collections
fn main() -> Bool {
    let a = collections.BitSet.new(16).set(1).set(3).set(5)
    let b = collections.BitSet.new(16).set(3).set(4).set(5)
    let u = a.union(b)
    let i = a.intersection(b)
    let d = a.difference(b)
    let x = a.xor(b)
    u.count() == 4
        && i.count() == 2
        && d.count() == 1 && d.test(1)
        && x.count() == 2 && x.test(1) && x.test(4)
}"#,
    );
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_bitset_is_immutable() {
    let val = run_ok(
        r#"import collections
fn main() -> Bool {
    let a = collections.BitSet.new(8).set(1)
    let b = a.set(2)
    a.count() == 1 && b.count() == 2 && !a.test(2) && b.test(2)
}"#,
    );
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_mutable_bitset_aliases_observe_in_place_mutation() {
    let val = run_ok(
        r#"import collections
fn main() -> Bool {
    let a = collections.MutableBitSet.new(8)
    let b = a
    a.set(1)
    b.flip(2)
    a.test(1) && a.test(2) && b.count() == 2
}"#,
    );
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_mutable_bitset_algebra_is_alias_visible_and_returns_self() {
    let val = run_ok(
        r#"import collections
fn main() -> Bool {
    let a = collections.MutableBitSet.new(8).set(1).set(3)
    let b = collections.MutableBitSet.new(8).set(3).set(4)
    let alias = a
    a.union(b)
    alias.count() == 3 && alias.test(1) && alias.test(3) && alias.test(4)
}"#,
    );
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_bitset_zero_size_is_valid() {
    let val = run_ok(
        r#"import collections
fn main() -> Bool {
    let bs = collections.BitSet.new(0)
    bs.size() == 0 && bs.count() == 0 && bs.is_empty() && bs.values().count() == 0
}"#,
    );
    assert!(matches!(val, Value::Bool(true)));
}

#[test]
fn eval_bitset_out_of_bounds_runtime_error() {
    let err = run_err(
        r#"import collections
fn main() -> Int {
    collections.BitSet.new(4).set(4).count()
}"#,
    );
    assert!(
        err.contains("bitset index out of bounds"),
        "expected bitset bounds error, got: {err}"
    );
}

#[test]
fn eval_bitset_negative_index_runtime_error() {
    let err = run_err(
        r#"import collections
fn main() -> Bool {
    collections.BitSet.new(4).test(0 - 1)
}"#,
    );
    assert!(
        err.contains("bitset index out of bounds"),
        "expected bitset negative-index bounds error, got: {err}"
    );
}

#[test]
fn eval_bitset_binary_size_mismatch_runtime_error() {
    let err = run_err(
        r#"import collections
fn main() -> Int {
    let a = collections.BitSet.new(4).set(1)
    let b = collections.BitSet.new(8).set(1)
    a.union(b).count()
}"#,
    );
    assert!(
        err.contains("bitset size mismatch"),
        "expected bitset size mismatch error, got: {err}"
    );
}

// ── List.sort() element type compile-time rejection ─────────────

#[test]
fn eval_list_sort_list_of_lists_is_allowed() {
    let val = run_ok(
        "fn main() -> Int {
            let a: List<Int> = collections.List.new()
            let b = collections.List.new().push(1)
            let xs = collections.List.new().push(b).push(a)
            let sorted = xs.sort()
            match (sorted.get(0)) {
                Some(inner) => inner.len()
                None => -1
            }
        }",
    );
    assert_eq!(val, Value::Int(0));
}

#[test]
fn eval_list_sort_unsortable_list_of_fns() {
    let err = run_err(
        "fn helper() -> Int { 0 }
         fn main() -> Int {
            let xs = collections.List.new().push(helper)
            let sorted = xs.sort()
            0
        }",
    );
    assert!(
        err.contains("cannot be sorted"),
        "expected compile-time sort rejection, got: {err}"
    );
}

#[test]
fn eval_list_sort_unsortable_list_of_maps() {
    let err = run_err(
        "fn main() -> Int {
            let xs = collections.List.new().push(collections.Map.new())
            let sorted = xs.sort()
            0
        }",
    );
    assert!(
        err.contains("cannot be sorted"),
        "expected compile-time sort rejection, got: {err}"
    );
}

#[test]
fn eval_list_sort_valid_types_no_rejection() {
    // Guard test: sortable builtin and structural value types should NOT be rejected.
    run_ok(
        r#"fn main() -> Bool {
            let ints = collections.List.new().push(3).push(1).push(2).sort()
            let strings = collections.List.new().push("c").push("a").push("b").sort()
            let chars = collections.List.new().push('c').push('a').push('b').sort()
            let bools = collections.List.new().push(true).push(false).sort()
            let lists = collections.List.new().push(collections.List.new().push(1)).push(collections.List.new()).sort()
            ints.len() == 3 && strings.len() == 3 && chars.len() == 3 && bools.len() == 2 && lists.len() == 2
        }"#,
    );
}

// ── Negative index and error consistency (#348, #349) ────────────

#[test]
fn eval_list_get_negative_index_returns_none() {
    // Negative index should return None, not silently wrap to huge usize.
    let val = run_ok(
        "fn main() -> Int {
           let xs = collections.List.new().push(10).push(20)
           match (xs.get(0 - 1)) {
             Some(_) => 1
             None => 0
           }
         }",
    );
    assert!(
        matches!(val, Value::Int(0)),
        "negative index should yield None, got: {val:?}"
    );
}

#[test]
fn eval_list_update_negative_index_error() {
    // Negative index in list_update should report IndexOutOfBounds, not TypeError.
    let err = run_err(
        "fn main() -> Int {
           collections.List.new().push(10).update(0 - 1, fn(n: Int) => n + 1).len()
         }",
    );
    assert!(
        err.contains("index out of bounds"),
        "expected IndexOutOfBounds error, got: {err}"
    );
    // Should NOT be a TypeError.
    assert!(
        !err.starts_with("type error"),
        "should use IndexOutOfBounds, not TypeError: {err}"
    );
}

#[test]
fn eval_list_update_oob_error_type() {
    // list_update with positive OOB should also use IndexOutOfBounds.
    let err = run_err(
        "fn main() -> Int {
           collections.List.new().push(10).update(5, fn(n: Int) => n + 1).len()
         }",
    );
    assert!(
        err.contains("index out of bounds"),
        "expected IndexOutOfBounds error, got: {err}"
    );
    assert!(
        !err.starts_with("type error"),
        "should use IndexOutOfBounds, not TypeError: {err}"
    );
}

#[test]
fn eval_list_set_oob_error_type() {
    // list_set with positive OOB should also use IndexOutOfBounds, not TypeError.
    let err = run_err(
        "fn main() -> Int {
           collections.List.new().push(10).set(5, 0).len()
         }",
    );
    assert!(
        err.contains("index out of bounds"),
        "expected IndexOutOfBounds error, got: {err}"
    );
    assert!(
        !err.starts_with("type error"),
        "should use IndexOutOfBounds, not TypeError: {err}"
    );
}
