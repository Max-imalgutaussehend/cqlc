//! Builds a [`Schema`] from parsed DDL statements.

use crate::ast::{CreateTable, CreateType, Ident, Order, Statement};
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
    (schema, diags)
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
        CqlType::List(t) | CqlType::Set(t) | CqlType::Frozen(t) => {
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
        span: t.span,
    });
}
