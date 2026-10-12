//! The typed intermediate representation consumed by code generators.
//!
//! Every query is fully resolved: bind parameters and result columns have
//! types and nullability, and names are unique.

use crate::schema::Udt;
use crate::span::Span;
use crate::types::{CqlType, QueryKind, RelOp};

/// The kind of CQL statement a query runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum StatementKind {
    /// `SELECT`
    Select,
    /// `INSERT`
    Insert,
    /// `UPDATE`
    Update,
    /// `DELETE`
    Delete,
}

/// Where a bind parameter comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParamSource {
    /// Compared in a `WHERE` clause. `column` is `token` for token relations.
    Restriction {
        /// The restricted column.
        column: String,
        /// The comparison operator.
        op: RelOp,
    },
    /// Compared in an `IF` condition.
    Condition {
        /// The tested column.
        column: String,
        /// The comparison operator.
        op: RelOp,
    },
    /// Value written to a column (`VALUES` / `SET`).
    Assignment {
        /// The written column.
        column: String,
    },
    /// Map key or list index of an element update, delete or restriction.
    ElementKey {
        /// The collection column.
        column: String,
    },
    /// Value of an element update (`col[?] = value`).
    ElementValue {
        /// The collection column.
        column: String,
    },
    /// `INSERT ... JSON ?`
    Json,
    /// Marker inside a selection expression.
    Expression,
    /// `LIMIT ?`
    Limit,
    /// `PER PARTITION LIMIT ?`
    PerPartitionLimit,
    /// `USING TTL ?`
    Ttl,
    /// `USING TIMESTAMP ?`
    Timestamp,
}

/// One bind parameter of a query.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Param {
    /// Unique within the query; derived from the column (or `limit`, `ttl`,
    /// `timestamp`, ...).
    pub name: String,
    /// The type the driver must bind.
    pub cql_type: CqlType,
    /// Whether the caller may pass NULL for this value. This concerns bind
    /// values only and is independent of result nullability.
    pub nullable: bool,
    /// What the marker stands for.
    pub source: ParamSource,
    /// Position of the `?` in the source.
    pub span: Span,
}

/// One column of a result row.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct ResultColumn {
    /// Alias if given, otherwise the column name. `[json]` for `SELECT JSON`.
    pub name: String,
    /// The column type.
    pub cql_type: CqlType,
    /// Whether the database cell may be absent.
    pub nullable: bool,
}

/// A fully analyzed query.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Query {
    /// Name exactly as written in the annotation.
    pub name: String,
    /// How the result is exposed.
    pub kind: QueryKind,
    /// The statement type.
    pub statement: StatementKind,
    /// Whitespace-normalized CQL text without comments or the trailing `;`.
    pub cql: String,
    /// Keyspace of the table, if the schema qualified it.
    pub keyspace: Option<String>,
    /// The table the query runs against.
    pub table: String,
    /// In bind-marker order.
    pub params: Vec<Param>,
    /// Result columns of a `SELECT`. For [`QueryKind::Lwt`] these are the
    /// columns the database reports back when the write was *not* applied;
    /// all of them are nullable.
    pub columns: Vec<ResultColumn>,
    /// For [`QueryKind::Lwt`]: every column of the table. Some servers (e.g.
    /// ScyllaDB) report the whole row for a failed conditional write, others
    /// only the tested columns. Empty for other queries.
    pub row_columns: Vec<ResultColumn>,
    /// The whole statement in the source.
    pub span: Span,
}

/// The result of analyzing all queries.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct Ir {
    /// Queries without errors, in source order.
    pub queries: Vec<Query>,
    /// All user-defined types of the schema.
    pub udts: Vec<Udt>,
}
