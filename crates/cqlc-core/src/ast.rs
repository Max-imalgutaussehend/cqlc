//! Abstract syntax tree for the supported CQL subset.

use crate::span::Span;
use crate::types::CqlType;

/// An identifier with its normalized name: unquoted identifiers are
/// lower-cased, quoted ones keep their exact spelling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableName {
    pub keyspace: Option<Ident>,
    pub name: Ident,
}

impl TableName {
    pub fn span(&self) -> Span {
        match &self.keyspace {
            Some(ks) => ks.span.to(self.name.span),
            None => self.name.span,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Statement {
    CreateKeyspace(CreateKeyspace),
    CreateTable(CreateTable),
    CreateType(CreateType),
    Select(Select),
    Insert(Insert),
    Update(Update),
    Delete(Delete),
}

impl Statement {
    pub fn is_ddl(&self) -> bool {
        matches!(
            self,
            Statement::CreateKeyspace(_) | Statement::CreateTable(_) | Statement::CreateType(_)
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CreateKeyspace {
    pub name: Ident,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ColumnDef {
    pub name: Ident,
    pub ty: CqlType,
    pub ty_span: Span,
    pub is_static: bool,
    pub inline_primary_key: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    Asc,
    Desc,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrimaryKeyDef {
    pub partition: Vec<Ident>,
    pub clustering: Vec<Ident>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CreateTable {
    pub name: TableName,
    pub columns: Vec<ColumnDef>,
    pub primary_key: Option<PrimaryKeyDef>,
    pub clustering_order: Vec<(Ident, Order)>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CreateType {
    pub name: TableName,
    pub fields: Vec<(Ident, CqlType)>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Literal {
    String(String),
    /// Integer literal, verbatim.
    Integer(String),
    /// Float literal, verbatim.
    Float(String),
    Bool(bool),
    Uuid(String),
    Blob(String),
    Null,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TermKind {
    /// Zero-based position among all bind markers of the statement.
    Marker(usize),
    Literal(Literal),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Term {
    pub kind: TermKind,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelOp {
    Eq,
    Lt,
    Le,
    Gt,
    Ge,
    In,
}

impl RelOp {
    pub fn is_range(self) -> bool {
        matches!(self, RelOp::Lt | RelOp::Le | RelOp::Gt | RelOp::Ge)
    }

    pub fn is_eq_or_in(self) -> bool {
        matches!(self, RelOp::Eq | RelOp::In)
    }

    pub fn suffix(self) -> &'static str {
        match self {
            RelOp::Eq => "eq",
            RelOp::Lt => "lt",
            RelOp::Le => "lte",
            RelOp::Gt => "gt",
            RelOp::Ge => "gte",
            RelOp::In => "in",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Rhs {
    Term(Term),
    /// `IN (a, b, c)`
    List(Vec<Term>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Relation {
    pub column: Ident,
    pub op: RelOp,
    pub rhs: Rhs,
    pub span: Span,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Using {
    pub ttl: Option<Term>,
    pub timestamp: Option<Term>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SelectItem {
    pub column: Ident,
    pub alias: Option<Ident>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Selection {
    Star(Span),
    Items(Vec<SelectItem>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Select {
    pub selection: Selection,
    pub table: TableName,
    pub where_clause: Vec<Relation>,
    pub order_by: Vec<(Ident, Order)>,
    pub limit: Option<Term>,
    pub allow_filtering: bool,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Insert {
    pub table: TableName,
    pub columns: Vec<Ident>,
    pub values: Vec<Term>,
    pub using: Using,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Assignment {
    pub column: Ident,
    pub value: Term,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Update {
    pub table: TableName,
    pub using: Using,
    pub assignments: Vec<Assignment>,
    pub where_clause: Vec<Relation>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Delete {
    pub columns: Vec<Ident>,
    pub table: TableName,
    pub using: Using,
    pub where_clause: Vec<Relation>,
    pub span: Span,
}

/// How a query's result is exposed in the generated API.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryKind {
    /// Zero or one row.
    One,
    /// All rows.
    Many,
    /// No result rows.
    Exec,
}

/// One annotated statement from a queries file.
#[derive(Clone, Debug, PartialEq)]
pub struct QueryDef {
    pub name: Ident,
    pub kind: QueryKind,
    pub statement: Statement,
    /// Whitespace-normalized CQL text without comments or trailing `;`.
    pub cql: String,
    pub span: Span,
}
