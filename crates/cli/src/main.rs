//! `kyokara-cli` — The Kyokara compiler CLI.
//!
//! Commands:
//! - `kyokara check <file>` — type-check a `.ky` file (v0.0)
//! - `kyokara run <file>` — interpret a `.ky` file (v0.1)
//! - `kyokara replay <file>` — replay execution trace (v0.2)
//! - `kyokara fmt <file>` — format a `.ky` file (v0.3)

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
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Check { file } => {
            eprintln!("kyokara check: not yet implemented (file: {file})");
            std::process::exit(1);
        }
    }
}
