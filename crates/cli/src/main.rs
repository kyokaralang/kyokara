//! `kyokara-cli` — The Kyokara compiler CLI.
//!
//! Commands:
//! - `kyokara check <file>` — type-check a `.ky` file (v0.0)
//! - `kyokara run <file>` — interpret a `.ky` file (v0.1)
//! - `kyokara fmt <file>` — format a `.ky` file (v0.1)
//! - `kyokara refactor <file>` — apply semantic refactors (v0.2)
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
    },
    /// Run a Kyokara source file.
    Run {
        /// Path to the .ky source file.
        file: String,
    },
    /// Format a Kyokara source file.
    Fmt {
        /// Path to the .ky source file.
        file: String,
        /// Check formatting without writing. Exits 1 if not formatted.
        #[arg(long)]
        check: bool,
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
        /// Apply edits to disk instead of printing JSON.
        #[arg(long)]
        apply: bool,
        /// Skip verification and apply edits even if they introduce errors.
        #[arg(long)]
        force: bool,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Check { file, format } => {
            let path = std::path::Path::new(&file);

            // Check if there are sibling .ky files (multi-file project).
            let is_multi_file = path.is_file()
                && path
                    .parent()
                    .is_some_and(|dir| has_sibling_ky_files(path, dir));

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
        Command::Run { file } => {
            let path = std::path::Path::new(&file);

            // Check if there are sibling .ky files (multi-file project).
            let is_multi_file = path.is_file()
                && path
                    .parent()
                    .is_some_and(|dir| has_sibling_ky_files(path, dir));

            if is_multi_file {
                // Multi-file project: use run_project.
                match kyokara_eval::run_project(path) {
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
                // Single file: use existing run.
                let source = match std::fs::read_to_string(&file) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("error: cannot read `{file}`: {e}");
                        std::process::exit(1);
                    }
                };

                match kyokara_eval::run(&source) {
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
        Command::Refactor {
            file,
            action,
            symbol,
            new_name,
            kind,
            offset,
            apply,
            force,
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
                    }
                }
                "add-missing-match-cases" => {
                    let off = offset.unwrap_or_else(|| {
                        eprintln!("error: --offset is required for add-missing-match-cases");
                        std::process::exit(1);
                    });
                    kyokara_refactor::RefactorAction::AddMissingMatchCases { offset: off }
                }
                "add-missing-capability" => {
                    let off = offset.unwrap_or_else(|| {
                        eprintln!("error: --offset is required for add-missing-capability");
                        std::process::exit(1);
                    });
                    kyokara_refactor::RefactorAction::AddMissingCapability { offset: off }
                }
                other => {
                    eprintln!("error: unknown refactor action `{other}`");
                    std::process::exit(1);
                }
            };

            let path = std::path::Path::new(&file);
            let is_multi_file = path.is_file()
                && path
                    .parent()
                    .is_some_and(|dir| has_sibling_ky_files(path, dir));

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
