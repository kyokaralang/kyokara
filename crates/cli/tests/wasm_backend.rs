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
fn run_backend_wasm_short_circuits_indexed_while_guards() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         fn main() -> Int {\n\
           let blocks = MutableList.new().push(0).push(-1).push(1).push(-1).push(2)\n\
           let left = MutableList.new().push(0)\n\
           while (left[0] < blocks.len() && blocks[left[0]] != -1) {\n\
             let _l = left.set(0, left[0] + 1)\n\
           }\n\
           left[0]\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "1",
        "run --backend wasm with indexed short-circuit while guard",
    );
}

#[test]
fn run_backend_wasm_preserves_short_circuit_if_results_across_loop_iterations() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from io import println\n\
         fn main() -> Unit {\n\
           var idx = 0\n\
           while (idx < 2) {\n\
             let ox1 = if (idx == 0) { 0 } else { 5 }\n\
             let ox2 = if (idx == 0) { 1 } else { 2 }\n\
             let oy1 = if (idx == 0) { 0 } else { 28 }\n\
             let oy2 = if (idx == 0) { 1 } else { -2 }\n\
             let oz1 = -17\n\
             let oz2 = -6\n\
             if (ox1 <= ox2 && oy1 <= oy2 && oz1 <= oz2) {\n\
               println(\"T\")\n\
             } else {\n\
               println(\"F\")\n\
             }\n\
             idx = idx + 1\n\
           }\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "T\nF",
        "run --backend wasm with short-circuit if inside loop",
    );
}

#[test]
fn run_backend_wasm_preserves_record_accumulators_across_nested_split_folds() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from io import println\n\
         type G = { a: Int, b: Int }\n\
         fn main() -> Unit {\n\
           let out = \"a,b;a,b\".split(\";\").fold(\n\
             G { a: 0, b: 0 },\n\
             fn(acc: G, s: String) => s.split(\",\").fold(\n\
               acc,\n\
               fn(game: G, part: String) => if (part == \"a\") {\n\
                     G { a: game.a + 1, b: game.b }\n\
                   } else {\n\
                     G { a: game.a, b: game.b + 1 }\n\
                   },\n\
             ),\n\
           )\n\
           println(\"Part 1: \".concat(out.a.to_string()).concat(\",\").concat(out.b.to_string()))\n\
           println(\"Part 2: ok\")\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "Part 1: 2,2\nPart 2: ok",
        "run --backend wasm preserves record accumulators across nested split folds",
    );
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
fn run_backend_wasm_handles_mutable_map_capacity_churn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableMap\n\
         fn main() -> Int {\n\
           let grid: MutableMap<Int, Int> = MutableMap.with_capacity(768)\n\
           for (i in 0 ..< 768) {\n\
             let _ = grid.insert(i, 1)\n\
           }\n\
           var round = 0\n\
           var acc = 0\n\
           while (round < 600) {\n\
             for (i in 0 ..< 768) {\n\
               let key = i + (round + 1) * 100000\n\
               if (grid.contains(key - 1)) {\n\
                 acc = acc + 1\n\
               }\n\
               let _ = grid.insert(key, i)\n\
               if (grid.get(key).unwrap_or(-1) == i) {\n\
                 acc = acc + 1\n\
               }\n\
               let _ = grid.remove(key)\n\
             }\n\
             round = round + 1\n\
           }\n\
           acc + grid.len()\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "461568",
        "run --backend wasm with mutable map capacity churn",
    );
}

#[test]
fn run_backend_wasm_handles_mutable_map_string_keys_after_growth() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableMap\n\
         fn main() -> Int {\n\
           let m = MutableMap.new().insert(\"k\", 1).insert(\"z\", 2)\n\
           m.get(\"k\").unwrap_or(-1) + m.get(\"z\").unwrap_or(-1)\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "3",
        "run --backend wasm with mutable map string keys after growth",
    );
}

#[test]
fn run_backend_wasm_preserves_mutable_loop_locals_across_if_merges() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "fn count(flag: Bool) -> Int {\n\
           var total = 0\n\
           for (step in (0..<420).to_list()) {\n\
             var x0 = step\n\
             var x1 = step\n\
             var x2 = step\n\
             var valid = true\n\
             if (flag) {\n\
               x0 = x0 + 0\n\
               x1 = x1 + 0\n\
               x2 = x2 + 0\n\
               if (x0 > 19 || x1 > 19 || x2 > 19) {\n\
                 valid = false\n\
               }\n\
             }\n\
             if (valid) {\n\
               total = total + 1\n\
             }\n\
           }\n\
           total\n\
         }\n\
         fn main() -> String {\n\
           count(false).to_string().concat(\":\").concat(count(true).to_string())\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "420:20",
        "run --backend wasm preserves mutable loop locals across if merges",
    );
}

#[test]
fn run_backend_wasm_preserves_side_effect_only_while_conditions() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         fn main() -> Int {\n\
           let i = MutableList.new().push(0)\n\
           while (i[0] >= 0) {\n\
             let _i = i.set(0, -1)\n\
           }\n\
           11\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "11",
        "run --backend wasm preserves side-effect-only while conditions",
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
fn run_backend_wasm_materializes_large_ranges_to_lists() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(&file, "fn main() -> Int { (0..<40000).to_list().len() }").expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "40000",
        "run --backend wasm with large range.to_list()",
    );
}

#[test]
fn run_backend_wasm_materializes_large_range_maps_before_mutable_list_copies() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         fn main() -> Int {\n\
           let xs = MutableList.from_list((0..<40000).map(fn(i: Int) => i % 10).to_list())\n\
           xs[39999]\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "9",
        "run --backend wasm with large range.map(...).to_list() feeding MutableList.from_list()",
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
fn run_backend_wasm_preserves_string_values_through_list_map() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         fn id_string(s: String) -> String { s }\n\
         fn main() -> Int {\n\
           let xs = MutableList.new().push(\"aa\").push(\"bbb\").push(\"cccc\").to_list().map(id_string).to_list()\n\
           xs[1].len()\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "3",
        "run --backend wasm with list<String>.map(identity)",
    );
}

#[test]
fn run_backend_wasm_preserves_char_values_through_list_map() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         fn id_char(ch: Char) -> Char { ch }\n\
         fn main() -> Int {\n\
           let xs = MutableList.new().push('a').push('b').push('c').to_list().map(id_char).to_list()\n\
           xs[1].code()\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "98",
        "run --backend wasm with list<Char>.map(identity)",
    );
}

#[test]
fn run_backend_wasm_preserves_direct_fnref_in_large_chars_map() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    let input = dir.path().join("input.txt");
    fs::write(&input, format!("{}\n", "0".repeat(16_384))).expect("write input");
    fs::write(
        &file,
        "from fs import read_file\n\
         fn digit_value(ch: Char) -> Int { if (ch == '0') { 0 } else { 1 } }\n\
         fn main() -> Int {\n\
           read_file(\"input.txt\").trim().chars().map(digit_value).to_list().len()\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "16384",
        "run --backend wasm with large chars().map(top_level_fn)",
    );
}

#[test]
fn run_backend_wasm_preserves_direct_fnref_in_large_chars_map_after_heap_pressure() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    let input = dir.path().join("input.txt");
    fs::write(&input, format!("{}\n", "0".repeat(16_369))).expect("write input");
    fs::write(
        &file,
        "from collections import MutableList\n\
         from fs import read_file\n\
         fn digit_value(ch: Char) -> Int { if (ch == '0') { 0 } else { 1 } }\n\
         fn burn() -> Unit {\n\
           let xs = MutableList.new()\n\
           var i = 0\n\
           while (i < 300000) {\n\
             let _ = xs.push(i)\n\
             i = i + 1\n\
           }\n\
         }\n\
         fn main() -> Int {\n\
           burn()\n\
           read_file(\"input.txt\").trim().chars().map(digit_value).to_list().len()\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "16369",
        "run --backend wasm with high-heap chars().map(top_level_fn)",
    );
}

#[test]
fn run_backend_wasm_preserves_chars_materialization_inside_filtered_string_loops() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         fn main() -> Int {\n\
           let lines = MutableList.new().push(\"algo\").push(\"\").push(\"aaa\").push(\"bbb\").push(\"ccc\").to_list()\n\
           let filtered = lines.filter(fn(line: String) => line.len() > 0).to_list()\n\
           var total = 0\n\
           var i = 0\n\
           while (i < filtered.len()) {\n\
             total = total + filtered[i].chars().to_list().len()\n\
             i = i + 1\n\
           }\n\
           total\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "13",
        "run --backend wasm with filtered strings feeding chars().to_list() in a loop",
    );
}

#[test]
fn run_backend_wasm_preserves_split_filter_to_list_over_strings() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "fn main() -> Int {\n\
           \"a b  c\".split(\" \").filter(fn(word: String) => word.len() > 0).to_list().len()\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "3",
        "run --backend wasm with split(...).filter(...).to_list() over strings",
    );
}

#[test]
fn run_backend_wasm_preserves_chars_materialization_inside_range_mapped_string_loops() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         fn main() -> Int {\n\
           let lines = MutableList.new().push(\"algo\").push(\"\").push(\"aaa\").push(\"bbb\").push(\"ccc\").to_list()\n\
           let mapped = (2 ..< lines.len()).map(fn(i: Int) => lines[i]).to_list()\n\
           var total = 0\n\
           var i = 0\n\
           while (i < mapped.len()) {\n\
             total = total + mapped[i].chars().to_list().len()\n\
             i = i + 1\n\
           }\n\
           total\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "9",
        "run --backend wasm with range-mapped strings feeding chars().to_list() in a loop",
    );
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
fn build_backend_wasm_handles_unused_loop_helpers_with_multiple_returns() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    let out = dir.path().join("out.wasm");
    fs::write(
        &file,
        "from collections import MutableList\n\
         type Table = { keys: MutableList<Int>, states: MutableList<Int>, size: Int }\n\
         fn helper(table: Table, key: Int) -> Int {\n\
           var idx = 0\n\
           while (true) {\n\
             let cur = table.keys[idx]\n\
             if (cur == -1) {\n\
               return 0\n\
             }\n\
             if (cur == key) {\n\
               return table.states[idx]\n\
             }\n\
             idx = (idx + 1) % table.size\n\
           }\n\
           0\n\
         }\n\
         fn main() -> Int { 1 }",
    )
    .expect("write source");

    let output = run_cli(
        dir.path(),
        &[
            "build",
            "main.ky",
            "--target",
            "wasm",
            "--out",
            out.to_str().expect("utf-8 out path"),
        ],
    );
    assert_success(
        &output,
        "wasm build with unused loop helper using multiple returns",
    );
    assert!(out.is_file(), "expected wasm artifact at {}", out.display());
}

#[test]
fn run_backend_wasm_handles_unused_mutating_loop_helpers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         type Table = { keys: MutableList<Int>, states: MutableList<Int>, size: Int }\n\
         fn set_state(table: Table, key: Int, state: Int) -> Unit {\n\
           var idx = 0\n\
           while (true) {\n\
             let cur = table.keys[idx]\n\
             if (cur == -1 || cur == key) {\n\
               if (cur == -1) {\n\
                 let _k = table.keys.set(idx, key)\n\
               }\n\
               let _s = table.states.set(idx, state)\n\
               return\n\
             }\n\
             idx = (idx + 1) % table.size\n\
           }\n\
         }\n\
         fn main() -> Int { 1 }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "1",
        "run --backend wasm with unused mutating loop helper",
    );
}

#[test]
fn run_backend_wasm_executes_aoc_2024_day09_sample() {
    let dir = tempfile::tempdir().expect("tempdir");
    let input = dir.path().join("day09.txt");
    fs::write(&input, "2333133121414131402\n").expect("write sample input");

    let output = run_cli(
        dir.path(),
        &[
            "run",
            "/Users/alpha/CodexProjects/polyglot-bench/adapters/kyokara/solutions/advent-of-code/2024/day09.ky",
            "--backend",
            "wasm",
        ],
    );
    assert_stdout_trimmed(
        &output,
        "Part 1: 1928\nPart 2: 2858",
        "run --backend wasm AoC 2024 day09 sample",
    );
}

#[test]
fn run_backend_wasm_executes_aoc_2024_day09_reduced_prefix() {
    let dir = tempfile::tempdir().expect("tempdir");
    let input = dir.path().join("day09.txt");
    let prefix_len = 16_368usize;
    let full_input = fs::read_to_string(
        "/Users/alpha/CodexProjects/polyglot-bench/corpus/advent-of-code/2024/day09.txt",
    )
    .expect("read full day09 input");
    let reduced = full_input
        .trim()
        .chars()
        .take(prefix_len)
        .collect::<String>();
    fs::write(&input, format!("{reduced}\n")).expect("write reduced input");

    let output = run_cli(
        dir.path(),
        &[
            "run",
            "/Users/alpha/CodexProjects/polyglot-bench/adapters/kyokara/solutions/advent-of-code/2024/day09.ky",
            "--backend",
            "wasm",
        ],
    );
    assert_stdout_trimmed(
        &output,
        "Part 1: 3383998181182\nPart 2: 3407986590223",
        "run --backend wasm AoC 2024 day09 reduced prefix",
    );
}

#[test]
fn run_backend_wasm_executes_aoc_2024_day09_threshold_prefix() {
    let dir = tempfile::tempdir().expect("tempdir");
    let input = dir.path().join("day09.txt");
    let prefix_len = 16_369usize;
    let full_input = fs::read_to_string(
        "/Users/alpha/CodexProjects/polyglot-bench/corpus/advent-of-code/2024/day09.txt",
    )
    .expect("read full day09 input");
    let reduced = full_input
        .trim()
        .chars()
        .take(prefix_len)
        .collect::<String>();
    fs::write(&input, format!("{reduced}\n")).expect("write threshold input");

    let output = run_cli(
        dir.path(),
        &[
            "run",
            "/Users/alpha/CodexProjects/polyglot-bench/adapters/kyokara/solutions/advent-of-code/2024/day09.ky",
            "--backend",
            "wasm",
        ],
    );
    assert_stdout_trimmed(
        &output,
        "Part 1: 3384249318768\nPart 2: 3408130749484",
        "run --backend wasm AoC 2024 day09 threshold prefix",
    );
}

#[test]
fn run_backend_wasm_preserves_nested_compaction_loops() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         fn main() -> Int {\n\
           let blocks = MutableList.new().push(0).push(-1).push(1).push(-1).push(2)\n\
           let left = MutableList.new().push(0)\n\
           let right = MutableList.new().push(blocks.len() - 1)\n\
           while (left[0] < right[0]) {\n\
             while (left[0] < blocks.len() && blocks[left[0]] != -1) {\n\
               let _l = left.set(0, left[0] + 1)\n\
             }\n\
             while (right[0] >= 0 && blocks[right[0]] == -1) {\n\
               let _r = right.set(0, right[0] - 1)\n\
             }\n\
             if (left[0] < right[0]) {\n\
               let value = blocks[right[0]]\n\
               let _a = blocks.set(left[0], value)\n\
               let _b = blocks.set(right[0], -1)\n\
               let _l = left.set(0, left[0] + 1)\n\
               let _r = right.set(0, right[0] - 1)\n\
             }\n\
           }\n\
           var total = 0\n\
           for (i in 0..<blocks.len()) {\n\
             let value = blocks[i]\n\
             if (value >= 0) {\n\
               total = total + i * value\n\
             }\n\
           }\n\
           total\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "4",
        "run --backend wasm with nested compaction loops",
    );
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
