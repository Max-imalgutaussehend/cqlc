//! Schema resolution, type inference and Cassandra rule checks for queries.
//!
//! Errors reject a query (it does not reach the IR); warnings are advisory.

use crate::ast::*;
use crate::diagnostic::{Diagnostic, DiagnosticCode as Code};
use crate::ir::{Ir, Param, ParamSource, Query, ResultColumn, StatementKind};
use crate::schema::{Column, ColumnRole, Lookup, Schema, Table};
use crate::span::Span;
use crate::types::CqlType;
use std::collections::{HashMap, HashSet};

/// Maximum number of bind markers; the driver serializes tuples up to 16.
pub const MAX_PARAMS: usize = 16;

/// Analyzes all queries against the schema.
pub fn analyze(schema: &Schema, queries: &[QueryDef]) -> (Ir, Vec<Diagnostic>) {
    let mut checker = Checker {
        schema,
        diags: Vec::new(),
    };
    let mut ir = Ir::default();
    let mut names: HashMap<&str, Span> = HashMap::new();
    for q in queries {
        if names.insert(&q.name.name, q.name.span).is_some() {
            checker.diags.push(Diagnostic::error(
                Code::Duplicate,
                q.name.span,
                format!("query `{}` is defined more than once", q.name.name),
            ));
            continue;
        }
        if let Some(query) = checker.query(q) {
            ir.queries.push(query);
        }
    }
    (ir, checker.diags)
}

struct Checker<'a> {
    schema: &'a Schema,
    diags: Vec<Diagnostic>,
}

#[derive(Default)]
struct Ctx {
    params: Vec<(usize, Param)>,
}

struct Restriction<'a> {
    column: &'a Column,
    op: RelOp,
}

impl<'a> Checker<'a> {
    fn error_count(&self) -> usize {
        self.diags.iter().filter(|d| d.is_error()).count()
    }

    fn query(&mut self, q: &QueryDef) -> Option<Query> {
        let before = self.error_count();
        let mut ctx = Ctx::default();
        let (kind, table, columns) = match &q.statement {
            Statement::Select(s) => {
                let (t, cols) = self.select(s, &mut ctx)?;
                (StatementKind::Select, t, cols)
            }
            Statement::Insert(s) => (StatementKind::Insert, self.insert(s, &mut ctx)?, vec![]),
            Statement::Update(s) => (StatementKind::Update, self.update(s, &mut ctx)?, vec![]),
            Statement::Delete(s) => (StatementKind::Delete, self.delete(s, &mut ctx)?, vec![]),
            _ => return None,
        };
        if self.error_count() > before {
            return None;
        }
        ctx.params.sort_by_key(|(i, _)| *i);
        let mut params: Vec<Param> = ctx.params.into_iter().map(|(_, p)| p).collect();
        if params.len() > MAX_PARAMS {
            self.diags.push(Diagnostic::error(
                Code::Arity,
                q.span,
                format!(
                    "query has {} bind markers; at most {MAX_PARAMS} are supported",
                    params.len()
                ),
            ));
            return None;
        }
        unique_names(&mut params);
        Some(Query {
            name: q.name.name.clone(),
            kind: q.kind,
            statement: kind,
            cql: q.cql.clone(),
            keyspace: table.keyspace.clone(),
            table: table.name.clone(),
            params,
            columns,
            span: q.span,
        })
    }

    // ----- lookups -------------------------------------------------------

    fn table(&mut self, name: &TableName) -> Option<&'a Table> {
        let schema = self.schema;
        let ks = name.keyspace.as_ref().map(|k| k.name.as_str());
        match schema.lookup_table(ks, &name.name.name) {
            Lookup::Found(t) => Some(t),
            Lookup::NotFound => {
                let known: Vec<String> = schema.tables.iter().map(|t| t.qualified_name()).collect();
                let mut d = Diagnostic::error(
                    Code::UnknownTable,
                    name.span(),
                    format!("unknown table `{}`", name.name.name),
                );
                if !known.is_empty() {
                    d = d.with_help(format!("known tables: {}", known.join(", ")));
                }
                self.diags.push(d);
                None
            }
            Lookup::Ambiguous(ts) => {
                let names: Vec<String> = ts.iter().map(|t| t.qualified_name()).collect();
                self.diags.push(
                    Diagnostic::error(
                        Code::UnknownTable,
                        name.span(),
                        format!("table name `{}` is ambiguous", name.name.name),
                    )
                    .with_help(format!(
                        "qualify it with a keyspace: {}",
                        names.join(" or ")
                    )),
                );
                None
            }
        }
    }

    fn column(&mut self, table: &'a Table, id: &Ident) -> Option<&'a Column> {
        match table.column(&id.name) {
            Some(c) => Some(c),
            None => {
                let cols: Vec<&str> = table.columns.iter().map(|c| c.name.as_str()).collect();
                self.diags.push(
                    Diagnostic::error(
                        Code::UnknownColumn,
                        id.span,
                        format!("unknown column `{}` in table `{}`", id.name, table.name),
                    )
                    .with_help(format!("available columns: {}", cols.join(", "))),
                );
                None
            }
        }
    }

    /// Rejects column types the generator cannot map yet.
    fn supported(&mut self, col: &Column, span: Span) -> bool {
        if col.cql_type.contains_udt() {
            self.diags.push(
                Diagnostic::error(
                    Code::UnsupportedType,
                    span,
                    format!(
                        "column `{}` has user-defined type `{}`, which is not supported yet",
                        col.name, col.cql_type
                    ),
                )
                .with_help(
                    "leave this column out of the query (explicit column lists work around it)",
                ),
            );
            return false;
        }
        true
    }

    // ----- binding -------------------------------------------------------

    fn bind(
        &mut self,
        ctx: &mut Ctx,
        term: &Term,
        ty: CqlType,
        nullable: bool,
        name: &str,
        source: ParamSource,
    ) {
        match &term.kind {
            TermKind::Marker(i) => ctx.params.push((
                *i,
                Param {
                    name: name.to_string(),
                    cql_type: ty,
                    nullable,
                    source,
                    span: term.span,
                },
            )),
            TermKind::Literal(l) => {
                if matches!(l, Literal::Null) && !nullable {
                    self.diags.push(Diagnostic::error(
                        Code::TypeMismatch,
                        term.span,
                        "NULL is not allowed here",
                    ));
                } else if !literal_fits(l, &ty) {
                    self.diags.push(Diagnostic::error(
                        Code::TypeMismatch,
                        term.span,
                        format!("literal does not fit type `{ty}`"),
                    ));
                }
            }
        }
    }

    fn bind_using(&mut self, using: &Using, ctx: &mut Ctx) {
        if let Some(t) = &using.ttl {
            self.bind(ctx, t, CqlType::Int, false, "ttl", ParamSource::Ttl);
        }
        if let Some(t) = &using.timestamp {
            self.bind(
                ctx,
                t,
                CqlType::BigInt,
                false,
                "timestamp",
                ParamSource::Timestamp,
            );
        }
    }

    fn relations(
        &mut self,
        table: &'a Table,
        rels: &[Relation],
        ctx: &mut Ctx,
    ) -> Vec<Restriction<'a>> {
        let mut out = Vec::new();
        for r in rels {
            let Some(col) = self.column(table, &r.column) else {
                continue;
            };
            if !self.supported(col, r.column.span) {
                continue;
            }
            let source = ParamSource::Restriction {
                column: col.name.clone(),
                op: r.op,
            };
            match &r.rhs {
                Rhs::Term(t) => {
                    let ty = if r.op == RelOp::In {
                        if matches!(t.kind, TermKind::Literal(_)) {
                            self.diags.push(Diagnostic::error(
                                Code::TypeMismatch,
                                t.span,
                                "`IN` needs `?` or a parenthesized list of values",
                            ));
                            continue;
                        }
                        CqlType::List(Box::new(col.cql_type.clone()))
                    } else {
                        col.cql_type.clone()
                    };
                    self.bind(ctx, t, ty, false, &col.name, source);
                }
                Rhs::List(ts) => {
                    for t in ts {
                        self.bind(
                            ctx,
                            t,
                            col.cql_type.clone(),
                            false,
                            &col.name,
                            source.clone(),
                        );
                    }
                }
            }
            out.push(Restriction {
                column: col,
                op: r.op,
            });
        }
        out
    }

    // ----- statements ----------------------------------------------------

    fn select(&mut self, s: &Select, ctx: &mut Ctx) -> Option<(&'a Table, Vec<ResultColumn>)> {
        let table = self.table(&s.table)?;
        let mut columns = Vec::new();
        match &s.selection {
            Selection::Star(span) => {
                for c in &table.columns {
                    if self.supported(c, *span) {
                        columns.push(result_column(c, &c.name));
                    }
                }
            }
            Selection::Items(items) => {
                for item in items {
                    let Some(c) = self.column(table, &item.column) else {
                        continue;
                    };
                    if self.supported(c, item.column.span) {
                        let name = item.alias.as_ref().map_or(&c.name, |a| &a.name);
                        columns.push(result_column(c, name));
                    }
                }
            }
        }
        let mut seen = HashSet::new();
        for c in &columns {
            if !seen.insert(c.name.clone()) {
                self.diags.push(
                    Diagnostic::error(
                        Code::Duplicate,
                        s.span,
                        format!("result column `{}` appears more than once", c.name),
                    )
                    .with_help("give one of them an alias with `AS`"),
                );
            }
        }
        let restrictions = self.relations(table, &s.where_clause, ctx);
        for (col, _) in &s.order_by {
            if self
                .column(table, col)
                .is_some_and(|c| c.role != ColumnRole::ClusteringKey)
            {
                self.diags.push(Diagnostic::error(
                    Code::InvalidRestriction,
                    col.span,
                    format!(
                        "cannot order by `{}`: only clustering columns can be ordered",
                        col.name
                    ),
                ));
            }
        }
        if let Some(limit) = &s.limit {
            self.bind(ctx, limit, CqlType::Int, false, "limit", ParamSource::Limit);
        }
        self.select_warnings(s, table, &restrictions);
        Some((table, columns))
    }

    fn select_warnings(&mut self, s: &Select, table: &Table, rs: &[Restriction<'_>]) {
        for r in rs {
            if r.column.role == ColumnRole::PartitionKey && !r.op.is_eq_or_in() {
                self.diags.push(
                    Diagnostic::error(
                        Code::InvalidRestriction,
                        s.span,
                        format!("partition key column `{}` only supports `=` and `IN` restrictions", r.column.name),
                    )
                    .with_help("range scans over partition keys need `token(...)`, which cqlc does not support yet"),
                );
            }
        }
        let restricted = |name: &str| rs.iter().any(|r| r.column.name == name);
        let missing: Vec<&str> = table
            .partition_key
            .iter()
            .filter(|k| !restricted(k))
            .map(String::as_str)
            .collect();
        if !missing.is_empty() {
            self.diags.push(
                Diagnostic::warning(
                    Code::BroadScan,
                    s.span,
                    format!(
                        "query does not restrict partition key column{} {} and may scan many partitions",
                        if missing.len() > 1 { "s" } else { "" },
                        missing.iter().map(|m| format!("`{m}`")).collect::<Vec<_>>().join(", ")
                    ),
                )
                .with_help("consider modeling a table around this access pattern"),
            );
        }
        if s.allow_filtering {
            self.diags.push(
                Diagnostic::warning(Code::AllowFiltering, s.span, "query uses `ALLOW FILTERING`")
                    .with_help("filtering can be expensive; prefer a table that matches the access pattern"),
            );
        } else if let Some(r) = rs.iter().find(|r| !r.column.is_key()) {
            self.diags.push(
                Diagnostic::warning(
                    Code::NonKeyRestriction,
                    s.span,
                    format!(
                        "restriction on non-key column `{}` requires an index or `ALLOW FILTERING`",
                        r.column.name
                    ),
                )
                .with_help("Cassandra rejects this unless a secondary index exists"),
            );
        }
        // Clustering columns must be restricted left to right.
        let mut blocked: Option<(&str, &str)> = None;
        for c in &table.clustering {
            let ops: Vec<RelOp> = rs
                .iter()
                .filter(|r| r.column.name == c.name)
                .map(|r| r.op)
                .collect();
            if ops.is_empty() {
                blocked.get_or_insert((&c.name, "is not restricted"));
                continue;
            }
            if let Some((prev, why)) = blocked {
                self.diags.push(
                    Diagnostic::warning(
                        Code::ClusteringGap,
                        s.span,
                        format!("clustering column `{}` is restricted but `{prev}` {why}", c.name),
                    )
                    .with_help("restrict clustering columns in order, or Cassandra will require `ALLOW FILTERING`"),
                );
                break;
            }
            if ops.iter().any(|o| o.is_range()) {
                blocked = Some((&c.name, "is restricted by a range"));
            }
        }
    }

    fn insert(&mut self, s: &Insert, ctx: &mut Ctx) -> Option<&'a Table> {
        let table = self.table(&s.table)?;
        if s.columns.len() != s.values.len() {
            self.diags.push(Diagnostic::error(
                Code::Arity,
                s.span,
                format!("{} columns but {} values", s.columns.len(), s.values.len()),
            ));
        }
        let mut seen = HashSet::new();
        for (id, term) in s.columns.iter().zip(&s.values) {
            let Some(col) = self.column(table, id) else {
                continue;
            };
            if !seen.insert(col.name.as_str()) {
                self.diags.push(Diagnostic::error(
                    Code::Duplicate,
                    id.span,
                    format!("column `{}` is listed more than once", col.name),
                ));
                continue;
            }
            if !self.supported(col, id.span) {
                continue;
            }
            self.bind(
                ctx,
                term,
                col.cql_type.clone(),
                col.nullable(),
                &col.name,
                ParamSource::Assignment {
                    column: col.name.clone(),
                },
            );
        }
        let missing: Vec<&str> = table
            .columns
            .iter()
            .filter(|c| c.is_key() && !s.columns.iter().any(|i| i.name == c.name))
            .map(|c| c.name.as_str())
            .collect();
        if !missing.is_empty() {
            self.diags.push(Diagnostic::error(
                Code::IncompleteKey,
                s.span,
                format!(
                    "INSERT must set all primary key columns; missing: {}",
                    missing.join(", ")
                ),
            ));
        }
        self.bind_using(&s.using, ctx);
        Some(table)
    }

    fn update(&mut self, s: &Update, ctx: &mut Ctx) -> Option<&'a Table> {
        let table = self.table(&s.table)?;
        for a in &s.assignments {
            let Some(col) = self.column(table, &a.column) else {
                continue;
            };
            if col.is_key() {
                self.diags.push(Diagnostic::error(
                    Code::InvalidRestriction,
                    a.column.span,
                    format!("cannot update primary key column `{}`", col.name),
                ));
                continue;
            }
            if !self.supported(col, a.column.span) {
                continue;
            }
            self.bind(
                ctx,
                &a.value,
                col.cql_type.clone(),
                true,
                &col.name,
                ParamSource::Assignment {
                    column: col.name.clone(),
                },
            );
        }
        self.bind_using(&s.using, ctx);
        let rs = self.relations(table, &s.where_clause, ctx);
        self.check_write_restrictions(table, &rs, s.span, true);
        Some(table)
    }

    fn delete(&mut self, s: &Delete, ctx: &mut Ctx) -> Option<&'a Table> {
        let table = self.table(&s.table)?;
        for id in &s.columns {
            if let Some(col) = self.column(table, id) {
                if col.is_key() {
                    self.diags.push(Diagnostic::error(
                        Code::InvalidRestriction,
                        id.span,
                        format!("cannot delete primary key column `{}`", col.name),
                    ));
                }
            }
        }
        self.bind_using(&s.using, ctx);
        let rs = self.relations(table, &s.where_clause, ctx);
        self.check_write_restrictions(table, &rs, s.span, false);
        Some(table)
    }

    /// UPDATE needs the whole primary key; DELETE needs at least the
    /// partition key (clustering columns may be ranges).
    fn check_write_restrictions(
        &mut self,
        table: &Table,
        rs: &[Restriction<'_>],
        span: Span,
        full_key: bool,
    ) {
        for r in rs {
            if !r.column.is_key() {
                self.diags.push(Diagnostic::error(
                    Code::InvalidRestriction,
                    span,
                    format!(
                        "cannot restrict non-key column `{}` in a write statement",
                        r.column.name
                    ),
                ));
            } else if (full_key || r.column.role == ColumnRole::PartitionKey) && !r.op.is_eq_or_in()
            {
                self.diags.push(Diagnostic::error(
                    Code::InvalidRestriction,
                    span,
                    format!(
                        "key column `{}` only supports `=` and `IN` here",
                        r.column.name
                    ),
                ));
            }
        }
        let required: Vec<&str> = if full_key {
            table
                .columns
                .iter()
                .filter(|c| c.is_key())
                .map(|c| c.name.as_str())
                .collect()
        } else {
            table.partition_key.iter().map(String::as_str).collect()
        };
        let missing: Vec<&str> = required
            .into_iter()
            .filter(|n| !rs.iter().any(|r| r.column.name == *n))
            .collect();
        if !missing.is_empty() {
            self.diags.push(Diagnostic::error(
                Code::IncompleteKey,
                span,
                format!(
                    "statement must restrict {}key column{}: {}",
                    if full_key {
                        "all primary "
                    } else {
                        "all partition "
                    },
                    if missing.len() > 1 { "s" } else { "" },
                    missing.join(", ")
                ),
            ));
        }
    }
}

fn result_column(c: &Column, name: &str) -> ResultColumn {
    ResultColumn {
        name: name.to_string(),
        cql_type: c.cql_type.clone(),
        nullable: c.nullable(),
    }
}

fn literal_fits(l: &Literal, ty: &CqlType) -> bool {
    use CqlType::*;
    let t = ty.unfrozen();
    match l {
        Literal::Null => true,
        Literal::String(_) => matches!(t, Ascii | Text | Inet | Timestamp | Date | Time),
        Literal::Integer(_) => matches!(
            t,
            TinyInt
                | SmallInt
                | Int
                | BigInt
                | VarInt
                | Counter
                | Float
                | Double
                | Decimal
                | Timestamp
                | Time
                | Date
        ),
        Literal::Float(_) => matches!(t, Float | Double | Decimal),
        Literal::Bool(_) => matches!(t, Boolean),
        Literal::Uuid(_) => matches!(t, Uuid | TimeUuid),
        Literal::Blob(_) => matches!(t, Blob),
    }
}

/// Makes parameter names unique: columns that are bound several times get
/// an operator suffix (`created_at_gte`), remaining clashes a number.
fn unique_names(params: &mut [Param]) {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for p in params.iter() {
        *counts.entry(p.name.clone()).or_default() += 1;
    }
    for p in params.iter_mut() {
        if counts[&p.name] > 1 {
            if let ParamSource::Restriction { op, .. } = &p.source {
                p.name = format!("{}_{}", p.name, op.suffix());
            }
        }
    }
    let mut used = HashSet::new();
    for p in params.iter_mut() {
        let base = p.name.clone();
        let mut n = 2;
        while !used.insert(p.name.clone()) {
            p.name = format!("{base}_{n}");
            n += 1;
        }
    }
}
