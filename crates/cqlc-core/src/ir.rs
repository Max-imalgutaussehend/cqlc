//! The typed intermediate representation consumed by code generators.

use crate::ast::{QueryKind, RelOp};
use crate::schema::Udt;
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
    /// Compared in a `WHERE` clause. `column` is `token` for token relations.
    Restriction {
        column: String,
        op: RelOp,
    },
    /// Compared in an `IF` condition.
    Condition {
        column: String,
        op: RelOp,
    },
    /// Value written to a column (`VALUES` / `SET`).
    Assignment {
        column: String,
    },
    /// Map key or list index of an element update or delete.
    ElementKey {
        column: String,
    },
    /// Value of an element update (`col[?] = value`).
    ElementValue {
        column: String,
    },
    /// `INSERT ... JSON ?`
    Json,
    /// Marker inside a selection expression.
    Expression,
    Limit,
    PerPartitionLimit,
    Ttl,
    Timestamp,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    /// Unique within the query; derived from the column (or `limit`, `ttl`,
    /// `timestamp`, ...).
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
    /// Alias if given, otherwise the column name. `[json]` for `SELECT JSON`.
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
    /// Result columns of a `SELECT`. For [`QueryKind::Lwt`] these are the
    /// columns the database reports back when the write was *not* applied;
    /// all of them are nullable.
    pub columns: Vec<ResultColumn>,
    pub span: Span,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ir {
    pub queries: Vec<Query>,
    /// All user-defined types of the schema.
    pub udts: Vec<Udt>,
}
