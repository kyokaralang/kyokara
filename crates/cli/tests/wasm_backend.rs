#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_kyokara"))
}

fn run_cli(cwd: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("run kyokara")
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_stdout_trimmed(output: &Output, expected: &str, context: &str) {
    assert_success(output, context);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        expected,
        "{context} stdout mismatch\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn run_backend_wasm_executes_single_file_program() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "fn main() -> String { \"he\".concat(\"llo\").concat(\" wasm\") }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(&output, "hello wasm", "run --backend wasm");
}

#[test]
fn run_backend_wasm_displays_structural_main_output() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "type Point = { x: Int, y: Int }\nfn main() -> Point { { x: 3, y: 4 } }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "{ x: 3, y: 4 }",
        "run --backend wasm structural main output",
    );
}

#[test]
fn run_backend_wasm_displays_collection_main_output() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableMap\nfn main() -> MutableMap<String, Int> { MutableMap.new().insert(\"k\", 1).insert(\"z\", 2) }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "MutableMap({k: 1, z: 2})",
        "run --backend wasm collection main output",
    );
}

#[test]
fn run_backend_wasm_supports_io_println_effects() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "import io\nfn main() -> Unit { io.println(\"hello from wasm io\") }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "hello from wasm io",
        "run --backend wasm with io.println",
    );
}

#[test]
fn run_backend_wasm_supports_direct_imported_hash_md5() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from hash import md5\nfn main() -> String { md5(\"abc\") }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "900150983cd24fb0d6963f7d28e17f72",
        "run --backend wasm with direct-imported hash.md5",
    );
}

#[test]
fn run_backend_wasm_supports_fs_read_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    let input = dir.path().join("input.txt");
    fs::write(&input, "hello from file").expect("write input");
    fs::write(
        &file,
        format!(
            "import fs\nfn main() -> String {{ fs.read_file(\"{}\") }}",
            input.display()
        ),
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "hello from file",
        "run --backend wasm with fs.read_file",
    );
}

#[test]
fn run_backend_wasm_supports_direct_imported_fs_read_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    let input = dir.path().join("input.txt");
    fs::write(&input, "hello from direct import").expect("write input");
    fs::write(
        &file,
        format!(
            "from fs import read_file\nfn main() -> String {{ read_file(\"{}\") }}",
            input.display()
        ),
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "hello from direct import",
        "run --backend wasm with direct-imported fs.read_file",
    );
}

#[test]
fn run_backend_wasm_supports_short_circuit_while_conditions() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "fn main() -> Int {\n  var i = 0\n  while (i < 7 && i < 10) {\n    i = i + 1\n  }\n  i\n}",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(&output, "7", "run --backend wasm with short-circuit while");
}

#[test]
fn run_backend_wasm_handles_deep_recursive_workloads_without_native_stack_overflow() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "fn dive(n: Int) -> Int { if (n == 0) { 0 } else { dive(n - 1) } }\nfn main() -> Int { dive(50000) }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(&output, "0", "run --backend wasm with deep recursion");
}

#[test]
fn run_backend_wasm_handles_mutable_list_growth_and_set_loops() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         fn main() -> Int {\n\
           let xs = MutableList.from_list((0..<200).map(fn(_i: Int) => 0).to_list())\n\
           var i = 0\n\
           while (i < 200) {\n\
             let _ = xs.set(i, i % 10)\n\
             i = i + 1\n\
           }\n\
           xs[199]\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "9",
        "run --backend wasm with MutableList growth and set loop",
    );
}

#[test]
fn run_backend_wasm_handles_large_mutable_list_push_workloads() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         fn main() -> Int {\n\
           let xs = MutableList.new()\n\
           var i = 0\n\
           while (i < 50000) {\n\
             let _ = xs.push(i % 10)\n\
             i = i + 1\n\
           }\n\
           xs[49999]\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "9",
        "run --backend wasm with large MutableList push workload",
    );
}

#[test]
fn run_backend_wasm_supports_for_loops_over_char_sequences() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    let text = "1234567890".repeat(100);
    fs::write(
        &file,
        format!(
            "fn main() -> Int {{\n  let text = \"{text}\"\n  var sum = 0\n  for (ch in text.chars()) {{\n    sum = sum + (ch.code() - '0'.code())\n  }}\n  sum\n}}"
        ),
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(&output, "4500", "run --backend wasm with for-over-chars");
}

#[test]
fn run_backend_wasm_supports_string_indexing_at_high_offsets() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    let text = "1234567890".repeat(20);
    fs::write(
        &file,
        format!("fn main() -> Int {{ \"{text}\"[199].code() - '0'.code() }}"),
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(&output, "0", "run --backend wasm with string indexing");
}

#[test]
fn run_backend_wasm_preserves_values_after_nested_while_loops() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "fn main() -> String {\n\
           var pos = 0\n\
           var s = \"\"\n\
           while (pos < 3) {\n\
             var total = 0\n\
             var start = pos\n\
             while (start < 3) {\n\
               total = total + 1\n\
               start = start + 1\n\
             }\n\
             s = s.concat(total.to_string())\n\
             pos = pos + 1\n\
           }\n\
           s\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "321",
        "run --backend wasm with nested while loop carried values",
    );
}

#[test]
fn run_backend_wasm_preserves_loop_values_through_one_armed_if() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "fn main() -> Int {\n\
           var total = 0\n\
           var start = 0\n\
           while (start < 1) {\n\
             total = total + 12\n\
             let cond = false\n\
             if (cond) {\n\
               total = total - 1\n\
             }\n\
             start = start + 1\n\
           }\n\
           total\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "12",
        "run --backend wasm with loop-carried values through one-armed if",
    );
}

#[test]
fn run_backend_wasm_matches_fft_sample_output() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         fn parse_digits(text: String) -> MutableList<Int> {\n\
           let digits = MutableList.new()\n\
           for (ch in text.chars()) {\n\
             let _ = digits.push(ch.code() - '0'.code())\n\
           }\n\
           digits\n\
         }\n\
         fn prefix_sums(xs: MutableList<Int>) -> MutableList<Int> {\n\
           let sums = MutableList.new().push(0)\n\
           var acc = 0\n\
           var i = 0\n\
           while (i < xs.len()) {\n\
             acc = acc + xs[i]\n\
             let _ = sums.push(acc)\n\
             i = i + 1\n\
           }\n\
           sums\n\
         }\n\
         fn abs_i(n: Int) -> Int {\n\
           if (n < 0) { -n } else { n }\n\
         }\n\
         fn phase(input: MutableList<Int>) -> MutableList<Int> {\n\
           let n = input.len()\n\
           let sums = prefix_sums(input)\n\
           let out = MutableList.new()\n\
           var pos = 0\n\
           while (pos < n) {\n\
             let step = pos + 1\n\
             var total = 0\n\
             var start = pos\n\
             while (start < n) {\n\
               let plus_end = if (start + step < n) { start + step } else { n }\n\
               total = total + (sums[plus_end] - sums[start])\n\
               let minus_start = start + (2 * step)\n\
               if (minus_start < n) {\n\
                 let minus_end = if (minus_start + step < n) { minus_start + step } else { n }\n\
                 total = total - (sums[minus_end] - sums[minus_start])\n\
               }\n\
               start = start + (4 * step)\n\
             }\n\
             let _ = out.push(abs_i(total) % 10)\n\
             pos = pos + 1\n\
           }\n\
           out\n\
         }\n\
         fn first8(xs: MutableList<Int>) -> String {\n\
           var s = \"\"\n\
           var i = 0\n\
           while (i < 8 && i < xs.len()) {\n\
             s = s.concat(xs[i].to_string())\n\
             i = i + 1\n\
           }\n\
           s\n\
         }\n\
         fn main() -> String {\n\
           var digits = parse_digits(\"12345678\")\n\
           var i = 0\n\
           while (i < 4) {\n\
             digits = phase(digits)\n\
             i = i + 1\n\
           }\n\
           first8(digits)\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(&output, "01029498", "run --backend wasm with fft sample");
}

#[test]
fn replay_dispatches_wasm_logs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    let log = dir.path().join("run.jsonl");
    fs::write(&file, "fn main() -> Int { 42 }").expect("write source");

    let run = run_cli(
        dir.path(),
        &[
            "run",
            "main.ky",
            "--backend",
            "wasm",
            "--replay-log",
            log.to_str().expect("utf-8 log path"),
        ],
    );
    assert_stdout_trimmed(&run, "42", "run --backend wasm --replay-log");

    let replay = run_cli(
        dir.path(),
        &["replay", log.to_str().expect("utf-8 log path")],
    );
    assert_stdout_trimmed(&replay, "42", "replay wasm log");
}

#[test]
fn replay_dispatches_wasm_logs_for_structural_output() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    let log = dir.path().join("run.jsonl");
    fs::write(
        &file,
        "type Point = { x: Int, y: Int }\nfn main() -> Point { { x: 5, y: 8 } }",
    )
    .expect("write source");

    let run = run_cli(
        dir.path(),
        &[
            "run",
            "main.ky",
            "--backend",
            "wasm",
            "--replay-log",
            log.to_str().expect("utf-8 log path"),
        ],
    );
    assert_stdout_trimmed(
        &run,
        "{ x: 5, y: 8 }",
        "run --backend wasm --replay-log structural output",
    );

    let replay = run_cli(
        dir.path(),
        &["replay", log.to_str().expect("utf-8 log path")],
    );
    assert_stdout_trimmed(&replay, "{ x: 5, y: 8 }", "replay wasm structural log");
}

#[test]
fn run_backend_wasm_executes_project_mode_program() {
    let dir = tempfile::tempdir().expect("tempdir");
    let app = dir.path().join("main.ky");
    let helper = dir.path().join("helper.ky");
    fs::write(
        &app,
        "from helper import value\nfn main() -> Int { value() }",
    )
    .expect("write app");
    fs::write(&helper, "pub fn value() -> Int { 7 }").expect("write helper");

    let output = run_cli(
        dir.path(),
        &["run", "main.ky", "--backend", "wasm", "--project"],
    );
    assert_stdout_trimmed(&output, "7", "wasm project-mode run");
}

#[test]
fn run_backend_wasm_displays_structural_project_output() {
    let dir = tempfile::tempdir().expect("tempdir");
    let app = dir.path().join("main.ky");
    let helper = dir.path().join("helper.ky");
    fs::write(
        &app,
        "from helper import make\nfn main() -> { x: Int, y: Int } { make() }",
    )
    .expect("write app");
    fs::write(
        &helper,
        "pub fn make() -> { x: Int, y: Int } { { x: 7, y: 9 } }",
    )
    .expect("write helper");

    let output = run_cli(
        dir.path(),
        &["run", "main.ky", "--backend", "wasm", "--project"],
    );
    assert_stdout_trimmed(
        &output,
        "{ x: 7, y: 9 }",
        "wasm project-mode structural output",
    );
}

#[test]
fn build_backend_wasm_supports_project_mode() {
    let dir = tempfile::tempdir().expect("tempdir");
    let app = dir.path().join("main.ky");
    let helper = dir.path().join("helper.ky");
    let out = dir.path().join("out").join("app.wasm");
    fs::write(
        &app,
        "from helper import value\nfn main() -> Int { value() }",
    )
    .expect("write app");
    fs::write(&helper, "pub fn value() -> Int { 7 }").expect("write helper");

    let output = run_cli(
        dir.path(),
        &[
            "build",
            "main.ky",
            "--project",
            "--target",
            "wasm",
            "--out",
            out.to_str().expect("utf-8 out path"),
        ],
    );
    assert_success(&output, "wasm project-mode build");
    assert!(out.is_file(), "expected wasm artifact at {}", out.display());
}

#[test]
fn replay_dispatches_wasm_project_logs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let app = dir.path().join("main.ky");
    let helper = dir.path().join("helper.ky");
    let log = dir.path().join("run.jsonl");
    fs::write(
        &app,
        "from helper import value\nfn main() -> Int { value() }",
    )
    .expect("write app");
    fs::write(&helper, "pub fn value() -> Int { 7 }").expect("write helper");

    let run = run_cli(
        dir.path(),
        &[
            "run",
            "main.ky",
            "--project",
            "--backend",
            "wasm",
            "--replay-log",
            log.to_str().expect("utf-8 log path"),
        ],
    );
    assert_stdout_trimmed(&run, "7", "wasm project-mode run with replay log");

    let replay = run_cli(
        dir.path(),
        &["replay", log.to_str().expect("utf-8 log path")],
    );
    assert_stdout_trimmed(&replay, "7", "replay wasm project log");
}
