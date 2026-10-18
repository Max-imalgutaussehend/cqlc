//! Builds a [`Schema`] from parsed DDL statements.

use crate::ast::{
    AlterTableAction, AlterTypeAction, CreateTable, CreateType, DropKind, Ident, Order, Statement,
};
use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::schema::*;
use crate::types::CqlType;
use std::collections::HashSet;

/// Keywords Cassandra reserves; they cannot be used as unquoted names.
const RESERVED: &[&str] = &[
    "add",
    "allow",
    "alter",
    "and",
    "apply",
    "asc",
    "authorize",
    "batch",
    "begin",
    "by",
    "columnfamily",
    "create",
    "delete",
    "desc",
    "describe",
    "drop",
    "entries",
    "execute",
    "from",
    "full",
    "grant",
    "if",
    "in",
    "index",
    "infinity",
    "insert",
    "into",
    "keyspace",
    "limit",
    "modify",
    "nan",
    "norecursive",
    "not",
    "null",
    "of",
    "on",
    "or",
    "order",
    "primary",
    "rename",
    "replace",
    "revoke",
    "schema",
    "select",
    "set",
    "table",
    "to",
    "token",
    "truncate",
    "unlogged",
    "update",
    "use",
    "using",
    "view",
    "where",
    "with",
];

fn check_reserved(id: &crate::ast::Ident, diags: &mut Vec<Diagnostic>) {
    if !id.quoted && RESERVED.contains(&id.name.as_str()) {
        diags.push(
            Diagnostic::error(
                DiagnosticCode::ReservedWord,
                id.span,
                format!(
                    "`{}` is a reserved CQL keyword and cannot be used as a name",
                    id.name
                ),
            )
            .with_help(format!("rename it or quote it as \"{}\"", id.name)),
        );
    }
}

pub fn build_schema(statements: &[Statement]) -> (Schema, Vec<Diagnostic>) {
    let mut schema = Schema::default();
    let mut diags = Vec::new();
    for stmt in statements {
        match stmt {
            Statement::CreateTable(t) => {
                check_reserved(&t.name.name, &mut diags);
                t.columns
                    .iter()
                    .for_each(|c| check_reserved(&c.name, &mut diags));
            }
            Statement::CreateType(t) => {
                check_reserved(&t.name.name, &mut diags);
                t.fields
                    .iter()
                    .for_each(|(f, _)| check_reserved(f, &mut diags));
            }
            _ => {}
        }
    }

    // UDTs first so tables may reference them regardless of file order.
    for stmt in statements {
        if let Statement::CreateType(t) = stmt {
            add_udt(&mut schema, &mut diags, t);
        }
    }
    let udt_names: HashSet<String> = schema.udts.iter().map(|u| u.name.clone()).collect();
    for u in &schema.udts {
        for (_, ty) in &u.fields {
            check_udt_refs(ty, &udt_names, u.span, &mut diags);
        }
    }
    for stmt in statements {
        if let Statement::CreateTable(t) = stmt {
            add_table(&mut schema, &mut diags, t, &udt_names);
        }
    }
    // Everything else is applied in file order: indexes, views, functions, ALTER and DROP.
    for stmt in statements {
        evolve(&mut schema, &mut diags, stmt);
    }
    (schema, diags)
}

fn find_table_index(schema: &Schema, ks: Option<&str>, name: &str) -> Option<usize> {
    schema
        .tables
        .iter()
        .position(|t| t.name == name && ks.is_none_or(|k| t.keyspace.as_deref() == Some(k)))
}

fn unknown_table(diags: &mut Vec<Diagnostic>, span: crate::span::Span, name: &str) {
    diags.push(Diagnostic::error(
        DiagnosticCode::UnknownTable,
        span,
        format!("unknown table `{name}`"),
    ));
}

fn evolve(schema: &mut Schema, diags: &mut Vec<Diagnostic>, stmt: &Statement) {
    match stmt {
        Statement::CreateIndex(ix) => {
            let ks = ix.table.keyspace.as_ref().map(|k| k.name.as_str());
            let Some(ti) = find_table_index(schema, ks, &ix.table.name.name) else {
                unknown_table(diags, ix.table.span(), &ix.table.name.name);
                return;
            };
            let table = &mut schema.tables[ti];
            let Some(col) = table.column(&ix.column.name) else {
                diags.push(Diagnostic::error(
                    DiagnosticCode::UnknownColumn,
                    ix.column.span,
                    format!(
                        "unknown column `{}` in table `{}`",
                        ix.column.name, table.name
                    ),
                ));
                return;
            };
            use crate::ast::IndexKind as K;
            let ty = col.cql_type.unfrozen().clone();
            let bad = match (ix.kind, &ty) {
                (K::Keys | K::Entries, CqlType::Map(..)) => None,
                (K::Keys | K::Entries, other) => Some(format!(
                    "`{}` needs a map column, but `{}` is `{other}`",
                    if ix.kind == K::Keys {
                        "KEYS"
                    } else {
                        "ENTRIES"
                    },
                    ix.column.name
                )),
                (K::Full, _) if !col.cql_type.is_frozen() => Some(format!(
                    "`FULL` needs a frozen collection, but `{}` is not frozen",
                    ix.column.name
                )),
                _ => None,
            };
            if let Some(msg) = bad {
                diags.push(Diagnostic::error(
                    DiagnosticCode::InvalidRestriction,
                    ix.column.span,
                    msg,
                ));
                return;
            }
            table.indexes.push(Index {
                name: ix.name.as_ref().map(|n| n.name.clone()),
                column: ix.column.name.clone(),
                kind: ix.kind,
                class: IndexClass::from_using(ix.class.as_deref()),
            });
        }
        Statement::CreateView(v) => {
            let ks = v.base.keyspace.as_ref().map(|k| k.name.as_str());
            let Some(bi) = find_table_index(schema, ks, &v.base.name.name) else {
                unknown_table(diags, v.base.span(), &v.base.name.name);
                return;
            };
            let base = schema.tables[bi].clone();
            let wanted: Vec<String> = match &v.columns {
                None => base.columns.iter().map(|c| c.name.clone()).collect(),
                Some(cols) => cols.iter().map(|c| c.name.clone()).collect(),
            };
            let mut columns = Vec::new();
            for name in &wanted {
                match base.column(name) {
                    Some(c) => columns.push(c.clone()),
                    None => {
                        diags.push(Diagnostic::error(
                            DiagnosticCode::UnknownColumn,
                            v.name.name.span,
                            format!("unknown column `{name}` in table `{}`", base.name),
                        ));
                        return;
                    }
                }
            }
            let pk: Vec<&Ident> = v
                .primary_key
                .partition
                .iter()
                .chain(&v.primary_key.clustering)
                .collect();
            for k in &pk {
                if !columns.iter().any(|c| c.name == k.name) {
                    // A view's key may use base columns that are not selected.
                    match base.column(&k.name) {
                        Some(c) => columns.push(c.clone()),
                        None => {
                            diags.push(Diagnostic::error(
                                DiagnosticCode::PrimaryKey,
                                k.span,
                                format!(
                                    "primary key column `{}` does not exist in `{}`",
                                    k.name, base.name
                                ),
                            ));
                            return;
                        }
                    }
                }
            }
            for c in &mut columns {
                c.role = if v.primary_key.partition.iter().any(|k| k.name == c.name) {
                    ColumnRole::PartitionKey
                } else if v.primary_key.clustering.iter().any(|k| k.name == c.name) {
                    ColumnRole::ClusteringKey
                } else {
                    ColumnRole::Regular
                };
                c.is_static = false;
            }
            let ks = v.name.keyspace.as_ref().map(|k| k.name.clone());
            if schema
                .tables
                .iter()
                .any(|t| t.name == v.name.name.name && t.keyspace == ks)
            {
                diags.push(Diagnostic::error(
                    DiagnosticCode::Duplicate,
                    v.name.name.span,
                    format!(
                        "table or view `{}` is defined more than once",
                        v.name.name.name
                    ),
                ));
                return;
            }
            let order_of = |col: &str| {
                v.clustering_order
                    .iter()
                    .find(|(c, _)| c.name == col)
                    .map_or(Order::Asc, |(_, o)| *o)
            };
            schema.tables.push(Table {
                keyspace: ks,
                name: v.name.name.name.clone(),
                columns,
                partition_key: v
                    .primary_key
                    .partition
                    .iter()
                    .map(|i| i.name.clone())
                    .collect(),
                clustering: v
                    .primary_key
                    .clustering
                    .iter()
                    .map(|i| ClusteringColumn {
                        name: i.name.clone(),
                        order: order_of(&i.name),
                    })
                    .collect(),
                base_table: Some(base.name.clone()),
                indexes: Vec::new(),
                span: v.span,
            });
        }
        Statement::CreateFunction(f) => schema.functions.push(Function {
            name: f.name.name.name.clone(),
            args: f.args.clone(),
            returns: f.returns.clone(),
            aggregate: false,
        }),
        Statement::CreateAggregate(a) => {
            let returns = a
                .final_func
                .as_ref()
                .and_then(|ff| schema.function(&ff.name))
                .map_or_else(|| a.state_type.clone(), |f| f.returns.clone());
            schema.functions.push(Function {
                name: a.name.name.name.clone(),
                args: a.args.clone(),
                returns,
                aggregate: true,
            });
        }
        Statement::AlterTable(a) => {
            let ks = a.table.keyspace.as_ref().map(|k| k.name.as_str());
            let Some(ti) = find_table_index(schema, ks, &a.table.name.name) else {
                unknown_table(diags, a.table.span(), &a.table.name.name);
                return;
            };
            let udt_names: HashSet<String> = schema.udts.iter().map(|u| u.name.clone()).collect();
            let table = &mut schema.tables[ti];
            match &a.action {
                AlterTableAction::Add(cols) => {
                    for c in cols {
                        check_reserved(&c.name, diags);
                        check_udt_refs(&c.ty, &udt_names, c.ty_span, diags);
                        if table.column(&c.name.name).is_some() {
                            diags.push(Diagnostic::error(
                                DiagnosticCode::Duplicate,
                                c.name.span,
                                format!(
                                    "column `{}` already exists in table `{}`",
                                    c.name.name, table.name
                                ),
                            ));
                            continue;
                        }
                        table.columns.push(Column {
                            name: c.name.name.clone(),
                            cql_type: c.ty.clone(),
                            role: ColumnRole::Regular,
                            is_static: c.is_static,
                            masked: c.masked,
                            span: c.name.span,
                        });
                    }
                }
                AlterTableAction::Drop(cols) => {
                    for c in cols {
                        match table.column(&c.name) {
                            None => diags.push(Diagnostic::error(
                                DiagnosticCode::UnknownColumn,
                                c.span,
                                format!("unknown column `{}` in table `{}`", c.name, table.name),
                            )),
                            Some(col) if col.is_key() => diags.push(Diagnostic::error(
                                DiagnosticCode::PrimaryKey,
                                c.span,
                                format!("cannot drop primary key column `{}`", c.name),
                            )),
                            Some(_) => {
                                table.columns.retain(|x| x.name != c.name);
                                table.indexes.retain(|i| i.column != c.name);
                            }
                        }
                    }
                }
                AlterTableAction::Rename(pairs) => {
                    for (from, to) in pairs {
                        let Some(col) = table.columns.iter_mut().find(|c| c.name == from.name)
                        else {
                            diags.push(Diagnostic::error(
                                DiagnosticCode::UnknownColumn,
                                from.span,
                                format!("unknown column `{}` in table `{}`", from.name, table.name),
                            ));
                            continue;
                        };
                        col.name = to.name.clone();
                        for k in &mut table.partition_key {
                            if *k == from.name {
                                *k = to.name.clone();
                            }
                        }
                        for k in &mut table.clustering {
                            if k.name == from.name {
                                k.name = to.name.clone();
                            }
                        }
                        for i in &mut table.indexes {
                            if i.column == from.name {
                                i.column = to.name.clone();
                            }
                        }
                    }
                }
                AlterTableAction::Other => {}
            }
        }
        Statement::AlterType(a) => {
            let Some(u) = schema.udts.iter_mut().find(|u| u.name == a.name.name.name) else {
                diags.push(Diagnostic::error(
                    DiagnosticCode::UnknownType,
                    a.name.span(),
                    format!("unknown type `{}`", a.name.name.name),
                ));
                return;
            };
            match &a.action {
                AlterTypeAction::Add(field, ty) => {
                    if u.fields.iter().any(|(f, _)| *f == field.name) {
                        diags.push(Diagnostic::error(
                            DiagnosticCode::Duplicate,
                            field.span,
                            format!("duplicate field `{}` in type `{}`", field.name, u.name),
                        ));
                    } else {
                        u.fields.push((field.name.clone(), ty.clone()));
                    }
                }
                AlterTypeAction::Rename(pairs) => {
                    for (from, to) in pairs {
                        if let Some(f) = u.fields.iter_mut().find(|(f, _)| *f == from.name) {
                            f.0 = to.name.clone();
                        } else {
                            diags.push(Diagnostic::error(
                                DiagnosticCode::UnknownColumn,
                                from.span,
                                format!("type `{}` has no field `{}`", u.name, from.name),
                            ));
                        }
                    }
                }
            }
        }
        Statement::Drop(d) => {
            let name = &d.name.name.name;
            let ks = d.name.keyspace.as_ref().map(|k| k.name.as_str());
            match d.kind {
                DropKind::Table | DropKind::View => {
                    if let Some(i) = find_table_index(schema, ks, name) {
                        schema.tables.remove(i);
                    } else if !d.if_exists {
                        unknown_table(diags, d.name.span(), name);
                    }
                }
                DropKind::Type => schema.udts.retain(|u| u.name != *name),
                DropKind::Index => {
                    for t in &mut schema.tables {
                        t.indexes
                            .retain(|i| i.name.as_deref() != Some(name.as_str()));
                    }
                }
                DropKind::Keyspace => schema
                    .tables
                    .retain(|t| t.keyspace.as_deref() != Some(name.as_str())),
                DropKind::Function | DropKind::Aggregate => {
                    schema.functions.retain(|f| f.name != *name)
                }
                DropKind::Other => {}
            }
        }
        _ => {}
    }
}

fn add_udt(schema: &mut Schema, diags: &mut Vec<Diagnostic>, t: &CreateType) {
    let ks = t.name.keyspace.as_ref().map(|k| k.name.clone());
    if schema
        .udts
        .iter()
        .any(|u| u.name == t.name.name.name && u.keyspace == ks)
    {
        diags.push(Diagnostic::error(
            DiagnosticCode::Duplicate,
            t.name.name.span,
            format!("type `{}` is defined more than once", t.name.name.name),
        ));
        return;
    }
    let mut seen = HashSet::new();
    for (f, _) in &t.fields {
        if !seen.insert(&f.name) {
            diags.push(Diagnostic::error(
                DiagnosticCode::Duplicate,
                f.span,
                format!(
                    "duplicate field `{}` in type `{}`",
                    f.name, t.name.name.name
                ),
            ));
        }
    }
    schema.udts.push(Udt {
        keyspace: ks,
        name: t.name.name.name.clone(),
        fields: t
            .fields
            .iter()
            .map(|(i, ty)| (i.name.clone(), ty.clone()))
            .collect(),
        span: t.span,
    });
}

fn check_udt_refs(
    ty: &CqlType,
    known: &HashSet<String>,
    span: crate::span::Span,
    diags: &mut Vec<Diagnostic>,
) {
    match ty {
        CqlType::Udt(name) => {
            if !known.contains(name) {
                diags.push(Diagnostic::error(
                    DiagnosticCode::UnknownType,
                    span,
                    format!("unknown type `{name}`"),
                ));
            }
        }
        CqlType::List(t) | CqlType::Set(t) | CqlType::Frozen(t) | CqlType::Vector(t, _) => {
            check_udt_refs(t, known, span, diags)
        }
        CqlType::Map(k, v) => {
            check_udt_refs(k, known, span, diags);
            check_udt_refs(v, known, span, diags);
        }
        CqlType::Tuple(ts) => ts
            .iter()
            .for_each(|t| check_udt_refs(t, known, span, diags)),
        _ => {}
    }
}

fn add_table(
    schema: &mut Schema,
    diags: &mut Vec<Diagnostic>,
    t: &CreateTable,
    udts: &HashSet<String>,
) {
    let errors_before = diags.len();
    let name = &t.name.name;
    let ks = t.name.keyspace.as_ref().map(|k| k.name.clone());
    if schema
        .tables
        .iter()
        .any(|x| x.name == name.name && x.keyspace == ks)
    {
        diags.push(Diagnostic::error(
            DiagnosticCode::Duplicate,
            name.span,
            format!("table `{}` is defined more than once", name.name),
        ));
        return;
    }

    let mut seen = HashSet::new();
    for c in &t.columns {
        if !seen.insert(c.name.name.as_str()) {
            diags.push(Diagnostic::error(
                DiagnosticCode::Duplicate,
                c.name.span,
                format!(
                    "duplicate column `{}` in table `{}`",
                    c.name.name, name.name
                ),
            ));
        }
        check_udt_refs(&c.ty, udts, c.ty_span, diags);
    }

    // Primary key: either a PRIMARY KEY clause or one inline marker.
    let inline: Vec<&Ident> = t
        .columns
        .iter()
        .filter(|c| c.inline_primary_key)
        .map(|c| &c.name)
        .collect();
    let (partition, clustering): (Vec<Ident>, Vec<Ident>) =
        match (&t.primary_key, inline.as_slice()) {
            (Some(pk), []) => (pk.partition.clone(), pk.clustering.clone()),
            (None, [one]) => (vec![(*one).clone()], vec![]),
            (None, []) => {
                diags.push(Diagnostic::error(
                    DiagnosticCode::PrimaryKey,
                    name.span,
                    format!("table `{}` has no primary key", name.name),
                ));
                return;
            }
            (Some(pk), _) => {
                diags.push(Diagnostic::error(
                    DiagnosticCode::PrimaryKey,
                    pk.span,
                    "primary key is declared both inline and with a PRIMARY KEY clause",
                ));
                return;
            }
            (None, many) => {
                diags.push(Diagnostic::error(
                    DiagnosticCode::PrimaryKey,
                    many[1].span,
                    "more than one column is marked PRIMARY KEY",
                ));
                return;
            }
        };

    let mut key_seen = HashSet::new();
    for k in partition.iter().chain(&clustering) {
        if !t.columns.iter().any(|c| c.name.name == k.name) {
            diags.push(Diagnostic::error(
                DiagnosticCode::PrimaryKey,
                k.span,
                format!(
                    "primary key column `{}` is not defined in table `{}`",
                    k.name, name.name
                ),
            ));
        } else if !key_seen.insert(k.name.as_str()) {
            diags.push(Diagnostic::error(
                DiagnosticCode::PrimaryKey,
                k.span,
                format!(
                    "column `{}` appears more than once in the primary key",
                    k.name
                ),
            ));
        }
    }

    let order_of = |col: &str| -> Order {
        t.clustering_order
            .iter()
            .find(|(c, _)| c.name == col)
            .map_or(Order::Asc, |(_, o)| *o)
    };
    for (c, _) in &t.clustering_order {
        if !clustering.iter().any(|k| k.name == c.name) {
            diags.push(Diagnostic::error(
                DiagnosticCode::PrimaryKey,
                c.span,
                format!(
                    "`{}` in CLUSTERING ORDER BY is not a clustering column",
                    c.name
                ),
            ));
        }
    }

    if diags.len() > errors_before {
        return;
    }

    let columns = t
        .columns
        .iter()
        .map(|c| {
            let role = if partition.iter().any(|k| k.name == c.name.name) {
                ColumnRole::PartitionKey
            } else if clustering.iter().any(|k| k.name == c.name.name) {
                ColumnRole::ClusteringKey
            } else {
                ColumnRole::Regular
            };
            Column {
                name: c.name.name.clone(),
                cql_type: c.ty.clone(),
                role,
                is_static: c.is_static,
                masked: c.masked,
                span: c.name.span,
            }
        })
        .collect();
    schema.tables.push(Table {
        keyspace: ks,
        name: name.name.clone(),
        columns,
        partition_key: partition.iter().map(|i| i.name.clone()).collect(),
        clustering: clustering
            .iter()
            .map(|i| ClusteringColumn {
                name: i.name.clone(),
                order: order_of(&i.name),
            })
            .collect(),
        base_table: None,
        indexes: Vec::new(),
        span: t.span,
    });
}
