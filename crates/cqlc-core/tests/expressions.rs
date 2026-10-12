//! Type checking of the full expression language.

use cqlc_core::ast::{QueryKind, RelOp};
use cqlc_core::ir::{ParamSource, Query};
use cqlc_core::types::CqlType;
use cqlc_core::{Compilation, Input, compile};

const SCHEMA: &str = "
CREATE TYPE address (street text, city text, zip int);
CREATE TABLE c (
    id uuid PRIMARY KEY,
    name text, email text, born date, created timestamp, vip boolean,
    addr frozen<address>, live address,
    tags set<text>, phones list<text>, m map<text, int>, fm frozen<map<text, int>>,
    tup tuple<int, text>, score double, big bigint
);
CREATE TABLE o (
    cid uuid, t timeuuid, oid uuid, status text, total decimal,
    PRIMARY KEY ((cid), t, oid)
) WITH CLUSTERING ORDER BY (t DESC, oid ASC);
CREATE TABLE cnt (id uuid PRIMARY KEY, n counter);
CREATE TABLE ts (s text, d date, ts timestamp, v double, PRIMARY KEY ((s, d), ts));
";

fn run(queries: &str) -> Compilation {
    compile(
        &[Input::new("schema.cql", SCHEMA)],
        &[Input::new("q.cql", queries)],
    )
}

fn codes(c: &Compilation) -> Vec<&'static str> {
    c.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

fn errors(c: &Compilation) -> Vec<&'static str> {
    c.diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| d.code.as_str())
        .collect()
}

/// Compiles one query and expects no diagnostics at all.
fn ok(kind: &str, cql: &str) -> Query {
    let c = run(&format!("-- name: Q :{kind}\n{cql};"));
    assert!(c.diagnostics.is_empty(), "{cql}\n{:?}", c.diagnostics);
    c.ir.queries.into_iter().next().expect("one query")
}

/// Compiles one query, tolerating warnings but no errors.
fn okw(kind: &str, cql: &str) -> Query {
    let c = run(&format!("-- name: Q :{kind}\n{cql};"));
    assert!(!c.has_errors(), "{cql}\n{:?}", c.diagnostics);
    c.ir.queries.into_iter().next().expect("one query")
}

/// Compiles one query and returns the error codes.
fn err(kind: &str, cql: &str) -> Vec<&'static str> {
    let c = run(&format!("-- name: Q :{kind}\n{cql};"));
    errors(&c)
}

fn names(q: &Query) -> Vec<&str> {
    q.params.iter().map(|p| p.name.as_str()).collect()
}

fn param_types(q: &Query) -> Vec<CqlType> {
    q.params.iter().map(|p| p.cql_type.clone()).collect()
}

fn text_set() -> CqlType {
    CqlType::Set(Box::new(CqlType::Text))
}

// ----- selections -------------------------------------------------------------

#[test]
fn scalar_functions_have_types_and_nullability() {
    let q = ok(
        "one",
        "SELECT now() AS a, uuid() AS b, toTimestamp(now()) AS c, toDate(now()) AS d, \
         toUnixTimestamp(created) AS e, toTimestamp(born) AS f, minTimeuuid(created) AS g, \
         currentTimestamp() AS h, currentDate() AS i, currentTime() AS j, currentTimeUUID() AS k \
         FROM c WHERE id = ?",
    );
    let types: Vec<_> = q.columns.iter().map(|c| c.cql_type.clone()).collect();
    use CqlType::*;
    assert_eq!(
        types,
        [
            TimeUuid, Uuid, Timestamp, Date, BigInt, Timestamp, TimeUuid, Timestamp, Date, Time,
            TimeUuid
        ]
    );
    let nullable: Vec<_> = q.columns.iter().map(|c| c.nullable).collect();
    // Functions of regular (nullable) columns are nullable, the rest is not.
    assert_eq!(
        nullable,
        [
            false, false, false, false, true, true, true, false, false, false, false
        ]
    );
}

#[test]
fn metadata_functions() {
    let q = ok(
        "one",
        "SELECT writetime(email) AS w, ttl(email) AS t, token(id) AS k, cast(vip AS text) AS v, \
         cast(big AS int) AS i, cast(created AS date) AS d FROM c WHERE id = ?",
    );
    use CqlType::*;
    let types: Vec<_> = q.columns.iter().map(|c| c.cql_type.clone()).collect();
    assert_eq!(types, [BigInt, Int, BigInt, Text, Int, Date]);
    assert!(q.columns[0].nullable && q.columns[1].nullable && !q.columns[2].nullable);
}

#[test]
fn aggregate_types() {
    let q = ok(
        "one",
        "SELECT count(*) AS a, count(name) AS b, sum(score) AS d, avg(score) AS e, \
         min(created) AS f, max(name) AS g FROM c WHERE id = ?",
    );
    use CqlType::*;
    let types: Vec<_> = q.columns.iter().map(|c| c.cql_type.clone()).collect();
    assert_eq!(types, [BigInt, BigInt, Double, Double, Timestamp, Text]);
    let nullable: Vec<_> = q.columns.iter().map(|c| c.nullable).collect();
    assert_eq!(nullable, [false, false, false, false, true, true]);
}

#[test]
fn udt_fields_and_element_access() {
    let q = ok(
        "one",
        "SELECT addr.city AS city, live.zip AS zip, m['k'] AS v, phones[0] AS first FROM c WHERE id = ?",
    );
    use CqlType::*;
    let types: Vec<_> = q.columns.iter().map(|c| c.cql_type.clone()).collect();
    assert_eq!(types, [Text, Int, Int, Text]);
    assert!(q.columns.iter().all(|c| c.nullable));
    let q = ok("one", "SELECT m[?] AS v FROM c WHERE id = ?");
    assert_eq!(param_types(&q), [Text, CqlType::Uuid]);
    assert_eq!(names(&q), ["m_key", "id"]);
}

#[test]
fn json_distinct_group_by_per_partition_limit() {
    let q = ok("one", "SELECT JSON name, email FROM c WHERE id = ?");
    assert_eq!(q.columns.len(), 1);
    assert_eq!(q.columns[0].name, "[json]");
    assert!(!q.columns[0].nullable);
    let q = okw("many", "SELECT DISTINCT cid FROM o");
    assert_eq!(q.columns[0].name, "cid");
    let q = okw("many", "SELECT cid, count(*) AS n FROM o GROUP BY cid");
    assert_eq!(q.columns.len(), 2);
    let q = okw(
        "many",
        "SELECT cid, status FROM o PER PARTITION LIMIT ? LIMIT ?",
    );
    assert_eq!(names(&q), ["per_partition_limit", "limit"]);
}

#[test]
fn selection_errors() {
    assert_eq!(err("one", "SELECT now() FROM c WHERE id = ?"), ["CQL114"]);
    assert_eq!(
        err("one", "SELECT nope(name) AS x FROM c WHERE id = ?"),
        ["CQL113"]
    );
    assert_eq!(err("one", "SELECT ? AS x FROM c WHERE id = ?"), ["CQL106"]);
    assert_eq!(
        err("one", "SELECT toTimestamp(?) AS x FROM c WHERE id = ?"),
        ["CQL106"]
    );
    assert_eq!(
        err("one", "SELECT toTimestamp(name) AS x FROM c WHERE id = ?"),
        ["CQL105"]
    );
    assert_eq!(
        err("one", "SELECT now(1) AS x FROM c WHERE id = ?"),
        ["CQL108"]
    );
    assert_eq!(
        err("one", "SELECT sum(name) AS x FROM c WHERE id = ?"),
        ["CQL105"]
    );
    assert_eq!(
        err("one", "SELECT writetime(id) AS x FROM c WHERE id = ?"),
        ["CQL112"]
    );
    assert_eq!(
        err("one", "SELECT writetime(tags) AS x FROM c WHERE id = ?"),
        ["CQL112"]
    );
    assert_eq!(
        err("one", "SELECT token(name) AS x FROM c WHERE id = ?"),
        ["CQL112"]
    );
    assert_eq!(
        err("one", "SELECT cast(tags AS text) AS x FROM c WHERE id = ?"),
        ["CQL105"]
    );
    assert_eq!(
        err("one", "SELECT cast(name AS int) AS x FROM c WHERE id = ?"),
        ["CQL105"]
    );
    assert_eq!(
        err("one", "SELECT name.x AS x FROM c WHERE id = ?"),
        ["CQL105"]
    );
    assert_eq!(
        err("one", "SELECT addr.nope AS x FROM c WHERE id = ?"),
        ["CQL102"]
    );
    assert_eq!(
        err("one", "SELECT name[0] AS x FROM c WHERE id = ?"),
        ["CQL105"]
    );
    assert_eq!(err("many", "SELECT DISTINCT name FROM c"), ["CQL112"]);
    assert_eq!(
        err("many", "SELECT cid, count(*) AS n FROM o GROUP BY oid"),
        ["CQL112"]
    );
    // Aggregates and writetime only belong in the selection list.
    assert_eq!(
        err("many", "SELECT name FROM c WHERE name = count(*)"),
        ["CQL112"]
    );
}

// ----- values -----------------------------------------------------------------

#[test]
fn function_values_in_inserts() {
    let q = ok(
        "exec",
        "INSERT INTO c (id, created, born) VALUES (uuid(), toTimestamp(now()), toDate(now()))",
    );
    assert!(q.params.is_empty());
    // timeuuid is assignable to uuid
    let q = ok("exec", "INSERT INTO c (id) VALUES (now())");
    assert!(q.params.is_empty());
    assert_eq!(
        err("exec", "INSERT INTO c (id, name) VALUES (uuid(), now())"),
        ["CQL105"]
    );
    assert_eq!(
        err("exec", "INSERT INTO c (id, vip) VALUES (uuid(), uuid())"),
        ["CQL105"]
    );
}

#[test]
fn collection_tuple_and_udt_literals() {
    let q = ok(
        "exec",
        "INSERT INTO c (id, tags, phones, m, tup, addr) VALUES \
         (?, {'a', ?}, ['x', ?], {'k': ?}, (1, ?), {street: 's', zip: ?})",
    );
    use CqlType::*;
    assert_eq!(param_types(&q), [Uuid, Text, Text, Int, Text, Int]);
    assert_eq!(names(&q), ["id", "tags", "phones", "m", "tup", "addr"]);
    ok("exec", "INSERT INTO c (id, tags, m) VALUES (?, {}, {})");
    assert_eq!(
        err("exec", "INSERT INTO c (id, tags) VALUES (?, [1])"),
        ["CQL105"]
    );
    assert_eq!(
        err("exec", "INSERT INTO c (id, phones) VALUES (?, {'a'})"),
        ["CQL105"]
    );
    assert_eq!(
        err("exec", "INSERT INTO c (id, tup) VALUES (?, (1, 'a', 3))"),
        ["CQL108"]
    );
    assert_eq!(
        err("exec", "INSERT INTO c (id, tags) VALUES (?, {1})"),
        ["CQL105"]
    );
    assert_eq!(
        err("exec", "INSERT INTO c (id, addr) VALUES (?, {nope: 1})"),
        ["CQL102"]
    );
    assert_eq!(
        err("exec", "INSERT INTO c (id, m) VALUES (?, {'k': 'v'})"),
        ["CQL105"]
    );
}

#[test]
fn json_insert() {
    let q = ok("exec", "INSERT INTO c JSON ?");
    assert_eq!(q.params.len(), 1);
    assert_eq!(q.params[0].cql_type, CqlType::Text);
    assert_eq!(q.params[0].source, ParamSource::Json);
    ok(
        "exec",
        "INSERT INTO c JSON '{\"id\": \"123e4567-e89b-12d3-a456-426614174000\"}'",
    );
}

// ----- relations --------------------------------------------------------------

#[test]
fn token_relations() {
    let q = okw(
        "many",
        "SELECT id FROM c WHERE token(id) > token(?) LIMIT ?",
    );
    assert_eq!(param_types(&q), [CqlType::Uuid, CqlType::Int]);
    assert_eq!(
        q.params[0].source,
        ParamSource::Restriction {
            column: "token".into(),
            op: RelOp::Gt
        }
    );
    let q = okw("many", "SELECT id FROM c WHERE token(id) >= ?");
    assert_eq!(param_types(&q), [CqlType::BigInt]);
    // A token scan is deliberate: no broad-scan warning.
    let q = okw("many", "SELECT s FROM ts WHERE token(s, d) > token(?, ?)");
    assert_eq!(param_types(&q), [CqlType::Text, CqlType::Date]);
    assert_eq!(
        err("many", "SELECT s FROM ts WHERE token(s) > ?"),
        ["CQL112"]
    );
    assert_eq!(
        err("many", "SELECT id FROM c WHERE token(id) IN (1, 2)"),
        ["CQL112"]
    );
}

#[test]
fn tuple_relations() {
    let q = ok(
        "many",
        "SELECT status FROM o WHERE cid = ? AND (t, oid) > (?, ?) LIMIT ?",
    );
    assert_eq!(names(&q), ["cid", "t", "oid", "limit"]);
    assert_eq!(param_types(&q)[1], CqlType::TimeUuid);
    let q = ok(
        "many",
        "SELECT status FROM o WHERE cid = ? AND (t, oid) > ?",
    );
    assert_eq!(
        q.params[1].cql_type,
        CqlType::Tuple(vec![CqlType::TimeUuid, CqlType::Uuid])
    );
    let q = ok(
        "many",
        "SELECT status FROM o WHERE cid = ? AND (t, oid) IN ((?, ?), (?, ?))",
    );
    assert_eq!(q.params.len(), 5);
    assert_eq!(
        err("many", "SELECT status FROM o WHERE (cid, t) = (?, ?)"),
        ["CQL112"]
    );
    assert_eq!(
        err(
            "many",
            "SELECT status FROM o WHERE cid = ? AND (t, oid) > (?, ?, ?)"
        ),
        ["CQL108"]
    );
}

#[test]
fn contains_and_element_restrictions() {
    let q = okw(
        "many",
        "SELECT id FROM c WHERE tags CONTAINS ? ALLOW FILTERING",
    );
    assert_eq!(param_types(&q), [CqlType::Text]);
    let q = okw(
        "many",
        "SELECT id FROM c WHERE m CONTAINS ? ALLOW FILTERING",
    );
    assert_eq!(param_types(&q), [CqlType::Int]);
    let q = okw(
        "many",
        "SELECT id FROM c WHERE m CONTAINS KEY ? ALLOW FILTERING",
    );
    assert_eq!(param_types(&q), [CqlType::Text]);
    let q = okw("many", "SELECT id FROM c WHERE m[?] = ? ALLOW FILTERING");
    assert_eq!(names(&q), ["m_key", "m_value"]);
    assert_eq!(param_types(&q), [CqlType::Text, CqlType::Int]);
    assert_eq!(
        err(
            "many",
            "SELECT id FROM c WHERE name CONTAINS ? ALLOW FILTERING"
        ),
        ["CQL105"]
    );
    assert_eq!(
        err(
            "many",
            "SELECT id FROM c WHERE tags CONTAINS KEY ? ALLOW FILTERING"
        ),
        ["CQL105"]
    );
}

#[test]
fn function_calls_in_relations() {
    let q = ok(
        "many",
        "SELECT status FROM o WHERE cid = ? AND t > minTimeuuid(?) AND t < maxTimeuuid(?)",
    );
    assert_eq!(names(&q), ["cid", "t_gt", "t_lt"]);
    assert_eq!(param_types(&q)[1], CqlType::Timestamp);
    ok("many", "SELECT status FROM o WHERE cid = ? AND t < now()");
    assert_eq!(
        err(
            "many",
            "SELECT status FROM o WHERE cid = ? AND t > toTimestamp(now())"
        ),
        ["CQL105"]
    );
    assert_eq!(err("many", "SELECT id FROM c WHERE name != ?"), ["CQL112"]);
}

#[test]
fn in_list_literals() {
    let q = ok(
        "many",
        "SELECT status FROM o WHERE cid IN (?, 123e4567-e89b-12d3-a456-426614174000)",
    );
    assert_eq!(q.params.len(), 1);
    assert_eq!(
        err("many", "SELECT status FROM o WHERE cid IN 5"),
        ["CQL105"]
    );
}

// ----- updates ----------------------------------------------------------------

#[test]
fn collection_and_counter_updates() {
    let q = ok(
        "exec",
        "UPDATE c SET tags = tags + ?, phones = ? + phones, m = m + ? WHERE id = ?",
    );
    assert_eq!(
        param_types(&q),
        [
            text_set(),
            CqlType::List(Box::new(CqlType::Text)),
            CqlType::Map(Box::new(CqlType::Text), Box::new(CqlType::Int)),
            CqlType::Uuid
        ]
    );
    assert!(
        q.params.iter().all(|p| !p.nullable),
        "partial updates never take NULL"
    );
    let q = ok("exec", "UPDATE c SET tags -= ?, m = m - ? WHERE id = ?");
    assert_eq!(param_types(&q)[0], text_set());
    assert_eq!(
        param_types(&q)[1],
        text_set(),
        "removing map entries takes the keys"
    );
    let q = ok("exec", "UPDATE cnt SET n = n + ? WHERE id = ?");
    assert_eq!(q.params[0].cql_type, CqlType::Counter);
    let q = ok("exec", "UPDATE cnt SET n -= ? WHERE id = ?");
    assert_eq!(q.params[0].cql_type, CqlType::Counter);
}

#[test]
fn invalid_updates() {
    assert_eq!(err("exec", "UPDATE cnt SET n = ? WHERE id = ?"), ["CQL112"]);
    assert_eq!(
        err("exec", "UPDATE c SET name = name + ? WHERE id = ?"),
        ["CQL112"]
    );
    assert_eq!(
        err("exec", "UPDATE c SET fm = fm + ? WHERE id = ?"),
        ["CQL112"]
    );
    assert_eq!(
        err(
            "exec",
            "UPDATE c SET phones = phones - ?, tags = ? + tags WHERE id = ?"
        ),
        ["CQL112"]
    );
    assert_eq!(err("exec", "UPDATE c SET id = ? WHERE id = ?"), ["CQL112"]);
    assert_eq!(
        err("exec", "UPDATE c SET nope = ? WHERE id = ?"),
        ["CQL102"]
    );
    assert_eq!(
        err("exec", "INSERT INTO cnt (id, n) VALUES (?, 1)"),
        ["CQL112"]
    );
}

#[test]
fn element_and_field_assignment() {
    let q = ok("exec", "UPDATE c SET m[?] = ?, phones[?] = ? WHERE id = ?");
    assert_eq!(
        names(&q),
        ["m_key", "m_value", "phones_key", "phones_value", "id"]
    );
    use CqlType::*;
    assert_eq!(param_types(&q), [Text, Int, Int, Text, Uuid]);
    let q = ok("exec", "UPDATE c SET live.zip = ? WHERE id = ?");
    assert_eq!(q.params[0].cql_type, CqlType::Int);
    assert_eq!(names(&q), ["live_zip", "id"]);
    assert_eq!(
        err("exec", "UPDATE c SET fm[?] = ? WHERE id = ?"),
        ["CQL112"]
    );
    assert_eq!(
        err("exec", "UPDATE c SET addr.zip = ? WHERE id = ?"),
        ["CQL105"]
    );
    assert_eq!(
        err("exec", "UPDATE c SET live.nope = ? WHERE id = ?"),
        ["CQL102"]
    );
    assert_eq!(
        err("exec", "UPDATE c SET name[?] = ? WHERE id = ?"),
        ["CQL105"]
    );
}

#[test]
fn regular_assignments_bind_nullable_values() {
    let q = ok("exec", "UPDATE c SET name = ?, tags = ? WHERE id = ?");
    assert!(q.params[0].nullable && q.params[1].nullable && !q.params[2].nullable);
    ok("exec", "UPDATE c SET name = null WHERE id = ?");
    ok(
        "exec",
        "UPDATE c USING TTL 60 SET tags = {'a'}, phones = ['b'] WHERE id = ?",
    );
    let q = ok(
        "exec",
        "UPDATE c USING TTL ? AND TIMESTAMP ? SET name = ? WHERE id = ?",
    );
    assert_eq!(names(&q), ["ttl", "timestamp", "name", "id"]);
    assert_eq!(param_types(&q)[..2], [CqlType::Int, CqlType::BigInt]);
}

// ----- deletes ----------------------------------------------------------------

#[test]
fn delete_targets() {
    let q = ok("exec", "DELETE tags, phones FROM c WHERE id = ?");
    assert_eq!(q.params.len(), 1);
    let q = ok("exec", "DELETE m[?] FROM c WHERE id = ?");
    assert_eq!(names(&q), ["m_key", "id"]);
    let q = ok("exec", "DELETE phones[?] FROM c WHERE id = ?");
    assert_eq!(param_types(&q)[0], CqlType::Int);
    ok("exec", "DELETE live.zip FROM c WHERE id = ?");
    ok("exec", "DELETE FROM c USING TIMESTAMP ? WHERE id = ?");
    assert_eq!(err("exec", "DELETE id FROM c WHERE id = ?"), ["CQL112"]);
    assert_eq!(err("exec", "DELETE fm[?] FROM c WHERE id = ?"), ["CQL112"]);
    assert_eq!(
        err("exec", "DELETE name[?] FROM c WHERE id = ?"),
        ["CQL112"]
    );
    assert_eq!(
        err("exec", "DELETE addr.zip FROM c WHERE id = ?"),
        ["CQL112"]
    );
}

// ----- lightweight transactions -------------------------------------------------

#[test]
fn lwt_if_not_exists_reports_the_whole_row() {
    let q = ok("lwt", "INSERT INTO cnt (id) VALUES (?) IF NOT EXISTS");
    assert_eq!(q.kind, QueryKind::Lwt);
    let cols: Vec<_> = q.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(cols, ["id", "n"]);
    assert!(q.columns.iter().all(|c| c.nullable));
}

#[test]
fn lwt_conditions_report_the_tested_columns() {
    let q = ok(
        "lwt",
        "UPDATE c SET name = ? WHERE id = ? IF name = ? AND vip = true",
    );
    let cols: Vec<_> = q.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(cols, ["name", "vip"]);
    // The column is both written and tested: roles disambiguate the names.
    assert_eq!(names(&q), ["name_new", "id", "name_if_eq"]);
    let q = ok("lwt", "DELETE FROM c WHERE id = ? IF EXISTS");
    assert!(q.columns.is_empty());
    let q = ok(
        "lwt",
        "UPDATE c SET name = ? WHERE id = ? IF born > ? AND score IN (1.5, 2.5)",
    );
    assert_eq!(q.columns.len(), 2);
    assert_eq!(
        err("lwt", "UPDATE c SET name = ? WHERE id = ? IF id = ?"),
        ["CQL112"]
    );
    assert_eq!(
        err(
            "lwt",
            "UPDATE c SET name = ? WHERE id = ? IF tags CONTAINS ?"
        ),
        ["CQL112"]
    );
    assert_eq!(
        err("lwt", "UPDATE c SET name = ? WHERE id = ? IF m['k'] = ?"),
        ["CQL002"]
    );
}

// ----- order, naming, schema checks ---------------------------------------------

#[test]
fn order_by_must_follow_the_clustering_order() {
    // Declared order is (t DESC, oid ASC).
    for cql in [
        "SELECT status FROM o WHERE cid = ? ORDER BY t DESC",
        "SELECT status FROM o WHERE cid = ? ORDER BY t ASC",
        "SELECT status FROM o WHERE cid = ? ORDER BY t DESC, oid ASC",
        "SELECT status FROM o WHERE cid = ? ORDER BY t ASC, oid DESC",
        "SELECT status FROM o WHERE cid = ? AND t = ? ORDER BY oid ASC",
    ] {
        let c = run(&format!("-- name: Q :many\n{cql};"));
        assert!(c.diagnostics.is_empty(), "{cql}: {:?}", c.diagnostics);
    }
    for cql in [
        "SELECT status FROM o WHERE cid = ? ORDER BY oid ASC",
        "SELECT status FROM o WHERE cid = ? ORDER BY t DESC, oid DESC",
    ] {
        let c = run(&format!("-- name: Q :many\n{cql};"));
        assert_eq!(codes(&c), ["CQL205"], "{cql}");
        assert!(!c.has_errors());
    }
    assert_eq!(
        err("many", "SELECT status FROM o WHERE cid = ? ORDER BY status"),
        ["CQL112"]
    );
}

#[test]
fn reserved_words_need_quotes() {
    let bad = |ddl: &str| {
        compile(&[Input::new("s.cql", ddl)], &[])
            .diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect::<Vec<_>>()
    };
    assert_eq!(bad("CREATE TABLE t (token text PRIMARY KEY);"), ["CQL115"]);
    assert_eq!(bad("CREATE TABLE select (a int PRIMARY KEY);"), ["CQL115"]);
    assert_eq!(bad("CREATE TYPE u (order int);"), ["CQL115"]);
    assert!(bad("CREATE TABLE t (\"token\" text PRIMARY KEY, name text, \"key\" int);").is_empty());
    assert!(bad("CREATE TABLE t (key text PRIMARY KEY, value int, timestamp int);").is_empty());
}

#[test]
fn every_marker_is_accounted_for() {
    // The checker verifies that all bind markers got a type.
    for cql in [
        "SELECT id FROM c WHERE id = ? LIMIT ?",
        "UPDATE c USING TTL ? SET name = ?, tags = tags + ? WHERE id = ?",
        "INSERT INTO c (id, tags, m) VALUES (?, {?, ?}, {?: ?})",
        "SELECT status FROM o WHERE cid IN ? AND (t, oid) > (?, ?)",
    ] {
        let kind = if cql.starts_with("SELECT") {
            "many"
        } else {
            "exec"
        };
        let c = run(&format!("-- name: Q :{kind}\n{cql};"));
        let markers = cql.matches('?').count();
        assert_eq!(
            c.ir.queries[0].params.len(),
            markers,
            "{cql}: {:?}",
            c.diagnostics
        );
    }
}

// ----- silencing warnings ---------------------------------------------------------

#[test]
fn allow_annotation_silences_only_the_named_warnings() {
    let q = "SELECT name FROM c WHERE name = ? ALLOW FILTERING";
    let c = run(&format!("-- name: A :many\n{q};"));
    assert_eq!(codes(&c), ["CQL201", "CQL202"]);
    let c = run(&format!("-- allow: CQL201\n-- name: A :many\n{q};"));
    assert_eq!(codes(&c), ["CQL202"]);
    let c = run(&format!("-- name: A :many\n-- allow: cql201, CQL202\n{q};"));
    assert!(c.diagnostics.is_empty());
    // It applies to the next statement only.
    let c = run(&format!(
        "-- allow: CQL201, CQL202\n-- name: A :many\n{q};\n-- name: B :many\n{q};"
    ));
    assert_eq!(codes(&c), ["CQL201", "CQL202"]);
    assert_eq!(c.diagnostics[0].span.start, c.diagnostics[1].span.start);
}

#[test]
fn errors_cannot_be_allowed() {
    let c = run("-- allow: CQL102\n-- name: A :many\nSELECT nope FROM c WHERE id = ?;");
    // The attempt is reported, and the real error is still reported.
    assert_eq!(errors(&c), ["CQL107", "CQL102"]);
    let c = run("-- allow: CQL999\n-- name: A :many\nSELECT name FROM c WHERE id = ?;");
    assert_eq!(codes(&c), ["CQL107"]);
}
