//! Pure CQL front end: lexer, parser, schema resolver, type checker and
//! typed IR. No I/O and no database access.

pub mod ast;
pub mod checker;
pub mod diagnostic;
pub mod ir;
pub mod lexer;
pub mod parser;
pub mod resolver;
pub mod schema;
pub mod span;
pub mod types;

use diagnostic::Diagnostic;
use span::SourceMap;

/// One input file for [`compile`].
pub struct Input {
    pub name: String,
    pub text: String,
}

/// Result of a full front-end run.
pub struct Compilation {
    pub sources: SourceMap,
    pub schema: schema::Schema,
    pub ir: ir::Ir,
    pub diagnostics: Vec<Diagnostic>,
}

impl Compilation {
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
