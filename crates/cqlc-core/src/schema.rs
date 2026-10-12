//! The resolved schema model.

use crate::span::Span;
use crate::types::{CqlType, Order};

/// The role of a column in the primary key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ColumnRole {
    /// Part of the partition key.
    PartitionKey,
    /// A clustering column.
    ClusteringKey,
    /// Not part of the primary key.
    Regular,
}

/// A table column.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Column {
    /// Normalized column name.
    pub name: String,
    /// The declared type.
    pub cql_type: CqlType,
    /// Position in the primary key.
    pub role: ColumnRole,
    /// Declared `STATIC`.
    pub is_static: bool,
    /// Where the column is defined.
    pub span: Span,
}

impl Column {
    /// Whether the column is part of the primary key.
    pub fn is_key(&self) -> bool {
        self.role != ColumnRole::Regular
    }

    /// Primary key columns are never null; everything else may be absent.
    pub fn nullable(&self) -> bool {
        !self.is_key()
    }
}

/// A clustering column with its sort order.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct ClusteringColumn {
    /// Column name.
    pub name: String,
    /// Declared order (`CLUSTERING ORDER BY`), ascending by default.
    pub order: Order,
}

/// A table definition.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Table {
    /// Keyspace, if the `CREATE TABLE` qualified the name.
    pub keyspace: Option<String>,
    /// Table name.
    pub name: String,
    /// In declaration order.
    pub columns: Vec<Column>,
    /// Names of the partition key columns, in key order.
    pub partition_key: Vec<String>,
    /// Clustering columns, in key order.
    pub clustering: Vec<ClusteringColumn>,
    /// Where the table is defined.
    pub span: Span,
}

impl Table {
    /// Finds a column by its normalized name.
    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|c| c.name == name)
    }

    /// Whether the table has a counter column.
    pub fn is_counter_table(&self) -> bool {
        self.columns
            .iter()
            .any(|c| matches!(c.cql_type, CqlType::Counter))
    }

    /// `keyspace.table`, or just the table name if unqualified.
    pub fn qualified_name(&self) -> String {
        match &self.keyspace {
            Some(ks) => format!("{ks}.{}", self.name),
            None => self.name.clone(),
        }
    }
}

/// A user-defined type.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Udt {
    /// Keyspace, if qualified.
    pub keyspace: Option<String>,
    /// Type name.
    pub name: String,
    /// Fields in declaration order.
    pub fields: Vec<(String, CqlType)>,
    /// Where the type is defined.
    pub span: Span,
}

/// All tables and user-defined types.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct Schema {
    /// Tables in definition order.
    pub tables: Vec<Table>,
    /// User-defined types in definition order.
    pub udts: Vec<Udt>,
}

impl Schema {
    /// Finds a user-defined type by name.
    pub fn udt(&self, name: &str) -> Option<&Udt> {
        self.udts.iter().find(|u| u.name == name)
    }
}

/// Result of [`Schema::lookup_table`].
#[derive(Debug)]
#[non_exhaustive]
pub enum Lookup<'a> {
    /// Exactly one table matches.
    Found(&'a Table),
    /// No table matches.
    NotFound,
    /// Unqualified name that exists in several keyspaces.
    Ambiguous(Vec<&'a Table>),
}

impl Schema {
    /// Qualified names must match a table declared in that keyspace;
    /// unqualified names match by table name alone.
    pub fn lookup_table(&self, keyspace: Option<&str>, name: &str) -> Lookup<'_> {
        let matches: Vec<&Table> = self
            .tables
            .iter()
            .filter(|t| {
                t.name == name && keyspace.is_none_or(|ks| t.keyspace.as_deref() == Some(ks))
            })
            .collect();
        match matches.len() {
            0 => Lookup::NotFound,
            1 => Lookup::Found(matches[0]),
            _ => Lookup::Ambiguous(matches),
        }
    }
}
