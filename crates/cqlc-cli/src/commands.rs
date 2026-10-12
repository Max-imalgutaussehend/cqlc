//! `check` and `generate` implementations.

use crate::config::Config;
use cqlc_codegen::{Options, generate_rust};
use cqlc_core::diagnostic::{Diagnostic, Severity};
use cqlc_core::{Compilation, Input, compile};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Every `*.cql` file of a directory (sorted by name) or the file itself.
fn collect(paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for p in paths {
        if p.is_dir() {
            let mut files: Vec<PathBuf> = std::fs::read_dir(p)
                .map_err(|e| format!("cannot read {}: {e}", p.display()))?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|f| f.extension().is_some_and(|x| x == "cql"))
                .collect();
            files.sort();
            out.extend(files);
        } else if p.is_file() {
            out.push(p.clone());
        } else {
            return Err(format!("path {} does not exist", p.display()));
        }
    }
    Ok(out)
}

fn read_inputs(paths: &[PathBuf]) -> Result<Vec<Input>, String> {
    collect(paths)?
        .into_iter()
        .map(|p| {
            let text = std::fs::read_to_string(&p)
                .map_err(|e| format!("cannot read {}: {e}", p.display()))?;
            Ok(Input {
                name: p.display().to_string(),
                text,
            })
        })
        .collect()
}

fn run_front_end(config: &Config) -> Result<Compilation, String> {
    let schema = read_inputs(&config.schema)?;
    let queries = read_inputs(&config.queries)?;
    if schema.is_empty() {
        return Err("no .cql files found in the schema paths".into());
    }
    Ok(compile(&schema, &queries))
}

fn report(c: &Compilation, diags: &[Diagnostic]) -> (usize, usize) {
    let mut errors = 0;
    let mut warnings = 0;
    for d in diags {
        eprintln!("{}", d.render(&c.sources));
        match d.severity {
            Severity::Error => errors += 1,
            Severity::Warning => warnings += 1,
        }
    }
    (errors, warnings)
}

fn summary(errors: usize, warnings: usize) {
    if errors > 0 || warnings > 0 {
        eprintln!("{errors} error(s), {warnings} warning(s)");
    }
}

fn io_failure(msg: String) -> ExitCode {
    eprintln!("error: {msg}");
    ExitCode::from(2)
}

pub fn check(config_path: &Path, deny_warnings: bool) -> ExitCode {
    let config = match Config::load(config_path) {
        Ok(c) => c,
        Err(e) => return io_failure(e),
    };
    let c = match run_front_end(&config) {
        Ok(c) => c,
        Err(e) => return io_failure(e),
    };
    let (errors, warnings) = report(&c, &c.diagnostics);
    summary(errors, warnings);
    if errors > 0 || (deny_warnings && warnings > 0) {
        return ExitCode::FAILURE;
    }
    println!("ok: {} queries checked", c.ir.queries.len());
    ExitCode::SUCCESS
}

pub fn generate(config_path: &Path, check_only: bool, deny_warnings: bool) -> ExitCode {
    let config = match Config::load(config_path) {
        Ok(c) => c,
        Err(e) => return io_failure(e),
    };
    let c = match run_front_end(&config) {
        Ok(c) => c,
        Err(e) => return io_failure(e),
    };
    let (mut errors, warnings) = report(&c, &c.diagnostics);
    let mut code = None;
    if errors == 0 {
        let opts = Options {
            time_library: config.time_library,
            streams: config.streams,
        };
        match generate_rust(&c.ir, &opts) {
            Ok(text) => code = Some(text),
            Err(diags) => errors += report(&c, &diags).0,
        }
    }
    summary(errors, warnings);
    let Some(text) = code else {
        return ExitCode::FAILURE;
    };
    if deny_warnings && warnings > 0 {
        return ExitCode::FAILURE;
    }
    let existing = std::fs::read_to_string(&config.out).ok();
    if check_only {
        if existing.as_deref() == Some(text.as_str()) {
            println!("{} is up to date", config.out.display());
            return ExitCode::SUCCESS;
        }
        eprintln!(
            "error: {} is out of date; run `cqlc generate`",
            config.out.display()
        );
        return ExitCode::FAILURE;
    }
    if existing.as_deref() == Some(text.as_str()) {
        println!("{} is already up to date", config.out.display());
        return ExitCode::SUCCESS;
    }
    if let Some(dir) = config.out.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        return io_failure(format!("cannot create {}: {e}", dir.display()));
    }
    if let Err(e) = std::fs::write(&config.out, text) {
        return io_failure(format!("cannot write {}: {e}", config.out.display()));
    }
    println!(
        "wrote {} ({} queries)",
        config.out.display(),
        c.ir.queries.len()
    );
    ExitCode::SUCCESS
}
