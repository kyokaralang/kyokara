//! `kyokara-cli` — The Kyokara compiler CLI.
//!
//! Commands:
//! - `kyokara check <file>` — type-check a `.ky` file (v0.0)
//! - `kyokara run <file>` — interpret a `.ky` file (v0.1)
//! - `kyokara fmt <file>` — format a `.ky` file (v0.1)
//! - `kyokara replay <file>` — replay execution trace (v0.2)

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
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Check { file, format } => {
            let source = match std::fs::read_to_string(&file) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("error: cannot read `{file}`: {e}");
                    std::process::exit(1);
                }
            };

            let output = kyokara_api::check(&source, &file);

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
