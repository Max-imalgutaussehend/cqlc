//! The typed intermediate representation consumed by code generators.

use crate::ast::{QueryKind, RelOp};
use crate::span::Span;
use crate::types::CqlType;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatementKind {
    Select,
    Insert,
    Update,
    Delete,
}

/// Where a bind parameter comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParamSource {
    /// Compared against a column in a `WHERE` clause.
    Restriction {
        column: String,
        op: RelOp,
    },
    /// Value written to a column (`VALUES` / `SET`).
    Assignment {
        column: String,
    },
    Limit,
    Ttl,
    Timestamp,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    /// Unique within the query; derived from the column (or `limit`, `ttl`,
    /// `timestamp`).
    pub name: String,
    pub cql_type: CqlType,
    /// Whether the caller may pass NULL for this value. This concerns bind
    /// values only and is independent of result nullability.
    pub nullable: bool,
    pub source: ParamSource,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResultColumn {
    /// Alias if given, otherwise the column name.
    pub name: String,
    pub cql_type: CqlType,
    /// Whether the database cell may be absent.
    pub nullable: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Query {
    /// Name exactly as written in the annotation.
    pub name: String,
    pub kind: QueryKind,
    pub statement: StatementKind,
    pub cql: String,
    pub keyspace: Option<String>,
    pub table: String,
    /// In bind-marker order.
    pub params: Vec<Param>,
    /// Empty for non-SELECT statements.
    pub columns: Vec<ResultColumn>,
    pub span: Span,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ir {
    pub queries: Vec<Query>,
}
