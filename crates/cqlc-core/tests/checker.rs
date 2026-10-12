use cqlc_core::ast::{QueryKind, RelOp};
use cqlc_core::ir::{Param, ParamSource, Query, StatementKind};
use cqlc_core::types::CqlType;
use cqlc_core::{Compilation, Input, compile};

const SCHEMA: &str = "
CREATE TABLE users_by_id (
    user_id uuid PRIMARY KEY,
    username text,
    email text,
    created_at timestamp
);
CREATE TABLE events (
    tenant_id uuid,
    event_time timestamp,
    event_id uuid,
    payload text,
    tags set<text>,
    PRIMARY KEY ((tenant_id), event_time, event_id)
);
CREATE TYPE address (street text, city text);
CREATE TABLE with_udt (id int PRIMARY KEY, home frozen<address>, note text);
";

fn run(queries: &str) -> Compilation {
    compile(
        &[Input {
            name: "schema.cql".into(),
            text: SCHEMA.into(),
        }],
        &[Input {
            name: "queries.cql".into(),
            text: queries.into(),
        }],
    )
}

fn codes(c: &Compilation) -> Vec<&'static str> {
    c.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

fn one(queries: &str) -> Query {
    let c = run(queries);
    assert!(c.diagnostics.is_empty(), "{:?}", c.diagnostics);
    c.ir.queries.into_iter().next().expect("one query")
}

fn names(p: &[Param]) -> Vec<&str> {
    p.iter().map(|p| p.name.as_str()).collect()
}

#[test]
fn infers_parameter_type_from_the_schema() {
    let q = one("-- name: Get :one\nSELECT username FROM users_by_id WHERE user_id = ?;");
    assert_eq!(q.params.len(), 1);
    assert_eq!(q.params[0].name, "user_id");
    assert_eq!(q.params[0].cql_type, CqlType::Uuid);
    assert!(!q.params[0].nullable);
    assert_eq!(q.statement, StatementKind::Select);
    assert_eq!(q.kind, QueryKind::One);
}

#[test]
fn result_nullability_follows_the_key_structure() {
    let q = one(
        "-- name: E :many\nSELECT tenant_id, event_time, payload FROM events WHERE tenant_id = ?;",
    );
    let nullable: Vec<_> = q.columns.iter().map(|c| c.nullable).collect();
    assert_eq!(nullable, [false, false, true]);
}

#[test]
fn star_is_expanded_in_declaration_order_and_aliases_are_applied() {
    let q = one("-- name: A :one\nSELECT * FROM users_by_id WHERE user_id = ?;");
    let cols: Vec<_> = q.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(cols, ["user_id", "username", "email", "created_at"]);
    let q =
        one("-- name: B :one\nSELECT username AS display_name FROM users_by_id WHERE user_id = ?;");
    assert_eq!(q.columns[0].name, "display_name");
    assert_eq!(q.columns[0].cql_type, CqlType::Text);
}

#[test]
fn bind_values_and_result_cells_have_separate_nullability() {
    let q = one(
        "-- name: I :exec\nINSERT INTO users_by_id (user_id, username, email, created_at) VALUES (?, ?, ?, ?);",
    );
    let nullable: Vec<_> = q.params.iter().map(|p| p.nullable).collect();
    assert_eq!(nullable, [false, true, true, true]);
}

#[test]
fn parameters_are_ordered_by_bind_marker_not_by_clause() {
    let q = one(
        "-- name: U :exec\nUPDATE users_by_id USING TTL ? SET email = ?, username = ? WHERE user_id = ?;",
    );
    assert_eq!(names(&q.params), ["ttl", "email", "username", "user_id"]);
    assert_eq!(q.params[0].source, ParamSource::Ttl);
    assert_eq!(q.params[0].cql_type, CqlType::Int);
}

#[test]
fn range_restrictions_on_one_column_get_operator_suffixes() {
    let q = one(
        "-- name: R :many\nSELECT payload FROM events WHERE tenant_id = ? AND event_time >= ? AND event_time < ? LIMIT ?;",
    );
    assert_eq!(
        names(&q.params),
        ["tenant_id", "event_time_gte", "event_time_lt", "limit"]
    );
    assert_eq!(
        q.params[1].source,
        ParamSource::Restriction {
            column: "event_time".into(),
            op: RelOp::Ge
        }
    );
}

#[test]
fn in_restrictions() {
    let q = one("-- name: A :many\nSELECT payload FROM events WHERE tenant_id IN ?;");
    assert_eq!(q.params[0].cql_type, CqlType::List(Box::new(CqlType::Uuid)));
    let q = one("-- name: B :many\nSELECT payload FROM events WHERE tenant_id IN (?, ?);");
    assert_eq!(names(&q.params), ["tenant_id_in", "tenant_id_in_2"]);
    assert_eq!(q.params[1].cql_type, CqlType::Uuid);
}

#[test]
fn literals_need_no_parameter_but_must_fit() {
    let q = one(
        "-- name: A :one\nSELECT username FROM users_by_id WHERE user_id = 123e4567-e89b-12d3-a456-426614174000;",
    );
    assert!(q.params.is_empty());
    let c = run("-- name: B :one\nSELECT username FROM users_by_id WHERE user_id = 'nope';");
    assert_eq!(codes(&c), ["CQL105"]);
    let c =
        run("-- name: C :exec\nINSERT INTO users_by_id (user_id, username) VALUES (null, 'x');");
    assert_eq!(codes(&c), ["CQL105"]);
}

#[test]
fn unknown_names_are_errors_with_help() {
    let c = run("-- name: A :one\nSELECT username FROM nope WHERE user_id = ?;");
    assert_eq!(codes(&c), ["CQL101"]);
    assert!(
        c.diagnostics[0]
            .help
            .as_deref()
            .unwrap()
            .contains("users_by_id")
    );
    let c = run("-- name: B :one\nSELECT nope FROM users_by_id WHERE user_id = ?;");
    assert_eq!(codes(&c), ["CQL102"]);
    assert!(
        c.diagnostics[0]
            .help
            .as_deref()
            .unwrap()
            .contains("username")
    );
    let c =
        run("-- name: C :one\nSELECT username FROM users_by_id WHERE user_id = ? AND nope = ?;");
    assert_eq!(codes(&c), ["CQL102"]);
    assert!(c.ir.queries.is_empty());
}

#[test]
fn query_on_a_partition_without_partition_key_warns_but_is_kept() {
    let c = run("-- name: Broad :many\nSELECT payload FROM events WHERE event_time >= ?;");
    assert_eq!(codes(&c), ["CQL201"]);
    assert!(!c.has_errors());
    assert_eq!(c.ir.queries.len(), 1);
    let d = &c.diagnostics[0];
    assert!(d.message.contains("tenant_id"));
    let rendered = d.render(&c.sources);
    assert!(rendered.contains("warning[CQL201]"), "{rendered}");
    assert!(rendered.contains("--> queries.cql:2:1"), "{rendered}");
    assert!(
        rendered.contains("help: consider modeling a table"),
        "{rendered}"
    );
}

#[test]
fn restricted_partition_key_does_not_warn() {
    let c = run(
        "-- name: Ok :many\nSELECT payload FROM events WHERE tenant_id = ? AND event_time >= ?;",
    );
    assert!(c.diagnostics.is_empty(), "{:?}", c.diagnostics);
}

#[test]
fn allow_filtering_and_non_key_restrictions_warn() {
    let c = run(
        "-- name: A :many\nSELECT payload FROM events WHERE tenant_id = ? AND payload = ? ALLOW FILTERING;",
    );
    assert_eq!(codes(&c), ["CQL202"]);
    let c =
        run("-- name: B :many\nSELECT payload FROM events WHERE tenant_id = ? AND payload = ?;");
    assert_eq!(codes(&c), ["CQL203"]);
}

#[test]
fn clustering_gaps_warn() {
    let c =
        run("-- name: A :many\nSELECT payload FROM events WHERE tenant_id = ? AND event_id = ?;");
    assert_eq!(codes(&c), ["CQL204"]);
    let c = run(
        "-- name: B :many\nSELECT payload FROM events WHERE tenant_id = ? AND event_time > ? AND event_id = ?;",
    );
    assert_eq!(codes(&c), ["CQL204"]);
    let c = run(
        "-- name: C :many\nSELECT payload FROM events WHERE tenant_id = ? AND event_time = ? AND event_id > ?;",
    );
    assert!(c.diagnostics.is_empty());
}

#[test]
fn partition_key_ranges_are_errors() {
    let c = run("-- name: A :many\nSELECT payload FROM events WHERE tenant_id > ?;");
    assert_eq!(codes(&c), ["CQL112"]);
}

#[test]
fn writes_must_address_the_key() {
    let c = run("-- name: A :exec\nINSERT INTO users_by_id (username) VALUES (?);");
    assert_eq!(codes(&c), ["CQL111"]);
    let c = run("-- name: B :exec\nUPDATE events SET payload = ? WHERE tenant_id = ?;");
    assert_eq!(codes(&c), ["CQL111"]);
    let c = run("-- name: C :exec\nUPDATE users_by_id SET user_id = ? WHERE user_id = ?;");
    assert_eq!(codes(&c), ["CQL112"]);
    let c = run("-- name: D :exec\nDELETE FROM events WHERE event_time = ?;");
    assert_eq!(codes(&c), ["CQL111"]);
    let c = run("-- name: E :exec\nDELETE FROM events WHERE tenant_id = ? AND event_time > ?;");
    assert!(c.diagnostics.is_empty(), "{:?}", c.diagnostics);
    let c = run("-- name: F :exec\nINSERT INTO users_by_id (user_id, username) VALUES (?);");
    assert!(codes(&c).contains(&"CQL108"));
}

#[test]
fn user_defined_types_are_rejected_only_where_used() {
    let c = run("-- name: A :one\nSELECT note FROM with_udt WHERE id = ?;");
    assert!(c.diagnostics.is_empty());
    let c = run("-- name: B :one\nSELECT home FROM with_udt WHERE id = ?;");
    assert_eq!(codes(&c), ["CQL110"]);
    let c = run("-- name: C :one\nSELECT * FROM with_udt WHERE id = ?;");
    assert_eq!(codes(&c), ["CQL110"]);
}

#[test]
fn duplicate_query_names_and_columns() {
    let c = run(
        "-- name: A :one\nSELECT username FROM users_by_id WHERE user_id = ?;\n-- name: A :one\nSELECT email FROM users_by_id WHERE user_id = ?;",
    );
    assert_eq!(codes(&c), ["CQL104"]);
    assert_eq!(c.ir.queries.len(), 1);
    let c = run(
        "-- name: B :one\nSELECT username, email AS username FROM users_by_id WHERE user_id = ?;",
    );
    assert_eq!(codes(&c), ["CQL104"]);
}

#[test]
fn too_many_bind_markers() {
    let cols: Vec<String> = (0..17).map(|i| format!("c{i} int")).collect();
    let schema = format!(
        "CREATE TABLE wide (k int PRIMARY KEY, {});",
        cols.join(", ")
    );
    let names: Vec<String> = (0..17).map(|i| format!("c{i}")).collect();
    let marks = vec!["?"; 17].join(", ");
    let q = format!(
        "-- name: W :exec\nINSERT INTO wide (k, {}) VALUES (?, {});",
        names[..16].join(", "),
        &marks[..marks.len() - 3]
    );
    let c = compile(
        &[Input {
            name: "s.cql".into(),
            text: schema,
        }],
        &[Input {
            name: "q.cql".into(),
            text: q,
        }],
    );
    assert_eq!(codes(&c), ["CQL108"], "{:?}", c.diagnostics);
}

#[test]
fn schema_errors() {
    let bad = |ddl: &str| -> Vec<&'static str> {
        compile(
            &[Input {
                name: "s.cql".into(),
                text: ddl.into(),
            }],
            &[],
        )
        .diagnostics
        .iter()
        .map(|d| d.code.as_str())
        .collect()
    };
    assert_eq!(bad("CREATE TABLE t (a int, b int);"), ["CQL109"]);
    assert_eq!(bad("CREATE TABLE t (a int, PRIMARY KEY (z));"), ["CQL109"]);
    assert_eq!(
        bad("CREATE TABLE t (a int PRIMARY KEY, b mystery);"),
        ["CQL103"]
    );
    assert_eq!(
        bad("CREATE TABLE t (a int PRIMARY KEY); CREATE TABLE t (b int PRIMARY KEY);"),
        ["CQL104"]
    );
    assert_eq!(
        bad("CREATE TABLE t (a int PRIMARY KEY, a text);"),
        ["CQL104"]
    );
    assert_eq!(
        bad("CREATE TABLE t (a int, b int, PRIMARY KEY (a)) WITH CLUSTERING ORDER BY (b DESC);"),
        ["CQL109"]
    );
    // UDTs may be declared after their first use.
    assert!(
        bad("CREATE TABLE t (a int PRIMARY KEY, u address); CREATE TYPE address (x int);")
            .is_empty()
    );
}

#[test]
fn keyspace_qualified_names() {
    let schema =
        "CREATE TABLE a.t (k int PRIMARY KEY, v int); CREATE TABLE b.t (k int PRIMARY KEY, v int);";
    let run = |q: &str| {
        compile(
            &[Input {
                name: "s.cql".into(),
                text: schema.into(),
            }],
            &[Input {
                name: "q.cql".into(),
                text: q.into(),
            }],
        )
    };
    let c = run("-- name: Q :one\nSELECT v FROM a.t WHERE k = ?;");
    assert!(c.diagnostics.is_empty());
    assert_eq!(c.ir.queries[0].keyspace.as_deref(), Some("a"));
    let c = run("-- name: Q :one\nSELECT v FROM t WHERE k = ?;");
    assert_eq!(codes(&c), ["CQL101"]);
}

#[test]
fn compilation_is_deterministic() {
    let q = "-- name: A :one\nSELECT * FROM users_by_id WHERE user_id = ?;\n-- name: B :exec\nDELETE FROM users_by_id WHERE user_id = ?;";
    assert_eq!(run(q).ir, run(q).ir);
}
