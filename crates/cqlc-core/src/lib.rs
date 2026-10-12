//! Pure CQL front end: lexer, parser, schema resolver, type checker and
//! typed IR. No I/O and no database access.
//!
//! # Stability
//!
//! The supported API is [`compile`] with its [`Input`] and [`Compilation`],
//! and the data it returns: [`ir`], [`schema`], [`types`], [`diagnostic`] and
//! [`span`]. Open enums and structs of those modules are `#[non_exhaustive]`,
//! so adding variants or fields is not a breaking change.
//!
//! The modules `ast`, `lexer`, `parser`, `resolver` and `checker` are the
//! compiler's internals. They are public for testing and tooling, hidden from
//! the documentation and **not covered by semver**.
//!
//! ```
//! use cqlc_core::{compile, Input};
//!
//! let c = compile(
//!     &[Input::new("schema.cql", "CREATE TABLE t (k uuid PRIMARY KEY, v text);")],
//!     &[Input::new("q.cql", "-- name: Get :one\nSELECT v FROM t WHERE k = ?;")],
//! );
//! assert!(c.diagnostics.is_empty());
//! assert_eq!(c.ir.queries[0].name, "Get");
//! ```

#![warn(missing_docs)]

#[doc(hidden)]
pub mod ast;
#[doc(hidden)]
pub mod checker;
pub mod diagnostic;
pub mod ir;
#[doc(hidden)]
pub mod lexer;
#[doc(hidden)]
pub mod parser;
#[doc(hidden)]
pub mod resolver;
pub mod schema;
pub mod span;
pub mod types;

use diagnostic::Diagnostic;
use span::SourceMap;

/// One input file for [`compile`].
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Input {
    /// Name shown in diagnostics, usually the file path.
    pub name: String,
    /// The file contents.
    pub text: String,
}

impl Input {
    /// Creates an input from a display name and the file contents.
    pub fn new(name: impl Into<String>, text: impl Into<String>) -> Self {
        Input {
            name: name.into(),
            text: text.into(),
        }
    }
}

/// Result of a full front-end run.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Compilation {
    /// All source files, needed to render diagnostics.
    pub sources: SourceMap,
    /// The resolved schema.
    pub schema: schema::Schema,
    /// Typed queries; queries with errors are not included.
    pub ir: ir::Ir,
    /// Errors and warnings, in file order.
    pub diagnostics: Vec<Diagnostic>,
}

impl Compilation {
    /// Whether any diagnostic is an error.
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(Diagnostic::is_error)
    }
}

/// Runs lexer, parser, resolver and checker over schema and query files.
/// File order determines output order; callers should sort for determinism.
pub fn compile(schema_files: &[Input], query_files: &[Input]) -> Compilation {
    let mut sources = SourceMap::default();
    let mut diagnostics = Vec::new();
    let mut ddl = Vec::new();
    for f in schema_files {
        let id = sources.add(&f.name, &f.text);
        let (stmts, mut d) = parser::parse_schema(id, &f.text);
        ddl.extend(stmts);
        diagnostics.append(&mut d);
    }
    let (schema, mut d) = resolver::build_schema(&ddl);
    diagnostics.append(&mut d);
    let mut queries = Vec::new();
    for f in query_files {
        let id = sources.add(&f.name, &f.text);
        let (qs, mut d) = parser::parse_queries(id, &f.text);
        queries.extend(qs);
        diagnostics.append(&mut d);
    }
    let (ir, mut d) = checker::analyze(&schema, &queries);
    diagnostics.append(&mut d);
    Compilation {
        sources,
        schema,
        ir,
        diagnostics,
    }
}
