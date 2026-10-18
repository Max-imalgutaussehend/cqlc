//! Abstract syntax tree for the supported CQL subset.

use crate::span::Span;
use crate::types::CqlType;
pub use crate::types::{Order, QueryKind, RelOp};

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
    CreateIndex(CreateIndex),
    CreateView(CreateView),
    CreateFunction(CreateFunction),
    CreateAggregate(CreateAggregate),
    AlterTable(AlterTable),
    AlterType(AlterType),
    Drop(Drop),
    /// Statements that do not influence typing (`USE`, `TRUNCATE`, `GRANT`, roles, triggers, ...).
    Ignored(Ignored),
    Select(Select),
    Insert(Insert),
    Update(Update),
    Delete(Delete),
}

impl Statement {
    pub fn is_ddl(&self) -> bool {
        !matches!(
            self,
            Statement::Select(_)
                | Statement::Insert(_)
                | Statement::Update(_)
                | Statement::Delete(_)
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
    /// `MASKED WITH ...` (Cassandra 5.0): dynamic data masking; no effect on the type.
    pub masked: bool,
    /// A `CHECK ...` constraint (Cassandra 6.0); no effect on the type.
    pub has_check: bool,
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

/// Which part of a column an index covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexKind {
    /// The column value (or the collection's values).
    Values,
    /// `KEYS(col)`: map keys.
    Keys,
    /// `ENTRIES(col)`: map entries.
    Entries,
    /// `FULL(col)`: a frozen collection as a whole.
    Full,
}

/// `CREATE [CUSTOM] INDEX [name] ON t (target) [USING 'class']`.
#[derive(Clone, Debug, PartialEq)]
pub struct CreateIndex {
    pub name: Option<Ident>,
    pub table: TableName,
    pub column: Ident,
    pub kind: IndexKind,
    /// The `USING` class, verbatim (`sai`, `org.apache.cassandra.index.sai.StorageAttachedIndex`, ...).
    pub class: Option<String>,
    pub span: Span,
}

/// `CREATE MATERIALIZED VIEW v AS SELECT ... FROM base WHERE ... PRIMARY KEY (...)`.
#[derive(Clone, Debug, PartialEq)]
pub struct CreateView {
    pub name: TableName,
    pub base: TableName,
    /// `None` for `SELECT *`.
    pub columns: Option<Vec<Ident>>,
    pub primary_key: PrimaryKeyDef,
    pub clustering_order: Vec<(Ident, Order)>,
    pub span: Span,
}

/// `CREATE FUNCTION f (a int, b text) ... RETURNS type ...` (the body is not analysed).
#[derive(Clone, Debug, PartialEq)]
pub struct CreateFunction {
    pub name: TableName,
    pub args: Vec<CqlType>,
    pub returns: CqlType,
    pub span: Span,
}

/// `CREATE AGGREGATE a (int) SFUNC f STYPE type [FINALFUNC g]`.
#[derive(Clone, Debug, PartialEq)]
pub struct CreateAggregate {
    pub name: TableName,
    pub args: Vec<CqlType>,
    pub state_type: CqlType,
    pub final_func: Option<Ident>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AlterTableAction {
    Add(Vec<ColumnDef>),
    Drop(Vec<Ident>),
    Rename(Vec<(Ident, Ident)>),
    /// `WITH options`, `DROP COMPACT STORAGE`, `ALTER col MASKED/DROP MASKED`: no effect on types.
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AlterTable {
    pub table: TableName,
    pub action: AlterTableAction,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AlterTypeAction {
    Add(Ident, CqlType),
    Rename(Vec<(Ident, Ident)>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct AlterType {
    pub name: TableName,
    pub action: AlterTypeAction,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropKind {
    Table,
    Type,
    Index,
    View,
    Keyspace,
    Function,
    Aggregate,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Drop {
    pub kind: DropKind,
    pub name: TableName,
    pub if_exists: bool,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ignored {
    /// The leading keywords, for diagnostics (`USE`, `TRUNCATE`, `GRANT`, ...).
    pub what: String,
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
    /// A duration literal such as `1d` or `1h30m` (Cassandra duration syntax).
    Duration(String),
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
    /// `a + b`, `a - b`, `a * b`, `a / b`, `a % b` (Cassandra 4.0 arithmetic).
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// `-expr`
    Neg(Box<Expr>),
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
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

impl BinOp {
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
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
    /// `ORDER BY col ANN OF vector` (Cassandra 5.0 vector search).
    pub ann: Option<(Ident, Expr)>,
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
    /// Warnings silenced with `-- allow: CQL201 ...`.
    pub allow: Vec<crate::diagnostic::DiagnosticCode>,
    pub span: Span,
}
