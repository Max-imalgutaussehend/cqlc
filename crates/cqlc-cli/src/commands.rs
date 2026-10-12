//! `check` and `generate` implementations.

use crate::config::Config;
use cqlc_codegen::{Options, generate_rust};
use cqlc_core::diagnostic::Diagnostic;
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
            Ok(Input::new(p.display().to_string(), text))
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

/// How diagnostics are printed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageFormat {
    /// rustc-style text on stderr.
    Human,
    /// One JSON object per line on stdout.
    Json,
}

fn json_line(c: &Compilation, d: &Diagnostic) -> String {
    let file = c.sources.get(d.span.file);
    let (line, column) = file.line_col(d.span.start as usize);
    let (end_line, end_column) = file.line_col(d.span.end as usize);
    serde_json::json!({
        "code": d.code.as_str(),
        "severity": if d.is_error() { "error" } else { "warning" },
        "message": d.message,
        "help": d.help,
        "file": file.name,
        "line": line,
        "column": column,
        "end_line": end_line,
        "end_column": end_column,
    })
    .to_string()
}

fn report(c: &Compilation, diags: &[Diagnostic], format: MessageFormat) -> (usize, usize) {
    let mut errors = 0;
    let mut warnings = 0;
    for d in diags {
        match format {
            MessageFormat::Human => eprintln!("{}", d.render(&c.sources)),
            MessageFormat::Json => println!("{}", json_line(c, d)),
        }
        if d.is_error() {
            errors += 1;
        } else {
            warnings += 1;
        }
    }
    (errors, warnings)
}

fn summary(errors: usize, warnings: usize) {
    if errors > 0 || warnings > 0 {
        eprintln!("{errors} error(s), {warnings} warning(s)");
    }
}

/// Progress messages; in JSON mode stdout carries only diagnostics.
fn status(format: MessageFormat, msg: String) {
    match format {
        MessageFormat::Human => println!("{msg}"),
        MessageFormat::Json => eprintln!("{msg}"),
    }
}

/// Writes through a temporary file in the same directory and renames it, so an
/// interrupted run never leaves a half-written module behind.
fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let tmp = path.with_file_name(format!(".{name}.cqlc-tmp"));
    let result = std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn io_failure(msg: String) -> ExitCode {
    eprintln!("error: {msg}");
    ExitCode::from(2)
}

pub fn check(config_path: &Path, deny_warnings: bool, format: MessageFormat) -> ExitCode {
    let config = match Config::load(config_path) {
        Ok(c) => c,
        Err(e) => return io_failure(e),
    };
    let c = match run_front_end(&config) {
        Ok(c) => c,
        Err(e) => return io_failure(e),
    };
    let (errors, warnings) = report(&c, &c.diagnostics, format);
    summary(errors, warnings);
    if errors > 0 || (deny_warnings && warnings > 0) {
        return ExitCode::FAILURE;
    }
    if format == MessageFormat::Human {
        println!("ok: {} queries checked", c.ir.queries.len());
    }
    ExitCode::SUCCESS
}

pub fn generate(
    config_path: &Path,
    check_only: bool,
    deny_warnings: bool,
    format: MessageFormat,
) -> ExitCode {
    let config = match Config::load(config_path) {
        Ok(c) => c,
        Err(e) => return io_failure(e),
    };
    let c = match run_front_end(&config) {
        Ok(c) => c,
        Err(e) => return io_failure(e),
    };
    let (mut errors, warnings) = report(&c, &c.diagnostics, format);
    let mut code = None;
    if errors == 0 {
        let opts = Options::new()
            .time_library(config.time_library)
            .streams(config.streams);
        match generate_rust(&c.ir, &opts) {
            Ok(text) => code = Some(text),
            Err(diags) => errors += report(&c, &diags, format).0,
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
            status(format, format!("{} is up to date", config.out.display()));
            return ExitCode::SUCCESS;
        }
        eprintln!(
            "error: {} is out of date; run `cqlc generate`",
            config.out.display()
        );
        return ExitCode::FAILURE;
    }
    if existing.as_deref() == Some(text.as_str()) {
        status(
            format,
            format!("{} is already up to date", config.out.display()),
        );
        return ExitCode::SUCCESS;
    }
    if let Some(dir) = config.out.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        return io_failure(format!("cannot create {}: {e}", dir.display()));
    }
    if let Err(e) = write_atomic(&config.out, &text) {
        return io_failure(format!("cannot write {}: {e}", config.out.display()));
    }
    status(
        format,
        format!(
            "wrote {} ({} queries)",
            config.out.display(),
            c.ir.queries.len()
        ),
    );
    ExitCode::SUCCESS
}

const INIT_CONFIG: &str = "\
# cqlc configuration. See https://github.com/Max-imalgutaussehend/cqlc
version = 1

# Directories (or single files) with CREATE statements and annotated queries.
schema = \"schema\"
queries = \"queries\"

[rust]
# Where the generated module is written.
out = \"src/queries.rs\"
# \"native\" uses the driver's own time types, \"chrono\" needs scylla's `chrono-04` feature.
time_library = \"native\"
# Also generate `stream_*` functions for :many queries (needs the `futures` crate).
streams = true
";

const INIT_SCHEMA: &str = "\
CREATE TABLE users (
    user_id uuid PRIMARY KEY,
    username text,
    email text
);
";

const INIT_QUERIES: &str = "\
-- name: GetUser :one
SELECT user_id, username, email
FROM users
WHERE user_id = ?;

-- name: CreateUser :exec
INSERT INTO users (user_id, username, email)
VALUES (?, ?, ?);
";

/// Writes a starter `cqlc.toml`, schema and queries, never overwriting anything.
pub fn init(config_path: &Path) -> ExitCode {
    let base = config_path.parent().unwrap_or(Path::new("."));
    let files = [
        (config_path.to_path_buf(), INIT_CONFIG),
        (base.join("schema/schema.cql"), INIT_SCHEMA),
        (base.join("queries/users.cql"), INIT_QUERIES),
    ];
    if let Some((existing, _)) = files.iter().find(|(p, _)| p.exists()) {
        return io_failure(format!(
            "{} already exists; not overwriting",
            existing.display()
        ));
    }
    for (path, content) in &files {
        if let Some(dir) = path.parent()
            && let Err(e) = std::fs::create_dir_all(dir)
        {
            return io_failure(format!("cannot create {}: {e}", dir.display()));
        }
        if let Err(e) = std::fs::write(path, content) {
            return io_failure(format!("cannot write {}: {e}", path.display()));
        }
        println!("created {}", path.display());
    }
    println!("next: edit the files, then run `cqlc generate`");
    ExitCode::SUCCESS
}
