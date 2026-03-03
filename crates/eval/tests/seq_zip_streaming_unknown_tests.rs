#![allow(clippy::unwrap_used)]

use kyokara_eval::value::Value;

fn run_ok(source: &str) -> Value {
    match kyokara_eval::run(source) {
        Ok(result) => result.value,
        Err(e) => panic!("runtime error: {e}"),
    }
}

#[test]
fn eval_seq_zip_does_not_force_full_left_when_lengths_unknown() {
    let val = run_ok(
        r#"fn main() -> Int {
            let left = Seq.range(0, 5)
                .filter(fn(_n: Int) => true)
                .map(fn(n: Int) => if (n == 4) { 1 / 0 } else { n })
            let right = Seq.range(0, 1).filter(fn(_n: Int) => true)
            let pairs = left.zip(right).to_list()
            pairs[0].left
        }"#,
    );
    assert_eq!(val, Value::Int(0));
}

#[test]
fn eval_seq_zip_does_not_force_full_right_when_lengths_unknown() {
    let val = run_ok(
        r#"fn main() -> Int {
            let left = Seq.range(0, 1).filter(fn(_n: Int) => true)
            let right = Seq.range(0, 5)
                .filter(fn(_n: Int) => true)
                .map(fn(n: Int) => if (n == 4) { 1 / 0 } else { n })
            let pairs = left.zip(right).to_list()
            pairs[0].right
        }"#,
    );
    assert_eq!(val, Value::Int(0));
}
