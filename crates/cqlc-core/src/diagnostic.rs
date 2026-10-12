//! Compiler diagnostics with stable codes.

use crate::span::{SourceMap, Span};
use std::fmt::Write as _;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// Stable diagnostic codes. `CQL0xx` are syntax problems, `CQL1xx` are
/// errors from schema resolution / type checking, `CQL2xx` are warnings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub severity: Severity,
    pub message: String,
    pub span: Span,
    pub help: Option<String>,
}

impl Diagnostic {
    pub fn error(code: DiagnosticCode, span: Span, message: impl Into<String>) -> Self {
        Diagnostic {
            code,
            severity: Severity::Error,
            message: message.into(),
            span,
            help: None,
        }
    }

    pub fn warning(code: DiagnosticCode, span: Span, message: impl Into<String>) -> Self {
        Diagnostic {
            severity: Severity::Warning,
            ..Diagnostic::error(code, span, message)
        }
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

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
        let span_end = (self.span.end as usize).clamp(self.span.start as usize, line_end);
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
