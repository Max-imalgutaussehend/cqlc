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

fn dml(src: &str) -> Statement {
    let q = queries(&format!("-- name: X :{}\n{src};", kind_for(src)));
    q.into_iter().next().unwrap().statement
}

fn kind_for(src: &str) -> &'static str {
    let up = src.to_ascii_uppercase();
    if up.starts_with("SELECT") {
        "one"
    } else if up.contains(" IF ") {
        "lwt"
    } else {
        "exec"
    }
}

fn marker(e: &Expr) -> usize {
    match e.kind {
        ExprKind::Marker(i) => i,
        ref other => panic!("not a marker: {other:?}"),
    }
}

#[test]
fn bind_markers_are_numbered_in_textual_order() {
    let q = queries(
        "-- name: Up :exec\nUPDATE t USING TTL ? SET a = ?, b = 'x?' WHERE k = ? AND c IN (?, ?);",
    );
    assert_eq!(q[0].marker_count, 5);
    let Statement::Update(u) = &q[0].statement else {
        panic!()
    };
    assert_eq!(marker(u.using.ttl.as_ref().unwrap()), 0);
    assert_eq!(marker(&u.assignments[0].value), 1);
    assert_eq!(
        u.assignments[1].value.kind,
        ExprKind::Literal(Literal::String("x?".into()))
    );
    let Rhs::Expr(e) = &u.where_clause[0].rhs else {
        panic!()
    };
    assert_eq!(marker(e), 2);
    let Rhs::InList(l) = &u.where_clause[1].rhs else {
        panic!()
    };
    assert_eq!((marker(&l[0]), marker(&l[1])), (3, 4));
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
fn conditional_writes_need_the_lwt_annotation() {
    assert_eq!(
        query_codes("-- name: X :exec\nINSERT INTO t (k) VALUES (?) IF NOT EXISTS;"),
        ["CQL107"]
    );
    assert_eq!(
        query_codes("-- name: X :exec\nUPDATE t SET a = ? WHERE k = ? IF a = ?;"),
        ["CQL107"]
    );
    assert_eq!(
        query_codes("-- name: X :lwt\nDELETE FROM t WHERE k = ?;"),
        ["CQL107"]
    );
    assert!(query_codes("-- name: X :lwt\nDELETE FROM t WHERE k = ? IF EXISTS;").is_empty());
    assert!(query_codes("-- name: X :lwt\nINSERT INTO t (k) VALUES (?) IF NOT EXISTS;").is_empty());
}

#[test]
fn constructs_outside_the_subset_are_rejected_explicitly() {
    let cases = [
        "SELECT a FROM t WHERE a LIKE ?",
        "SELECT a FROM t WHERE f(a) = ?",
        "SELECT a FROM t WHERE k = :named",
        "UPDATE t SET a = 1 + 2 WHERE k = ?",
        "UPDATE t SET a = b + ? WHERE k = ?",
        "UPDATE t SET a = ? WHERE k = ? IF NOT EXISTS",
        "INSERT INTO t JSON ? DEFAULT NULL",
        "TRUNCATE t",
    ];
    for c in cases {
        let src = format!("-- name: X :one\n{c};");
        assert_eq!(query_codes(&src), ["CQL002"], "{c}");
    }
}

#[test]
fn bind_marker_in_selection_is_a_checker_error_not_a_syntax_error() {
    // The parser accepts it; the checker rejects it because the type is unknown.
    assert!(query_codes("-- name: X :one\nSELECT ? AS a FROM t WHERE k = ?;").is_empty());
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
    let Statement::Insert(i) = dml(
        "INSERT INTO t (a, b, c, d, e, f, g) VALUES (-5, 1.5, true, 123e4567-e89b-12d3-a456-426614174000, null, 'x', 0xCAFE)",
    ) else {
        panic!()
    };
    let InsertSource::Values { values, .. } = &i.source else {
        panic!()
    };
    let lits: Vec<_> = values.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(lits[0], ExprKind::Literal(Literal::Integer("-5".into())));
    assert_eq!(lits[1], ExprKind::Literal(Literal::Float("1.5".into())));
    assert_eq!(lits[2], ExprKind::Literal(Literal::Bool(true)));
    assert!(matches!(lits[3], ExprKind::Literal(Literal::Uuid(_))));
    assert_eq!(lits[4], ExprKind::Literal(Literal::Null));
    assert!(matches!(lits[6], ExprKind::Literal(Literal::Blob(_))));
}

#[test]
fn collection_tuple_and_udt_literals() {
    let Statement::Insert(i) = dml(
        "INSERT INTO t (a, b, c, d, e, f) VALUES ([1, ?], {'x', 'y'}, {'k': 1}, (1, 'a'), {street: 'Main', zip: ?}, {})",
    ) else {
        panic!()
    };
    let InsertSource::Values { values, .. } = &i.source else {
        panic!()
    };
    assert!(matches!(&values[0].kind, ExprKind::List(l) if l.len() == 2));
    assert!(matches!(&values[1].kind, ExprKind::Set(l) if l.len() == 2));
    assert!(matches!(&values[2].kind, ExprKind::Map(m) if m.len() == 1));
    assert!(matches!(&values[3].kind, ExprKind::Tuple(l) if l.len() == 2));
    assert!(matches!(&values[4].kind, ExprKind::Udt(f) if f[0].0.name == "street"));
    assert_eq!(values[5].kind, ExprKind::EmptyBraces);
}

#[test]
fn functions_casts_fields_and_indexes_in_selections() {
    let Statement::Select(s) = dml(
        "SELECT count(*) AS n, toTimestamp(now()) AS t, cast(a AS text) AS c, u.city AS city, m['k'] AS v FROM t",
    ) else {
        panic!()
    };
    let Selection::Items(items) = &s.selection else {
        panic!()
    };
    assert!(
        matches!(&items[0].expr.kind, ExprKind::Call { star: true, name, .. } if name.name == "count")
    );
    assert!(matches!(&items[1].expr.kind, ExprKind::Call { args, .. } if args.len() == 1));
    assert!(matches!(
        &items[2].expr.kind,
        ExprKind::Cast(_, CqlType::Text)
    ));
    assert!(matches!(&items[3].expr.kind, ExprKind::Field(..)));
    assert!(matches!(&items[4].expr.kind, ExprKind::Index(..)));
    assert_eq!(items[0].alias.as_ref().unwrap().name, "n");
}

#[test]
fn select_clauses() {
    let Statement::Select(s) = dml(
        "SELECT DISTINCT a FROM t WHERE k = ? GROUP BY k ORDER BY c DESC PER PARTITION LIMIT 2 LIMIT ? ALLOW FILTERING",
    ) else {
        panic!()
    };
    assert_eq!(s.mode, SelectMode::Distinct);
    assert_eq!(s.group_by.len(), 1);
    assert_eq!(s.order_by[0].1, Order::Desc);
    assert!(s.per_partition_limit.is_some() && s.limit.is_some() && s.allow_filtering);
    let Statement::Select(s) = dml("SELECT JSON * FROM t") else {
        panic!()
    };
    assert_eq!(s.mode, SelectMode::Json);
}

#[test]
fn relations() {
    let Statement::Select(s) = dml(
        "SELECT a FROM t WHERE token(k) > token(?) AND (a, b) < (?, ?) AND c CONTAINS ? AND d CONTAINS KEY ? \
         AND m[?] = ? AND (a, b) IN ((?, ?), (?, ?))",
    ) else {
        panic!()
    };
    let w = &s.where_clause;
    assert!(matches!(&w[0].lhs, RelLhs::Token(c) if c.len() == 1));
    assert!(matches!(&w[1].lhs, RelLhs::Tuple(c) if c.len() == 2));
    assert_eq!(w[2].op, RelOp::Contains);
    assert_eq!(w[3].op, RelOp::ContainsKey);
    assert!(matches!(&w[4].lhs, RelLhs::Element(..)));
    assert!(matches!(&w[5].rhs, Rhs::InList(l) if l.len() == 2));
}

#[test]
fn assignments() {
    let Statement::Update(u) = dml(
        "UPDATE t SET a = a + ?, b = b - ?, c = ? + c, d += ?, e -= ?, m[?] = ?, u.f = ?, g = ? WHERE k = ?",
    ) else {
        panic!()
    };
    let ops: Vec<_> = u.assignments.iter().map(|a| a.op).collect();
    assert_eq!(
        ops,
        [
            AssignOp::Add,
            AssignOp::Sub,
            AssignOp::Prepend,
            AssignOp::Add,
            AssignOp::Sub,
            AssignOp::Set,
            AssignOp::Set,
            AssignOp::Set
        ]
    );
    assert!(u.assignments[5].target.key.is_some());
    assert!(u.assignments[6].target.field.is_some());
}

#[test]
fn deletes_conditions_and_json_inserts() {
    let Statement::Delete(d) =
        dml("DELETE a, m[?], u.f FROM t USING TIMESTAMP ? WHERE k = ? IF b = ? AND c > ?")
    else {
        panic!()
    };
    assert_eq!(d.targets.len(), 3);
    assert!(d.using.timestamp.is_some());
    assert!(matches!(&d.if_clause, Some(IfClause::Conditions(c)) if c.len() == 2));
    let Statement::Insert(i) = dml("INSERT INTO t JSON ? IF NOT EXISTS USING TTL 5") else {
        panic!()
    };
    assert!(matches!(i.source, InsertSource::Json(_)) && i.if_not_exists && i.using.ttl.is_some());
}
