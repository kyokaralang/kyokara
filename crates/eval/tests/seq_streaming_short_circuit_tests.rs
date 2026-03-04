#![allow(clippy::unwrap_used)]

use kyokara_eval::value::Value;

fn run_ok(source: &str) -> Value {
    match kyokara_eval::run(source) {
        Ok(result) => result.value,
        Err(e) => panic!("runtime error: {e}"),
    }
}

#[test]
fn eval_seq_zip_does_not_force_full_right_input_before_first_output() {
    let val = run_ok(
        r#"fn main() -> Int {
            let rhs = (0..<5).map(fn(n: Int) =>
                if (n == 4) { 1 / 0 } else { n }
            )
            let pairs = (0..<1).zip(rhs).to_list()
            pairs[0].right
        }"#,
    );
    assert_eq!(val, Value::Int(0));
}

#[test]
fn eval_seq_zip_does_not_force_full_left_input_before_first_output() {
    let val = run_ok(
        r#"fn main() -> Int {
            let lhs = (0..<5).map(fn(n: Int) =>
                if (n == 4) { 1 / 0 } else { n }
            )
            let pairs = lhs.zip((0..<1)).to_list()
            pairs[0].left
        }"#,
    );
    assert_eq!(val, Value::Int(0));
}

#[test]
fn eval_seq_chunks_short_circuits_without_traversing_full_input() {
    let val = run_ok(
        r#"fn main() -> Bool {
            let xs = (0..<5).map(fn(n: Int) =>
                if (n == 4) { 1 / 0 } else { n }
            )
            xs.chunks(2).any(fn(_chunk: List<Int>) => true)
        }"#,
    );
    assert_eq!(val, Value::Bool(true));
}

#[test]
fn eval_seq_windows_short_circuits_without_traversing_full_input() {
    let val = run_ok(
        r#"fn main() -> Bool {
            let xs = (0..<5).map(fn(n: Int) =>
                if (n == 4) { 1 / 0 } else { n }
            )
            xs.windows(2).any(fn(_window: List<Int>) => true)
        }"#,
    );
    assert_eq!(val, Value::Bool(true));
}
