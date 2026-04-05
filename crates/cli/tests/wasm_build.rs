#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use wasmtime::{Engine, Module};

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

#[test]
fn build_target_wasm_writes_single_file_module() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    let out = dir.path().join("dist").join("app.wasm");
    fs::write(&file, "fn main() -> Int { 42 }").expect("write source");

    let output = run_cli(
        dir.path(),
        &[
            "build",
            "main.ky",
            "--target",
            "wasm",
            "--out",
            out.to_str().expect("utf-8 output path"),
        ],
    );
    assert_success(&output, "build --target wasm");

    let bytes = fs::read(&out).expect("read wasm artifact");
    assert!(
        bytes.starts_with(b"\0asm"),
        "built file should be a wasm module\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn build_target_wasm_supports_project_mode() {
    let dir = tempfile::tempdir().expect("tempdir");
    let app = dir.path().join("main.ky");
    let helper = dir.path().join("helper.ky");
    let out = dir.path().join("out.wasm");
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
            out.to_str().expect("utf-8 output path"),
        ],
    );
    assert_success(&output, "build --project --target wasm");

    let bytes = fs::read(&out).expect("read wasm artifact");
    assert!(
        bytes.starts_with(b"\0asm"),
        "project-mode build should produce a wasm module\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn build_target_wasm_validates_nested_boundary_checks_in_large_parse_function() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    let out = dir.path().join("out.wasm");
    fs::write(&file, include_str!("fixtures/day20_h_id_outer.ky")).expect("write source");

    let output = run_cli(
        dir.path(),
        &[
            "build",
            "main.ky",
            "--target",
            "wasm",
            "--out",
            out.to_str().expect("utf-8 output path"),
        ],
    );
    assert_success(&output, "build --target wasm nested boundary checks");

    let bytes = fs::read(&out).expect("read wasm artifact");
    Module::new(&Engine::default(), &bytes).unwrap_or_else(|err| {
        panic!(
            "built module should validate under wasmtime: {err}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
}

#[test]
fn build_target_wasm_validates_aoc_2018_day24_module() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    let out = dir.path().join("out.wasm");
    fs::copy(
        "/Users/alpha/CodexProjects/polyglot-bench/adapters/kyokara/solutions/advent-of-code/2018/day24.ky",
        &file,
    )
    .expect("copy source");

    let output = run_cli(
        dir.path(),
        &[
            "build",
            "main.ky",
            "--target",
            "wasm",
            "--out",
            out.to_str().expect("utf-8 output path"),
        ],
    );
    assert_success(&output, "build --target wasm AoC 2018 day24");

    let bytes = fs::read(&out).expect("read wasm artifact");
    Module::new(&Engine::default(), &bytes).unwrap_or_else(|err| {
        panic!(
            "AoC 2018 day24 module should validate under wasmtime: {err}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
}

#[test]
fn build_target_wasm_validates_aoc_2021_day23_module() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    let out = dir.path().join("out.wasm");
    fs::copy(
        "/Users/alpha/CodexProjects/polyglot-bench/adapters/kyokara/solutions/advent-of-code/2021/day23.ky",
        &file,
    )
    .expect("copy source");

    let output = run_cli(
        dir.path(),
        &[
            "build",
            "main.ky",
            "--target",
            "wasm",
            "--out",
            out.to_str().expect("utf-8 output path"),
        ],
    );
    assert_success(&output, "build --target wasm AoC 2021 day23");

    let bytes = fs::read(&out).expect("read wasm artifact");
    Module::new(&Engine::default(), &bytes).unwrap_or_else(|err| {
        panic!(
            "AoC 2021 day23 module should validate under wasmtime: {err}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
}

#[test]
fn build_target_wasm_validates_aoc_2022_day13_module() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    let out = dir.path().join("out.wasm");
    fs::copy(
        "/Users/alpha/CodexProjects/polyglot-bench/adapters/kyokara/solutions/advent-of-code/2022/day13.ky",
        &file,
    )
    .expect("copy source");

    let output = run_cli(
        dir.path(),
        &[
            "build",
            "main.ky",
            "--target",
            "wasm",
            "--out",
            out.to_str().expect("utf-8 output path"),
        ],
    );
    assert_success(&output, "build --target wasm AoC 2022 day13");

    let bytes = fs::read(&out).expect("read wasm artifact");
    Module::new(&Engine::default(), &bytes).unwrap_or_else(|err| {
        panic!(
            "AoC 2022 day13 module should validate under wasmtime: {err}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
}

#[test]
fn build_target_wasm_validates_aoc_2025_day08_module() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    let out = dir.path().join("out.wasm");
    fs::copy(
        "/Users/alpha/CodexProjects/polyglot-bench/adapters/kyokara/solutions/advent-of-code/2025/day08.ky",
        &file,
    )
    .expect("copy source");

    let output = run_cli(
        dir.path(),
        &[
            "build",
            "main.ky",
            "--target",
            "wasm",
            "--out",
            out.to_str().expect("utf-8 output path"),
        ],
    );
    assert_success(&output, "build --target wasm AoC 2025 day08");

    let bytes = fs::read(&out).expect("read wasm artifact");
    Module::new(&Engine::default(), &bytes).unwrap_or_else(|err| {
        panic!(
            "AoC 2025 day08 module should validate under wasmtime: {err}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
}

#[test]
fn build_target_wasm_validates_loop_match_string_fallthrough() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    let out = dir.path().join("out.wasm");
    fs::write(
        &file,
        "from io import println\n\
         from Option import Some, None\n\
         fn main() -> Unit {\n\
           var i = 0\n\
           while (i < 1) {\n\
             let key = \"abc\"\n\
             let found: Option<Int> = None\n\
             match (found) {\n\
               Some(prev) => { println(prev.to_string()); return },\n\
               None => { println(key) },\n\
             }\n\
             i = i + 1\n\
           }\n\
         }\n",
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
            out.to_str().expect("utf-8 output path"),
        ],
    );
    assert_success(&output, "build --target wasm loop match string fallthrough");

    let bytes = fs::read(&out).expect("read wasm artifact");
    Module::new(&Engine::default(), &bytes).unwrap_or_else(|err| {
        panic!(
            "loop match string fallthrough module should validate under wasmtime: {err}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
}

#[test]
fn build_target_wasm_validates_branch_range_map_to_bool_to_list() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    let out = dir.path().join("out.wasm");
    fs::write(
        &file,
        "fn main() -> Int {\n\
           let n = 3\n\
           if (n <= 1) {\n\
             0\n\
           } else {\n\
             let xs = (0..<n).map(fn(_i: Int) => false).to_list()\n\
             xs.len()\n\
           }\n\
         }\n",
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
            out.to_str().expect("utf-8 output path"),
        ],
    );
    assert_success(&output, "build --target wasm branch range map to bool to_list");

    let bytes = fs::read(&out).expect("read wasm artifact");
    Module::new(&Engine::default(), &bytes).unwrap_or_else(|err| {
        panic!(
            "branch range map to bool to_list module should validate under wasmtime: {err}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
}

#[test]
fn build_target_wasm_validates_nested_loop_exit_stack_cleanup() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("main.ky");
    let out = dir.path().join("out.wasm");
    fs::write(
        &file,
        "from collections import MutableList\n\
         \n\
         fn main() -> Int {\n\
           let units = MutableList.new().push(2).push(3).push(4)\n\
           while (true) {\n\
             let target = MutableList.from_list((0 ..< units.len()).map(fn(_i: Int) => -1).to_list())\n\
             let picked = MutableList.new().push(0).push(0).push(0)\n\
             var i = 0\n\
             while (i < 3) {\n\
               let a = i\n\
               var bt = -1\n\
               var j = 0\n\
               while (j < units.len()) {\n\
                 if (units[j] > 0 && picked[j] == 0) {\n\
                   bt = j\n\
                 }\n\
                 j = j + 1\n\
               }\n\
               if (bt >= 0) {\n\
                 let _t = target.set(a, bt)\n\
                 let _p = picked.set(bt, 1)\n\
               }\n\
               i = i + 1\n\
             }\n\
             var killed_any = 0\n\
             i = 0\n\
             while (i < units.len()) {\n\
               let t = target[i]\n\
               if (t >= 0) {\n\
                 let _u = units.set(t, units[t] - 1)\n\
                 killed_any = killed_any + 1\n\
               }\n\
               i = i + 1\n\
             }\n\
             if (killed_any == 0) { return 1 }\n\
           }\n\
           0\n\
         }\n",
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
            out.to_str().expect("utf-8 output path"),
        ],
    );
    assert_success(&output, "build --target wasm nested loop exit cleanup");

    let bytes = fs::read(&out).expect("read wasm artifact");
    Module::new(&Engine::default(), &bytes).unwrap_or_else(|err| {
        panic!(
            "nested loop exit cleanup module should validate under wasmtime: {err}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
}
