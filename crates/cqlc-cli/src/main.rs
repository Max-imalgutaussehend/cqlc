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
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
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
        Command::Check => commands::check(&cli.config, cli.deny_warnings),
        Command::Generate { check } => commands::generate(&cli.config, check, cli.deny_warnings),
    }
}
