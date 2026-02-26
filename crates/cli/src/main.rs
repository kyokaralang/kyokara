//! `kyokara-cli` — The Kyokara compiler CLI.
//!
//! Commands:
//! - `kyokara check <file>` — type-check a `.ky` file (v0.0)
//! - `kyokara run <file>` — interpret a `.ky` file (v0.1)
//! - `kyokara fmt <file>` — format a `.ky` file (v0.1)
//! - `kyokara refactor <file>` — apply semantic refactors (v0.2)
//! - `kyokara lsp` — start the Language Server Protocol server (v0.2)
//! - `kyokara replay <file>` — replay execution trace (planned v0.3)

use clap::{Parser, Subcommand};

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

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Check {
            file,
            format,
            project,
        } => {
            let path = std::path::Path::new(&file);
            let is_multi_file = should_use_project_mode(path, project);

            let output = if is_multi_file {
                kyokara_api::check_project(path)
            } else {
                let source = match std::fs::read_to_string(&file) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("error: cannot read `{file}`: {e}");
                        std::process::exit(1);
                    }
                };
                kyokara_api::check(&source, &file)
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
        } => {
            let path = std::path::Path::new(&file);
            let is_multi_file = should_use_project_mode(path, project);

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

            if is_multi_file {
                match kyokara_eval::run_project_with_manifest(path, manifest) {
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
                let source = match std::fs::read_to_string(&file) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("error: cannot read `{file}`: {e}");
                        std::process::exit(1);
                    }
                };

                match kyokara_eval::run_with_manifest(&source, manifest) {
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
                        target_file: None,
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
                // Use patched sources from the transaction when available.
                if let Some(patched) = &output.patched_sources {
                    for ps in patched {
                        if let Err(e) = std::fs::write(&ps.file, &ps.source) {
                            eprintln!("error: cannot write `{}`: {e}", ps.file);
                            std::process::exit(1);
                        }
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
/// Auto-detection requires BOTH:
/// 1. The entry file is named `main.ky` (the convention-based entry point).
/// 2. There are sibling `.ky` files in the same directory.
///
/// Use the `--project` flag to force project mode for non-`main.ky` entry files.
fn should_use_project_mode(path: &std::path::Path, force_project: bool) -> bool {
    if force_project {
        // --project flag forces project mode if there are siblings.
        return path.is_file()
            && path
                .parent()
                .is_some_and(|dir| has_sibling_ky_files(path, dir));
    }

    // Auto-detect: only if entry is main.ky and has siblings.
    path.is_file()
        && path.file_name().is_some_and(|name| name == "main.ky")
        && path
            .parent()
            .is_some_and(|dir| has_sibling_ky_files(path, dir))
}

#[cfg(test)]
mod tests {
    use super::*;

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

        // --project but no siblings → NOT project mode (nothing to discover).
        assert!(
            !should_use_project_mode(&main_path, true),
            "--project with no siblings should not be project mode"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
