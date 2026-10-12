//! `cqlc.toml` loading.

use cqlc_codegen::TimeLibrary;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    fn into_vec(self) -> Vec<String> {
        match self {
            OneOrMany::One(s) => vec![s],
            OneOrMany::Many(v) => v,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    schema: OneOrMany,
    queries: OneOrMany,
    rust: RawRust,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRust {
    out: String,
    #[serde(default)]
    time_library: RawTimeLibrary,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RawTimeLibrary {
    #[default]
    Native,
    Chrono,
}

/// Configuration with all paths resolved relative to the config file.
#[derive(Debug)]
pub struct Config {
    pub schema: Vec<PathBuf>,
    pub queries: Vec<PathBuf>,
    pub out: PathBuf,
    pub time_library: TimeLibrary,
}

impl Config {
    pub fn load(path: &Path) -> Result<Config, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let raw: RawConfig =
            toml::from_str(&text).map_err(|e| format!("invalid {}: {e}", path.display()))?;
        let base = path.parent().unwrap_or(Path::new("."));
        let resolve = |v: Vec<String>| v.into_iter().map(|p| base.join(p)).collect();
        Ok(Config {
            schema: resolve(raw.schema.into_vec()),
            queries: resolve(raw.queries.into_vec()),
            out: base.join(raw.rust.out),
            time_library: match raw.rust.time_library {
                RawTimeLibrary::Native => TimeLibrary::Native,
                RawTimeLibrary::Chrono => TimeLibrary::Chrono,
            },
        })
    }
}
