//! `kyokara-cli` — The Kyokara compiler CLI.
//!
//! Commands:
//! - `kyokara check <file>` — type-check a `.ky` file (v0.0)
//! - `kyokara run <file>` — interpret a `.ky` file (v0.1)
//! - `kyokara fmt <file>` — format a `.ky` file (v0.1)
//! - `kyokara refactor <file>` — apply semantic refactors (v0.2)
//! - `kyokara lsp` — start the Language Server Protocol server (v0.2)
//! - `kyokara test <file>` — property-based testing of contract functions (v0.3)
//! - `kyokara replay <file>` — replay execution trace (v0.3)

use std::collections::HashSet;

use clap::{Parser, Subcommand};
use semver::{Version, VersionReq};

#[derive(Parser)]
#[command(name = "kyokara", version, about = "The Kyokara compiler")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Type-check a Kyokara source file.
    Check {
        /// Path to the .ky source file.
        file: String,
        /// Output format: "human" (default) or "json".
        #[arg(long, default_value = "human")]
        format: String,
        /// Optional JSON-only payloads.
        #[arg(long, value_name = "WHAT", value_parser = ["typed-ast"])]
        emit: Option<String>,
        /// Force multi-file project mode (auto-detected for main.ky).
        #[arg(long)]
        project: bool,
    },
    /// Run a Kyokara source file.
    Run {
        /// Path to the .ky source file.
        file: String,
        /// Force multi-file project mode (auto-detected for main.ky).
        #[arg(long)]
        project: bool,
        /// Path to capability manifest (caps.json). Deny-by-default when set.
        #[arg(long)]
        caps: Option<String>,
        /// Path to write a replay log (JSONL). No log is written unless set.
        #[arg(long)]
        replay_log: Option<String>,
    },
    /// Replay a recorded Kyokara execution log.
    Replay {
        /// Path to the replay log written by `kyokara run --replay-log`.
        file: String,
        /// Replay mode: `replay` reuses recorded outcomes, `verify` also checks write intent.
        #[arg(long, default_value = "replay", value_parser = ["replay", "verify"])]
        mode: String,
    },
    /// Format a Kyokara source file.
    Fmt {
        /// Path to the .ky source file.
        file: String,
        /// Check formatting without writing. Exits 1 if not formatted.
        #[arg(long)]
        check: bool,
    },
    /// Start the Language Server Protocol server.
    Lsp,
    /// Property-based test functions with contracts.
    Test {
        /// Path to the .ky source file.
        file: String,
        /// Explore: generate random inputs (without this, only replays corpus).
        #[arg(long)]
        explore: bool,
        /// Number of random test cases per function (default: 100).
        #[arg(long, default_value = "100")]
        num_tests: usize,
        /// Fixed seed for deterministic generation.
        #[arg(long)]
        seed: Option<u64>,
        /// Output format: "human" (default) or "json".
        #[arg(long, default_value = "human")]
        format: String,
        /// Force multi-file project mode.
        #[arg(long)]
        project: bool,
    },
    /// Add a package dependency to a package manifest.
    Add {
        /// Path to the package entry file (`src/main.ky` or `src/lib.ky`).
        file: String,
        /// Local dependency alias used under `deps.<alias>`.
        #[arg(long = "as")]
        as_alias: String,
        /// Local path dependency source.
        #[arg(long)]
        path: Option<String>,
        /// Git dependency source.
        #[arg(long)]
        git: Option<String>,
        /// Exact git revision for `--git`.
        #[arg(long)]
        rev: Option<String>,
        /// Registry package ID for registry dependencies.
        package: Option<String>,
        /// Registry version requirement for registry dependencies.
        #[arg(long)]
        version: Option<String>,
        /// Optional external registry root to copy packages from.
        #[arg(long)]
        registry: Option<String>,
    },
    /// Refresh package dependency resolution.
    Update {
        /// Path to the package entry file (`src/main.ky` or `src/lib.ky`).
        file: String,
        /// Optional single alias to refresh from the external registry.
        #[arg(long)]
        alias: Option<String>,
        /// Optional external registry root to copy packages from.
        #[arg(long)]
        registry: Option<String>,
    },
    /// Publish a library package into a source-first registry store.
    Publish {
        /// Path to the package entry file (`src/lib.ky`).
        file: String,
        /// Registry root directory.
        #[arg(long)]
        registry: String,
    },
    /// Apply a semantic refactor to a Kyokara source file.
    Refactor {
        /// Path to the .ky source file.
        file: String,
        /// Refactor action: rename, add-missing-match-cases, add-missing-capability.
        #[arg(long)]
        action: String,
        /// Symbol name (for rename).
        #[arg(long)]
        symbol: Option<String>,
        /// New name (for rename).
        #[arg(long)]
        new_name: Option<String>,
        /// Symbol kind: function, type, capability, variant (default: function).
        #[arg(long, default_value = "function")]
        kind: String,
        /// Byte offset (for quickfix actions).
        #[arg(long)]
        offset: Option<u32>,
        /// Target file path (for quickfix actions in project mode).
        /// Disambiguates which module the offset refers to.
        #[arg(long)]
        target_file: Option<String>,
        /// Apply edits to disk instead of printing JSON.
        #[arg(long)]
        apply: bool,
        /// Skip verification and apply edits even if they introduce errors.
        #[arg(long)]
        force: bool,
        /// Force multi-file project mode (auto-detected for main.ky).
        #[arg(long)]
        project: bool,
    },
}

enum DependencySource {
    Path { path: String },
    Git { git: String, rev: String },
    Registry { package: String, version: String },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Check {
            file,
            format,
            emit,
            project,
        } => {
            let path = std::path::Path::new(&file);
            let is_multi_file = should_use_project_mode(path, project);
            let include_typed_ast = emit.as_deref() == Some("typed-ast");

            if let Err(message) = sync_project_lockfile_if_needed(path, is_multi_file) {
                eprintln!("error: {message}");
                std::process::exit(1);
            }

            if let Err(message) = validate_check_emit_format(&format, include_typed_ast) {
                eprintln!("error: {message}");
                std::process::exit(1);
            }

            let options = kyokara_api::CheckOptions { include_typed_ast };

            let output = if is_multi_file {
                kyokara_api::check_project_with_options(path, &options)
            } else {
                let source = match std::fs::read_to_string(&file) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("error: cannot read `{file}`: {e}");
                        std::process::exit(1);
                    }
                };
                kyokara_api::check_with_options(&source, &file, &options)
            };

            match format.as_str() {
                "json" => {
                    let json =
                        serde_json::to_string_pretty(&output).expect("failed to serialize output");
                    println!("{json}");
                }
                _ => {
                    for diag in &output.diagnostics {
                        eprintln!(
                            "{file}:{start}: error[{code}]: {msg}",
                            file = diag.span.file,
                            start = diag.span.start,
                            code = diag.code,
                            msg = diag.message,
                        );
                    }
                    if output.diagnostics.is_empty() {
                        eprintln!("no errors found.");
                    }
                }
            }

            let has_errors = output.diagnostics.iter().any(|d| d.severity == "error");
            if has_errors {
                std::process::exit(1);
            }
        }
        Command::Run {
            file,
            project,
            caps,
            replay_log,
        } => {
            let path = std::path::Path::new(&file);
            let is_multi_file = should_use_project_mode(path, project);

            if let Err(message) = sync_project_lockfile_if_needed(path, is_multi_file) {
                eprintln!("error: {message}");
                std::process::exit(1);
            }

            // Load capability manifest if --caps is provided.
            let manifest = caps.map(|caps_path| {
                let json = match std::fs::read_to_string(&caps_path) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("error: cannot read manifest `{caps_path}`: {e}");
                        std::process::exit(1);
                    }
                };
                match kyokara_eval::manifest::CapabilityManifest::from_json(&json) {
                    Ok(m) => m,
                    Err(e) => {
                        eprintln!("error: invalid manifest `{caps_path}`: {e}");
                        std::process::exit(1);
                    }
                }
            });
            let options = kyokara_eval::RunOptions {
                manifest,
                replay_log: replay_log.as_deref().map(std::path::Path::new),
            };

            if is_multi_file {
                match kyokara_eval::run_project_with_options(path, &options) {
                    Ok(result) => {
                        if !matches!(result.value, kyokara_eval::value::Value::Unit) {
                            println!("{}", result.value.display(&result.interner));
                        }
                    }
                    Err(e) => {
                        eprintln!("runtime error: {e}");
                        std::process::exit(1);
                    }
                }
            } else {
                match kyokara_eval::run_file_with_options(path, &options) {
                    Ok(result) => {
                        if !matches!(result.value, kyokara_eval::value::Value::Unit) {
                            println!("{}", result.value.display(&result.interner));
                        }
                    }
                    Err(e) => {
                        eprintln!("runtime error: {e}");
                        std::process::exit(1);
                    }
                }
            }
        }
        Command::Replay { file, mode } => {
            let mode = match mode.as_str() {
                "verify" => kyokara_eval::ReplayMode::Verify,
                _ => kyokara_eval::ReplayMode::Replay,
            };
            let path = std::path::Path::new(&file);
            match kyokara_eval::replay_from_log(path, mode) {
                Ok(result) => {
                    if !matches!(result.value, kyokara_eval::value::Value::Unit) {
                        println!("{}", result.value.display(&result.interner));
                    }
                }
                Err(e) => {
                    eprintln!("runtime error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Command::Test {
            file,
            explore,
            num_tests,
            seed,
            format,
            project,
        } => {
            let path = std::path::Path::new(&file);
            let is_multi_file = should_use_project_mode(path, project);

            if let Err(message) = sync_project_lockfile_if_needed(path, is_multi_file) {
                eprintln!("error: {message}");
                std::process::exit(1);
            }

            let corpus_base = path
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .to_path_buf();

            let config = kyokara_pbt::TestConfig {
                num_tests,
                explore,
                seed: seed.unwrap_or(0),
                format: format.clone(),
                corpus_base,
            };

            let report = if is_multi_file {
                kyokara_pbt::run_project_tests(path, &config)
            } else {
                let source = match std::fs::read_to_string(&file) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("error: cannot read `{file}`: {e}");
                        std::process::exit(1);
                    }
                };
                kyokara_pbt::run_tests(&source, &config)
            };

            match report {
                Ok(report) => {
                    match format.as_str() {
                        "json" => print!("{}", report.format_json()),
                        _ => print!("{}", report.format_human()),
                    }
                    if !report.all_passed() {
                        std::process::exit(1);
                    }
                    if report.results.is_empty()
                        && !explore
                        && !kyokara_pbt::corpus::has_any_corpus(&config.corpus_base)
                    {
                        eprintln!("No corpus found. Run with --explore to generate test cases.");
                    }
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Command::Add {
            file,
            as_alias,
            path,
            git,
            rev,
            package,
            version,
            registry,
        } => {
            let entry_path = std::path::Path::new(&file);
            let source = match (path, git, rev, package, version) {
                (Some(path), None, None, None, None) => DependencySource::Path { path },
                (None, Some(git), Some(rev), None, None) => DependencySource::Git { git, rev },
                (None, None, None, Some(package), Some(version)) => {
                    DependencySource::Registry { package, version }
                }
                _ => {
                    eprintln!(
                        "error: add requires exactly one source form: --path, --git with --rev, or <package> with --version"
                    );
                    std::process::exit(1);
                }
            };
            let registry_root = registry.as_deref().map(std::path::Path::new);
            if let Err(message) =
                add_package_dependency(entry_path, source, &as_alias, registry_root)
            {
                eprintln!("error: {message}");
                std::process::exit(1);
            }
        }
        Command::Update {
            file,
            alias,
            registry,
        } => {
            let entry_path = std::path::Path::new(&file);
            let registry_root = registry.as_deref().map(std::path::Path::new);
            if let Err(message) =
                update_package_dependencies(entry_path, alias.as_deref(), registry_root)
            {
                eprintln!("error: {message}");
                std::process::exit(1);
            }
        }
        Command::Publish { file, registry } => {
            let entry_path = std::path::Path::new(&file);
            let registry_root = std::path::Path::new(&registry);
            if let Err(message) = publish_package_to_registry(entry_path, registry_root) {
                eprintln!("error: {message}");
                std::process::exit(1);
            }
        }
        Command::Lsp => {
            let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
            rt.block_on(kyokara_lsp::run_lsp());
        }
        Command::Refactor {
            file,
            action,
            symbol,
            new_name,
            kind,
            offset,
            target_file,
            apply,
            force,
            project,
        } => {
            let refactor_action = match action.as_str() {
                "rename" => {
                    let sym = symbol.unwrap_or_else(|| {
                        eprintln!("error: --symbol is required for rename");
                        std::process::exit(1);
                    });
                    let new = new_name.unwrap_or_else(|| {
                        eprintln!("error: --new-name is required for rename");
                        std::process::exit(1);
                    });
                    let sk = match kind.as_str() {
                        "function" | "fn" => kyokara_refactor::SymbolKind::Function,
                        "type" => kyokara_refactor::SymbolKind::Type,
                        "capability" | "cap" => kyokara_refactor::SymbolKind::Capability,
                        "variant" => kyokara_refactor::SymbolKind::Variant,
                        other => {
                            eprintln!("error: unknown symbol kind `{other}`");
                            std::process::exit(1);
                        }
                    };
                    kyokara_refactor::RefactorAction::RenameSymbol {
                        old_name: sym,
                        new_name: new,
                        kind: sk,
                        target_file: target_file.clone(),
                    }
                }
                "add-missing-match-cases" => {
                    let off = offset.unwrap_or_else(|| {
                        eprintln!("error: --offset is required for add-missing-match-cases");
                        std::process::exit(1);
                    });
                    kyokara_refactor::RefactorAction::AddMissingMatchCases {
                        offset: off,
                        target_file: target_file.clone(),
                    }
                }
                "add-missing-capability" => {
                    let off = offset.unwrap_or_else(|| {
                        eprintln!("error: --offset is required for add-missing-capability");
                        std::process::exit(1);
                    });
                    kyokara_refactor::RefactorAction::AddMissingCapability {
                        offset: off,
                        target_file: target_file.clone(),
                    }
                }
                other => {
                    eprintln!("error: unknown refactor action `{other}`");
                    std::process::exit(1);
                }
            };

            let path = std::path::Path::new(&file);
            let is_multi_file = should_use_project_mode(path, project);

            let output = if is_multi_file {
                kyokara_api::refactor_project(path, refactor_action, force)
            } else {
                let source = match std::fs::read_to_string(&file) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("error: cannot read `{file}`: {e}");
                        std::process::exit(1);
                    }
                };
                kyokara_api::refactor(&source, &file, refactor_action, force)
            };

            if output.status == "failed" {
                eprintln!("warning: verification failed after refactor");
                for d in &output.verification_diagnostics {
                    if let Some(span) = &d.span {
                        let code = d.code.as_deref().unwrap_or("????");
                        eprintln!(
                            "  [{code}] {}:{}-{}: {}",
                            span.file, span.start, span.end, d.message
                        );
                    } else {
                        eprintln!("  {}", d.message);
                    }
                }
            }

            if apply && (output.status == "typechecked" || output.status == "skipped") {
                if output.status == "skipped" {
                    eprintln!("warning: verification skipped due to --force flag");
                }
                // Use patched sources from the transaction when available.
                if let Some(patched) = &output.patched_sources {
                    if let Err(e) = apply_patched_sources_atomically(patched) {
                        eprintln!("error: {e}");
                        std::process::exit(1);
                    }
                    for ps in patched {
                        eprintln!("wrote {}", ps.file);
                    }
                }
            } else if apply && output.status == "failed" {
                eprintln!(
                    "error: refusing to apply edits that fail verification (use --force to override)"
                );
                std::process::exit(1);
            } else {
                let json =
                    serde_json::to_string_pretty(&output).expect("failed to serialize output");
                println!("{json}");
            }

            if output.status == "error" {
                std::process::exit(1);
            }
        }
        Command::Fmt { file, check } => {
            let source = match std::fs::read_to_string(&file) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("error: cannot read `{file}`: {e}");
                    std::process::exit(1);
                }
            };

            let formatted = kyokara_fmt::format_source(&source);

            if check {
                if formatted != source {
                    eprintln!("{file}");
                    std::process::exit(1);
                }
            } else if formatted != source
                && let Err(e) = std::fs::write(&file, &formatted)
            {
                eprintln!("error: cannot write `{file}`: {e}");
                std::process::exit(1);
            }
        }
    }
}

fn apply_patched_sources_atomically(
    patched: &[kyokara_api::PatchedSourceDto],
) -> Result<(), String> {
    apply_patched_sources_with_ops(
        patched,
        |path| std::fs::read_to_string(path),
        |path, source| std::fs::write(path, source),
    )
}

fn apply_patched_sources_with_ops<Read, Write>(
    patched: &[kyokara_api::PatchedSourceDto],
    mut read: Read,
    mut write: Write,
) -> Result<(), String>
where
    Read: FnMut(&str) -> std::io::Result<String>,
    Write: FnMut(&str, &str) -> std::io::Result<()>,
{
    let mut originals = Vec::with_capacity(patched.len());
    for ps in patched {
        let original =
            read(&ps.file).map_err(|e| format!("cannot read `{}` before apply: {e}", ps.file))?;
        originals.push((ps.file.as_str(), original));
    }

    let mut applied_indices = Vec::new();
    for (idx, ps) in patched.iter().enumerate() {
        if let Err(e) = write(&ps.file, &ps.source) {
            let rollback_errors =
                rollback_applied_sources(&applied_indices, &originals, &mut write);
            let mut msg = format!("cannot write `{}`: {e}", ps.file);
            if !rollback_errors.is_empty() {
                msg.push_str("; ");
                msg.push_str(&rollback_errors.join("; "));
            }
            return Err(msg);
        }
        applied_indices.push(idx);
    }

    Ok(())
}

fn rollback_applied_sources<Write>(
    applied_indices: &[usize],
    originals: &[(&str, String)],
    write: &mut Write,
) -> Vec<String>
where
    Write: FnMut(&str, &str) -> std::io::Result<()>,
{
    let mut rollback_errors = Vec::new();
    for &idx in applied_indices.iter().rev() {
        let (file, original) = &originals[idx];
        if let Err(e) = write(file, original) {
            rollback_errors.push(format!("cannot rollback `{file}`: {e}"));
        }
    }
    rollback_errors
}

fn validate_check_emit_format(format: &str, include_typed_ast: bool) -> Result<(), &'static str> {
    if include_typed_ast && format != "json" {
        return Err("`--emit typed-ast` requires `--format json`");
    }
    Ok(())
}

fn sync_project_lockfile_if_needed(
    path: &std::path::Path,
    is_multi_file: bool,
) -> Result<(), String> {
    if !is_multi_file || !kyokara_hir::has_package_manifest_candidate(path) {
        return Ok(());
    }

    kyokara_hir::sync_package_lockfile_for_entry(path)
        .map(|_| ())
        .map_err(|err| {
            format!(
                "cannot write package lockfile for `{}`: {err}",
                path.display()
            )
        })
}

fn add_package_dependency(
    entry_file: &std::path::Path,
    source: DependencySource,
    alias: &str,
    registry_root: Option<&std::path::Path>,
) -> Result<(), String> {
    if !is_identifier(alias) {
        return Err(format!(
            "dependency alias `{alias}` must be a valid identifier"
        ));
    }

    let manifest_path = package_manifest_path_for_entry(entry_file)?;
    if let DependencySource::Registry { package, version } = &source
        && let Some(registry_root) = registry_root
    {
        copy_selected_registry_dependency_closure_into_local_store(
            &manifest_path,
            registry_root,
            package,
            version,
        )?;
    }

    let manifest_before = std::fs::read_to_string(&manifest_path)
        .map_err(|err| format!("cannot read `{}`: {err}", manifest_path.display()))?;
    let lockfile_path = package_lockfile_path(&manifest_path);
    let lockfile_before = read_existing_file(&lockfile_path)
        .map_err(|err| format!("cannot read `{}`: {err}", lockfile_path.display()))?;
    update_manifest_dependencies(&manifest_path, |dependencies| {
        dependencies.insert(alias.to_string(), dependency_source_to_toml_value(source));
        Ok(())
    })?;

    if let Err(err) = sync_project_lockfile_if_needed(entry_file, true) {
        std::fs::write(&manifest_path, manifest_before).map_err(|write_err| {
            format!(
                "{err}; additionally failed to restore `{}`: {write_err}",
                manifest_path.display()
            )
        })?;
        restore_file(&lockfile_path, lockfile_before).map_err(|write_err| {
            format!(
                "{err}; additionally failed to restore `{}`: {write_err}",
                lockfile_path.display()
            )
        })?;
        return Err(err);
    }

    Ok(())
}

fn update_package_dependencies(
    entry_file: &std::path::Path,
    alias: Option<&str>,
    registry_root: Option<&std::path::Path>,
) -> Result<(), String> {
    let manifest_path = package_manifest_path_for_entry(entry_file)?;
    let lockfile_path = package_lockfile_path(&manifest_path);
    let lockfile_before = read_existing_file(&lockfile_path)
        .map_err(|err| format!("cannot read `{}`: {err}", lockfile_path.display()))?;
    if let Some(registry_root) = registry_root {
        let manifest = read_manifest_value(&manifest_path)?;
        if let Some(dependencies) = manifest.get("dependencies").and_then(toml::Value::as_table) {
            for (dep_alias, spec) in dependencies {
                if alias.is_some_and(|requested| requested != dep_alias) {
                    continue;
                }
                let Some(spec_table) = spec.as_table() else {
                    continue;
                };
                let Some(package) = spec_table.get("package").and_then(toml::Value::as_str) else {
                    continue;
                };
                let Some(version_req) = spec_table.get("version").and_then(toml::Value::as_str)
                else {
                    continue;
                };
                copy_selected_registry_dependency_closure_into_local_store(
                    &manifest_path,
                    registry_root,
                    package,
                    version_req,
                )?;
            }
        }
    }

    let update_result = if let Some(alias) = alias {
        kyokara_hir::update_package_lockfile_for_entry(entry_file, alias)
            .map(|_| ())
            .map_err(|err| {
                format!(
                    "cannot write package lockfile for `{}`: {err}",
                    entry_file.display()
                )
            })
    } else {
        let _ = std::fs::remove_file(&lockfile_path);
        sync_project_lockfile_if_needed(entry_file, true)
    };
    if let Err(err) = update_result {
        restore_file(&lockfile_path, lockfile_before).map_err(|write_err| {
            format!(
                "{err}; additionally failed to restore `{}`: {write_err}",
                lockfile_path.display()
            )
        })?;
        return Err(err);
    }
    Ok(())
}

fn publish_package_to_registry(
    entry_file: &std::path::Path,
    registry_root: &std::path::Path,
) -> Result<(), String> {
    let manifest_path = package_manifest_path_for_entry(entry_file)?;
    let manifest = read_manifest_value(&manifest_path)?;
    let package = manifest
        .get("package")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| {
            format!(
                "invalid package manifest `{}`: missing [package] table",
                manifest_path.display()
            )
        })?;
    let kind = package
        .get("kind")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| {
            format!(
                "invalid package manifest `{}`: package.kind must be present",
                manifest_path.display()
            )
        })?;
    if kind != "lib" {
        return Err("only lib packages are publishable".to_string());
    }
    let package_name = package
        .get("name")
        .and_then(toml::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            format!(
                "invalid package manifest `{}`: package.name must be a non-empty string",
                manifest_path.display()
            )
        })?;
    let version = package
        .get("version")
        .and_then(toml::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            format!(
                "invalid package manifest `{}`: published packages require package.version",
                manifest_path.display()
            )
        })?;

    if let Some(dependencies) = manifest.get("dependencies").and_then(toml::Value::as_table) {
        for (alias, spec) in dependencies {
            let Some(spec_table) = spec.as_table() else {
                continue;
            };
            if spec_table.contains_key("path") {
                return Err(format!(
                    "package `{package_name}` is not publishable because dependency `{alias}` uses path dependencies"
                ));
            }
            if spec_table.contains_key("git") {
                return Err(format!(
                    "package `{package_name}` is not publishable because dependency `{alias}` uses git dependencies"
                ));
            }
        }
    }

    let package_root = manifest_path
        .parent()
        .expect("manifest path should have parent");
    let dest_root = registry_root
        .join("packages")
        .join(package_name)
        .join(version);
    if dest_root.exists() {
        return Err(format!(
            "package `{package_name}` version `{version}` is already published in `{}`",
            registry_root.display()
        ));
    }
    std::fs::create_dir_all(&dest_root)
        .map_err(|err| format!("cannot create `{}`: {err}", dest_root.display()))?;
    std::fs::copy(&manifest_path, dest_root.join("kyokara.toml"))
        .map(|_| ())
        .map_err(|err| {
            format!(
                "cannot copy `{}` to `{}`: {err}",
                manifest_path.display(),
                dest_root.join("kyokara.toml").display()
            )
        })?;
    copy_dir_recursive(&package_root.join("src"), &dest_root.join("src"), |_| false)?;
    Ok(())
}

fn dependency_source_to_toml_value(source: DependencySource) -> toml::Value {
    let mut table = toml::map::Map::new();
    match source {
        DependencySource::Path { path } => {
            table.insert("path".to_string(), toml::Value::String(path));
        }
        DependencySource::Git { git, rev } => {
            table.insert("git".to_string(), toml::Value::String(git));
            table.insert("rev".to_string(), toml::Value::String(rev));
        }
        DependencySource::Registry { package, version } => {
            table.insert("package".to_string(), toml::Value::String(package));
            table.insert("version".to_string(), toml::Value::String(version));
        }
    }
    toml::Value::Table(table)
}

fn package_manifest_path_for_entry(
    entry_file: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    if !kyokara_hir::has_package_manifest_candidate(entry_file) {
        return Err(format!(
            "`{}` is not a package entry file with a nearby kyokara.toml",
            entry_file.display()
        ));
    }
    entry_file
        .parent()
        .and_then(std::path::Path::parent)
        .map(|path| path.join("kyokara.toml"))
        .ok_or_else(|| {
            format!(
                "cannot locate package manifest for `{}`",
                entry_file.display()
            )
        })
}

fn read_manifest_value(manifest_path: &std::path::Path) -> Result<toml::Value, String> {
    let source = std::fs::read_to_string(manifest_path)
        .map_err(|err| format!("cannot read `{}`: {err}", manifest_path.display()))?;
    source.parse::<toml::Value>().map_err(|err| {
        format!(
            "invalid package manifest `{}`: {err}",
            manifest_path.display()
        )
    })
}

fn package_lockfile_path(manifest_path: &std::path::Path) -> std::path::PathBuf {
    manifest_path
        .parent()
        .expect("manifest path should have parent")
        .join("kyokara.lock")
}

fn read_existing_file(path: &std::path::Path) -> Result<Option<String>, std::io::Error> {
    match std::fs::read_to_string(path) {
        Ok(source) => Ok(Some(source)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

fn restore_file(path: &std::path::Path, source: Option<String>) -> Result<(), std::io::Error> {
    match source {
        Some(source) => std::fs::write(path, source),
        None => match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err),
        },
    }
}

fn update_manifest_dependencies(
    manifest_path: &std::path::Path,
    mutate: impl FnOnce(&mut toml::map::Map<String, toml::Value>) -> Result<(), String>,
) -> Result<(), String> {
    let mut manifest = read_manifest_value(manifest_path)?;
    let manifest_table = manifest.as_table_mut().ok_or_else(|| {
        format!(
            "invalid package manifest `{}`: manifest must be a TOML table",
            manifest_path.display()
        )
    })?;
    let dependencies = manifest_table
        .entry("dependencies".to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    let dependencies = dependencies.as_table_mut().ok_or_else(|| {
        format!(
            "invalid package manifest `{}`: [dependencies] must be a table",
            manifest_path.display()
        )
    })?;

    mutate(dependencies)?;

    let rendered =
        toml::to_string(&manifest).map_err(|err| format!("cannot render manifest: {err}"))?;
    std::fs::write(manifest_path, rendered)
        .map_err(|err| format!("cannot write `{}`: {err}", manifest_path.display()))
}

fn copy_registry_version_into_local_store(
    manifest_path: &std::path::Path,
    registry_root: &std::path::Path,
    package: &str,
    version: &str,
) -> Result<(), String> {
    let source_root = registry_package_version_root(registry_root, package, version);
    if !source_root.is_dir() {
        return Err(format!(
            "registry package `{package}` version `{version}` not found in `{}`",
            registry_root.display(),
        ));
    }
    let package_root = manifest_path
        .parent()
        .expect("manifest path should have parent");
    let dest_root = package_root
        .join(".kyokara")
        .join("registry")
        .join("packages")
        .join(package)
        .join(version);
    copy_dir_recursive(&source_root, &dest_root, |_| false)
}

fn copy_selected_registry_dependency_closure_into_local_store(
    manifest_path: &std::path::Path,
    registry_root: &std::path::Path,
    package: &str,
    version_req: &str,
) -> Result<(), String> {
    let mut visited = HashSet::new();
    let _ = copy_selected_registry_dependency_closure_into_local_store_inner(
        manifest_path,
        registry_root,
        package,
        version_req,
        &mut visited,
    )?;
    Ok(())
}

fn copy_selected_registry_dependency_closure_into_local_store_inner(
    manifest_path: &std::path::Path,
    registry_root: &std::path::Path,
    package: &str,
    version_req: &str,
    visited: &mut HashSet<(String, String)>,
) -> Result<String, String> {
    let version = resolve_registry_version_from_source_registry(
        manifest_path,
        registry_root,
        package,
        version_req,
    )?;
    if !visited.insert((package.to_string(), version.clone())) {
        return Ok(version);
    }

    let source_manifest_path =
        registry_package_version_root(registry_root, package, &version).join("kyokara.toml");
    let source_manifest = read_manifest_value(&source_manifest_path)?;
    let mut pinned_transitives = Vec::new();
    if let Some(dependencies) = source_manifest
        .get("dependencies")
        .and_then(toml::Value::as_table)
    {
        for (alias, spec) in dependencies {
            let Some(spec_table) = spec.as_table() else {
                continue;
            };
            let Some(package) = spec_table.get("package").and_then(toml::Value::as_str) else {
                continue;
            };
            let Some(version_req) = spec_table.get("version").and_then(toml::Value::as_str) else {
                continue;
            };
            let selected_version =
                copy_selected_registry_dependency_closure_into_local_store_inner(
                    manifest_path,
                    registry_root,
                    package,
                    version_req,
                    visited,
                )?;
            pinned_transitives.push((alias.clone(), selected_version));
        }
    }

    copy_registry_version_into_local_store(manifest_path, registry_root, package, &version)?;

    if !pinned_transitives.is_empty() {
        pin_vendored_registry_dependency_versions(
            &package_local_registry_manifest_path(manifest_path, package, &version),
            &pinned_transitives,
        )?;
    }

    Ok(version)
}

fn pin_vendored_registry_dependency_versions(
    manifest_path: &std::path::Path,
    pinned_versions: &[(String, String)],
) -> Result<(), String> {
    let mut manifest = read_manifest_value(manifest_path)?;
    let Some(dependencies) = manifest
        .as_table_mut()
        .and_then(|table| table.get_mut("dependencies"))
        .and_then(toml::Value::as_table_mut)
    else {
        return Ok(());
    };
    for (alias, version) in pinned_versions {
        let Some(spec_table) = dependencies
            .get_mut(alias)
            .and_then(toml::Value::as_table_mut)
        else {
            continue;
        };
        if spec_table.contains_key("package") {
            spec_table.insert(
                "version".to_string(),
                toml::Value::String(format!("={version}")),
            );
        }
    }
    write_manifest_value(manifest_path, &manifest)
}

fn resolve_registry_version_from_source_registry(
    manifest_path: &std::path::Path,
    registry_root: &std::path::Path,
    package: &str,
    version_req: &str,
) -> Result<String, String> {
    let req = VersionReq::parse(version_req).map_err(|err| {
        format!(
            "invalid package manifest `{}`: dependency on `{package}` has invalid version requirement `{version_req}`: {err}",
            manifest_path.display()
        )
    })?;
    let package_root = registry_root.join("packages").join(package);
    let entries = std::fs::read_dir(&package_root).map_err(|err| {
        format!(
            "registry package `{package}` not found in `{}`: {err}",
            registry_root.display()
        )
    })?;

    let mut candidates = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|err| format!("cannot read `{}`: {err}", package_root.display()))?;
        let raw_version = entry.file_name().to_string_lossy().to_string();
        let Ok(version) = Version::parse(&raw_version) else {
            continue;
        };
        if req.matches(&version) {
            candidates.push(version);
        }
    }
    candidates.sort();
    candidates
        .pop()
        .map(|version| version.to_string())
        .ok_or_else(|| {
            format!(
                "registry package `{package}` could not resolve version requirement `{version_req}` in `{}`",
                registry_root.display()
            )
        })
}

fn registry_package_version_root(
    registry_root: &std::path::Path,
    package: &str,
    version: &str,
) -> std::path::PathBuf {
    registry_root.join("packages").join(package).join(version)
}

fn package_local_registry_manifest_path(
    manifest_path: &std::path::Path,
    package: &str,
    version: &str,
) -> std::path::PathBuf {
    manifest_path
        .parent()
        .expect("manifest path should have parent")
        .join(".kyokara")
        .join("registry")
        .join("packages")
        .join(package)
        .join(version)
        .join("kyokara.toml")
}

fn write_manifest_value(
    manifest_path: &std::path::Path,
    manifest: &toml::Value,
) -> Result<(), String> {
    let rendered =
        toml::to_string(manifest).map_err(|err| format!("cannot render manifest: {err}"))?;
    std::fs::write(manifest_path, rendered)
        .map_err(|err| format!("cannot write `{}`: {err}", manifest_path.display()))
}

fn copy_dir_recursive(
    src: &std::path::Path,
    dst: &std::path::Path,
    skip: impl Fn(&std::path::Path) -> bool + Copy,
) -> Result<(), String> {
    if skip(src) {
        return Ok(());
    }
    if src.is_dir() {
        std::fs::create_dir_all(dst)
            .map_err(|err| format!("cannot create `{}`: {err}", dst.display()))?;
        for entry in std::fs::read_dir(src)
            .map_err(|err| format!("cannot read `{}`: {err}", src.display()))?
        {
            let entry = entry.map_err(|err| format!("cannot read `{}`: {err}", src.display()))?;
            let child_src = entry.path();
            let child_dst = dst.join(entry.file_name());
            copy_dir_recursive(&child_src, &child_dst, skip)?;
        }
        return Ok(());
    }

    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("cannot create `{}`: {err}", parent.display()))?;
    }
    std::fs::copy(src, dst).map(|_| ()).map_err(|err| {
        format!(
            "cannot copy `{}` to `{}`: {err}",
            src.display(),
            dst.display()
        )
    })
}

fn is_identifier(raw: &str) -> bool {
    let mut chars = raw.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

/// Check if there are other `.ky` files alongside the given file.
fn has_sibling_ky_files(entry: &std::path::Path, dir: &std::path::Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry_result in entries {
        let Ok(dir_entry) = entry_result else {
            continue;
        };
        let path = dir_entry.path();
        if path != entry && path.extension().is_some_and(|ext| ext == "ky") {
            return true;
        }
    }
    false
}

/// Determine if the given file should be treated as a multi-file project.
///
/// Auto-detection requires EITHER:
/// 1. A package-style entry (`src/main.ky` or `src/lib.ky`) with a nearby
///    `kyokara.toml`, or
/// 2. A legacy `main.ky` entry with sibling `.ky` files.
///
/// Use the `--project` flag to force project mode whenever the entry path
/// exists, without relying on sibling-file heuristics.
fn should_use_project_mode(path: &std::path::Path, force_project: bool) -> bool {
    if force_project {
        // --project is explicit user intent: bypass heuristics.
        return path.is_file();
    }

    if !path.is_file() {
        return false;
    }

    if kyokara_hir::has_package_manifest_candidate(path) {
        return true;
    }

    // Legacy auto-detect: main.ky with siblings.
    path.file_name().is_some_and(|name| name == "main.ky")
        && path
            .parent()
            .is_some_and(|dir| has_sibling_ky_files(path, dir))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::io;
    use std::process::Command as ProcessCommand;

    fn patched(file: &str, source: &str) -> kyokara_api::PatchedSourceDto {
        kyokara_api::PatchedSourceDto {
            file: file.to_string(),
            source: source.to_string(),
        }
    }

    fn init_git_package_repo(
        repo_dir: &std::path::Path,
        package_name: &str,
        version: &str,
        lib_source: &str,
    ) -> String {
        let src_dir = repo_dir.join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(
            repo_dir.join("kyokara.toml"),
            format!(
                "[package]\nname = \"{package_name}\"\nversion = \"{version}\"\nedition = \"2026\"\nkind = \"lib\"\n"
            ),
        )
        .unwrap();
        std::fs::write(src_dir.join("lib.ky"), lib_source).unwrap();

        let run = |args: &[&str]| {
            let output = ProcessCommand::new("git")
                .args(args)
                .current_dir(repo_dir)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {:?} should succeed\nstdout:\n{}\nstderr:\n{}",
                args,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        };

        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.name", "Kyokara Tests"]);
        run(&["config", "user.email", "tests@kyokara.invalid"]);
        run(&["add", "."]);
        run(&["commit", "-qm", "init"]);

        let output = ProcessCommand::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo_dir)
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn commit_git_package_repo_change(repo_dir: &std::path::Path, lib_source: &str) -> String {
        std::fs::write(repo_dir.join("src").join("lib.ky"), lib_source).unwrap();

        let run = |args: &[&str]| {
            let output = ProcessCommand::new("git")
                .args(args)
                .current_dir(repo_dir)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {:?} should succeed\nstdout:\n{}\nstderr:\n{}",
                args,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        };

        run(&["add", "."]);
        run(&["commit", "-qm", "update"]);

        let output = ProcessCommand::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo_dir)
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn write_registry_package(
        registry_root: &std::path::Path,
        package_name: &str,
        version: &str,
        lib_source: &str,
    ) {
        let package_dir = registry_root
            .join("packages")
            .join(package_name)
            .join(version);
        let src_dir = package_dir.join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(
            package_dir.join("kyokara.toml"),
            format!(
                "[package]\nname = \"{package_name}\"\nversion = \"{version}\"\nedition = \"2026\"\nkind = \"lib\"\n"
            ),
        )
        .unwrap();
        std::fs::write(src_dir.join("lib.ky"), lib_source).unwrap();
    }

    #[test]
    fn apply_patched_sources_rolls_back_on_late_write_failure() {
        let files = RefCell::new(BTreeMap::from([
            ("a.ky".to_string(), "fn a() -> Int { 1 }\n".to_string()),
            ("b.ky".to_string(), "fn b() -> Int { 2 }\n".to_string()),
        ]));
        let writes = RefCell::new(Vec::<(String, String)>::new());
        let patched = vec![
            patched("a.ky", "fn a() -> Int { 10 }\n"),
            patched("b.ky", "fn b() -> Int { 20 }\n"),
        ];

        let err = apply_patched_sources_with_ops(
            &patched,
            |path| {
                files.borrow().get(path).cloned().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::NotFound, format!("missing file {path}"))
                })
            },
            |path, source| {
                writes
                    .borrow_mut()
                    .push((path.to_string(), source.to_string()));
                if path == "b.ky" && source == "fn b() -> Int { 20 }\n" {
                    return Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"));
                }
                files
                    .borrow_mut()
                    .insert(path.to_string(), source.to_string());
                Ok(())
            },
        )
        .unwrap_err();

        assert!(
            err.contains("cannot write `b.ky`"),
            "expected write error for b.ky, got: {err}"
        );

        let files = files.borrow();
        assert_eq!(
            files.get("a.ky").map(String::as_str),
            Some("fn a() -> Int { 1 }\n"),
            "first file should be rolled back after later write failure"
        );
        assert_eq!(
            files.get("b.ky").map(String::as_str),
            Some("fn b() -> Int { 2 }\n"),
            "failing file should keep original content"
        );

        let writes = writes.borrow();
        assert_eq!(
            writes.as_slice(),
            &[
                ("a.ky".to_string(), "fn a() -> Int { 10 }\n".to_string()),
                ("b.ky".to_string(), "fn b() -> Int { 20 }\n".to_string()),
                ("a.ky".to_string(), "fn a() -> Int { 1 }\n".to_string()),
            ],
            "expected rollback write to restore already-applied files"
        );
    }

    #[test]
    fn apply_patched_sources_successfully_writes_all_files() {
        let dir = tempfile::tempdir().unwrap();
        let a_path = dir.path().join("a.ky");
        let b_path = dir.path().join("b.ky");
        std::fs::write(&a_path, "fn a() -> Int { 1 }\n").unwrap();
        std::fs::write(&b_path, "fn b() -> Int { 2 }\n").unwrap();

        let patched = vec![
            kyokara_api::PatchedSourceDto {
                file: a_path.display().to_string(),
                source: "fn a() -> Int { 10 }\n".to_string(),
            },
            kyokara_api::PatchedSourceDto {
                file: b_path.display().to_string(),
                source: "fn b() -> Int { 20 }\n".to_string(),
            },
        ];

        apply_patched_sources_atomically(&patched).expect("apply should succeed");

        assert_eq!(
            std::fs::read_to_string(&a_path).unwrap(),
            "fn a() -> Int { 10 }\n"
        );
        assert_eq!(
            std::fs::read_to_string(&b_path).unwrap(),
            "fn b() -> Int { 20 }\n"
        );
    }

    #[test]
    fn auto_detect_requires_main_ky() {
        let dir = std::env::temp_dir().join("kyokara_autodetect_test_main");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let main_path = dir.join("main.ky");
        let math_path = dir.join("math.ky");
        std::fs::write(&main_path, "fn foo() -> Int { 1 }").unwrap();
        std::fs::write(&math_path, "pub fn bar() -> Int { 2 }").unwrap();

        // main.ky with siblings → project mode.
        assert!(
            should_use_project_mode(&main_path, false),
            "main.ky with siblings should auto-detect as project"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn auto_detect_rejects_non_main_ky() {
        let dir = std::env::temp_dir().join("kyokara_autodetect_test_nonmain");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let other_path = dir.join("other.ky");
        let math_path = dir.join("math.ky");
        std::fs::write(&other_path, "fn foo() -> Int { 1 }").unwrap();
        std::fs::write(&math_path, "pub fn bar() -> Int { 2 }").unwrap();

        // other.ky with siblings → NOT project mode (auto-detect requires main.ky).
        assert!(
            !should_use_project_mode(&other_path, false),
            "non-main.ky should NOT auto-detect as project"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn force_project_overrides_name_check() {
        let dir = std::env::temp_dir().join("kyokara_autodetect_test_force");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let other_path = dir.join("other.ky");
        let math_path = dir.join("math.ky");
        std::fs::write(&other_path, "fn foo() -> Int { 1 }").unwrap();
        std::fs::write(&math_path, "pub fn bar() -> Int { 2 }").unwrap();

        // other.ky with --project → project mode.
        assert!(
            should_use_project_mode(&other_path, true),
            "--project flag should force project mode even for non-main.ky"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn auto_detect_single_file_no_siblings() {
        let dir = std::env::temp_dir().join("kyokara_autodetect_test_single");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let main_path = dir.join("main.ky");
        std::fs::write(&main_path, "fn foo() -> Int { 1 }").unwrap();

        // main.ky without siblings → NOT project mode.
        assert!(
            !should_use_project_mode(&main_path, false),
            "main.ky without siblings should NOT be project mode"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn force_project_no_siblings_still_single() {
        let dir = std::env::temp_dir().join("kyokara_autodetect_test_force_single");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let main_path = dir.join("main.ky");
        std::fs::write(&main_path, "fn foo() -> Int { 1 }").unwrap();

        // --project always forces project mode for an existing entry file.
        assert!(
            should_use_project_mode(&main_path, true),
            "--project should force project mode even without sibling .ky files"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn force_project_with_subdir_only_modules_uses_project_mode() {
        let dir = std::env::temp_dir().join("kyokara_autodetect_test_force_subdir_only");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a")).unwrap();

        let main_path = dir.join("main.ky");
        let nested_mod = dir.join("a").join("b.ky");
        std::fs::write(&main_path, "import a.b\nfn main() -> Int { b.foo() }").unwrap();
        std::fs::write(&nested_mod, "pub fn foo() -> Int { 1 }").unwrap();

        assert!(
            should_use_project_mode(&main_path, true),
            "--project should force project mode for subdir-only module layouts"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn auto_detect_package_root_for_bin_entry_without_siblings() {
        let dir = std::env::temp_dir().join("kyokara_autodetect_test_package_bin");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();

        let manifest_path = dir.join("kyokara.toml");
        let main_path = dir.join("src").join("main.ky");
        std::fs::write(
            &manifest_path,
            "[package]\nname = \"acme/app\"\nedition = \"2026\"\nkind = \"bin\"\n",
        )
        .unwrap();
        std::fs::write(&main_path, "fn main() -> Int { 1 }").unwrap();

        assert!(
            should_use_project_mode(&main_path, false),
            "package main entry should auto-detect as project even without sibling modules"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn auto_detect_package_root_for_lib_entry() {
        let dir = std::env::temp_dir().join("kyokara_autodetect_test_package_lib");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();

        let manifest_path = dir.join("kyokara.toml");
        let lib_path = dir.join("src").join("lib.ky");
        std::fs::write(
            &manifest_path,
            "[package]\nname = \"acme/lib\"\nedition = \"2026\"\nkind = \"lib\"\n",
        )
        .unwrap();
        std::fs::write(&lib_path, "pub fn answer() -> Int { 42 }").unwrap();

        assert!(
            should_use_project_mode(&lib_path, false),
            "package lib entry should auto-detect as project"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn auto_detect_invalid_package_manifest_still_uses_project_mode() {
        let dir = std::env::temp_dir().join("kyokara_autodetect_test_package_invalid");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();

        let manifest_path = dir.join("kyokara.toml");
        let main_path = dir.join("src").join("main.ky");
        std::fs::write(&manifest_path, "[package]\nname = 123\nkind = \"bin\"\n").unwrap();
        std::fs::write(&main_path, "fn main() -> Int { 1 }").unwrap();

        assert!(
            should_use_project_mode(&main_path, false),
            "package-style entry should stay in project mode so invalid manifests surface diagnostics"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sync_project_lockfile_if_needed_writes_lockfile_for_package_entry() {
        let dir = tempfile::tempdir().unwrap();
        let src_dir = dir.path().join("src");
        let json_dir = dir.path().join("json-pkg");
        let json_src = json_dir.join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::create_dir_all(&json_src).unwrap();

        let main_path = src_dir.join("main.ky");
        std::fs::write(
            json_dir.join("kyokara.toml"),
            "[package]\nname = \"acme/json\"\nedition = \"2026\"\nkind = \"lib\"\n",
        )
        .unwrap();
        std::fs::write(json_src.join("lib.ky"), "pub fn answer() -> Int { 42 }\n").unwrap();
        std::fs::write(
            dir.path().join("kyokara.toml"),
            "[package]\nname = \"acme/app\"\nedition = \"2026\"\nkind = \"bin\"\n\n[dependencies]\njson = { path = \"json-pkg\" }\n",
        )
        .unwrap();
        std::fs::write(&main_path, "fn main() -> Int { 0 }\n").unwrap();

        sync_project_lockfile_if_needed(&main_path, true).expect("lockfile sync should succeed");

        let lockfile = std::fs::read_to_string(dir.path().join("kyokara.lock"))
            .expect("lockfile should be written");
        assert!(
            lockfile.contains("json = { path = \"json-pkg\" }"),
            "expected lockfile to include dependency snapshot, got: {lockfile}"
        );
    }

    #[test]
    fn add_package_dependency_adds_git_dependency_and_syncs_lockfile() {
        let dir = tempfile::tempdir().unwrap();
        let src_dir = dir.path().join("src");
        let git_repo = dir.path().join("git-json");
        std::fs::create_dir_all(&src_dir).unwrap();

        let git_rev = init_git_package_repo(
            &git_repo,
            "acme/git-json",
            "0.2.0",
            "pub fn from_git() -> Int { 7 }\n",
        );

        let main_path = src_dir.join("main.ky");
        std::fs::write(
            dir.path().join("kyokara.toml"),
            "[package]\nname = \"acme/app\"\nedition = \"2026\"\nkind = \"bin\"\n",
        )
        .unwrap();
        std::fs::write(&main_path, "fn main() -> Int { 0 }\n").unwrap();

        add_package_dependency(
            &main_path,
            DependencySource::Git {
                git: git_repo.display().to_string(),
                rev: git_rev.clone(),
            },
            "git_json",
            None,
        )
        .expect("git add should succeed");

        let manifest = std::fs::read_to_string(dir.path().join("kyokara.toml")).unwrap();
        let lockfile = std::fs::read_to_string(dir.path().join("kyokara.lock")).unwrap();
        let manifest_toml = manifest.parse::<toml::Value>().unwrap();
        let manifest_dep = manifest_toml
            .get("dependencies")
            .and_then(toml::Value::as_table)
            .and_then(|deps| deps.get("git_json"))
            .and_then(toml::Value::as_table)
            .unwrap();

        assert!(
            manifest_dep
                .get("git")
                .and_then(toml::Value::as_str)
                .is_some_and(|value| value == git_repo.display().to_string()),
            "expected git dependency in manifest, got: {manifest}"
        );
        assert!(
            manifest_dep
                .get("rev")
                .and_then(toml::Value::as_str)
                .is_some_and(|value| value == git_rev),
            "expected git revision in manifest, got: {manifest}"
        );
        assert!(
            lockfile.contains(&format!(
                "git_json = {{ git = \"{}\", rev = \"{git_rev}\", commit = \"{git_rev}\" }}",
                git_repo.display()
            )),
            "expected git dependency in lockfile, got: {lockfile}"
        );
    }

    #[test]
    fn update_package_dependencies_refreshes_moving_git_refs() {
        let dir = tempfile::tempdir().unwrap();
        let src_dir = dir.path().join("src");
        let git_repo = dir.path().join("git-json");
        std::fs::create_dir_all(&src_dir).unwrap();

        let first_rev = init_git_package_repo(
            &git_repo,
            "acme/git-json",
            "0.2.0",
            "pub fn from_git() -> Int { 7 }\n",
        );

        let main_path = src_dir.join("main.ky");
        std::fs::write(
            dir.path().join("kyokara.toml"),
            "[package]\nname = \"acme/app\"\nedition = \"2026\"\nkind = \"bin\"\n",
        )
        .unwrap();
        std::fs::write(
            &main_path,
            "import deps.git_json\nfn main() -> Int { git_json.from_git() }\n",
        )
        .unwrap();

        add_package_dependency(
            &main_path,
            DependencySource::Git {
                git: git_repo.display().to_string(),
                rev: "main".to_string(),
            },
            "git_json",
            None,
        )
        .expect("initial git add should succeed");

        let first_run =
            kyokara_eval::run_project(&main_path).expect("initial project run should succeed");
        let second_rev =
            commit_git_package_repo_change(&git_repo, "pub fn from_git() -> Int { 8 }\n");

        update_package_dependencies(&main_path, None, None).expect("git update should succeed");

        let lockfile = std::fs::read_to_string(dir.path().join("kyokara.lock")).unwrap();
        let second_run =
            kyokara_eval::run_project(&main_path).expect("updated project run should succeed");

        assert_eq!(
            first_run.value,
            kyokara_eval::value::Value::Int(7),
            "expected initial run to use the first commit"
        );
        assert_eq!(
            second_run.value,
            kyokara_eval::value::Value::Int(8),
            "expected update to refresh the moving ref checkout"
        );
        assert!(
            lockfile.contains("rev = \"main\""),
            "expected lockfile to preserve the requested moving ref, got: {lockfile}"
        );
        assert!(
            lockfile.contains(&format!("commit = \"{second_rev}\"")),
            "expected lockfile to refresh to the latest resolved commit, got: {lockfile}"
        );
        assert!(
            !lockfile.contains(&format!("commit = \"{first_rev}\"")),
            "stale resolved commit should not remain after update: {lockfile}"
        );
    }

    #[test]
    fn update_package_dependencies_only_refreshes_requested_alias() {
        let dir = tempfile::tempdir().unwrap();
        let src_dir = dir.path().join("src");
        let one_repo = dir.path().join("one");
        let two_repo = dir.path().join("two");
        std::fs::create_dir_all(&src_dir).unwrap();

        let _first_one_rev = init_git_package_repo(
            &one_repo,
            "acme/one",
            "0.1.0",
            "pub fn value() -> Int { 1 }\n",
        );
        let first_two_rev = init_git_package_repo(
            &two_repo,
            "acme/two",
            "0.1.0",
            "pub fn value() -> Int { 10 }\n",
        );

        let main_path = src_dir.join("main.ky");
        std::fs::write(
            dir.path().join("kyokara.toml"),
            "[package]\nname = \"acme/app\"\nedition = \"2026\"\nkind = \"bin\"\n",
        )
        .unwrap();
        std::fs::write(&main_path, "fn main() -> Int { 0 }\n").unwrap();

        add_package_dependency(
            &main_path,
            DependencySource::Git {
                git: one_repo.display().to_string(),
                rev: "main".to_string(),
            },
            "one",
            None,
        )
        .expect("first git add should succeed");
        add_package_dependency(
            &main_path,
            DependencySource::Git {
                git: two_repo.display().to_string(),
                rev: "main".to_string(),
            },
            "two",
            None,
        )
        .expect("second git add should succeed");

        let second_one_rev =
            commit_git_package_repo_change(&one_repo, "pub fn value() -> Int { 2 }\n");
        let second_two_rev =
            commit_git_package_repo_change(&two_repo, "pub fn value() -> Int { 20 }\n");

        update_package_dependencies(&main_path, Some("one"), None)
            .expect("alias-scoped git update should succeed");

        let lockfile = std::fs::read_to_string(dir.path().join("kyokara.lock")).unwrap();
        let lockfile = lockfile.parse::<toml::Value>().unwrap();
        let dependencies = lockfile
            .get("dependencies")
            .and_then(toml::Value::as_table)
            .unwrap();
        let one = dependencies
            .get("one")
            .and_then(toml::Value::as_table)
            .unwrap();
        let two = dependencies
            .get("two")
            .and_then(toml::Value::as_table)
            .unwrap();
        assert!(
            one.get("commit").and_then(toml::Value::as_str) == Some(second_one_rev.as_str()),
            "requested alias should refresh to its new commit, got: {lockfile}"
        );
        assert!(
            two.get("commit").and_then(toml::Value::as_str) == Some(first_two_rev.as_str()),
            "unrequested alias should keep its previous commit, got: {lockfile}"
        );
        assert!(
            two.get("commit").and_then(toml::Value::as_str) != Some(second_two_rev.as_str()),
            "unrequested alias should not refresh, got: {lockfile}"
        );
    }

    #[test]
    fn add_package_dependency_copies_registry_package_and_syncs_lockfile() {
        let dir = tempfile::tempdir().unwrap();
        let src_dir = dir.path().join("src");
        let registry_root = dir.path().join("registry");
        std::fs::create_dir_all(&src_dir).unwrap();
        write_registry_package(
            &registry_root,
            "core/json",
            "1.4.2",
            "pub fn from_registry() -> Int { 35 }\n",
        );

        let main_path = src_dir.join("main.ky");
        std::fs::write(
            dir.path().join("kyokara.toml"),
            "[package]\nname = \"acme/app\"\nedition = \"2026\"\nkind = \"bin\"\n",
        )
        .unwrap();
        std::fs::write(&main_path, "fn main() -> Int { 0 }\n").unwrap();

        add_package_dependency(
            &main_path,
            DependencySource::Registry {
                package: "core/json".to_string(),
                version: "^1.4.0".to_string(),
            },
            "json",
            Some(&registry_root),
        )
        .expect("registry add should succeed");

        let manifest = std::fs::read_to_string(dir.path().join("kyokara.toml")).unwrap();
        let lockfile = std::fs::read_to_string(dir.path().join("kyokara.lock")).unwrap();
        let local_registry_manifest = dir
            .path()
            .join(".kyokara")
            .join("registry")
            .join("packages")
            .join("core/json")
            .join("1.4.2")
            .join("kyokara.toml");
        let manifest_toml = manifest.parse::<toml::Value>().unwrap();
        let manifest_dep = manifest_toml
            .get("dependencies")
            .and_then(toml::Value::as_table)
            .and_then(|deps| deps.get("json"))
            .and_then(toml::Value::as_table)
            .unwrap();

        assert!(
            manifest_dep
                .get("package")
                .and_then(toml::Value::as_str)
                .is_some_and(|value| value == "core/json"),
            "expected registry dependency in manifest, got: {manifest}"
        );
        assert!(
            manifest_dep
                .get("version")
                .and_then(toml::Value::as_str)
                .is_some_and(|value| value == "^1.4.0"),
            "expected registry version requirement in manifest, got: {manifest}"
        );
        assert!(
            lockfile.contains("json = { package = \"core/json\", version = \"1.4.2\" }"),
            "expected exact registry version in lockfile, got: {lockfile}"
        );
        assert!(
            local_registry_manifest.is_file(),
            "expected registry package to be copied locally"
        );
    }

    #[test]
    fn update_package_dependencies_refreshes_registry_lockfile_to_newer_matching_version() {
        let dir = tempfile::tempdir().unwrap();
        let src_dir = dir.path().join("src");
        let registry_root = dir.path().join("registry");
        std::fs::create_dir_all(&src_dir).unwrap();
        write_registry_package(
            &registry_root,
            "core/json",
            "1.4.2",
            "pub fn from_registry() -> Int { 35 }\n",
        );

        let main_path = src_dir.join("main.ky");
        std::fs::write(
            dir.path().join("kyokara.toml"),
            "[package]\nname = \"acme/app\"\nedition = \"2026\"\nkind = \"bin\"\n",
        )
        .unwrap();
        std::fs::write(&main_path, "fn main() -> Int { 0 }\n").unwrap();

        add_package_dependency(
            &main_path,
            DependencySource::Registry {
                package: "core/json".to_string(),
                version: "^1.4.0".to_string(),
            },
            "json",
            Some(&registry_root),
        )
        .expect("registry add should succeed");

        write_registry_package(
            &registry_root,
            "core/json",
            "1.5.0",
            "pub fn from_registry() -> Int { 40 }\n",
        );
        update_package_dependencies(&main_path, None, Some(&registry_root))
            .expect("registry update should succeed");

        let lockfile = std::fs::read_to_string(dir.path().join("kyokara.lock")).unwrap();
        assert!(
            lockfile.contains("json = { package = \"core/json\", version = \"1.5.0\" }"),
            "expected updated registry version in lockfile, got: {lockfile}"
        );
    }

    #[test]
    fn add_package_dependency_supports_transitive_registry_dependencies() {
        let dir = tempfile::tempdir().unwrap();
        let src_dir = dir.path().join("src");
        let registry_root = dir.path().join("registry");
        std::fs::create_dir_all(&src_dir).unwrap();

        write_registry_package(
            &registry_root,
            "core/json",
            "1.2.0",
            "pub fn answer() -> Int { 41 }\n",
        );
        let util_dir = registry_root
            .join("packages")
            .join("core/util")
            .join("1.0.0");
        let util_src = util_dir.join("src");
        std::fs::create_dir_all(&util_src).unwrap();
        std::fs::write(
            util_dir.join("kyokara.toml"),
            "[package]\nname = \"core/util\"\nversion = \"1.0.0\"\nedition = \"2026\"\nkind = \"lib\"\n\n[dependencies]\njson = { package = \"core/json\", version = \"^1.2.0\" }\n",
        )
        .unwrap();
        std::fs::write(
            util_src.join("lib.ky"),
            "import deps.json\npub fn value() -> Int { json.answer() + 1 }\n",
        )
        .unwrap();

        let main_path = src_dir.join("main.ky");
        std::fs::write(
            dir.path().join("kyokara.toml"),
            "[package]\nname = \"acme/app\"\nedition = \"2026\"\nkind = \"bin\"\n",
        )
        .unwrap();
        std::fs::write(
            &main_path,
            "import deps.util\nfn main() -> Int { util.value() }\n",
        )
        .unwrap();

        add_package_dependency(
            &main_path,
            DependencySource::Registry {
                package: "core/util".to_string(),
                version: "^1.0.0".to_string(),
            },
            "util",
            Some(&registry_root),
        )
        .expect("registry add should succeed");

        let result = kyokara_hir::check_project(&main_path);
        let local_json_manifest = dir
            .path()
            .join(".kyokara")
            .join("registry")
            .join("packages")
            .join("core/json")
            .join("1.2.0")
            .join("kyokara.toml");

        assert!(
            result.lowering_diagnostics.is_empty(),
            "expected no lowering diagnostics, got: {:?}",
            result
                .lowering_diagnostics
                .iter()
                .map(|diag| diag.message.as_str())
                .collect::<Vec<_>>()
        );
        assert!(
            result
                .type_checks
                .iter()
                .all(|(_, tc)| tc.diagnostics.is_empty()),
            "expected no type diagnostics, got: {:?}",
            result
                .type_checks
                .iter()
                .flat_map(|(_, tc)| tc.diagnostics.iter().map(|diag| diag.message.clone()))
                .collect::<Vec<_>>()
        );
        assert!(
            local_json_manifest.is_file(),
            "expected transitive registry package to be copied locally"
        );
    }

    #[test]
    fn add_package_dependency_only_copies_selected_registry_version_closure() {
        let dir = tempfile::tempdir().unwrap();
        let src_dir = dir.path().join("src");
        let registry_root = dir.path().join("registry");
        let util_v2_dir = registry_root
            .join("packages")
            .join("core/util")
            .join("2.0.0");
        let util_v2_src = util_v2_dir.join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::create_dir_all(&util_v2_src).unwrap();

        write_registry_package(
            &registry_root,
            "core/util",
            "1.0.0",
            "pub fn value() -> Int { 1 }\n",
        );
        std::fs::write(
            util_v2_dir.join("kyokara.toml"),
            "[package]\nname = \"core/util\"\nversion = \"2.0.0\"\nedition = \"2026\"\nkind = \"lib\"\n[dependencies]\nmissing = { package = \"core/missing\", version = \"=9.9.9\" }\n",
        )
        .unwrap();
        std::fs::write(util_v2_src.join("lib.ky"), "pub fn value() -> Int { 2 }\n").unwrap();

        let main_path = src_dir.join("main.ky");
        std::fs::write(
            dir.path().join("kyokara.toml"),
            "[package]\nname = \"acme/app\"\nedition = \"2026\"\nkind = \"bin\"\n",
        )
        .unwrap();
        std::fs::write(
            &main_path,
            "import deps.util\nfn main() -> Int { util.value() }\n",
        )
        .unwrap();

        add_package_dependency(
            &main_path,
            DependencySource::Registry {
                package: "core/util".to_string(),
                version: "=1.0.0".to_string(),
            },
            "util",
            Some(&registry_root),
        )
        .expect("registry add should succeed for the selected valid version");

        let lockfile = std::fs::read_to_string(dir.path().join("kyokara.lock")).unwrap();
        let local_util_v1_manifest = dir
            .path()
            .join(".kyokara")
            .join("registry")
            .join("packages")
            .join("core/util")
            .join("1.0.0")
            .join("kyokara.toml");
        let local_util_v2_manifest = dir
            .path()
            .join(".kyokara")
            .join("registry")
            .join("packages")
            .join("core/util")
            .join("2.0.0")
            .join("kyokara.toml");

        assert!(
            lockfile.contains("util = { package = \"core/util\", version = \"1.0.0\" }"),
            "expected selected registry version in lockfile, got: {lockfile}"
        );
        assert!(
            local_util_v1_manifest.is_file(),
            "expected selected registry version to be copied locally"
        );
        assert!(
            !local_util_v2_manifest.exists(),
            "unselected registry versions should not be copied into the local store"
        );
    }

    #[test]
    fn add_package_dependency_pins_vendored_registry_transitives_to_selected_versions() {
        let dir = tempfile::tempdir().unwrap();
        let src_dir = dir.path().join("src");
        let registry_root = dir.path().join("registry");
        let stale_json_dir = dir
            .path()
            .join(".kyokara")
            .join("registry")
            .join("packages")
            .join("core/json")
            .join("1.5.0")
            .join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::create_dir_all(&stale_json_dir).unwrap();

        std::fs::write(
            stale_json_dir
                .parent()
                .expect("stale version dir")
                .join("kyokara.toml"),
            "[package]\nname = \"core/json\"\nversion = \"1.5.0\"\nedition = \"2026\"\nkind = \"lib\"\n",
        )
        .unwrap();
        std::fs::write(
            stale_json_dir.join("lib.ky"),
            "pub fn from_registry() -> Int { 15 }\n",
        )
        .unwrap();

        write_registry_package(
            &registry_root,
            "core/json",
            "1.2.0",
            "pub fn from_registry() -> Int { 12 }\n",
        );
        let util_dir = registry_root
            .join("packages")
            .join("core/util")
            .join("1.0.0");
        let util_src = util_dir.join("src");
        std::fs::create_dir_all(&util_src).unwrap();
        std::fs::write(
            util_dir.join("kyokara.toml"),
            "[package]\nname = \"core/util\"\nversion = \"1.0.0\"\nedition = \"2026\"\nkind = \"lib\"\n\n[dependencies]\njson = { package = \"core/json\", version = \"^1.0.0\" }\n",
        )
        .unwrap();
        std::fs::write(
            util_src.join("lib.ky"),
            "import deps.json\npub fn value() -> Int { json.from_registry() }\n",
        )
        .unwrap();

        let main_path = src_dir.join("main.ky");
        std::fs::write(
            dir.path().join("kyokara.toml"),
            "[package]\nname = \"acme/app\"\nedition = \"2026\"\nkind = \"bin\"\n",
        )
        .unwrap();
        std::fs::write(
            &main_path,
            "import deps.util\nfn main() -> Int { util.value() }\n",
        )
        .unwrap();

        add_package_dependency(
            &main_path,
            DependencySource::Registry {
                package: "core/util".to_string(),
                version: "=1.0.0".to_string(),
            },
            "util",
            Some(&registry_root),
        )
        .expect("registry add should succeed");

        let vendored_util_manifest = std::fs::read_to_string(
            dir.path()
                .join(".kyokara")
                .join("registry")
                .join("packages")
                .join("core/util")
                .join("1.0.0")
                .join("kyokara.toml"),
        )
        .unwrap();
        let result = kyokara_eval::run_project(&main_path).expect("project run should succeed");

        let vendored_util_manifest = vendored_util_manifest.parse::<toml::Value>().unwrap();
        let vendored_json_dep = vendored_util_manifest
            .get("dependencies")
            .and_then(toml::Value::as_table)
            .and_then(|deps| deps.get("json"))
            .and_then(toml::Value::as_table)
            .unwrap();
        assert!(
            vendored_json_dep
                .get("version")
                .and_then(toml::Value::as_str)
                .is_some_and(|value| value == "=1.2.0"),
            "expected vendored manifest to pin selected transitive version, got: {vendored_util_manifest}"
        );
        assert_eq!(
            result.value,
            kyokara_eval::value::Value::Int(12),
            "expected runtime to use selected source-registry transitive version"
        );
    }

    #[test]
    fn update_package_dependencies_repins_vendored_registry_transitives() {
        let dir = tempfile::tempdir().unwrap();
        let src_dir = dir.path().join("src");
        let registry_root = dir.path().join("registry");
        let stale_json_dir = dir
            .path()
            .join(".kyokara")
            .join("registry")
            .join("packages")
            .join("core/json")
            .join("1.5.0")
            .join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::create_dir_all(&stale_json_dir).unwrap();

        std::fs::write(
            stale_json_dir
                .parent()
                .expect("stale version dir")
                .join("kyokara.toml"),
            "[package]\nname = \"core/json\"\nversion = \"1.5.0\"\nedition = \"2026\"\nkind = \"lib\"\n",
        )
        .unwrap();
        std::fs::write(
            stale_json_dir.join("lib.ky"),
            "pub fn from_registry() -> Int { 15 }\n",
        )
        .unwrap();

        write_registry_package(
            &registry_root,
            "core/json",
            "1.2.0",
            "pub fn from_registry() -> Int { 12 }\n",
        );
        let util_v1_dir = registry_root
            .join("packages")
            .join("core/util")
            .join("1.0.0");
        let util_v1_src = util_v1_dir.join("src");
        std::fs::create_dir_all(&util_v1_src).unwrap();
        std::fs::write(
            util_v1_dir.join("kyokara.toml"),
            "[package]\nname = \"core/util\"\nversion = \"1.0.0\"\nedition = \"2026\"\nkind = \"lib\"\n\n[dependencies]\njson = { package = \"core/json\", version = \"^1.0.0\" }\n",
        )
        .unwrap();
        std::fs::write(
            util_v1_src.join("lib.ky"),
            "import deps.json\npub fn value() -> Int { json.from_registry() }\n",
        )
        .unwrap();

        let main_path = src_dir.join("main.ky");
        std::fs::write(
            dir.path().join("kyokara.toml"),
            "[package]\nname = \"acme/app\"\nedition = \"2026\"\nkind = \"bin\"\n",
        )
        .unwrap();
        std::fs::write(
            &main_path,
            "import deps.util\nfn main() -> Int { util.value() }\n",
        )
        .unwrap();

        add_package_dependency(
            &main_path,
            DependencySource::Registry {
                package: "core/util".to_string(),
                version: "^1.0.0".to_string(),
            },
            "util",
            Some(&registry_root),
        )
        .expect("initial registry add should succeed");

        write_registry_package(
            &registry_root,
            "core/json",
            "1.3.0",
            "pub fn from_registry() -> Int { 13 }\n",
        );
        let util_v2_dir = registry_root
            .join("packages")
            .join("core/util")
            .join("1.1.0");
        let util_v2_src = util_v2_dir.join("src");
        std::fs::create_dir_all(&util_v2_src).unwrap();
        std::fs::write(
            util_v2_dir.join("kyokara.toml"),
            "[package]\nname = \"core/util\"\nversion = \"1.1.0\"\nedition = \"2026\"\nkind = \"lib\"\n\n[dependencies]\njson = { package = \"core/json\", version = \"^1.0.0\" }\n",
        )
        .unwrap();
        std::fs::write(
            util_v2_src.join("lib.ky"),
            "import deps.json\npub fn value() -> Int { json.from_registry() + 100 }\n",
        )
        .unwrap();

        update_package_dependencies(&main_path, None, Some(&registry_root))
            .expect("registry update should succeed");

        let vendored_util_manifest = std::fs::read_to_string(
            dir.path()
                .join(".kyokara")
                .join("registry")
                .join("packages")
                .join("core/util")
                .join("1.1.0")
                .join("kyokara.toml"),
        )
        .unwrap();
        let result = kyokara_eval::run_project(&main_path).expect("project run should succeed");

        let vendored_util_manifest = vendored_util_manifest.parse::<toml::Value>().unwrap();
        let vendored_json_dep = vendored_util_manifest
            .get("dependencies")
            .and_then(toml::Value::as_table)
            .and_then(|deps| deps.get("json"))
            .and_then(toml::Value::as_table)
            .unwrap();
        assert!(
            vendored_json_dep
                .get("version")
                .and_then(toml::Value::as_str)
                .is_some_and(|value| value == "=1.3.0"),
            "expected updated vendored manifest to pin selected transitive version, got: {vendored_util_manifest}"
        );
        assert_eq!(
            result.value,
            kyokara_eval::value::Value::Int(113),
            "expected runtime to use updated selected source-registry transitive version"
        );
    }

    #[test]
    fn add_package_dependency_rejects_non_lib_registry_packages() {
        let dir = tempfile::tempdir().unwrap();
        let src_dir = dir.path().join("src");
        let registry_root = dir.path().join("registry");
        let bad_dir = registry_root
            .join("packages")
            .join("core/bad")
            .join("1.0.0");
        let bad_src = bad_dir.join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::create_dir_all(&bad_src).unwrap();

        std::fs::write(
            bad_dir.join("kyokara.toml"),
            "[package]\nname = \"core/bad\"\nversion = \"1.0.0\"\nedition = \"2026\"\nkind = \"bin\"\n",
        )
        .unwrap();
        std::fs::write(bad_src.join("main.ky"), "fn main() -> Int { 1 }\n").unwrap();

        let main_path = src_dir.join("main.ky");
        std::fs::write(
            dir.path().join("kyokara.toml"),
            "[package]\nname = \"acme/app\"\nedition = \"2026\"\nkind = \"bin\"\n",
        )
        .unwrap();
        std::fs::write(&main_path, "fn main() -> Int { 0 }\n").unwrap();

        let err = add_package_dependency(
            &main_path,
            DependencySource::Registry {
                package: "core/bad".to_string(),
                version: "^1.0.0".to_string(),
            },
            "bad",
            Some(&registry_root),
        )
        .expect_err("registry add should reject non-lib dependency packages");

        let manifest = std::fs::read_to_string(dir.path().join("kyokara.toml")).unwrap();
        let lockfile_path = dir.path().join("kyokara.lock");

        assert!(
            err.contains("dependencies must be lib packages"),
            "expected non-lib dependency rejection, got: {err}"
        );
        assert!(
            !manifest.contains("core/bad"),
            "manifest should stay unchanged on failed add: {manifest}"
        );
        assert!(
            !lockfile_path.exists(),
            "failed add should not write a lockfile"
        );
    }

    #[test]
    fn add_package_dependency_preserves_existing_lockfile_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let src_dir = dir.path().join("src");
        let good_dir = dir.path().join("good");
        let good_src = good_dir.join("src");
        let registry_root = dir.path().join("registry");
        let bad_dir = registry_root
            .join("packages")
            .join("core/bad")
            .join("1.0.0");
        let bad_src = bad_dir.join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::create_dir_all(&good_src).unwrap();
        std::fs::create_dir_all(&bad_src).unwrap();

        std::fs::write(
            dir.path().join("kyokara.toml"),
            "[package]\nname = \"acme/app\"\nedition = \"2026\"\nkind = \"bin\"\n[dependencies]\ngood = { path = \"good\" }\n",
        )
        .unwrap();
        std::fs::write(
            good_dir.join("kyokara.toml"),
            "[package]\nname = \"acme/good\"\nedition = \"2026\"\nkind = \"lib\"\n",
        )
        .unwrap();
        std::fs::write(good_src.join("lib.ky"), "pub fn value() -> Int { 1 }\n").unwrap();

        let main_path = src_dir.join("main.ky");
        std::fs::write(
            &main_path,
            "import deps.good\nfn main() -> Int { good.value() }\n",
        )
        .unwrap();

        sync_project_lockfile_if_needed(&main_path, true)
            .expect("initial lockfile sync should succeed");
        let lockfile_path = dir.path().join("kyokara.lock");
        let lockfile_before = std::fs::read_to_string(&lockfile_path).unwrap();

        std::fs::write(
            bad_dir.join("kyokara.toml"),
            "[package]\nname = \"core/bad\"\nversion = \"1.0.0\"\nedition = \"2026\"\nkind = \"bin\"\n",
        )
        .unwrap();
        std::fs::write(bad_src.join("main.ky"), "fn main() -> Int { 1 }\n").unwrap();

        let err = add_package_dependency(
            &main_path,
            DependencySource::Registry {
                package: "core/bad".to_string(),
                version: "^1.0.0".to_string(),
            },
            "bad",
            Some(&registry_root),
        )
        .expect_err("registry add should reject non-lib dependency packages");

        let manifest = std::fs::read_to_string(dir.path().join("kyokara.toml")).unwrap();
        let lockfile_after = std::fs::read_to_string(&lockfile_path)
            .expect("existing lockfile should survive failed add");

        assert!(
            err.contains("dependencies must be lib packages"),
            "expected non-lib dependency rejection, got: {err}"
        );
        assert!(
            !manifest.contains("core/bad"),
            "manifest should stay unchanged on failed add: {manifest}"
        );
        assert_eq!(
            lockfile_before, lockfile_after,
            "failed add should preserve the prior lockfile"
        );
    }

    #[test]
    fn update_package_dependencies_preserves_existing_lockfile_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let src_dir = dir.path().join("src");
        let registry_root = dir.path().join("registry");
        std::fs::create_dir_all(&src_dir).unwrap();

        write_registry_package(
            &registry_root,
            "core/json",
            "1.0.0",
            "pub fn from_registry() -> Int { 35 }\n",
        );

        let main_path = src_dir.join("main.ky");
        std::fs::write(
            dir.path().join("kyokara.toml"),
            "[package]\nname = \"acme/app\"\nedition = \"2026\"\nkind = \"bin\"\n",
        )
        .unwrap();
        std::fs::write(
            &main_path,
            "import deps.json\nfn main() -> Int { json.from_registry() }\n",
        )
        .unwrap();

        add_package_dependency(
            &main_path,
            DependencySource::Registry {
                package: "core/json".to_string(),
                version: "^1.0.0".to_string(),
            },
            "json",
            Some(&registry_root),
        )
        .expect("initial registry add should succeed");

        let lockfile_path = dir.path().join("kyokara.lock");
        let lockfile_before = std::fs::read_to_string(&lockfile_path).unwrap();

        let bad_dir = registry_root
            .join("packages")
            .join("core/json")
            .join("1.1.0");
        let bad_src = bad_dir.join("src");
        std::fs::create_dir_all(&bad_src).unwrap();
        std::fs::write(
            bad_dir.join("kyokara.toml"),
            "[package]\nname = \"core/json\"\nversion = \"1.1.0\"\nedition = \"2026\"\nkind = \"bin\"\n",
        )
        .unwrap();
        std::fs::write(bad_src.join("main.ky"), "fn main() -> Int { 1 }\n").unwrap();

        let err = update_package_dependencies(&main_path, None, Some(&registry_root))
            .expect_err("registry update should reject non-lib dependency packages");
        let lockfile_after = std::fs::read_to_string(&lockfile_path)
            .expect("existing lockfile should survive failed update");

        assert!(
            err.contains("dependencies must be lib packages"),
            "expected non-lib dependency rejection, got: {err}"
        );
        assert_eq!(
            lockfile_before, lockfile_after,
            "failed update should preserve the prior lockfile"
        );
    }

    #[test]
    fn publish_package_to_registry_copies_lib_package_and_rejects_path_dependencies() {
        let dir = tempfile::tempdir().unwrap();
        let registry_root = dir.path().join("registry");
        let src_dir = dir.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::create_dir_all(dir.path().join("target").join("debug")).unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();

        let lib_path = src_dir.join("lib.ky");
        std::fs::write(
            dir.path().join("kyokara.toml"),
            "[package]\nname = \"acme/json\"\nversion = \"1.2.0\"\nedition = \"2026\"\nkind = \"lib\"\n",
        )
        .unwrap();
        std::fs::write(&lib_path, "pub fn answer() -> Int { 42 }\n").unwrap();
        std::fs::write(
            dir.path().join("target").join("debug").join("junk.txt"),
            "artifact\n",
        )
        .unwrap();
        std::fs::write(dir.path().join(".DS_Store"), "note\n").unwrap();
        std::fs::write(
            dir.path().join(".git").join("HEAD"),
            "ref: refs/heads/main\n",
        )
        .unwrap();

        publish_package_to_registry(&lib_path, &registry_root).expect("publish should succeed");

        let published_root = registry_root
            .join("packages")
            .join("acme/json")
            .join("1.2.0");
        let published_manifest = published_root.join("kyokara.toml");
        let published_lib = published_root.join("src").join("lib.ky");
        assert!(published_manifest.is_file(), "expected published manifest");
        assert!(published_lib.is_file(), "expected published source");
        assert!(
            !published_root.join("target").exists(),
            "target directory should not be published"
        );
        assert!(
            !published_root.join(".DS_Store").exists(),
            "dotfiles should not be published"
        );
        assert!(
            !published_root.join(".git").exists(),
            "git metadata should not be published"
        );

        std::fs::write(
            dir.path().join("kyokara.toml"),
            "[package]\nname = \"acme/json\"\nversion = \"1.2.0\"\nedition = \"2026\"\nkind = \"lib\"\n\n[dependencies]\nutil = { path = \"../util\" }\n",
        )
        .unwrap();
        let err = publish_package_to_registry(&lib_path, &registry_root)
            .expect_err("path dependencies should block publish");
        assert!(
            err.contains("path dependencies"),
            "expected path dependency rejection, got: {err}"
        );
    }

    #[test]
    fn check_emit_typed_ast_requires_json_format() {
        let err = validate_check_emit_format("human", true).expect_err("human format must fail");
        assert_eq!(err, "`--emit typed-ast` requires `--format json`");
    }

    #[test]
    fn check_emit_typed_ast_allows_json_format() {
        validate_check_emit_format("json", true).expect("json format should be accepted");
    }

    #[test]
    fn clap_parses_check_emit_typed_ast() {
        let cli = Cli::try_parse_from([
            "kyokara",
            "check",
            "main.ky",
            "--format",
            "json",
            "--emit",
            "typed-ast",
        ])
        .expect("check args with --emit typed-ast should parse");

        match cli.command {
            Command::Check {
                emit: Some(value),
                format,
                ..
            } => {
                assert_eq!(value, "typed-ast");
                assert_eq!(format, "json");
            }
            _ => panic!("expected check command with emit"),
        }
    }
}
