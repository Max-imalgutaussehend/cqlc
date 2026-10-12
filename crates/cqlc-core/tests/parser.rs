use cqlc_core::ast::*;
use cqlc_core::parser::{parse_queries, parse_schema};
use cqlc_core::span::FileId;
use cqlc_core::types::CqlType;

fn schema(src: &str) -> Vec<Statement> {
    let (stmts, diags) = parse_schema(FileId(0), src);
    assert!(diags.is_empty(), "{diags:?}");
    stmts
}

fn table(src: &str) -> CreateTable {
    match schema(src).remove(0) {
        Statement::CreateTable(t) => t,
        other => panic!("not a table: {other:?}"),
    }
}

fn queries(src: &str) -> Vec<QueryDef> {
    let (q, diags) = parse_queries(FileId(0), src);
    assert!(diags.is_empty(), "{diags:?}");
    q
}

fn query_codes(src: &str) -> Vec<&'static str> {
    parse_queries(FileId(0), src)
        .1
        .iter()
        .map(|d| d.code.as_str())
        .collect()
}

#[test]
fn composite_primary_key_and_clustering_order() {
    let t = table(
        "CREATE TABLE ks.users_by_status (
            status text, created_at timestamp, user_id uuid, username text,
            PRIMARY KEY ((status), created_at, user_id)
        ) WITH CLUSTERING ORDER BY (created_at DESC, user_id ASC)
          AND comment = 'x; y' AND compaction = {'class': 'LeveledCompactionStrategy'};",
    );
    assert_eq!(t.name.keyspace.unwrap().name, "ks");
    assert_eq!(t.name.name.name, "users_by_status");
    assert_eq!(t.columns.len(), 4);
    let pk = t.primary_key.unwrap();
    assert_eq!(pk.partition.len(), 1);
    assert_eq!(pk.clustering.len(), 2);
    assert_eq!(t.clustering_order[0].1, Order::Desc);
    assert_eq!(t.clustering_order[1].1, Order::Asc);
}

#[test]
fn inline_key_static_and_collection_types() {
    let t = table(
        "CREATE TABLE IF NOT EXISTS t (id uuid PRIMARY KEY, tags set<text>, m map<text, frozen<list<int>>>,
                                       pair tuple<int, text>, s text STATIC)",
    );
    assert!(t.columns[0].inline_primary_key);
    assert_eq!(t.columns[1].ty, CqlType::Set(Box::new(CqlType::Text)));
    assert_eq!(
        t.columns[2].ty,
        CqlType::Map(
            Box::new(CqlType::Text),
            Box::new(CqlType::Frozen(Box::new(CqlType::List(Box::new(
                CqlType::Int
            )))))
        )
    );
    assert_eq!(
        t.columns[3].ty,
        CqlType::Tuple(vec![CqlType::Int, CqlType::Text])
    );
    assert!(t.columns[4].is_static);
}

#[test]
fn keywords_are_case_insensitive_and_identifiers_normalized() {
    let t = table("create table T (ID uuid primary key, \"MixedCase\" text)");
    assert_eq!(t.name.name.name, "t");
    assert_eq!(t.columns[0].name.name, "id");
    assert_eq!(t.columns[1].name.name, "MixedCase");
}

#[test]
fn query_annotation_and_normalized_cql() {
    let q = queries(
        "-- header comment\n\
         -- name: GetUser :one\n\
         SELECT  user_id, -- inline\n   username AS name\n\
         FROM users WHERE user_id = ?;\n\
         -- name: Gone :exec\nDELETE FROM users WHERE user_id = ?;",
    );
    assert_eq!(q.len(), 2);
    assert_eq!(q[0].name.name, "GetUser");
    assert_eq!(q[0].kind, QueryKind::One);
    assert_eq!(
        q[0].cql,
        "SELECT user_id, username AS name FROM users WHERE user_id = ?"
    );
    assert_eq!(q[1].kind, QueryKind::Exec);
}

#[test]
fn bind_markers_are_numbered_in_textual_order() {
    let q = queries(
        "-- name: Up :exec\nUPDATE t USING TTL ? SET a = ?, b = 'x?' WHERE k = ? AND c IN (?, ?);",
    );
    let Statement::Update(u) = &q[0].statement else {
        panic!()
    };
    assert_eq!(u.using.ttl.as_ref().unwrap().kind, TermKind::Marker(0));
    assert_eq!(u.assignments[0].value.kind, TermKind::Marker(1));
    assert_eq!(
        u.assignments[1].value.kind,
        TermKind::Literal(Literal::String("x?".into()))
    );
    let Rhs::Term(t) = &u.where_clause[0].rhs else {
        panic!()
    };
    assert_eq!(t.kind, TermKind::Marker(2));
    let Rhs::List(l) = &u.where_clause[1].rhs else {
        panic!()
    };
    assert_eq!(l[0].kind, TermKind::Marker(3));
    assert_eq!(l[1].kind, TermKind::Marker(4));
}

#[test]
fn missing_or_invalid_annotations() {
    assert_eq!(query_codes("SELECT a FROM t WHERE k = ?;"), ["CQL107"]);
    assert_eq!(
        query_codes("-- name: X\nSELECT a FROM t WHERE k = ?;"),
        ["CQL107"]
    );
    assert_eq!(
        query_codes("-- name: X :one\nDELETE FROM t WHERE k = ?;"),
        ["CQL107"]
    );
    assert_eq!(
        query_codes("-- name: X :exec\nSELECT a FROM t WHERE k = ?;"),
        ["CQL107"]
    );
    assert_eq!(
        query_codes("-- name: X :bogus\nSELECT a FROM t WHERE k = ?;"),
        ["CQL107"]
    );
    // An annotation only counts for the statement directly after it.
    assert_eq!(
        query_codes("-- name: A :one\nSELECT a FROM t WHERE k = ?;\nSELECT b FROM t WHERE k = ?;"),
        ["CQL107"]
    );
}

#[test]
fn unsupported_constructs_are_rejected_explicitly() {
    let cases = [
        "SELECT count(*) FROM t WHERE k = ?",
        "SELECT JSON a FROM t WHERE k = ?",
        "SELECT DISTINCT a FROM t",
        "SELECT a FROM t WHERE k = now()",
        "SELECT a FROM t WHERE token(k) > ?",
        "SELECT a FROM t WHERE (a, b) > (?, ?)",
        "SELECT a FROM t WHERE a != ?",
        "SELECT a FROM t WHERE a CONTAINS ?",
        "SELECT a FROM t GROUP BY a",
        "INSERT INTO t (a) VALUES (?) IF NOT EXISTS",
        "UPDATE t SET a = ? WHERE k = ? IF a = ?",
        "UPDATE t SET a = a + ? WHERE k = ?",
        "UPDATE t SET a += ? WHERE k = ?",
        "DELETE FROM t WHERE k = ? IF EXISTS",
        "SELECT a FROM t WHERE k = :named",
        "TRUNCATE t",
    ];
    for c in cases {
        let src = format!("-- name: X :one\n{c};");
        assert_eq!(query_codes(&src), ["CQL002"], "{c}");
    }
}

#[test]
fn bind_marker_in_selection_is_rejected() {
    assert_eq!(
        query_codes("-- name: X :one\nSELECT ? FROM t WHERE k = ?;"),
        ["CQL106"]
    );
}

#[test]
fn syntax_errors_recover_at_the_next_statement() {
    let (q, diags) = parse_queries(
        FileId(0),
        "-- name: Bad :one\nSELECT FROM WHERE;\n-- name: Good :one\nSELECT a FROM t WHERE k = ?;",
    );
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code.as_str(), "CQL001");
    assert_eq!(q.len(), 1);
    assert_eq!(q[0].name.name, "Good");
}

#[test]
fn ddl_and_dml_do_not_mix() {
    let (_, d) = parse_schema(FileId(0), "SELECT a FROM t WHERE k = 1;");
    assert_eq!(d[0].code.as_str(), "CQL001");
    let (_, d) = parse_queries(
        FileId(0),
        "-- name: X :exec\nCREATE TABLE t (k int PRIMARY KEY);",
    );
    assert_eq!(d[0].code.as_str(), "CQL001");
}

#[test]
fn literals() {
    let q = queries(
        "-- name: X :exec\nINSERT INTO t (a, b, c, d, e, f) VALUES (-5, 1.5, true, 123e4567-e89b-12d3-a456-426614174000, null, 'x');",
    );
    let Statement::Insert(i) = &q[0].statement else {
        panic!()
    };
    let lits: Vec<_> = i.values.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(lits[0], TermKind::Literal(Literal::Integer("-5".into())));
    assert_eq!(lits[1], TermKind::Literal(Literal::Float("1.5".into())));
    assert_eq!(lits[2], TermKind::Literal(Literal::Bool(true)));
    assert!(matches!(lits[3], TermKind::Literal(Literal::Uuid(_))));
    assert_eq!(lits[4], TermKind::Literal(Literal::Null));
}
