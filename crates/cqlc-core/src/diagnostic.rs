//! Compiler diagnostics with stable codes.

use crate::span::{SourceMap, Span};
use std::fmt::Write as _;

/// How serious a diagnostic is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Severity {
    /// The query is rejected.
    Error,
    /// Advice; the query is still generated.
    Warning,
}

/// Stable diagnostic codes. `CQL0xx` are syntax problems, `CQL1xx` are
/// errors from schema resolution / type checking, `CQL2xx` are warnings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DiagnosticCode {
    /// Syntax error.
    Syntax,
    /// Valid CQL that cqlc does not support (yet).
    Unsupported,
    /// Unknown table.
    UnknownTable,
    /// Unknown column.
    UnknownColumn,
    /// Unknown type.
    UnknownType,
    /// Duplicate definition.
    Duplicate,
    /// Literal or value does not fit the column type.
    TypeMismatch,
    /// Bind marker in a position whose type cannot be inferred.
    UntypedBindMarker,
    /// Missing or invalid `-- name:` annotation.
    Annotation,
    /// Wrong number of values / too many bind markers.
    Arity,
    /// Invalid primary key definition.
    PrimaryKey,
    /// Type has no supported Rust mapping.
    UnsupportedType,
    /// Statement misses required primary key restrictions.
    IncompleteKey,
    /// Restriction or assignment that Cassandra rejects.
    InvalidRestriction,
    /// Query may scan more than one partition.
    BroadScan,
    /// `ALLOW FILTERING` in use.
    AllowFiltering,
    /// Restriction on a non-key column.
    NonKeyRestriction,
    /// Clustering restriction skips a column.
    ClusteringGap,
    /// Unknown function.
    UnknownFunction,
    /// Expression in the selection list needs an `AS` alias.
    MissingAlias,
    /// `ORDER BY` may not match the clustering order.
    OrderBy,
    /// Reserved CQL keyword used as an unquoted identifier.
    ReservedWord,
}

impl DiagnosticCode {
    /// Every warning code, i.e. the ones `-- allow:` may silence.
    pub const WARNINGS: [DiagnosticCode; 5] = [
        DiagnosticCode::BroadScan,
        DiagnosticCode::AllowFiltering,
        DiagnosticCode::NonKeyRestriction,
        DiagnosticCode::ClusteringGap,
        DiagnosticCode::OrderBy,
    ];

    /// Parses a code such as `CQL201` (case-insensitive) if it is a warning.
    pub fn parse_warning(s: &str) -> Option<DiagnosticCode> {
        Self::WARNINGS
            .into_iter()
            .find(|c| c.as_str().eq_ignore_ascii_case(s))
    }

    /// The code as shown to users, e.g. `CQL201`.
    pub fn as_str(self) -> &'static str {
        use DiagnosticCode::*;
        match self {
            Syntax => "CQL001",
            Unsupported => "CQL002",
            UnknownTable => "CQL101",
            UnknownColumn => "CQL102",
            UnknownType => "CQL103",
            Duplicate => "CQL104",
            TypeMismatch => "CQL105",
            UntypedBindMarker => "CQL106",
            Annotation => "CQL107",
            Arity => "CQL108",
            PrimaryKey => "CQL109",
            UnsupportedType => "CQL110",
            IncompleteKey => "CQL111",
            InvalidRestriction => "CQL112",
            BroadScan => "CQL201",
            AllowFiltering => "CQL202",
            NonKeyRestriction => "CQL203",
            ClusteringGap => "CQL204",
            UnknownFunction => "CQL113",
            MissingAlias => "CQL114",
            OrderBy => "CQL205",
            ReservedWord => "CQL115",
        }
    }
}

/// One error or warning with its location.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Diagnostic {
    /// Stable code, e.g. `CQL102`.
    pub code: DiagnosticCode,
    /// Error or warning.
    pub severity: Severity,
    /// What is wrong.
    pub message: String,
    /// Where it is wrong.
    pub span: Span,
    /// How to fix it, if there is a suggestion.
    pub help: Option<String>,
}

impl Diagnostic {
    /// Creates an error.
    pub fn error(code: DiagnosticCode, span: Span, message: impl Into<String>) -> Self {
        Diagnostic {
            code,
            severity: Severity::Error,
            message: message.into(),
            span,
            help: None,
        }
    }

    /// Creates a warning.
    pub fn warning(code: DiagnosticCode, span: Span, message: impl Into<String>) -> Self {
        Diagnostic {
            severity: Severity::Warning,
            ..Diagnostic::error(code, span, message)
        }
    }

    /// Adds a help message.
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Whether this is an error.
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    /// Render in a rustc-like format.
    pub fn render(&self, map: &SourceMap) -> String {
        let file = map.get(self.span.file);
        let (line, col) = file.line_col(self.span.start as usize);
        let line_text = file.line_text(line);
        let line_start = file.line_start(line);
        let line_end = line_start + line_text.len();
        let span_end = (self.span.end as usize)
            .min(line_end)
            .max(self.span.start as usize);
        let width = file
            .text
            .get(self.span.start as usize..span_end)
            .map_or(1, |s| s.chars().count())
            .max(1);
        let sev = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        let gutter = " ".repeat(line.to_string().len());
        let mut out = String::new();
        let _ = writeln!(out, "{sev}[{}]: {}", self.code.as_str(), self.message);
        let _ = writeln!(out, "{gutter}--> {}:{line}:{col}", file.name);
        let _ = writeln!(out, "{gutter} |");
        let _ = writeln!(out, "{line} | {line_text}");
        let _ = writeln!(
            out,
            "{gutter} | {}{}",
            " ".repeat(col - 1),
            "^".repeat(width)
        );
        if let Some(help) = &self.help {
            let _ = writeln!(out, "{gutter} |");
            let _ = writeln!(out, "{gutter} = help: {help}");
        }
        out
    }
}
