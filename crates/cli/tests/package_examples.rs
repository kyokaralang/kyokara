#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_kyokara"))
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root should canonicalize")
}

fn package_examples_root() -> PathBuf {
    repo_root().join("examples").join("packages")
}

fn copy_example(name: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = package_examples_root().join(name);
    let dst = dir.path().join(name);
    copy_dir_recursive(&src, &dst);
    (dir, dst)
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    if src.is_dir() {
        fs::create_dir_all(dst).expect("create dir");
        for entry in fs::read_dir(src).expect("read dir") {
            let entry = entry.expect("dir entry");
            copy_dir_recursive(&entry.path(), &dst.join(entry.file_name()));
        }
    } else {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).expect("create parent dirs");
        }
        fs::copy(src, dst).expect("copy file");
    }
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
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim(),
        expected,
        "{context} stdout mismatch\nstdout:\n{}\nstderr:\n{}",
        stdout,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_git_repo(repo_dir: &Path) -> String {
    run_git(repo_dir, &["init", "-q", "-b", "main"]);
    run_git(repo_dir, &["config", "user.name", "Kyokara Examples"]);
    run_git(
        repo_dir,
        &["config", "user.email", "examples@kyokara.invalid"],
    );
    run_git(repo_dir, &["add", "."]);
    run_git(repo_dir, &["commit", "-q", "-m", "init"]);
    git_head(repo_dir)
}

fn commit_git_change(repo_dir: &Path, source: &str) -> String {
    fs::write(repo_dir.join("src").join("lib.ky"), source).expect("write git source");
    run_git(repo_dir, &["add", "."]);
    run_git(repo_dir, &["commit", "-q", "-m", "update"]);
    git_head(repo_dir)
}

fn run_git(repo_dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(repo_dir)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_head(repo_dir: &Path) -> String {
    let output = Command::new("git")
        .current_dir(repo_dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("run git rev-parse");
    assert!(output.status.success(), "git rev-parse failed");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

#[test]
fn package_example_registry_selected_closure_runs_with_pinned_transitives() {
    let (_dir, example_root) = copy_example("registry_selected_closure");
    let app_root = example_root.join("app");

    copy_dir_recursive(
        &example_root.join("stale-cache").join("packages"),
        &app_root.join(".kyokara").join("registry").join("packages"),
    );

    let add = run_cli(
        &example_root,
        &[
            "add",
            "app/src/main.ky",
            "core/util",
            "--as",
            "util",
            "--registry",
            "registry",
            "--version",
            "=1.0.0",
        ],
    );
    assert_success(&add, "registry selected-closure add");

    let vendored_util_manifest = fs::read_to_string(
        app_root
            .join(".kyokara")
            .join("registry")
            .join("packages")
            .join("core/util")
            .join("1.0.0")
            .join("kyokara.toml"),
    )
    .expect("read vendored util manifest");
    let vendored_util_manifest: Value = vendored_util_manifest
        .parse::<toml::Value>()
        .expect("vendored manifest should parse")
        .try_into()
        .expect("toml to json");
    let json_dep = vendored_util_manifest
        .get("dependencies")
        .and_then(Value::as_object)
        .and_then(|deps| deps.get("json"))
        .and_then(Value::as_object)
        .expect("vendored json dependency");
    assert_eq!(
        json_dep.get("version").and_then(Value::as_str),
        Some("=1.2.0"),
        "vendored util manifest should pin selected transitive version"
    );

    let run = run_cli(&example_root, &["run", "app/src/main.ky"]);
    assert_stdout_trimmed(&run, "12", "registry selected-closure run");
}

#[test]
fn package_example_git_moving_ref_refreshes_after_update() {
    let (_dir, example_root) = copy_example("git_moving_ref");
    let repo_dir = example_root.join("git-json");
    let app_manifest_path = example_root.join("app").join("kyokara.toml");
    let app_manifest = fs::read_to_string(&app_manifest_path).expect("read app manifest");
    let app_manifest = app_manifest.replace(
        "git = \"../git-json\"",
        &format!("git = \"{}\"", repo_dir.display()),
    );
    fs::write(&app_manifest_path, app_manifest).expect("rewrite app manifest git path");
    let first_commit = init_git_repo(&repo_dir);

    let first_run = run_cli(&example_root, &["run", "app/src/main.ky"]);
    assert_stdout_trimmed(&first_run, "7", "git moving-ref first run");

    let second_commit = commit_git_change(&repo_dir, "pub fn from_git() -> Int { 8 }\n");
    let update = run_cli(&example_root, &["update", "app/src/main.ky"]);
    assert_success(&update, "git moving-ref update");

    let lockfile =
        fs::read_to_string(example_root.join("app").join("kyokara.lock")).expect("read lockfile");
    assert!(
        lockfile.contains("rev = \"main\""),
        "lockfile should preserve requested moving ref, got: {lockfile}"
    );
    assert!(
        lockfile.contains(&format!("commit = \"{second_commit}\"")),
        "lockfile should record refreshed resolved commit, got: {lockfile}"
    );
    assert!(
        !lockfile.contains(&format!("commit = \"{first_commit}\"")),
        "stale commit should not remain in lockfile: {lockfile}"
    );

    let second_run = run_cli(&example_root, &["run", "app/src/main.ky"]);
    assert_stdout_trimmed(&second_run, "8", "git moving-ref second run");
}
