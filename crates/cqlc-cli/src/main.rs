//! cqlc: static CQL analyzer and Rust code generator.

mod commands;
mod config;

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "cqlc", version, about)]
struct Cli {
    /// Path to the configuration file.
    #[arg(short, long, global = true, default_value = "cqlc.toml")]
    config: PathBuf,
    /// Treat warnings as errors.
    #[arg(long, global = true)]
    deny_warnings: bool,
    /// How diagnostics are printed: `human` (stderr) or `json` (one object per line on stdout).
    #[arg(long, global = true, value_enum, default_value_t = Format::Human)]
    message_format: Format,
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Format {
    Human,
    Json,
}

impl From<Format> for commands::MessageFormat {
    fn from(f: Format) -> Self {
        match f {
            Format::Human => commands::MessageFormat::Human,
            Format::Json => commands::MessageFormat::Json,
        }
    }
}

#[derive(Subcommand)]
enum Command {
    /// Create a starter cqlc.toml with an example schema and queries.
    Init,
    /// Analyze schema and queries without writing any code.
    Check,
    /// Generate the Rust module.
    Generate {
        /// Do not write; fail if the generated file differs from the output.
        #[arg(long)]
        check: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Init => commands::init(&cli.config),
        Command::Check => {
            commands::check(&cli.config, cli.deny_warnings, cli.message_format.into())
        }
        Command::Generate { check } => commands::generate(
            &cli.config,
            check,
            cli.deny_warnings,
            cli.message_format.into(),
        ),
    }
}
