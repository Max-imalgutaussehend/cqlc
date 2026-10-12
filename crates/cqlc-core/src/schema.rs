//! The resolved schema model.

use crate::ast::Order;
use crate::span::Span;
use crate::types::CqlType;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnRole {
    PartitionKey,
    ClusteringKey,
    Regular,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    pub name: String,
    pub cql_type: CqlType,
    pub role: ColumnRole,
    pub is_static: bool,
    pub span: Span,
}

impl Column {
    pub fn is_key(&self) -> bool {
        self.role != ColumnRole::Regular
    }

    /// Primary key columns are never null; everything else may be absent.
    pub fn nullable(&self) -> bool {
        !self.is_key()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClusteringColumn {
    pub name: String,
    pub order: Order,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub keyspace: Option<String>,
    pub name: String,
    /// In declaration order.
    pub columns: Vec<Column>,
    pub partition_key: Vec<String>,
    pub clustering: Vec<ClusteringColumn>,
    pub span: Span,
}

impl Table {
    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|c| c.name == name)
    }

    pub fn qualified_name(&self) -> String {
        match &self.keyspace {
            Some(ks) => format!("{ks}.{}", self.name),
            None => self.name.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Udt {
    pub keyspace: Option<String>,
    pub name: String,
    pub fields: Vec<(String, CqlType)>,
    pub span: Span,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Schema {
    pub tables: Vec<Table>,
    pub udts: Vec<Udt>,
}

pub enum Lookup<'a> {
    Found(&'a Table),
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
