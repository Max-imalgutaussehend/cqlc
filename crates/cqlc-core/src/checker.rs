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
    let mut ir = Ir {
        queries: Vec::new(),
        udts: schema.udts.clone(),
    };
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

/// How a bind marker found while checking an expression is named and typed
/// on the Rust side.
#[derive(Clone)]
struct Hint {
    name: String,
    source: ParamSource,
    /// May the caller bind NULL (and may a `null` literal appear)?
    nullable: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    Select,
    Where,
    Condition,
    Value,
}

struct Typed {
    ty: CqlType,
    nullable: bool,
}

struct Restriction<'a> {
    column: &'a Column,
    op: RelOp,
    via_token: bool,
    /// Part of a multi-column (tuple) relation, which covers its columns together.
    in_tuple: bool,
}

struct Outcome<'a> {
    kind: StatementKind,
    table: &'a Table,
    columns: Vec<ResultColumn>,
}

impl<'a> Checker<'a> {
    fn error_count(&self) -> usize {
        self.diags.iter().filter(|d| d.is_error()).count()
    }

    fn error(&mut self, code: Code, span: Span, msg: impl Into<String>) {
        self.diags.push(Diagnostic::error(code, span, msg));
    }

    fn unsupported(&mut self, span: Span, what: &str) {
        self.diags.push(
            Diagnostic::error(
                Code::Unsupported,
                span,
                format!("{what} is not supported by cqlc yet"),
            )
            .with_help("see docs/SUPPORTED_CQL.md for the supported subset"),
        );
    }

    fn query(&mut self, q: &QueryDef) -> Option<Query> {
        let start = self.diags.len();
        let result = self.query_inner(q);
        if !q.allow.is_empty() {
            let allow = &q.allow;
            let mut i = start;
            self.diags.retain(|d| {
                let keep = i < start || d.is_error() || !allow.contains(&d.code);
                i += 1;
                keep
            });
        }
        result
    }

    fn query_inner(&mut self, q: &QueryDef) -> Option<Query> {
        let before = self.error_count();
        let mut ctx = Ctx::default();
        let out = match &q.statement {
            Statement::Select(s) => self.select(s, &mut ctx),
            Statement::Insert(s) => self.insert(s, &mut ctx),
            Statement::Update(s) => self.update(s, &mut ctx),
            Statement::Delete(s) => self.delete(s, &mut ctx),
            _ => None,
        }?;
        if self.error_count() > before {
            return None;
        }
        if ctx.params.len() != q.marker_count {
            self.error(
                Code::UntypedBindMarker,
                q.span,
                format!(
                    "{} of {} bind markers could not be typed",
                    q.marker_count - ctx.params.len().min(q.marker_count),
                    q.marker_count
                ),
            );
            return None;
        }
        ctx.params.sort_by_key(|(i, _)| *i);
        let mut params: Vec<Param> = ctx.params.into_iter().map(|(_, p)| p).collect();
        if params.len() > MAX_PARAMS {
            self.error(
                Code::Arity,
                q.span,
                format!(
                    "query has {} bind markers; at most {MAX_PARAMS} are supported",
                    params.len()
                ),
            );
            return None;
        }
        unique_names(&mut params);
        Some(Query {
            name: q.name.name.clone(),
            kind: q.kind,
            statement: out.kind,
            cql: q.cql.clone(),
            keyspace: out.table.keyspace.clone(),
            table: out.table.name.clone(),
            params,
            columns: out.columns,
            row_columns: if q.kind == QueryKind::Lwt {
                out.table.columns.iter().map(lwt_column).collect()
            } else {
                Vec::new()
            },
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

    // ----- expressions ---------------------------------------------------

    /// Checks `e` and returns its type. Markers take the `expect`ed type;
    /// a marker without an expected type is an error.
    fn expr(
        &mut self,
        ctx: &mut Ctx,
        table: &'a Table,
        e: &Expr,
        expect: Option<&CqlType>,
        hint: &Hint,
        place: Place,
    ) -> Option<Typed> {
        let typed = match &e.kind {
            ExprKind::Marker(i) => {
                let Some(t) = expect else {
                    self.diags.push(
                        Diagnostic::error(
                            Code::UntypedBindMarker,
                            e.span,
                            "the type of this bind marker cannot be inferred",
                        )
                        .with_help("bind markers need a column, assignment or function parameter that fixes their type"),
                    );
                    return None;
                };
                ctx.params.push((
                    *i,
                    Param {
                        name: hint.name.clone(),
                        cql_type: t.clone(),
                        nullable: hint.nullable,
                        source: hint.source.clone(),
                        span: e.span,
                    },
                ));
                return Some(Typed {
                    ty: t.clone(),
                    nullable: false,
                });
            }
            ExprKind::Literal(l) => return self.literal(l, e.span, expect, hint),
            ExprKind::Column(id) => {
                if place != Place::Select {
                    self.error(
                        Code::InvalidRestriction,
                        e.span,
                        "column references are only allowed in the selection list",
                    );
                    return None;
                }
                let col = self.column(table, id)?;
                Typed {
                    ty: col.cql_type.clone(),
                    nullable: col.nullable(),
                }
            }
            ExprKind::Field(base, field) => {
                let b = self.expr(ctx, table, base, None, hint, place)?;
                match b.ty.unfrozen() {
                    CqlType::Udt(name) => {
                        let udt = self.schema.udt(name)?;
                        match udt.fields.iter().find(|(f, _)| *f == field.name) {
                            Some((_, ty)) => Typed {
                                ty: ty.clone(),
                                nullable: true,
                            },
                            None => {
                                let names: Vec<&str> =
                                    udt.fields.iter().map(|(f, _)| f.as_str()).collect();
                                self.diags.push(
                                    Diagnostic::error(
                                        Code::UnknownColumn,
                                        field.span,
                                        format!("type `{name}` has no field `{}`", field.name),
                                    )
                                    .with_help(format!("fields: {}", names.join(", "))),
                                );
                                return None;
                            }
                        }
                    }
                    other => {
                        self.error(
                            Code::TypeMismatch,
                            e.span,
                            format!("`{other}` has no fields; `.field` needs a user-defined type"),
                        );
                        return None;
                    }
                }
            }
            ExprKind::Index(base, idx) => {
                let b = self.expr(ctx, table, base, None, hint, place)?;
                let col_name = match &base.kind {
                    ExprKind::Column(c) => c.name.clone(),
                    _ => "element".to_string(),
                };
                let key_hint = Hint {
                    name: format!("{col_name}_key"),
                    source: ParamSource::ElementKey { column: col_name },
                    nullable: false,
                };
                match b.ty.unfrozen().clone() {
                    CqlType::Map(k, v) => {
                        self.expr(ctx, table, idx, Some(&k), &key_hint, place)?;
                        Typed {
                            ty: *v,
                            nullable: true,
                        }
                    }
                    CqlType::List(el) => {
                        self.expr(ctx, table, idx, Some(&CqlType::Int), &key_hint, place)?;
                        Typed {
                            ty: *el,
                            nullable: true,
                        }
                    }
                    other => {
                        self.error(
                            Code::TypeMismatch,
                            e.span,
                            format!("`{other}` cannot be indexed; `[..]` needs a map or list"),
                        );
                        return None;
                    }
                }
            }
            ExprKind::Call { name, args, star } => {
                self.call(ctx, table, e, name, args, *star, hint, place)?
            }
            ExprKind::Cast(inner, to) => {
                let t = self.expr(ctx, table, inner, None, hint, place)?;
                if !cast_ok(&t.ty, to) {
                    self.error(
                        Code::TypeMismatch,
                        e.span,
                        format!("cannot cast `{}` to `{to}`", t.ty),
                    );
                    return None;
                }
                Typed {
                    ty: to.clone(),
                    nullable: t.nullable,
                }
            }
            ExprKind::List(_)
            | ExprKind::Tuple(_)
            | ExprKind::Set(_)
            | ExprKind::Map(_)
            | ExprKind::Udt(_)
            | ExprKind::EmptyBraces => return self.composite(ctx, table, e, expect, hint, place),
        };
        if let Some(want) = expect {
            #[allow(clippy::collapsible_if)]
            if !assignable(&typed.ty, want) {
                self.error(
                    Code::TypeMismatch,
                    e.span,
                    format!(
                        "expression has type `{}` but `{want}` is expected here",
                        typed.ty
                    ),
                );
                return None;
            }
        }
        Some(typed)
    }

    fn literal(
        &mut self,
        l: &Literal,
        span: Span,
        expect: Option<&CqlType>,
        hint: &Hint,
    ) -> Option<Typed> {
        match expect {
            Some(t) => {
                if matches!(l, Literal::Null) {
                    if !hint.nullable {
                        self.error(Code::TypeMismatch, span, "NULL is not allowed here");
                        return None;
                    }
                } else if !literal_fits(l, t) {
                    self.error(
                        Code::TypeMismatch,
                        span,
                        format!("literal does not fit type `{t}`"),
                    );
                    return None;
                }
                Some(Typed {
                    ty: t.clone(),
                    nullable: matches!(l, Literal::Null),
                })
            }
            None => {
                let ty = match l {
                    Literal::String(_) => CqlType::Text,
                    Literal::Integer(n) => {
                        if n.parse::<i32>().is_ok() {
                            CqlType::Int
                        } else {
                            CqlType::BigInt
                        }
                    }
                    Literal::Float(_) => CqlType::Double,
                    Literal::Bool(_) => CqlType::Boolean,
                    Literal::Uuid(_) => CqlType::Uuid,
                    Literal::Blob(_) => CqlType::Blob,
                    Literal::Null => {
                        self.error(
                            Code::TypeMismatch,
                            span,
                            "the type of NULL cannot be inferred here",
                        );
                        return None;
                    }
                };
                Some(Typed {
                    ty,
                    nullable: false,
                })
            }
        }
    }

    /// Collection, tuple and UDT literals: typed by the expected type.
    fn composite(
        &mut self,
        ctx: &mut Ctx,
        table: &'a Table,
        e: &Expr,
        expect: Option<&CqlType>,
        hint: &Hint,
        place: Place,
    ) -> Option<Typed> {
        let Some(want) = expect else {
            self.diags.push(
                Diagnostic::error(
                    Code::UntypedBindMarker,
                    e.span,
                    "the type of this collection literal cannot be inferred here",
                )
                .with_help("use it where a column or parameter fixes its type"),
            );
            return None;
        };
        let inner = Hint {
            nullable: false,
            ..hint.clone()
        };
        let mismatch = |this: &mut Self, what: &str| {
            this.error(
                Code::TypeMismatch,
                e.span,
                format!("{what} literal does not fit type `{want}`"),
            );
            None
        };
        match (&e.kind, want.unfrozen()) {
            (ExprKind::List(items), CqlType::List(el))
            | (ExprKind::Set(items), CqlType::Set(el)) => {
                let mut ok = true;
                for it in items {
                    ok &= self.expr(ctx, table, it, Some(el), &inner, place).is_some();
                }
                ok.then(|| Typed {
                    ty: want.clone(),
                    nullable: false,
                })
            }
            (ExprKind::Map(pairs), CqlType::Map(k, v)) => {
                let mut ok = true;
                for (ke, ve) in pairs {
                    ok &= self.expr(ctx, table, ke, Some(k), &inner, place).is_some();
                    ok &= self.expr(ctx, table, ve, Some(v), &inner, place).is_some();
                }
                ok.then(|| Typed {
                    ty: want.clone(),
                    nullable: false,
                })
            }
            (ExprKind::EmptyBraces, CqlType::Set(_) | CqlType::Map(..)) => Some(Typed {
                ty: want.clone(),
                nullable: false,
            }),
            (ExprKind::Tuple(items), CqlType::Tuple(ts)) => {
                if items.len() != ts.len() {
                    self.error(
                        Code::Arity,
                        e.span,
                        format!(
                            "tuple has {} elements but `{want}` has {}",
                            items.len(),
                            ts.len()
                        ),
                    );
                    return None;
                }
                let mut ok = true;
                for (it, t) in items.iter().zip(ts) {
                    ok &= self.expr(ctx, table, it, Some(t), &inner, place).is_some();
                }
                ok.then(|| Typed {
                    ty: want.clone(),
                    nullable: false,
                })
            }
            (ExprKind::Udt(fields), CqlType::Udt(name)) => {
                let udt = self.schema.udt(name)?;
                let mut ok = true;
                let mut seen = HashSet::new();
                for (f, val) in fields {
                    if !seen.insert(f.name.as_str()) {
                        self.error(
                            Code::Duplicate,
                            f.span,
                            format!("field `{}` is set more than once", f.name),
                        );
                        ok = false;
                        continue;
                    }
                    match udt.fields.iter().find(|(n, _)| *n == f.name) {
                        Some((_, fty)) => {
                            let field_hint = Hint {
                                nullable: true,
                                ..hint.clone()
                            };
                            ok &= self
                                .expr(ctx, table, val, Some(fty), &field_hint, place)
                                .is_some();
                        }
                        None => {
                            let names: Vec<&str> =
                                udt.fields.iter().map(|(n, _)| n.as_str()).collect();
                            self.diags.push(
                                Diagnostic::error(
                                    Code::UnknownColumn,
                                    f.span,
                                    format!("type `{name}` has no field `{}`", f.name),
                                )
                                .with_help(format!("fields: {}", names.join(", "))),
                            );
                            ok = false;
                        }
                    }
                }
                ok.then(|| Typed {
                    ty: want.clone(),
                    nullable: false,
                })
            }
            (ExprKind::List(_), _) => mismatch(self, "list"),
            (ExprKind::Set(_) | ExprKind::EmptyBraces, _) => mismatch(self, "set"),
            (ExprKind::Map(_), _) => mismatch(self, "map"),
            (ExprKind::Tuple(_), _) => mismatch(self, "tuple"),
            (ExprKind::Udt(_), _) => mismatch(self, "user-defined type"),
            _ => unreachable!("composite called with a non-composite expression"),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn call(
        &mut self,
        ctx: &mut Ctx,
        table: &'a Table,
        e: &Expr,
        name: &Ident,
        args: &[Expr],
        star: bool,
        hint: &Hint,
        place: Place,
    ) -> Option<Typed> {
        let lname = name.name.to_ascii_lowercase();
        let want_args = |this: &mut Self, n: usize| -> bool {
            if star || args.len() != n {
                this.error(
                    Code::Arity,
                    e.span,
                    format!(
                        "function `{lname}` takes {n} argument{}",
                        if n == 1 { "" } else { "s" }
                    ),
                );
                false
            } else {
                true
            }
        };
        let only_select = |this: &mut Self| -> bool {
            if place != Place::Select {
                this.error(
                    Code::InvalidRestriction,
                    e.span,
                    format!("`{lname}` is only allowed in the selection list"),
                );
                false
            } else {
                true
            }
        };
        let plain = |ty: CqlType| {
            Some(Typed {
                ty,
                nullable: false,
            })
        };
        match lname.as_str() {
            "now" | "currenttimeuuid" => {
                if want_args(self, 0) {
                    plain(CqlType::TimeUuid)
                } else {
                    None
                }
            }
            "uuid" => {
                if want_args(self, 0) {
                    plain(CqlType::Uuid)
                } else {
                    None
                }
            }
            "currenttimestamp" => {
                if want_args(self, 0) {
                    plain(CqlType::Timestamp)
                } else {
                    None
                }
            }
            "currentdate" => {
                if want_args(self, 0) {
                    plain(CqlType::Date)
                } else {
                    None
                }
            }
            "currenttime" => {
                if want_args(self, 0) {
                    plain(CqlType::Time)
                } else {
                    None
                }
            }
            "totimestamp" | "todate" | "tounixtimestamp" => {
                if !want_args(self, 1) {
                    return None;
                }
                let a = self.expr(ctx, table, &args[0], None, hint, place)?;
                use CqlType::*;
                let allowed: &[CqlType] = match lname.as_str() {
                    "totimestamp" => &[TimeUuid, Date],
                    "todate" => &[TimeUuid, Timestamp],
                    _ => &[TimeUuid, Timestamp, Date],
                };
                if !allowed.contains(a.ty.unfrozen()) {
                    self.error(
                        Code::TypeMismatch,
                        args[0].span,
                        format!("`{lname}` does not accept `{}`", a.ty),
                    );
                    return None;
                }
                let ty = match lname.as_str() {
                    "totimestamp" => Timestamp,
                    "todate" => Date,
                    _ => BigInt,
                };
                Some(Typed {
                    ty,
                    nullable: a.nullable,
                })
            }
            "mintimeuuid" | "maxtimeuuid" => {
                if !want_args(self, 1) {
                    return None;
                }
                let a = self.expr(ctx, table, &args[0], Some(&CqlType::Timestamp), hint, place)?;
                Some(Typed {
                    ty: CqlType::TimeUuid,
                    nullable: a.nullable,
                })
            }
            "token" => {
                if place != Place::Select {
                    self.error(
                        Code::InvalidRestriction,
                        e.span,
                        "`token(...)` is only valid in the selection list or as a relation",
                    );
                    return None;
                }
                let names: Vec<Option<&str>> = args
                    .iter()
                    .map(|a| match &a.kind {
                        ExprKind::Column(c) => Some(c.name.as_str()),
                        _ => None,
                    })
                    .collect();
                let pk: Vec<Option<&str>> = table
                    .partition_key
                    .iter()
                    .map(|k| Some(k.as_str()))
                    .collect();
                if star || names != pk {
                    self.error(
                        Code::InvalidRestriction,
                        e.span,
                        format!(
                            "`token(...)` must list all partition key columns in order: {}",
                            table.partition_key.join(", ")
                        ),
                    );
                    return None;
                }
                plain(CqlType::BigInt)
            }
            "writetime" | "ttl" => {
                if !only_select(self) || !want_args(self, 1) {
                    return None;
                }
                let ExprKind::Column(id) = &args[0].kind else {
                    self.error(
                        Code::InvalidRestriction,
                        args[0].span,
                        format!("`{lname}` needs a plain column"),
                    );
                    return None;
                };
                let col = self.column(table, id)?;
                if col.is_key()
                    || col.cql_type.is_collection()
                    || matches!(col.cql_type, CqlType::Counter)
                {
                    self.error(
                        Code::InvalidRestriction,
                        args[0].span,
                        format!("`{lname}` is not defined for primary key, counter or collection column `{}`", col.name),
                    );
                    return None;
                }
                Some(Typed {
                    ty: if lname == "ttl" {
                        CqlType::Int
                    } else {
                        CqlType::BigInt
                    },
                    nullable: true,
                })
            }
            "count" => {
                if !only_select(self) {
                    return None;
                }
                if star {
                    return plain(CqlType::BigInt);
                }
                if !want_args(self, 1) {
                    return None;
                }
                match &args[0].kind {
                    ExprKind::Literal(Literal::Integer(n)) if n == "1" => {}
                    ExprKind::Column(_) => {
                        self.expr(ctx, table, &args[0], None, hint, place)?;
                    }
                    _ => {
                        self.error(
                            Code::InvalidRestriction,
                            args[0].span,
                            "`count` needs `*`, `1` or a column",
                        );
                        return None;
                    }
                }
                plain(CqlType::BigInt)
            }
            "sum" | "avg" | "min" | "max" => {
                if !only_select(self) || !want_args(self, 1) {
                    return None;
                }
                let ExprKind::Column(_) = &args[0].kind else {
                    self.error(
                        Code::InvalidRestriction,
                        args[0].span,
                        format!("`{lname}` needs a plain column"),
                    );
                    return None;
                };
                let a = self.expr(ctx, table, &args[0], None, hint, place)?;
                let summable = a.ty.is_numeric() && !matches!(a.ty.unfrozen(), CqlType::Counter);
                let orderable = summable
                    || matches!(
                        a.ty.unfrozen(),
                        CqlType::Text
                            | CqlType::Ascii
                            | CqlType::Timestamp
                            | CqlType::Date
                            | CqlType::Time
                            | CqlType::Uuid
                            | CqlType::TimeUuid
                            | CqlType::Boolean
                            | CqlType::Blob
                            | CqlType::Inet
                    );
                let ok = if lname == "sum" || lname == "avg" {
                    summable
                } else {
                    orderable
                };
                if !ok {
                    self.error(
                        Code::TypeMismatch,
                        args[0].span,
                        format!("`{lname}` is not defined for `{}`", a.ty),
                    );
                    return None;
                }
                Some(Typed {
                    ty: a.ty.unfrozen().clone(),
                    nullable: lname == "min" || lname == "max",
                })
            }
            _ => {
                self.diags.push(
                    Diagnostic::error(
                        Code::UnknownFunction,
                        name.span,
                        format!("unknown or unsupported function `{}`", name.name),
                    )
                    .with_help(
                        "supported: now, uuid, currentTimestamp/Date/Time/TimeUUID, toTimestamp, toDate, \
                         toUnixTimestamp, minTimeuuid, maxTimeuuid, token, writetime, ttl, count, sum, avg, \
                         min, max, cast",
                    ),
                );
                None
            }
        }
    }

    // ----- relations -----------------------------------------------------

    fn relations(
        &mut self,
        table: &'a Table,
        rels: &[Relation],
        ctx: &mut Ctx,
        place: Place,
    ) -> Vec<Restriction<'a>> {
        let mut out = Vec::new();
        let cond = place == Place::Condition;
        let source = |column: &str, op: RelOp| {
            if cond {
                ParamSource::Condition {
                    column: column.to_string(),
                    op,
                }
            } else {
                ParamSource::Restriction {
                    column: column.to_string(),
                    op,
                }
            }
        };
        for r in rels {
            if r.op == RelOp::Ne && !cond {
                self.error(
                    Code::InvalidRestriction,
                    r.span,
                    "`!=` is only valid in `IF` conditions, not in `WHERE`",
                );
                continue;
            }
            if cond && matches!(r.op, RelOp::Contains | RelOp::ContainsKey) {
                self.error(
                    Code::InvalidRestriction,
                    r.span,
                    "`CONTAINS` is not valid in `IF` conditions",
                );
                continue;
            }
            match &r.lhs {
                RelLhs::Column(id) => {
                    let Some(col) = self.column(table, id) else {
                        continue;
                    };
                    if cond && col.is_key() {
                        self.error(
                            Code::InvalidRestriction,
                            id.span,
                            format!(
                                "`IF` conditions cannot test primary key column `{}`",
                                col.name
                            ),
                        );
                        continue;
                    }
                    let hint = Hint {
                        name: col.name.clone(),
                        source: source(&col.name, r.op),
                        nullable: false,
                    };
                    let ty = &col.cql_type;
                    let ok = match (r.op, &r.rhs) {
                        (RelOp::In, Rhs::InList(items)) => items
                            .iter()
                            .map(|it| {
                                self.expr(ctx, table, it, Some(ty), &hint, Place::Where)
                                    .is_some()
                            })
                            .collect::<Vec<_>>()
                            .iter()
                            .all(|b| *b),
                        (RelOp::In, Rhs::Expr(e)) => {
                            let list = CqlType::List(Box::new(ty.clone()));
                            self.expr(ctx, table, e, Some(&list), &hint, Place::Where)
                                .is_some()
                        }
                        (RelOp::Contains | RelOp::ContainsKey, Rhs::Expr(e)) => {
                            let want = match (r.op, ty.unfrozen()) {
                                (RelOp::Contains, CqlType::List(t) | CqlType::Set(t)) => {
                                    Some((**t).clone())
                                }
                                (RelOp::Contains, CqlType::Map(_, v)) => Some((**v).clone()),
                                (RelOp::ContainsKey, CqlType::Map(k, _)) => Some((**k).clone()),
                                _ => None,
                            };
                            match want {
                                Some(w) => self
                                    .expr(ctx, table, e, Some(&w), &hint, Place::Where)
                                    .is_some(),
                                None => {
                                    self.error(
                                        Code::TypeMismatch,
                                        id.span,
                                        format!(
                                            "`CONTAINS` needs a collection column, `{}` is `{ty}`",
                                            col.name
                                        ),
                                    );
                                    false
                                }
                            }
                        }
                        (_, Rhs::Expr(e)) => self
                            .expr(ctx, table, e, Some(ty), &hint, Place::Where)
                            .is_some(),
                        (_, Rhs::InList(_)) => false,
                    };
                    let _ = ok; // errors are already recorded; keep the restriction to avoid follow-up warnings
                    out.push(Restriction {
                        column: col,
                        op: r.op,
                        via_token: false,
                        in_tuple: false,
                    });
                }
                RelLhs::Token(ids) => {
                    if cond {
                        self.error(
                            Code::InvalidRestriction,
                            r.span,
                            "`token(...)` is not valid in `IF` conditions",
                        );
                        continue;
                    }
                    let names: Vec<&str> = ids.iter().map(|i| i.name.as_str()).collect();
                    let pk: Vec<&str> = table.partition_key.iter().map(String::as_str).collect();
                    if names != pk {
                        self.error(
                            Code::InvalidRestriction,
                            r.span,
                            format!(
                                "`token(...)` must list all partition key columns in order: {}",
                                pk.join(", ")
                            ),
                        );
                        continue;
                    }
                    let hint = Hint {
                        name: "token".into(),
                        source: source("token", r.op),
                        nullable: false,
                    };
                    let Rhs::Expr(rhs) = &r.rhs else {
                        self.error(
                            Code::InvalidRestriction,
                            r.span,
                            "`token(...)` does not support `IN`",
                        );
                        continue;
                    };
                    let ok = match &rhs.kind {
                        ExprKind::Call { name, args, star } if name.name == "token" => {
                            if *star || args.len() != ids.len() {
                                self.error(
                                    Code::Arity,
                                    rhs.span,
                                    format!("`token(...)` takes {} arguments", ids.len()),
                                );
                                false
                            } else {
                                let mut ok = true;
                                for (a, key) in args.iter().zip(&table.partition_key) {
                                    let col =
                                        table.column(key).expect("partition key column exists");
                                    let h = Hint {
                                        name: col.name.clone(),
                                        ..hint.clone()
                                    };
                                    ok &= self
                                        .expr(ctx, table, a, Some(&col.cql_type), &h, Place::Where)
                                        .is_some();
                                }
                                ok
                            }
                        }
                        _ => self
                            .expr(ctx, table, rhs, Some(&CqlType::BigInt), &hint, Place::Where)
                            .is_some(),
                    };
                    if ok {
                        for k in &table.partition_key {
                            out.push(Restriction {
                                column: table.column(k).expect("partition key column exists"),
                                op: r.op,
                                via_token: true,
                                in_tuple: false,
                            });
                        }
                    }
                }
                RelLhs::Tuple(ids) => {
                    if cond {
                        self.error(
                            Code::InvalidRestriction,
                            r.span,
                            "tuple relations are not valid in `IF` conditions",
                        );
                        continue;
                    }
                    let cols: Vec<&'a Column> =
                        ids.iter().filter_map(|i| self.column(table, i)).collect();
                    if cols.len() != ids.len() {
                        continue;
                    }
                    if let Some(c) = cols.iter().find(|c| c.role != ColumnRole::ClusteringKey) {
                        self.error(
                            Code::InvalidRestriction,
                            r.span,
                            format!("tuple relations are only valid on clustering columns, `{}` is not one", c.name),
                        );
                        continue;
                    }
                    let tuple_ty =
                        CqlType::Tuple(cols.iter().map(|c| c.cql_type.clone()).collect());
                    let ok = self.tuple_rhs(ctx, table, r, &cols, &tuple_ty);
                    if ok {
                        for c in cols {
                            out.push(Restriction {
                                column: c,
                                op: r.op,
                                via_token: false,
                                in_tuple: true,
                            });
                        }
                    }
                }
                RelLhs::Element(id, key) => {
                    if cond {
                        self.unsupported(r.span, "element conditions (`IF col[key] = ?`)");
                        continue;
                    }
                    let Some(col) = self.column(table, id) else {
                        continue;
                    };
                    let (key_ty, val_ty) = match col.cql_type.unfrozen() {
                        CqlType::Map(k, v) => ((**k).clone(), (**v).clone()),
                        CqlType::List(el) => (CqlType::Int, (**el).clone()),
                        other => {
                            self.error(
                                Code::TypeMismatch,
                                id.span,
                                format!("`{other}` cannot be indexed; `[..]` needs a map or list"),
                            );
                            continue;
                        }
                    };
                    let key_hint = Hint {
                        name: format!("{}_key", col.name),
                        source: ParamSource::ElementKey {
                            column: col.name.clone(),
                        },
                        nullable: false,
                    };
                    let val_hint = Hint {
                        name: format!("{}_value", col.name),
                        source: source(&col.name, r.op),
                        nullable: false,
                    };
                    let Rhs::Expr(rhs) = &r.rhs else {
                        self.error(
                            Code::InvalidRestriction,
                            r.span,
                            "`IN` is not supported on element restrictions",
                        );
                        continue;
                    };
                    let a = self
                        .expr(ctx, table, key, Some(&key_ty), &key_hint, Place::Where)
                        .is_some();
                    let b = self
                        .expr(ctx, table, rhs, Some(&val_ty), &val_hint, Place::Where)
                        .is_some();
                    if a && b {
                        out.push(Restriction {
                            column: col,
                            op: r.op,
                            via_token: false,
                            in_tuple: false,
                        });
                    }
                }
                RelLhs::Field(..) => {
                    self.unsupported(r.span, "restrictions on user-defined type fields")
                }
            }
        }
        out
    }

    /// Right side of a tuple relation: tuple literals are typed per
    /// element so each marker keeps its column's name.
    fn tuple_rhs(
        &mut self,
        ctx: &mut Ctx,
        table: &'a Table,
        r: &Relation,
        cols: &[&'a Column],
        tuple_ty: &CqlType,
    ) -> bool {
        let source = ParamSource::Restriction {
            column: cols[0].name.clone(),
            op: r.op,
        };
        let one = |this: &mut Self, ctx: &mut Ctx, e: &Expr| -> bool {
            match &e.kind {
                ExprKind::Tuple(items) if items.len() == cols.len() => {
                    let mut ok = true;
                    for (it, col) in items.iter().zip(cols) {
                        let h = Hint {
                            name: col.name.clone(),
                            source: ParamSource::Restriction {
                                column: col.name.clone(),
                                op: r.op,
                            },
                            nullable: false,
                        };
                        ok &= this
                            .expr(ctx, table, it, Some(&col.cql_type), &h, Place::Where)
                            .is_some();
                    }
                    ok
                }
                _ => {
                    let h = Hint {
                        name: cols
                            .iter()
                            .map(|c| c.name.as_str())
                            .collect::<Vec<_>>()
                            .join("_"),
                        source: source.clone(),
                        nullable: false,
                    };
                    this.expr(ctx, table, e, Some(tuple_ty), &h, Place::Where)
                        .is_some()
                }
            }
        };
        match (&r.op, &r.rhs) {
            (RelOp::In, Rhs::InList(items)) => items
                .iter()
                .map(|e| one(self, ctx, e))
                .collect::<Vec<_>>()
                .iter()
                .all(|b| *b),
            (RelOp::In, Rhs::Expr(e)) => {
                let list = CqlType::List(Box::new(tuple_ty.clone()));
                let h = Hint {
                    name: cols
                        .iter()
                        .map(|c| c.name.as_str())
                        .collect::<Vec<_>>()
                        .join("_"),
                    source: source.clone(),
                    nullable: false,
                };
                self.expr(ctx, table, e, Some(&list), &h, Place::Where)
                    .is_some()
            }
            (_, Rhs::Expr(e)) => one(self, ctx, e),
            _ => false,
        }
    }

    // ----- SELECT --------------------------------------------------------

    fn select(&mut self, s: &Select, ctx: &mut Ctx) -> Option<Outcome<'a>> {
        let table = self.table(&s.table)?;
        let mut columns = Vec::new();
        match &s.selection {
            Selection::Star(_) => {
                for c in &table.columns {
                    columns.push(result_column(c, &c.name));
                }
            }
            Selection::Items(items) => {
                for item in items {
                    if let ExprKind::Column(id) = &item.expr.kind {
                        let Some(c) = self.column(table, id) else {
                            continue;
                        };
                        let name = item.alias.as_ref().map_or(&c.name, |a| &a.name);
                        if s.mode == SelectMode::Distinct
                            && !(c.role == ColumnRole::PartitionKey || c.is_static)
                        {
                            self.error(
                                Code::InvalidRestriction,
                                id.span,
                                format!("`SELECT DISTINCT` only accepts partition key and static columns, `{}` is neither", c.name),
                            );
                            continue;
                        }
                        columns.push(result_column(c, name));
                        continue;
                    }
                    let Some(alias) = &item.alias else {
                        self.diags.push(
                            Diagnostic::error(
                                Code::MissingAlias,
                                item.expr.span,
                                "this expression needs an alias",
                            )
                            .with_help(
                                "add `AS name`; cqlc relies on the alias to name the result field",
                            ),
                        );
                        continue;
                    };
                    if s.mode == SelectMode::Distinct {
                        self.error(
                            Code::InvalidRestriction,
                            item.expr.span,
                            "`SELECT DISTINCT` only accepts columns",
                        );
                        continue;
                    }
                    let hint = Hint {
                        name: alias.name.clone(),
                        source: ParamSource::Expression,
                        nullable: false,
                    };
                    if let Some(t) = self.expr(ctx, table, &item.expr, None, &hint, Place::Select) {
                        columns.push(ResultColumn {
                            name: alias.name.clone(),
                            cql_type: t.ty,
                            nullable: t.nullable,
                        });
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
        if s.mode == SelectMode::Json {
            columns = vec![ResultColumn {
                name: "[json]".into(),
                cql_type: CqlType::Text,
                nullable: false,
            }];
        }
        let restrictions = self.relations(table, &s.where_clause, ctx, Place::Where);

        if !s.group_by.is_empty() {
            let key_order: Vec<&str> = table
                .partition_key
                .iter()
                .map(String::as_str)
                .chain(table.clustering.iter().map(|c| c.name.as_str()))
                .collect();
            for (i, g) in s.group_by.iter().enumerate() {
                if self.column(table, g).is_none() {
                    continue;
                }
                if key_order.get(i) != Some(&g.name.as_str()) {
                    self.error(
                        Code::InvalidRestriction,
                        g.span,
                        "`GROUP BY` must list primary key columns in key order, starting with the partition key",
                    );
                    break;
                }
            }
        }
        for (col, _) in &s.order_by {
            if self
                .column(table, col)
                .is_some_and(|c| c.role != ColumnRole::ClusteringKey)
            {
                self.error(
                    Code::InvalidRestriction,
                    col.span,
                    format!(
                        "cannot order by `{}`: only clustering columns can be ordered",
                        col.name
                    ),
                );
            }
        }
        for (e, name, source) in [
            (
                &s.per_partition_limit,
                "per_partition_limit",
                ParamSource::PerPartitionLimit,
            ),
            (&s.limit, "limit", ParamSource::Limit),
        ] {
            if let Some(e) = e {
                let hint = Hint {
                    name: name.into(),
                    source,
                    nullable: false,
                };
                self.expr(ctx, table, e, Some(&CqlType::Int), &hint, Place::Where);
            }
        }
        self.select_warnings(s, table, &restrictions);
        Some(Outcome {
            kind: StatementKind::Select,
            table,
            columns,
        })
    }

    fn select_warnings(&mut self, s: &Select, table: &Table, rs: &[Restriction<'_>]) {
        for r in rs {
            if r.column.role == ColumnRole::PartitionKey && !r.op.is_eq_or_in() && !r.via_token {
                self.diags.push(
                    Diagnostic::error(
                        Code::InvalidRestriction,
                        s.span,
                        format!(
                            "partition key column `{}` only supports `=` and `IN` restrictions",
                            r.column.name
                        ),
                    )
                    .with_help("for range scans over partitions use `token(...)`"),
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
            let own: Vec<&Restriction<'_>> =
                rs.iter().filter(|r| r.column.name == c.name).collect();
            let ops: Vec<RelOp> = own.iter().map(|r| r.op).collect();
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
            if own.iter().any(|r| r.op.is_range() && !r.in_tuple) {
                blocked = Some((&c.name, "is restricted by a range"));
            }
        }
        // ORDER BY must follow the clustering order (or exactly reverse it).
        if !s.order_by.is_empty() {
            let eq_restricted: HashSet<&str> = rs
                .iter()
                .filter(|r| r.op == RelOp::Eq)
                .map(|r| r.column.name.as_str())
                .collect();
            let seq: Vec<_> = table
                .clustering
                .iter()
                .filter(|c| !eq_restricted.contains(c.name.as_str()))
                .collect();
            let prefix_ok = s.order_by.len() <= seq.len()
                && s.order_by
                    .iter()
                    .zip(&seq)
                    .all(|((o, _), c)| o.name == c.name);
            let declared = |name: &str| {
                table
                    .clustering
                    .iter()
                    .find(|c| c.name == name)
                    .map(|c| c.order)
            };
            let same = s
                .order_by
                .iter()
                .all(|(o, d)| declared(&o.name) == Some(*d));
            let reversed = s
                .order_by
                .iter()
                .all(|(o, d)| declared(&o.name).is_some_and(|x| x != *d));
            if !(prefix_ok && (same || reversed)) {
                self.diags.push(
                    Diagnostic::warning(Code::OrderBy, s.span, "`ORDER BY` does not follow the clustering order")
                        .with_help(
                            "Cassandra only accepts the declared clustering order or its exact reverse, \
                             skipping columns restricted by `=`",
                        ),
                );
            }
        }
    }

    // ----- writes --------------------------------------------------------

    fn using(&mut self, ctx: &mut Ctx, table: &'a Table, using: &Using) {
        for (e, name, ty, source) in [
            (&using.ttl, "ttl", CqlType::Int, ParamSource::Ttl),
            (
                &using.timestamp,
                "timestamp",
                CqlType::BigInt,
                ParamSource::Timestamp,
            ),
        ] {
            if let Some(e) = e {
                let hint = Hint {
                    name: name.into(),
                    source,
                    nullable: false,
                };
                self.expr(ctx, table, e, Some(&ty), &hint, Place::Value);
            }
        }
    }

    fn insert(&mut self, s: &Insert, ctx: &mut Ctx) -> Option<Outcome<'a>> {
        let table = self.table(&s.table)?;
        match &s.source {
            InsertSource::Json(e) => {
                let hint = Hint {
                    name: "json".into(),
                    source: ParamSource::Json,
                    nullable: false,
                };
                self.expr(ctx, table, e, Some(&CqlType::Text), &hint, Place::Value);
            }
            InsertSource::Values { columns, values } => {
                if columns.len() != values.len() {
                    self.error(
                        Code::Arity,
                        s.span,
                        format!("{} columns but {} values", columns.len(), values.len()),
                    );
                }
                let mut seen = HashSet::new();
                for (id, e) in columns.iter().zip(values) {
                    let Some(col) = self.column(table, id) else {
                        continue;
                    };
                    if !seen.insert(col.name.as_str()) {
                        self.error(
                            Code::Duplicate,
                            id.span,
                            format!("column `{}` is listed more than once", col.name),
                        );
                        continue;
                    }
                    if matches!(col.cql_type, CqlType::Counter) {
                        self.error(
                            Code::InvalidRestriction,
                            id.span,
                            format!("counter column `{}` cannot be inserted; use `UPDATE ... SET {0} = {0} + ?`", col.name),
                        );
                        continue;
                    }
                    let hint = Hint {
                        name: col.name.clone(),
                        source: ParamSource::Assignment {
                            column: col.name.clone(),
                        },
                        nullable: col.nullable(),
                    };
                    self.expr(ctx, table, e, Some(&col.cql_type), &hint, Place::Value);
                }
                let missing: Vec<&str> = table
                    .columns
                    .iter()
                    .filter(|c| c.is_key() && !columns.iter().any(|i| i.name == c.name))
                    .map(|c| c.name.as_str())
                    .collect();
                if !missing.is_empty() {
                    self.error(
                        Code::IncompleteKey,
                        s.span,
                        format!(
                            "INSERT must set all primary key columns; missing: {}",
                            missing.join(", ")
                        ),
                    );
                }
            }
        }
        self.using(ctx, table, &s.using);
        let columns = if s.if_not_exists {
            table.columns.iter().map(lwt_column).collect()
        } else {
            vec![]
        };
        Some(Outcome {
            kind: StatementKind::Insert,
            table,
            columns,
        })
    }

    fn update(&mut self, s: &Update, ctx: &mut Ctx) -> Option<Outcome<'a>> {
        let table = self.table(&s.table)?;
        for a in &s.assignments {
            self.assignment(ctx, table, a);
        }
        self.using(ctx, table, &s.using);
        let rs = self.relations(table, &s.where_clause, ctx, Place::Where);
        self.check_write_restrictions(table, &rs, s.span, true);
        let columns = self.if_clause(ctx, table, &s.if_clause);
        Some(Outcome {
            kind: StatementKind::Update,
            table,
            columns,
        })
    }

    fn delete(&mut self, s: &Delete, ctx: &mut Ctx) -> Option<Outcome<'a>> {
        let table = self.table(&s.table)?;
        for t in &s.targets {
            self.delete_target(ctx, table, t);
        }
        self.using(ctx, table, &s.using);
        let rs = self.relations(table, &s.where_clause, ctx, Place::Where);
        self.check_write_restrictions(table, &rs, s.span, false);
        let columns = self.if_clause(ctx, table, &s.if_clause);
        Some(Outcome {
            kind: StatementKind::Delete,
            table,
            columns,
        })
    }

    /// Checks `IF` conditions and returns the columns the database reports
    /// back when they fail.
    fn if_clause(
        &mut self,
        ctx: &mut Ctx,
        table: &'a Table,
        clause: &Option<IfClause>,
    ) -> Vec<ResultColumn> {
        match clause {
            None | Some(IfClause::Exists) => vec![],
            Some(IfClause::Conditions(rels)) => {
                let rs = self.relations(table, rels, ctx, Place::Condition);
                let mut seen = HashSet::new();
                rs.iter()
                    .filter(|r| seen.insert(r.column.name.as_str()))
                    .map(|r| lwt_column(r.column))
                    .collect()
            }
        }
    }

    fn assignment(&mut self, ctx: &mut Ctx, table: &'a Table, a: &Assignment) {
        let Some(col) = self.column(table, &a.target.column) else {
            return;
        };
        let span = a.target.column.span;
        if col.is_key() {
            self.error(
                Code::InvalidRestriction,
                span,
                format!("cannot update primary key column `{}`", col.name),
            );
            return;
        }
        let name = col.name.clone();
        let ty = &col.cql_type;
        let whole = |nullable| Hint {
            name: name.clone(),
            source: ParamSource::Assignment {
                column: name.clone(),
            },
            nullable,
        };
        match (&a.target.key, &a.target.field) {
            (None, None) => match a.op {
                AssignOp::Set => {
                    if matches!(ty, CqlType::Counter) {
                        self.error(
                            Code::InvalidRestriction,
                            span,
                            format!(
                                "counter column `{}` can only be changed with `{0} = {0} + ?`",
                                col.name
                            ),
                        );
                        return;
                    }
                    self.expr(ctx, table, &a.value, Some(ty), &whole(true), Place::Value);
                }
                AssignOp::Add | AssignOp::Sub | AssignOp::Prepend => {
                    let op = match a.op {
                        AssignOp::Add => "+",
                        AssignOp::Sub => "-",
                        _ => "prepend",
                    };
                    let want = match (ty.unfrozen(), a.op) {
                        (CqlType::Counter, AssignOp::Add | AssignOp::Sub) => Some(CqlType::Counter),
                        (CqlType::List(_), AssignOp::Add | AssignOp::Sub | AssignOp::Prepend)
                        | (CqlType::Set(_), AssignOp::Add | AssignOp::Sub)
                        | (CqlType::Map(..), AssignOp::Add) => {
                            (!ty.is_frozen()).then(|| ty.clone())
                        }
                        (CqlType::Map(k, _), AssignOp::Sub) => {
                            (!ty.is_frozen()).then(|| CqlType::Set(k.clone()))
                        }
                        _ => None,
                    };
                    match want {
                        Some(w) => {
                            self.expr(ctx, table, &a.value, Some(&w), &whole(false), Place::Value);
                        }
                        None => self.error(
                            Code::InvalidRestriction,
                            span,
                            format!("`{op}` is not valid for `{}` of type `{ty}` (frozen collections cannot be updated partially)", col.name),
                        ),
                    }
                }
            },
            (Some(key), None) => {
                if a.op != AssignOp::Set || ty.is_frozen() {
                    self.error(
                        Code::InvalidRestriction,
                        span,
                        "element assignment needs `col[key] = value` on a non-frozen map or list",
                    );
                    return;
                }
                let (kt, vt) = match ty.unfrozen() {
                    CqlType::Map(k, v) => ((**k).clone(), (**v).clone()),
                    CqlType::List(el) => (CqlType::Int, (**el).clone()),
                    _ => {
                        self.error(
                            Code::TypeMismatch,
                            span,
                            format!("`{}` is not a map or list", col.name),
                        );
                        return;
                    }
                };
                let kh = Hint {
                    name: format!("{name}_key"),
                    source: ParamSource::ElementKey {
                        column: name.clone(),
                    },
                    nullable: false,
                };
                let vh = Hint {
                    name: format!("{name}_value"),
                    source: ParamSource::ElementValue {
                        column: name.clone(),
                    },
                    nullable: false,
                };
                self.expr(ctx, table, key, Some(&kt), &kh, Place::Value);
                self.expr(ctx, table, &a.value, Some(&vt), &vh, Place::Value);
            }
            (None, Some(field)) => {
                let CqlType::Udt(udt_name) = ty else {
                    self.error(
                        Code::TypeMismatch,
                        span,
                        format!("`{}` is not a non-frozen user-defined type; field assignment needs one", col.name),
                    );
                    return;
                };
                let Some(udt) = self.schema.udt(udt_name) else {
                    return;
                };
                let Some((_, fty)) = udt.fields.iter().find(|(f, _)| *f == field.name) else {
                    self.error(
                        Code::UnknownColumn,
                        field.span,
                        format!("type `{udt_name}` has no field `{}`", field.name),
                    );
                    return;
                };
                if a.op != AssignOp::Set {
                    self.error(
                        Code::InvalidRestriction,
                        span,
                        "only `=` is valid for field assignment",
                    );
                    return;
                }
                let h = Hint {
                    name: format!("{name}_{}", field.name),
                    source: ParamSource::Assignment {
                        column: name.clone(),
                    },
                    nullable: true,
                };
                self.expr(ctx, table, &a.value, Some(fty), &h, Place::Value);
            }
            (Some(_), Some(_)) => unreachable!("the parser never produces key and field together"),
        }
    }

    fn delete_target(&mut self, ctx: &mut Ctx, table: &'a Table, t: &Target) {
        let Some(col) = self.column(table, &t.column) else {
            return;
        };
        if col.is_key() {
            self.error(
                Code::InvalidRestriction,
                t.column.span,
                format!("cannot delete primary key column `{}`", col.name),
            );
            return;
        }
        match (&t.key, &t.field) {
            (None, None) => {}
            (Some(key), None) => {
                let kt = match col.cql_type.unfrozen() {
                    CqlType::Map(k, _) if !col.cql_type.is_frozen() => (**k).clone(),
                    CqlType::List(_) if !col.cql_type.is_frozen() => CqlType::Int,
                    _ => {
                        self.error(
                            Code::InvalidRestriction,
                            t.column.span,
                            format!("`{}` is not a non-frozen map or list", col.name),
                        );
                        return;
                    }
                };
                let h = Hint {
                    name: format!("{}_key", col.name),
                    source: ParamSource::ElementKey {
                        column: col.name.clone(),
                    },
                    nullable: false,
                };
                self.expr(ctx, table, key, Some(&kt), &h, Place::Value);
            }
            (None, Some(field)) => {
                let ok = matches!(&col.cql_type, CqlType::Udt(n)
                    if self.schema.udt(n).is_some_and(|u| u.fields.iter().any(|(f, _)| *f == field.name)));
                if !ok {
                    self.error(
                        Code::InvalidRestriction,
                        field.span,
                        format!(
                            "`{}` is not a non-frozen user-defined type with field `{}`",
                            col.name, field.name
                        ),
                    );
                }
            }
            (Some(_), Some(_)) => unreachable!(),
        }
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
            if r.via_token {
                self.error(
                    Code::InvalidRestriction,
                    span,
                    "`token(...)` restrictions are not valid in write statements",
                );
            } else if !r.column.is_key() {
                self.error(
                    Code::InvalidRestriction,
                    span,
                    format!(
                        "cannot restrict non-key column `{}` in a write statement",
                        r.column.name
                    ),
                );
            } else if (full_key || r.column.role == ColumnRole::PartitionKey) && !r.op.is_eq_or_in()
            {
                self.error(
                    Code::InvalidRestriction,
                    span,
                    format!(
                        "key column `{}` only supports `=` and `IN` here",
                        r.column.name
                    ),
                );
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
            self.error(
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
            );
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

/// Columns reported by a failed conditional write may always be absent.
fn lwt_column(c: &Column) -> ResultColumn {
    ResultColumn {
        name: c.name.clone(),
        cql_type: c.cql_type.clone(),
        nullable: true,
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

/// Structural equality ignoring `frozen<>` wrappers.
fn same_type(a: &CqlType, b: &CqlType) -> bool {
    use CqlType::*;
    match (a.unfrozen(), b.unfrozen()) {
        (List(x), List(y)) | (Set(x), Set(y)) => same_type(x, y),
        (Map(k1, v1), Map(k2, v2)) => same_type(k1, k2) && same_type(v1, v2),
        (Tuple(x), Tuple(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same_type(p, q)),
        (x, y) => x == y,
    }
}

/// Can a value of type `from` be used where `to` is expected?
fn assignable(from: &CqlType, to: &CqlType) -> bool {
    use CqlType::*;
    same_type(from, to)
        || matches!(
            (from.unfrozen(), to.unfrozen()),
            (TimeUuid, Uuid) | (Ascii, Text)
        )
}

/// The conversions Cassandra's `cast` function accepts.
fn cast_ok(from: &CqlType, to: &CqlType) -> bool {
    use CqlType::*;
    let (f, t) = (from.unfrozen(), to.unfrozen());
    let primitive = |x: &CqlType| {
        !matches!(
            x,
            List(_) | Set(_) | Map(..) | Tuple(_) | Udt(_) | Frozen(_)
        )
    };
    if !primitive(f) || !primitive(t) {
        return false;
    }
    if f == t || matches!(t, Text | Ascii) {
        return true;
    }
    if f.is_numeric() && t.is_numeric() {
        return true;
    }
    matches!(
        (f, t),
        (Timestamp, Date) | (Date, Timestamp) | (TimeUuid, Timestamp) | (TimeUuid, Date)
    )
}

/// Makes parameter names unique: names used more than once get a suffix
/// derived from their role (`created_at_gte`, `stock_new`, `stock_if_eq`),
/// remaining clashes a number.
fn unique_names(params: &mut [Param]) {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for p in params.iter() {
        *counts.entry(p.name.clone()).or_default() += 1;
    }
    for p in params.iter_mut() {
        if counts[&p.name] > 1 {
            let suffix = match &p.source {
                ParamSource::Restriction { op, .. } => Some(op.suffix().to_string()),
                ParamSource::Condition { op, .. } => Some(format!("if_{}", op.suffix())),
                ParamSource::Assignment { .. } => Some("new".to_string()),
                _ => None,
            };
            if let Some(s) = suffix {
                p.name = format!("{}_{s}", p.name);
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
