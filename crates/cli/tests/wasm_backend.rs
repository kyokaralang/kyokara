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
fn run_backend_wasm_preserves_repeated_md5_special_strings() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        r#"from collections import MutableList

fn stretch(seed: String, rounds: Int) -> String {
  var hash = seed
  var i = 0
  while (i < rounds) {
    hash = hash.md5()
    i = i + 1
  }
  hash
}

fn main() -> String {
  let hashes = MutableList.new()
  var i = 0
  while (i < 2000) {
    let _ = hashes.push(stretch("abc".concat(i.to_string()), 200))
    i = i + 1
  }
  hashes.get(1999).unwrap_or("")
}"#,
    )
    .expect("write source");

    let native = run_cli(dir.path(), &["run", "main.ky"]);
    assert_success(&native, "run native with repeated md5 special strings");
    let expected = String::from_utf8_lossy(&native.stdout).trim().to_string();

    let wasm = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &wasm,
        &expected,
        "run --backend wasm preserves repeated md5 special strings",
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
fn run_backend_wasm_handles_repeated_large_char_fold_to_string() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         from io import println\n\
         type Grid = { cells: MutableList<Char> }\n\
         fn grid_key(grid: Grid) -> String {\n\
           grid.cells.to_list().fold(\"\", fn(acc: String, ch: Char) => acc.concat(ch.to_string()))\n\
         }\n\
         fn main() -> Unit {\n\
           let cells = MutableList.new()\n\
           for (i in 0 ..< 10000) {\n\
             let ch = if (i % 13 == 0) { 'O' } else if (i % 17 == 0) { '#' } else { '.' }\n\
             let _c = cells.push(ch)\n\
           }\n\
           let grid = Grid { cells: cells }\n\
           let total = MutableList.new().push(0)\n\
           for (_i in 0 ..< 2000) {\n\
             let key = grid_key(grid)\n\
             let _t = total.set(0, total[0] + key.len())\n\
           }\n\
           println(total[0].to_string())\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "20000000",
        "run --backend wasm with repeated large char fold to string",
    );
}

#[test]
fn run_backend_wasm_hashes_folded_large_string_map_keys() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList, MutableMap\n\
         fn grid_key(cells: MutableList<Char>) -> String {\n\
           cells.to_list().fold(\"\", fn(acc: String, ch: Char) => acc.concat(ch.to_string()))\n\
         }\n\
         fn build_cells() -> MutableList<Char> {\n\
           let cells = MutableList.new()\n\
           for (i in 0 ..< 12000) {\n\
             let ch = if (i % 13 == 0) { 'O' } else if (i % 17 == 0) { '#' } else { '.' }\n\
             let _c = cells.push(ch)\n\
           }\n\
           cells\n\
         }\n\
         fn main() -> Int {\n\
           let seen = MutableMap.new()\n\
           let key1 = grid_key(build_cells())\n\
           let key2 = grid_key(build_cells())\n\
           let _s = seen.insert(key1, 41)\n\
           seen.get(key2).unwrap_or(-1)\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "41",
        "run --backend wasm with folded large string map keys",
    );
}

#[test]
fn run_backend_wasm_handles_loop_match_continue_with_none_fallthrough() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from Option import Some, None\n\
         fn main() -> Int {\n\
           var i = 0\n\
           while (i < 3) {\n\
             let x = if (i < 2) { Some(i) } else { None }\n\
             match (x) {\n\
               Some(v) => {\n\
                 if (v == 0) {\n\
                   i = i + 1\n\
                   continue\n\
                 }\n\
               }\n\
               None => {}\n\
             }\n\
             i = i + 1\n\
           }\n\
           i\n\
         }\n",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "3",
        "run --backend wasm loop match continue with none fallthrough",
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
fn run_backend_wasm_preserves_chars_filter_to_list_builders() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "fn keep(_ch: Char) -> Bool { true }\n\
         fn row_types(line: String) -> Int {\n\
           line\n\
             .chars()\n\
             .filter(keep)\n\
             .map(fn(ch: Char) => if (ch == '#') { '.' } else { ch })\n\
             .filter(fn(ch: Char) => ch != '.')\n\
             .to_list()\n\
             .len()\n\
         }\n\
         fn main() -> String {\n\
           let line = \"###B#A#D#C###\"\n\
           row_types(line)\n\
             .to_string()\n\
             .concat(\",\")\n\
             .concat(line.chars().filter(keep).to_list().len().to_string())\n\
         }\n",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "4,13",
        "run --backend wasm preserves chars().filter(...).to_list() builders",
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
fn run_backend_wasm_handles_mutable_map_int_keys_after_growth() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableMap\n\
         fn main() -> Int {\n\
           let m = MutableMap.new()\n\
           for (i in 0 ..< 6000) {\n\
             let _ = m.insert(i, i * 2)\n\
           }\n\
           m.keys().to_list().len()\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "6000",
        "run --backend wasm with mutable map int keys after growth",
    );
}

#[test]
fn run_backend_wasm_handles_mutable_map_int_values_after_growth() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableMap\n\
         fn main() -> Int {\n\
           let m = MutableMap.new()\n\
           for (i in 0 ..< 6000) {\n\
             let _ = m.insert(i, i * 2)\n\
           }\n\
           m.values().filter(fn(value: Int) => value >= 0).count()\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "6000",
        "run --backend wasm with mutable map int values after growth",
    );
}

#[test]
fn run_backend_wasm_handles_mutable_set_values_after_growth() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableSet\n\
         fn main() -> Int {\n\
           let s = MutableSet.new()\n\
           for (i in 0 ..< 6000) {\n\
             let _ = s.insert(i)\n\
           }\n\
           s.values().to_list().len()\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "6000",
        "run --backend wasm with mutable set values after growth",
    );
}

#[test]
fn run_backend_wasm_handles_range_mapped_mutable_list_from_list() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         fn build_occ(width: Int, height: Int) -> MutableList<Int> {\n\
           MutableList.from_list((0 ..< (width * height)).map(fn(_i: Int) => -1).to_list())\n\
         }\n\
         fn main() -> Int {\n\
           let occ = build_occ(32, 32)\n\
           var total = 0\n\
           var i = 0\n\
           while (i < occ.len()) {\n\
             total = total + occ[i]\n\
             i = i + 1\n\
           }\n\
           total\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "-1024",
        "run --backend wasm with range-mapped MutableList.from_list",
    );
}

#[test]
fn run_backend_wasm_handles_day15_build_occ_shape() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         fn build_occ(width: Int, height: Int, alive: MutableList<Int>, ux: MutableList<Int>, uy: MutableList<Int>) -> MutableList<Int> {\n\
           let occ = MutableList.from_list((0 ..< (width * height)).map(fn(_i: Int) => -1).to_list())\n\
           var i = 0\n\
           while (i < alive.len()) {\n\
             if (alive[i] == 1) {\n\
               let _o = occ.set(uy[i] * width + ux[i], i)\n\
             }\n\
             i = i + 1\n\
           }\n\
           occ\n\
         }\n\
         fn main() -> Int {\n\
           let alive = MutableList.from_list((0 ..< 512).map(fn(_i: Int) => 1).to_list())\n\
           let ux = MutableList.from_list((0 ..< 512).map(fn(i: Int) => i % 32).to_list())\n\
           let uy = MutableList.from_list((0 ..< 512).map(fn(i: Int) => i / 32).to_list())\n\
           let occ = build_occ(32, 32, alive, ux, uy)\n\
           occ[0] + occ[31] + occ[32] + occ[511] + occ[900]\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "573",
        "run --backend wasm with day15 build_occ shape",
    );
}

#[test]
fn run_backend_wasm_preserves_mutable_list_record_set_and_get() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         type Point = { row: Int, col: Int }\n\
         fn main() -> String {\n\
           let points = MutableList.from_list((0 ..< 3).map(fn(_i: Int) => Point { row: 0, col: 0 }).to_list())\n\
           let _a = points.set(1, Point { row: 23, col: 141 })\n\
           let _b = points.set(2, Point { row: 35, col: 133 })\n\
           points[1].row.to_string().concat(\",\").concat(points[1].col.to_string()).concat(\";\")\n\
             .concat(points[2].row.to_string()).concat(\",\").concat(points[2].col.to_string())\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "23,141;35,133",
        "run --backend wasm with MutableList record set/get",
    );
}

#[test]
fn run_backend_wasm_preserves_loop_progress_with_nested_never_taken_return() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         fn main() -> Int {\n\
           let xs = MutableList.new().push(0).push(1).push(2).push(3)\n\
           var i = 0\n\
           while (i < xs.len()) {\n\
             let x = xs[i]\n\
             if (x > 10) {\n\
               if (x == 99) {\n\
                 return 99\n\
               }\n\
             }\n\
             i = i + 1\n\
           }\n\
           i\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "4",
        "run --backend wasm preserves loop progress with nested never-taken return",
    );
}

#[test]
fn run_backend_wasm_executes_tiny_intcode_io_loop() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import List, MutableList\n\
         fn ensure(mem: MutableList<Int>, idx: Int) -> Unit {\n\
           while (mem.len() <= idx) {\n\
             let _m = mem.push(0)\n\
           }\n\
         }\n\
         fn read_mem(mem: MutableList<Int>, idx: Int) -> Int {\n\
           if (idx < 0) {\n\
             0\n\
           } else if (idx >= mem.len()) {\n\
             0\n\
           } else {\n\
             mem[idx]\n\
           }\n\
         }\n\
         fn param(mem: MutableList<Int>, idx: Int, mode: Int, rb: Int) -> Int {\n\
           let raw = read_mem(mem, idx)\n\
           if (mode == 0) {\n\
             read_mem(mem, raw)\n\
           } else if (mode == 1) {\n\
             raw\n\
           } else {\n\
             read_mem(mem, rb + raw)\n\
           }\n\
         }\n\
         fn addr(mem: MutableList<Int>, idx: Int, mode: Int, rb: Int) -> Int {\n\
           let raw = read_mem(mem, idx)\n\
           if (mode == 2) { rb + raw } else { raw }\n\
         }\n\
         fn run(program: List<Int>, x: Int) -> Int {\n\
           let mem = MutableList.from_list(program)\n\
           var ip = 0\n\
           var rb = 0\n\
           var input_pos = 0\n\
           var out = 0\n\
           while (true) {\n\
             let instr = read_mem(mem, ip)\n\
             let op = instr % 100\n\
             let m1 = (instr / 100) % 10\n\
             let m2 = (instr / 1000) % 10\n\
             let m3 = (instr / 10000) % 10\n\
             if (op == 99) {\n\
               return out\n\
             } else if (op == 1 || op == 2) {\n\
               let dst = addr(mem, ip + 3, m3, rb)\n\
               ensure(mem, dst)\n\
               let _m = mem.set(dst, if (op == 1) { param(mem, ip + 1, m1, rb) + param(mem, ip + 2, m2, rb) } else { param(mem, ip + 1, m1, rb) * param(mem, ip + 2, m2, rb) })\n\
               ip = ip + 4\n\
             } else if (op == 3) {\n\
               let dst = addr(mem, ip + 1, m1, rb)\n\
               ensure(mem, dst)\n\
               let value = if (input_pos == 0) { x } else { 0 }\n\
               let _m = mem.set(dst, value)\n\
               input_pos = input_pos + 1\n\
               ip = ip + 2\n\
             } else if (op == 4) {\n\
               out = param(mem, ip + 1, m1, rb)\n\
               ip = ip + 2\n\
             } else if (op == 5) {\n\
               if (param(mem, ip + 1, m1, rb) != 0) {\n\
                 ip = param(mem, ip + 2, m2, rb)\n\
               } else {\n\
                 ip = ip + 3\n\
               }\n\
             } else if (op == 6) {\n\
               if (param(mem, ip + 1, m1, rb) == 0) {\n\
                 ip = param(mem, ip + 2, m2, rb)\n\
               } else {\n\
                 ip = ip + 3\n\
               }\n\
             } else if (op == 7 || op == 8) {\n\
               let dst = addr(mem, ip + 3, m3, rb)\n\
               ensure(mem, dst)\n\
               let cond = if (op == 7) { param(mem, ip + 1, m1, rb) < param(mem, ip + 2, m2, rb) } else { param(mem, ip + 1, m1, rb) == param(mem, ip + 2, m2, rb) }\n\
               let _m = mem.set(dst, if (cond) { 1 } else { 0 })\n\
               ip = ip + 4\n\
             } else if (op == 9) {\n\
               rb = rb + param(mem, ip + 1, m1, rb)\n\
               ip = ip + 2\n\
             } else {\n\
               return out\n\
             }\n\
           }\n\
           out\n\
         }\n\
         fn main() -> Int {\n\
           run(MutableList.new().push(3).push(0).push(4).push(0).push(99).to_list(), 7)\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "7",
        "run --backend wasm executes tiny intcode io loop",
    );
}

#[test]
fn run_backend_wasm_preserves_loop_fallthrough_after_long_else_if_chain() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "fn main() -> Int {\n\
         var ip = 0\n\
         var input_pos = 0\n\
         var steps = 0\n\
         while (steps < 2) {\n\
           let op = if (steps == 0) { 3 } else { 99 }\n\
           if (op == 99) {\n\
             return 7 + ip + input_pos\n\
           } else if (op == 1 || op == 2) {\n\
             ip = 100\n\
           } else if (op == 3) {\n\
             let value = if (input_pos == 0) { 7 } else { 0 }\n\
             input_pos = input_pos + value\n\
             ip = ip + 2\n\
           } else if (op == 4) {\n\
             ip = 40\n\
           } else if (op == 5) {\n\
             ip = 50\n\
           } else if (op == 6) {\n\
             ip = 60\n\
           } else if (op == 7 || op == 8) {\n\
             ip = 70\n\
           } else if (op == 9) {\n\
             ip = 90\n\
           } else {\n\
             return 111\n\
           }\n\
           steps = steps + 1\n\
         }\n\
         222\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "16",
        "run --backend wasm preserves loop fallthrough after long else-if chain",
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
fn run_backend_wasm_preserves_mutable_list_char_pop_payloads() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         from Option import Some, None\n\
         from io import println\n\
         fn show_one(xs: MutableList<Char>) -> Unit {\n\
           match (xs.pop()) {\n\
             Some(ch) => println(ch.code().to_string())\n\
             None => println(\"none\")\n\
           }\n\
         }\n\
         fn main() -> Unit {\n\
           let stack = MutableList.new()\n\
           let _a = stack.push('{')\n\
           let _b = stack.push('[')\n\
           let _c = stack.push('(')\n\
           let _d = stack.push('<')\n\
           show_one(stack)\n\
           show_one(stack)\n\
           show_one(stack)\n\
           show_one(stack)\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "60\n40\n91\n123",
        "run --backend wasm with MutableList<Char>.pop() payloads",
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
fn run_backend_wasm_preserves_repeated_chars_to_list_get_over_large_strings() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    let input = dir.path().join("input.txt");
    let contents = "12 ".repeat(12_000);
    let expected: i64 = contents.chars().map(|ch| i64::from(u32::from(ch))).sum();
    fs::write(&input, &contents).expect("write input");
    fs::write(
        &file,
        "from fs import read_file\n\
         fn main() -> Int {\n\
           let input = read_file(\"input.txt\")\n\
           var i = 0\n\
           var total = 0\n\
           while (i < input.len()) {\n\
             total = total + input.chars().to_list().get(i).unwrap_or(' ').code()\n\
             i = i + 1\n\
           }\n\
           total\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        &expected.to_string(),
        "run --backend wasm with repeated chars().to_list().get(i) over a large string",
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
fn run_backend_wasm_preserves_split_filter_to_list_with_string_inequality() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "fn main() -> Int {\n\
           \"a b  c\".split(\" \").filter(fn(part: String) => part != \"\").to_list().len()\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "3",
        "run --backend wasm with split(...).filter(part != \"\").to_list()",
    );
}

#[test]
fn run_backend_wasm_preserves_chars_frequencies() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from io import println\n\
         fn main() -> Unit {\n\
           let counts = \"32T3K\".chars().frequencies()\n\
           println(counts.len().to_string())\n\
           println(counts.get('3').unwrap_or(0).to_string())\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "4\n2",
        "run --backend wasm with chars().frequencies()",
    );
}

#[test]
fn run_backend_wasm_preserves_sorted_string_lists() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         from io import println\n\
         fn main() -> Unit {\n\
           let names = MutableList.new().push(\"z19\").push(\"fgn\").push(\"dck\").push(\"z37\").push(\"qdg\").push(\"nvh\").push(\"vvf\").push(\"z12\").to_list().sorted()\n\
           println(names.len().to_string())\n\
           println(names.get(0).unwrap_or(\"?\"))\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "8\ndck",
        "run --backend wasm with List<String>.sorted()",
    );
}

#[test]
fn run_backend_wasm_preserves_mutable_priority_queue_pop_record_payloads() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutablePriorityQueue\n\
         from io import println\n\
         from Option import Some, None\n\
         fn main() -> Unit {\n\
           let pq: MutablePriorityQueue<Int, Int> = MutablePriorityQueue.new_min()\n\
           let _ = pq.push(2, 20)\n\
           match (pq.pop()) {\n\
             Some(item) => {\n\
               println(item.priority.to_string())\n\
               println(item.value.to_string())\n\
             }\n\
             None => println(\"none\")\n\
           }\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "2\n20",
        "run --backend wasm preserves MutablePriorityQueue pop record payloads",
    );
}

#[test]
fn run_backend_wasm_preserves_mutable_priority_queue_pop_order_after_removal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutablePriorityQueue\n\
         from io import println\n\
         from Option import Some, None\n\
         fn main() -> Unit {\n\
           let pq: MutablePriorityQueue<Int, Int> = MutablePriorityQueue.new_min()\n\
           let _ = pq.push(0, 0)\n\
           let _ = pq.push(1, 1)\n\
           let _ = pq.push(2, 2)\n\
           match (pq.pop()) {\n\
             Some(item) => println(item.value.to_string())\n\
             None => println(\"none\")\n\
           }\n\
           match (pq.pop()) {\n\
             Some(item) => println(item.value.to_string())\n\
             None => println(\"none\")\n\
           }\n\
           println(pq.len().to_string())\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "0\n1\n1",
        "run --backend wasm preserves MutablePriorityQueue pop order after removal",
    );
}

#[test]
fn run_backend_wasm_preserves_large_mutable_priority_queue_pushes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutablePriorityQueue\n\
         from io import println\n\
         from Option import Some, None\n\
         fn main() -> Unit {\n\
           let pq: MutablePriorityQueue<Int, Int> = MutablePriorityQueue.new_min()\n\
           var i = 0\n\
           while (i < 50000) {\n\
             let _ = pq.push(i, i)\n\
             i = i + 1\n\
           }\n\
           match (pq.pop()) {\n\
             Some(item) => println(item.value.to_string())\n\
             None => println(\"none\")\n\
           }\n\
           match (pq.pop()) {\n\
             Some(item) => println(item.value.to_string())\n\
             None => println(\"none\")\n\
           }\n\
           println(pq.len().to_string())\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "0\n1\n49998",
        "run --backend wasm with large MutablePriorityQueue push/pop workload",
    );
}

#[test]
fn run_backend_wasm_preserves_mutable_priority_queue_max_heap_pop_order_after_prior_min_heap_pop() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutablePriorityQueue\n\
         from io import println\n\
         from Option import Some, None\n\
         fn main() -> Unit {\n\
           let minq: MutablePriorityQueue<Int, Int> = MutablePriorityQueue.new_min()\n\
           let _ = minq.push(5, 50).push(1, 10).push(1, 11).push(3, 30)\n\
           let _ = minq.pop()\n\
           let maxq: MutablePriorityQueue<Int, Int> = MutablePriorityQueue.new_max()\n\
           let _ = maxq.push(5, 50).push(7, 70).push(7, 71).push(1, 10)\n\
           match (maxq.pop()) {\n\
             Some(item) => println((item.priority * 100 + item.value).to_string())\n\
             None => println(\"none\")\n\
           }\n\
           match (maxq.pop()) {\n\
             Some(item) => println((item.priority * 100 + item.value).to_string())\n\
             None => println(\"none\")\n\
           }\n\
           match (maxq.pop()) {\n\
             Some(item) => println((item.priority * 100 + item.value).to_string())\n\
             None => println(\"none\")\n\
           }\n\
           match (maxq.pop()) {\n\
             Some(item) => println((item.priority * 100 + item.value).to_string())\n\
             None => println(\"none\")\n\
           }\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "770\n771\n550\n110",
        "run --backend wasm preserves max-heap pop order after prior min-heap pop",
    );
}

#[test]
fn run_backend_wasm_preserves_mutable_priority_queue_min_heap_order_with_list_payloads() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import List, MutableList, MutablePriorityQueue\n\
         from io import println\n\
         from Option import Some, None\n\
         fn build_payload(seed: Int) -> List<Int> {\n\
           let xs = MutableList.new()\n\
           for (i in 0 ..< 15) {\n\
             let _ = xs.push(seed * 100 + i)\n\
           }\n\
           xs.to_list()\n\
         }\n\
         fn main() -> Unit {\n\
           let pq: MutablePriorityQueue<Int, List<Int>> = MutablePriorityQueue.new_min()\n\
           let _ = pq.push(900, build_payload(9))\n\
           let _ = pq.push(20, build_payload(1))\n\
           let _ = pq.push(20, build_payload(2))\n\
           let _ = pq.push(30, build_payload(3))\n\
           let _ = pq.push(40, build_payload(4))\n\
           let _ = pq.push(50, build_payload(5))\n\
           let _ = pq.push(60, build_payload(6))\n\
           let _ = pq.push(60, build_payload(7))\n\
           let _ = pq.push(60, build_payload(8))\n\
           let _ = pq.push(70, build_payload(10))\n\
           let _ = pq.push(70, build_payload(11))\n\
           let _ = pq.push(80, build_payload(12))\n\
           let _ = pq.push(80, build_payload(13))\n\
           let _ = pq.push(90, build_payload(14))\n\
           let _ = pq.push(90, build_payload(15))\n\
           let _ = pq.push(90, build_payload(16))\n\
           while (pq.is_empty() == false) {\n\
             match (pq.pop()) {\n\
               Some(item) => println(item.priority.to_string().concat(\":\").concat(item.value[0].to_string())),\n\
               None => println(\"none\"),\n\
             }\n\
           }\n\
         }\n",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "20:100\n20:200\n30:300\n40:400\n50:500\n60:600\n60:700\n60:800\n70:1000\n70:1100\n80:1200\n80:1300\n90:1400\n90:1500\n90:1600\n900:900",
        "run --backend wasm preserves min-heap order with list payloads",
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
fn run_backend_wasm_handles_loop_match_with_nested_if() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from Option import Some, None\n\
         fn main() -> Int {\n\
           let x = Some(7)\n\
           while (true) {\n\
             match (x) {\n\
               Some(n) => {\n\
                 let cond = true\n\
                 if (cond) {\n\
                   return 1\n\
                 }\n\
                 return n\n\
               }\n\
               None => {}\n\
             }\n\
           }\n\
           -1\n\
         }",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "1",
        "run --backend wasm handles looped match with nested if",
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
fn run_backend_wasm_executes_day21_grid17_constant_repl_shape() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    let input = dir.path().join("day21.txt");
    fs::write(
        &file,
        include_str!("fixtures/day21_grid17_constant_repl.ky"),
    )
    .expect("write source");
    fs::copy(
        "/Users/alpha/CodexProjects/polyglot-bench/corpus/advent-of-code/2017/day21.txt",
        &input,
    )
    .expect("copy day21 input");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "2187",
        "run --backend wasm with reduced AoC 2017 day21 grid17 constant repl shape",
    );
}

#[test]
fn run_backend_wasm_executes_aoc_2020_day22() {
    let dir = tempfile::tempdir().expect("tempdir");
    let input = dir.path().join("day22.txt");
    fs::copy(
        "/Users/alpha/CodexProjects/polyglot-bench/corpus/advent-of-code/2020/day22.txt",
        &input,
    )
    .expect("copy day22 input");

    let output = run_cli(
        dir.path(),
        &[
            "run",
            "/Users/alpha/CodexProjects/polyglot-bench/adapters/kyokara/solutions/advent-of-code/2020/day22.ky",
            "--backend",
            "wasm",
        ],
    );
    assert_stdout_trimmed(
        &output,
        "Part 1: 33403\nPart 2: 29177",
        "run --backend wasm AoC 2020 day22",
    );
}

#[test]
fn run_backend_wasm_executes_aoc_2022_day13() {
    let dir = tempfile::tempdir().expect("tempdir");
    let input = dir.path().join("day13.txt");
    fs::copy(
        "/Users/alpha/CodexProjects/polyglot-bench/corpus/advent-of-code/2022/day13.txt",
        &input,
    )
    .expect("copy day13 input");

    let output = run_cli(
        dir.path(),
        &[
            "run",
            "/Users/alpha/CodexProjects/polyglot-bench/adapters/kyokara/solutions/advent-of-code/2022/day13.ky",
            "--backend",
            "wasm",
        ],
    );
    assert_stdout_trimmed(
        &output,
        "Part 1: 5393\nPart 2: 26712",
        "run --backend wasm AoC 2022 day13",
    );
}

#[test]
fn run_backend_wasm_executes_aoc_2017_day25() {
    let dir = tempfile::tempdir().expect("tempdir");
    let input = dir.path().join("day25.txt");
    fs::copy(
        "/Users/alpha/CodexProjects/polyglot-bench/corpus/advent-of-code/2017/day25.txt",
        &input,
    )
    .expect("copy day25 input");

    let output = run_cli(
        dir.path(),
        &[
            "run",
            "/Users/alpha/CodexProjects/polyglot-bench/adapters/kyokara/solutions/advent-of-code/2017/day25.ky",
            "--backend",
            "wasm",
        ],
    );
    assert_stdout_trimmed(
        &output,
        "Part 1: 4385\nPart 2: (no part 2)",
        "run --backend wasm AoC 2017 day25",
    );
}

#[test]
fn run_backend_wasm_executes_aoc_2018_day04() {
    let dir = tempfile::tempdir().expect("tempdir");
    let input = dir.path().join("day04.txt");
    fs::copy(
        "/Users/alpha/CodexProjects/polyglot-bench/corpus/advent-of-code/2018/day04.txt",
        &input,
    )
    .expect("copy day04 input");

    let output = run_cli(
        dir.path(),
        &[
            "run",
            "/Users/alpha/CodexProjects/polyglot-bench/adapters/kyokara/solutions/advent-of-code/2018/day04.ky",
            "--backend",
            "wasm",
        ],
    );
    assert_stdout_trimmed(
        &output,
        "Part 1: 19830\nPart 2: 43695",
        "run --backend wasm AoC 2018 day04",
    );
}

#[test]
fn run_backend_wasm_executes_aoc_2018_day05() {
    let dir = tempfile::tempdir().expect("tempdir");
    let input = dir.path().join("day05.txt");
    fs::copy(
        "/Users/alpha/CodexProjects/polyglot-bench/corpus/advent-of-code/2018/day05.txt",
        &input,
    )
    .expect("copy day05 input");

    let output = run_cli(
        dir.path(),
        &[
            "run",
            "/Users/alpha/CodexProjects/polyglot-bench/adapters/kyokara/solutions/advent-of-code/2018/day05.ky",
            "--backend",
            "wasm",
        ],
    );
    assert_stdout_trimmed(
        &output,
        "Part 1: 9808\nPart 2: 6484",
        "run --backend wasm AoC 2018 day05",
    );
}

#[test]
fn run_backend_wasm_executes_aoc_2018_day24() {
    let dir = tempfile::tempdir().expect("tempdir");
    let input = dir.path().join("day24.txt");
    fs::copy(
        "/Users/alpha/CodexProjects/polyglot-bench/corpus/advent-of-code/2018/day24.txt",
        &input,
    )
    .expect("copy day24 input");

    let output = run_cli(
        dir.path(),
        &[
            "run",
            "/Users/alpha/CodexProjects/polyglot-bench/adapters/kyokara/solutions/advent-of-code/2018/day24.ky",
            "--backend",
            "wasm",
        ],
    );
    assert_stdout_trimmed(
        &output,
        "Part 1: 24009\nPart 2: 379",
        "run --backend wasm AoC 2018 day24",
    );
}

#[test]
fn run_backend_wasm_executes_aoc_2019_day11() {
    let dir = tempfile::tempdir().expect("tempdir");
    let input = dir.path().join("day11.txt");
    fs::copy(
        "/Users/alpha/CodexProjects/polyglot-bench/corpus/advent-of-code/2019/day11.txt",
        &input,
    )
    .expect("copy day11 input");

    let output = run_cli(
        dir.path(),
        &[
            "run",
            "/Users/alpha/CodexProjects/polyglot-bench/adapters/kyokara/solutions/advent-of-code/2019/day11.ky",
            "--backend",
            "wasm",
        ],
    );
    assert_stdout_trimmed(
        &output,
        "Part 1: 2184\nPart 2: AHCHZEPK",
        "run --backend wasm AoC 2019 day11",
    );
}

#[test]
fn run_backend_wasm_executes_aoc_2019_day17() {
    let dir = tempfile::tempdir().expect("tempdir");
    let input = dir.path().join("day17.txt");
    fs::copy(
        "/Users/alpha/CodexProjects/polyglot-bench/corpus/advent-of-code/2019/day17.txt",
        &input,
    )
    .expect("copy day17 input");

    let output = run_cli(
        dir.path(),
        &[
            "run",
            "/Users/alpha/CodexProjects/polyglot-bench/adapters/kyokara/solutions/advent-of-code/2019/day17.ky",
            "--backend",
            "wasm",
        ],
    );
    assert_stdout_trimmed(
        &output,
        "Part 1: 5972\nPart 2: 933214",
        "run --backend wasm AoC 2019 day17",
    );
}

#[test]
fn run_backend_wasm_executes_aoc_2023_day14() {
    let dir = tempfile::tempdir().expect("tempdir");
    let input = dir.path().join("day14.txt");
    fs::copy(
        "/Users/alpha/CodexProjects/polyglot-bench/corpus/advent-of-code/2023/day14.txt",
        &input,
    )
    .expect("copy day14 input");

    let output = run_cli(
        dir.path(),
        &[
            "run",
            "/Users/alpha/CodexProjects/polyglot-bench/adapters/kyokara/solutions/advent-of-code/2023/day14.ky",
            "--backend",
            "wasm",
        ],
    );
    assert_stdout_trimmed(
        &output,
        "Part 1: 108840\nPart 2: 103445",
        "run --backend wasm AoC 2023 day14",
    );
}

#[test]
fn run_backend_wasm_decodes_aoc_2019_day11_letter_rows() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import List, MutableList\n\
         fn decode_pattern(pattern: String) -> String {\n\
           if (pattern == \".##..#..#.#..#.####.#..#.#..#.\") {\n\
             \"A\"\n\
           } else if (pattern == \".##..#..#.#....#....#..#..##..\") {\n\
             \"C\"\n\
           } else if (pattern == \"#..#.#..#.####.#..#.#..#.#..#.\") {\n\
             \"H\"\n\
           } else if (pattern == \"#..#.#.#..##...#.#..#.#..#..#.\") {\n\
             \"K\"\n\
           } else if (pattern == \"###..#..#.#..#.###..#....#....\") {\n\
             \"P\"\n\
           } else if (pattern == \"####.#....###..#....#....####.\") {\n\
             \"E\"\n\
           } else if (pattern == \"####....#...#...#...#....####.\") {\n\
             \"Z\"\n\
           } else {\n\
             \"?\"\n\
           }\n\
         }\n\
         fn pattern_for(rows: List<String>, start_x: Int, width: Int) -> String {\n\
           let out = MutableList.new().push(\"\")\n\
           for (row in rows) {\n\
             for (x in start_x ..< (start_x + width)) {\n\
               let pixel = row.substring(x, x + 1)\n\
               let _o = out.set(0, out[0].concat(if (pixel == \"#\") { \"#\" } else { \".\" }))\n\
             }\n\
           }\n\
           out[0]\n\
         }\n\
         fn pattern_norm(rows: List<String>, start_x: Int, width: Int) -> String {\n\
           if (width == 5) {\n\
             pattern_for(rows, start_x, 5)\n\
           } else if (width == 4) {\n\
             let out = MutableList.new().push(\"\")\n\
             for (row in rows) {\n\
               for (x in start_x ..< (start_x + 4)) {\n\
                 let pixel = row.substring(x, x + 1)\n\
                 let _o = out.set(0, out[0].concat(if (pixel == \"#\") { \"#\" } else { \".\" }))\n\
               }\n\
               let _o = out.set(0, out[0].concat(\".\"))\n\
             }\n\
             out[0]\n\
           } else {\n\
             pattern_for(rows, start_x, width)\n\
           }\n\
         }\n\
         fn col_blank(rows: List<String>, x: Int) -> Bool {\n\
           rows.all(fn(row: String) => row.substring(x, x + 1) != \"#\")\n\
         }\n\
         fn decode_rows(rows: List<String>) -> String {\n\
           let width = rows[0].len()\n\
           if (width % 5 == 0) {\n\
             var out = \"\"\n\
             for (letter in 0 ..< (width / 5)) {\n\
               out = out.concat(decode_pattern(pattern_norm(rows, letter * 5, 5)))\n\
             }\n\
             return out\n\
           }\n\
           var x = 0\n\
           var out = \"\"\n\
           while (x < width) {\n\
             while (x < width && col_blank(rows, x)) {\n\
               x = x + 1\n\
             }\n\
             if (x >= width) {\n\
               break\n\
             }\n\
             var end = x\n\
             while (end < width && !col_blank(rows, end)) {\n\
               end = end + 1\n\
             }\n\
             out = out.concat(decode_pattern(pattern_norm(rows, x, end - x)))\n\
             x = end\n\
           }\n\
           out\n\
         }\n\
         fn main() -> String {\n\
           let rows = MutableList.new()\n\
             .push(\".##..#..#..##..#..#.####.####.###..#..#\")\n\
             .push(\"#..#.#..#.#..#.#..#....#.#....#..#.#.#.\")\n\
             .push(\"#..#.####.#....####...#..###..#..#.##..\")\n\
             .push(\"####.#..#.#....#..#..#...#....###..#.#.\")\n\
             .push(\"#..#.#..#.#..#.#..#.#....#....#....#.#.\")\n\
             .push(\"#..#.#..#..##..#..#.####.####.#....#..#\")\n\
             .to_list()\n\
           decode_rows(rows)\n\
         }\n",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "AHCHZEPK",
        "run --backend wasm decodes AoC 2019 day11 letter rows",
    );
}

#[test]
fn run_backend_wasm_preserves_aoc_2019_day11_row_len_and_substring() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "fn main() -> String {\n\
           let row = \".##..#..#..##..#..#.####.###..####.#..#\"\n\
           row.len().to_string()\n\
             .concat(\"\\n\")\n\
             .concat(row.substring(0, 5))\n\
             .concat(\"\\n\")\n\
             .concat(row.substring(5, 10))\n\
             .concat(\"\\n\")\n\
             .concat(row.substring(35, 40))\n\
         }\n",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "39\n.##..\n#..#.\n#..#",
        "run --backend wasm preserves AoC 2019 day11 row len and substring",
    );
}

#[test]
fn run_backend_wasm_preserves_aoc_2019_day11_pattern_norm() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import List, MutableList\n\
         fn pattern_for(rows: List<String>, start_x: Int, width: Int) -> String {\n\
           let out = MutableList.new().push(\"\")\n\
           for (row in rows) {\n\
             for (x in start_x ..< (start_x + width)) {\n\
               let pixel = row.substring(x, x + 1)\n\
               let _o = out.set(0, out[0].concat(if (pixel == \"#\") { \"#\" } else { \".\" }))\n\
             }\n\
           }\n\
           out[0]\n\
         }\n\
         fn pattern_norm(rows: List<String>, start_x: Int, width: Int) -> String {\n\
           if (width == 5) {\n\
             pattern_for(rows, start_x, 5)\n\
           } else if (width == 4) {\n\
             let out = MutableList.new().push(\"\")\n\
             for (row in rows) {\n\
               for (x in start_x ..< (start_x + 4)) {\n\
                 let pixel = row.substring(x, x + 1)\n\
                 let _o = out.set(0, out[0].concat(if (pixel == \"#\") { \"#\" } else { \".\" }))\n\
               }\n\
               let _o = out.set(0, out[0].concat(\".\"))\n\
             }\n\
             out[0]\n\
           } else {\n\
             pattern_for(rows, start_x, width)\n\
           }\n\
         }\n\
         fn main() -> String {\n\
           let rows = MutableList.new()\n\
             .push(\".##..#..#..##..#..#.####.####.###..#..#\")\n\
             .push(\"#..#.#..#.#..#.#..#....#.#....#..#.#.#.\")\n\
             .push(\"#..#.####.#....####...#..###..#..#.##..\")\n\
             .push(\"####.#..#.#....#..#..#...#....###..#.#.\")\n\
             .push(\"#..#.#..#.#..#.#..#.#....#....#....#.#.\")\n\
             .push(\"#..#.#..#..##..#..#.####.####.#....#..#\")\n\
             .to_list()\n\
           pattern_norm(rows, 0, 4)\n\
             .concat(\"\\n\")\n\
             .concat(pattern_norm(rows, 5, 4))\n\
         }\n",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        ".##..#..#.#..#.####.#..#.#..#.\n#..#.#..#.####.#..#.#..#.#..#.",
        "run --backend wasm preserves AoC 2019 day11 pattern_norm",
    );
}

#[test]
fn run_backend_wasm_preserves_mutable_list_string_self_concat_set() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         fn main() -> String {\n\
           let out = MutableList.new().push(\"\")\n\
           let _ = out.set(0, out[0].concat(\"A\"))\n\
           let _ = out.set(0, out[0].concat(\"B\"))\n\
           out[0]\n\
         }\n",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "AB",
        "run --backend wasm preserves MutableList<String> self-concat set",
    );
}

#[test]
fn run_backend_wasm_preserves_concat_of_string_loaded_from_mutable_list() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         fn main() -> String {\n\
           let out = MutableList.new().push(\"A\")\n\
           out[0].concat(\"B\")\n\
         }\n",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "AB",
        "run --backend wasm preserves concat of string loaded from MutableList",
    );
}

#[test]
fn run_backend_wasm_preserves_concat_of_special_string_returned_from_call() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         fn build() -> String {\n\
           let out = MutableList.new().push(\"\")\n\
           let _ = out.set(0, out[0].concat(\"A\"))\n\
           let _ = out.set(0, out[0].concat(\"B\"))\n\
           out[0]\n\
         }\n\
         fn main() -> String {\n\
           build().concat(\"C\")\n\
         }\n",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "ABC",
        "run --backend wasm preserves concat of special string returned from call",
    );
}

#[test]
fn run_backend_wasm_preserves_string_loaded_from_mutable_list() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         fn main() -> String {\n\
           let out = MutableList.new().push(\"A\")\n\
           out[0]\n\
         }\n",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "A",
        "run --backend wasm preserves string loaded from MutableList",
    );
}

#[test]
fn run_backend_wasm_parses_int_from_special_string_returned_from_call() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         fn build() -> String {\n\
           let digits = MutableList.new().push(\"\")\n\
           let _ = digits.set(0, digits[0].concat(\"1\"))\n\
           let _ = digits.set(0, digits[0].concat(\"0\"))\n\
           digits[0]\n\
         }\n\
         fn main() -> String {\n\
           build().parse_int().unwrap_or(0).to_string()\n\
         }\n",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "10",
        "run --backend wasm parses int from special string returned from call",
    );
}

#[test]
fn run_backend_wasm_parses_int_from_substring_built_special_string() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         fn build() -> String {\n\
           let src = \" 12 \"\n\
           let digits = MutableList.new().push(\"\")\n\
           var i = 0\n\
           while (i < src.len()) {\n\
             let _ = digits.set(0, digits[0].concat(src.substring(i, i + 1)))\n\
             i = i + 1\n\
           }\n\
           digits[0]\n\
         }\n\
         fn main() -> String {\n\
           build().trim().parse_int().unwrap_or(0).to_string()\n\
         }\n",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "12",
        "run --backend wasm parses int from substring-built special string",
    );
}

#[test]
fn run_backend_wasm_trims_substring_built_special_string() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         fn build() -> String {\n\
           let src = \" 12 \"\n\
           let digits = MutableList.new().push(\"\")\n\
           var i = 0\n\
           while (i < src.len()) {\n\
             let _ = digits.set(0, digits[0].concat(src.substring(i, i + 1)))\n\
             i = i + 1\n\
           }\n\
           digits[0]\n\
         }\n\
         fn main() -> String {\n\
           build().trim()\n\
         }\n",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "12",
        "run --backend wasm trims substring-built special string",
    );
}

#[test]
fn run_backend_wasm_parses_single_digit_substrings_inside_loop() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         fn main() -> Int {\n\
           let text = \"10110\"\n\
           let value = MutableList.new().push(0)\n\
           for (i in 0 ..< text.len()) {\n\
             let _ = value.set(0, value[0] * 2 + text.substring(i, i + 1).parse_int().unwrap_or(0))\n\
           }\n\
           value[0]\n\
         }\n",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "22",
        "run --backend wasm parses single-digit substrings inside loop",
    );
}

#[test]
fn run_backend_wasm_prefers_user_defined_fn_over_intrinsic_name() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         fn char_code(ch: String) -> Int {\n\
           if (ch == \"A\") {\n\
             65\n\
           } else if (ch == \",\") {\n\
             44\n\
           } else {\n\
             32\n\
           }\n\
         }\n\
         fn main() -> String {\n\
           let out = MutableList.new().push(\"\")\n\
           let _ = out.set(0, out[0].concat(char_code(\"A\").to_string()))\n\
           let _ = out.set(0, out[0].concat(\",\"))\n\
           let _ = out.set(0, out[0].concat(char_code(\",\").to_string()))\n\
           out[0]\n\
         }\n",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "65,44",
        "run --backend wasm prefers user-defined fn over intrinsic name",
    );
}

#[test]
fn run_backend_wasm_parses_int_from_chars_built_special_string() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         fn build() -> String {\n\
           let src = \" 12 \"\n\
           let digits = MutableList.new().push(\"\")\n\
           for (ch in src.chars()) {\n\
             let _ = digits.set(0, digits[0].concat(ch.to_string()))\n\
           }\n\
           digits[0]\n\
         }\n\
         fn main() -> String {\n\
           build().trim().parse_int().unwrap_or(0).to_string()\n\
         }\n",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "12",
        "run --backend wasm parses int from chars-built special string",
    );
}

#[test]
fn run_backend_wasm_slices_concat_built_special_string() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    fs::write(
        &file,
        "from collections import MutableList\n\
         fn build() -> String {\n\
           let src = \"10110\"\n\
           let digits = MutableList.new().push(\"\")\n\
           var i = 0\n\
           while (i < src.len()) {\n\
             let _ = digits.set(0, digits[0].concat(src.substring(i, i + 1)))\n\
             i = i + 1\n\
           }\n\
           digits[0]\n\
         }\n\
         fn main() -> String {\n\
           build().substring(1, 4)\n\
         }\n",
    )
    .expect("write source");

    let output = run_cli(dir.path(), &["run", "main.ky", "--backend", "wasm"]);
    assert_stdout_trimmed(
        &output,
        "011",
        "run --backend wasm slices concat-built special string",
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
