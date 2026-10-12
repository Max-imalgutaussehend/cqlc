//! Abstract syntax tree for the supported CQL subset.

use crate::span::Span;
use crate::types::CqlType;

/// An identifier with its normalized name: unquoted identifiers are
/// lower-cased, quoted ones keep their exact spelling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ident {
    pub name: String,
    pub span: Span,
    /// Written as `"quoted"` in the source.
    pub quoted: bool,
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

    /// `INSERT ... IF NOT EXISTS`, `UPDATE/DELETE ... IF ...`.
    pub fn is_conditional(&self) -> bool {
        match self {
            Statement::Insert(i) => i.if_not_exists,
            Statement::Update(u) => u.if_clause.is_some(),
            Statement::Delete(d) => d.if_clause.is_some(),
            _ => false,
        }
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
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    /// Zero-based position among all bind markers of the statement.
    Marker(usize),
    Literal(Literal),
    Column(Ident),
    /// `expr.field` (UDT field).
    Field(Box<Expr>, Ident),
    /// `expr[index]` (map or list element).
    Index(Box<Expr>, Box<Expr>),
    /// `name(args)`; `star` is set for `count(*)`.
    Call {
        name: Ident,
        args: Vec<Expr>,
        star: bool,
    },
    Cast(Box<Expr>, CqlType),
    /// `[a, b]`
    List(Vec<Expr>),
    /// `(a, b)`
    Tuple(Vec<Expr>),
    /// `{a, b}`
    Set(Vec<Expr>),
    /// `{k: v, ...}` with literal keys.
    Map(Vec<(Expr, Expr)>),
    /// `{field: v, ...}` with identifier keys.
    Udt(Vec<(Ident, Expr)>),
    /// `{}`: an empty set or map, decided by the expected type.
    EmptyBraces,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    In,
    Contains,
    ContainsKey,
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
            RelOp::Ne => "ne",
            RelOp::Lt => "lt",
            RelOp::Le => "lte",
            RelOp::Gt => "gt",
            RelOp::Ge => "gte",
            RelOp::In => "in",
            RelOp::Contains => "contains",
            RelOp::ContainsKey => "contains_key",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RelLhs {
    Column(Ident),
    /// `token(a, b)`
    Token(Vec<Ident>),
    /// `(a, b)`
    Tuple(Vec<Ident>),
    /// `col[key]`
    Element(Ident, Expr),
    /// `col.field`
    Field(Ident, Ident),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Rhs {
    Expr(Expr),
    /// `IN (a, b, c)`
    InList(Vec<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Relation {
    pub lhs: RelLhs,
    pub op: RelOp,
    pub rhs: Rhs,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum IfClause {
    /// `IF EXISTS`
    Exists,
    /// `IF cond AND cond`
    Conditions(Vec<Relation>),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Using {
    pub ttl: Option<Expr>,
    pub timestamp: Option<Expr>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SelectItem {
    pub expr: Expr,
    pub alias: Option<Ident>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Selection {
    Star(Span),
    Items(Vec<SelectItem>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectMode {
    Normal,
    Json,
    Distinct,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Select {
    pub mode: SelectMode,
    pub selection: Selection,
    pub table: TableName,
    pub where_clause: Vec<Relation>,
    pub group_by: Vec<Ident>,
    pub order_by: Vec<(Ident, Order)>,
    pub per_partition_limit: Option<Expr>,
    pub limit: Option<Expr>,
    pub allow_filtering: bool,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum InsertSource {
    Values {
        columns: Vec<Ident>,
        values: Vec<Expr>,
    },
    /// `INSERT INTO t JSON <expr>`
    Json(Expr),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Insert {
    pub table: TableName,
    pub source: InsertSource,
    pub if_not_exists: bool,
    pub using: Using,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignOp {
    /// `col = value`
    Set,
    /// `col = col + value` / `col += value`
    Add,
    /// `col = col - value` / `col -= value`
    Sub,
    /// `col = value + col` (lists)
    Prepend,
}

/// `col`, `col[key]` or `col.field`.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub column: Ident,
    pub key: Option<Expr>,
    pub field: Option<Ident>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Assignment {
    pub target: Target,
    pub op: AssignOp,
    pub value: Expr,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Update {
    pub table: TableName,
    pub using: Using,
    pub assignments: Vec<Assignment>,
    pub where_clause: Vec<Relation>,
    pub if_clause: Option<IfClause>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Delete {
    pub targets: Vec<Target>,
    pub table: TableName,
    pub using: Using,
    pub where_clause: Vec<Relation>,
    pub if_clause: Option<IfClause>,
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
    /// Conditional write (lightweight transaction) with an `applied` result.
    Lwt,
}

/// One annotated statement from a queries file.
#[derive(Clone, Debug, PartialEq)]
pub struct QueryDef {
    pub name: Ident,
    pub kind: QueryKind,
    pub statement: Statement,
    /// Whitespace-normalized CQL text without comments or trailing `;`.
    pub cql: String,
    /// Number of `?` markers in the statement.
    pub marker_count: usize,
    pub span: Span,
}
