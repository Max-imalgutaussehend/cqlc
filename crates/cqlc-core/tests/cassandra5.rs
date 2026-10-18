//! Cassandra 4.0, 4.1 and 5.0 CQL: vectors, SAI, masking, arithmetic, new functions, views, ALTER/DROP.
//! The reference is the ANTLR grammar and `OperationFcts`/`MathFcts`/`CollectionFcts`/`VectorFcts` of apache/cassandra.

use cqlc_core::ir::Query;
use cqlc_core::types::CqlType;
use cqlc_core::{Compilation, Input, compile};

fn run(schema: &str, queries: &str) -> Compilation {
    compile(
        &[Input::new("schema.cql", schema)],
        &[Input::new("queries.cql", queries)],
    )
}

fn codes(c: &Compilation) -> Vec<&'static str> {
    c.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

fn ok(schema: &str, queries: &str) -> Vec<Query> {
    let c = run(schema, queries);
    assert!(
        c.diagnostics.is_empty(),
        "unexpected: {:?}",
        c.diagnostics
            .iter()
            .map(|d| (d.code.as_str(), d.message.clone()))
            .collect::<Vec<_>>()
    );
    c.ir.queries
}

const ITEMS: &str = "
CREATE TABLE items (id int PRIMARY KEY, category text, price double, qty int, embedding vector<float, 3>, tags set<text>, attrs map<text, int>);
CREATE INDEX items_embedding ON items(embedding) USING 'sai';
CREATE INDEX items_category ON items(category) USING 'sai';
CREATE INDEX items_price ON items(price) USING 'sai';
";

#[test]
fn vector_columns_and_ann_search() {
    let q = &ok(
        ITEMS,
        "-- name: Nearest :many\nSELECT id, category FROM items ORDER BY embedding ANN OF ? LIMIT 5;",
    )[0];
    assert_eq!(
        q.params[0].cql_type,
        CqlType::Vector(Box::new(CqlType::Float), 3)
    );
    // vector literal of the right dimension
    ok(
        ITEMS,
        "-- name: N :many\nSELECT id FROM items ORDER BY embedding ANN OF [0.1, 0.2, 0.3] LIMIT 5;",
    );
    // insert with a vector value
    let w = &ok(
        ITEMS,
        "-- name: Add :exec\nINSERT INTO items (id, embedding) VALUES (?, ?);",
    )[0];
    assert_eq!(
        w.params[1].cql_type,
        CqlType::Vector(Box::new(CqlType::Float), 3)
    );
}

#[test]
fn ann_rules_match_cassandra() {
    let no_limit = run(
        ITEMS,
        "-- name: N :many\nSELECT id FROM items ORDER BY embedding ANN OF ?;",
    );
    assert_eq!(codes(&no_limit), ["CQL112"]);
    let big = run(
        ITEMS,
        "-- name: N :many\nSELECT id FROM items ORDER BY embedding ANN OF ? LIMIT 5000;",
    );
    assert_eq!(codes(&big), ["CQL112"]);
    let not_vector = run(
        ITEMS,
        "-- name: N :many\nSELECT id FROM items ORDER BY price ANN OF ? LIMIT 5;",
    );
    assert!(codes(&not_vector).contains(&"CQL105"));
    let no_index = run(
        "CREATE TABLE t (id int PRIMARY KEY, v vector<float, 3>);",
        "-- name: N :many\nSELECT id FROM t ORDER BY v ANN OF ? LIMIT 5;",
    );
    assert_eq!(codes(&no_index), ["CQL112"]);
    let wrong_dim = run(
        ITEMS,
        "-- name: N :many\nSELECT id FROM items ORDER BY embedding ANN OF [1.0, 2.0] LIMIT 5;",
    );
    assert_eq!(codes(&wrong_dim), ["CQL105"]);
}

#[test]
fn sai_indexes_make_non_key_restrictions_legal() {
    // equality and ranges are served by SAI: no CQL203 (the scatter-gather warning CQL201 stays)
    let c = run(
        ITEMS,
        "-- name: A :many\nSELECT id FROM items WHERE category = ? AND price > ?;",
    );
    assert_eq!(codes(&c), ["CQL201"]);
    // a legacy index only serves equality
    let legacy = "CREATE TABLE t (id int PRIMARY KEY, a int); CREATE INDEX ON t(a);";
    assert_eq!(
        codes(&run(
            legacy,
            "-- name: A :many\nSELECT id FROM t WHERE a = ?;"
        )),
        ["CQL201"]
    );
    assert_eq!(
        codes(&run(
            legacy,
            "-- name: A :many\nSELECT id FROM t WHERE a > ?;"
        )),
        ["CQL201", "CQL203"]
    );
    // no index at all
    assert_eq!(
        codes(&run(
            "CREATE TABLE t (id int PRIMARY KEY, a int);",
            "-- name: A :many\nSELECT id FROM t WHERE a = ?;"
        )),
        ["CQL201", "CQL203"]
    );
    // CONTAINS served by a collection index
    let c = run(
        "CREATE TABLE t (id int PRIMARY KEY, tags set<text>); CREATE CUSTOM INDEX ON t(tags) USING 'sai';",
        "-- name: A :many\nSELECT id FROM t WHERE tags CONTAINS ?;",
    );
    assert_eq!(codes(&c), ["CQL201"]);
    let keys = "CREATE TABLE t (id int PRIMARY KEY, m map<text, int>); CREATE INDEX ON t(KEYS(m));";
    assert_eq!(
        codes(&run(
            keys,
            "-- name: A :many\nSELECT id FROM t WHERE m CONTAINS KEY ?;"
        )),
        ["CQL201"]
    );
}

#[test]
fn like_needs_a_text_column() {
    let sasi = "CREATE TABLE t (id int PRIMARY KEY, name text, n int); CREATE CUSTOM INDEX ON t(name) USING 'org.apache.cassandra.index.sasi.SASIIndex';";
    assert_eq!(
        codes(&run(
            sasi,
            "-- name: A :many\nSELECT id FROM t WHERE name LIKE ?;"
        )),
        ["CQL201"]
    );
    assert!(
        codes(&run(
            sasi,
            "-- name: A :many\nSELECT id FROM t WHERE n LIKE ?;"
        ))
        .contains(&"CQL105")
    );
}

#[test]
fn masking_and_constraints_parse() {
    let schema = "
CREATE TABLE people (
    id int PRIMARY KEY,
    email text MASKED WITH mask_default(),
    ssn text MASKED WITH DEFAULT,
    nick text MASKED WITH mask_inner(1, 1),
    age int CHECK age > 0 AND age < 150,
    score int CHECK score >= 0
) WITH comment = 'pii';
";
    let q = &ok(
        schema,
        "-- name: P :one\nSELECT id, email, age FROM people WHERE id = ?;",
    )[0];
    assert_eq!(q.columns.len(), 3);
}

#[test]
fn arithmetic_follows_cassandra_result_types() {
    let schema = "CREATE TABLE t (id int PRIMARY KEY, a int, b bigint, f float, d double, ts timestamp, dt date, s text, dec decimal, v varint, t8 tinyint);";
    let types = |expr: &str| {
        let q = &ok(
            schema,
            &format!("-- name: Q :one\nSELECT {expr} AS x FROM t WHERE id = ?;"),
        )[0];
        q.columns[0].cql_type.clone()
    };
    assert_eq!(types("a + a"), CqlType::Int);
    assert_eq!(types("a + b"), CqlType::BigInt);
    assert_eq!(types("a * f"), CqlType::Float);
    assert_eq!(types("b / f"), CqlType::Double);
    assert_eq!(types("a - d"), CqlType::Double);
    assert_eq!(types("a % 3"), CqlType::Int);
    assert_eq!(types("t8 + t8"), CqlType::TinyInt);
    assert_eq!(types("a + dec"), CqlType::Decimal);
    assert_eq!(types("a + v"), CqlType::VarInt);
    assert_eq!(types("f + v"), CqlType::Decimal);
    assert_eq!(types("-a"), CqlType::Int);
    assert_eq!(types("(a + b) * 2"), CqlType::BigInt);
    assert_eq!(types("a + 1 * 2"), CqlType::Int);
    assert_eq!(types("ts + 1d"), CqlType::Timestamp);
    assert_eq!(types("dt - 1h30m"), CqlType::Date);
    assert_eq!(types("s + 'x'"), CqlType::Text);
    // timestamp - timestamp does not exist in Cassandra
    let bad = run(
        schema,
        "-- name: Q :one\nSELECT ts - ts AS x FROM t WHERE id = ?;",
    );
    assert_eq!(codes(&bad), ["CQL105"]);
    // a marker takes the type of the other operand
    let q = &ok(
        schema,
        "-- name: Q :one\nSELECT a + ? AS x FROM t WHERE id = ?;",
    )[0];
    assert_eq!(q.params[0].cql_type, CqlType::Int);
}

#[test]
fn new_native_functions() {
    let schema = "CREATE TABLE t (id int PRIMARY KEY, a int, d double, tags set<text>, nums list<int>, m map<text, int>, v vector<float, 3>, v2 vector<float, 3>, body text);";
    let ty = |expr: &str| {
        let q = &ok(
            schema,
            &format!("-- name: Q :one\nSELECT {expr} AS x FROM t WHERE id = ?;"),
        )[0];
        q.columns[0].cql_type.clone()
    };
    assert_eq!(ty("abs(a)"), CqlType::Int);
    assert_eq!(ty("round(d)"), CqlType::Double);
    assert_eq!(ty("log10(d)"), CqlType::Double);
    assert_eq!(ty("exp(a)"), CqlType::Int);
    assert_eq!(ty("collection_count(tags)"), CqlType::Int);
    assert_eq!(ty("collection_max(nums)"), CqlType::Int);
    assert_eq!(ty("collection_avg(nums)"), CqlType::Int);
    assert_eq!(ty("map_keys(m)"), CqlType::Set(Box::new(CqlType::Text)));
    assert_eq!(ty("map_values(m)"), CqlType::List(Box::new(CqlType::Int)));
    assert_eq!(ty("similarity_cosine(v, v2)"), CqlType::Float);
    assert_eq!(
        ty("similarity_euclidean(v, [1.0, 2.0, 3.0])"),
        CqlType::Float
    );
    assert_eq!(ty("maxwritetime(body)"), CqlType::BigInt);
    assert_eq!(ty("maxwritetime(tags)"), CqlType::BigInt);
    assert_eq!(ty("toJson(a)"), CqlType::Text);
    assert_eq!(ty("intAsBlob(a)"), CqlType::Blob);
    assert_eq!(ty("blobAsInt(intAsBlob(a))"), CqlType::Int);
    // writetime still rejects collections
    assert_eq!(
        codes(&run(
            schema,
            "-- name: Q :one\nSELECT writetime(tags) AS x FROM t WHERE id = ?;"
        )),
        ["CQL112"]
    );
    // wrong argument types
    assert_eq!(
        codes(&run(
            schema,
            "-- name: Q :one\nSELECT map_keys(a) AS x FROM t WHERE id = ?;"
        )),
        ["CQL105"]
    );
    assert_eq!(
        codes(&run(
            schema,
            "-- name: Q :one\nSELECT similarity_cosine(v, d) AS x FROM t WHERE id = ?;"
        )),
        ["CQL105"]
    );
    let q = &ok(
        schema,
        "-- name: S :many\nSELECT id, similarity_cosine(v, ?) AS score FROM t WHERE id = ?;",
    )[0];
    assert_eq!(
        q.params[0].cql_type,
        CqlType::Vector(Box::new(CqlType::Float), 3)
    );
}

#[test]
fn materialized_views_are_queryable_and_read_only() {
    let schema = "
CREATE TABLE users (id uuid PRIMARY KEY, email text, name text, age int);
CREATE MATERIALIZED VIEW users_by_email AS
    SELECT id, email, name FROM users
    WHERE email IS NOT NULL AND id IS NOT NULL
    PRIMARY KEY (email, id)
    WITH CLUSTERING ORDER BY (id DESC);
";
    let q = &ok(
        schema,
        "-- name: ByEmail :one\nSELECT id, name FROM users_by_email WHERE email = ?;",
    )[0];
    assert_eq!(q.params[0].cql_type, CqlType::Text);
    let bad = run(
        schema,
        "-- name: W :exec\nINSERT INTO users_by_email (email, id) VALUES (?, ?);",
    );
    assert_eq!(codes(&bad), ["CQL112"]);
    // selecting a column the view does not carry
    assert_eq!(
        codes(&run(
            schema,
            "-- name: B :one\nSELECT age FROM users_by_email WHERE email = ?;"
        )),
        ["CQL102"]
    );
}

#[test]
fn alter_and_drop_evolve_the_schema_in_order() {
    let schema = "
CREATE TABLE t (id int PRIMARY KEY, a text, b text);
ALTER TABLE t ADD c int;
ALTER TABLE t ADD (d double, e text);
ALTER TABLE t DROP b;
ALTER TABLE t WITH comment = 'x' AND gc_grace_seconds = 100;
CREATE TABLE gone (k int PRIMARY KEY);
DROP TABLE gone;
CREATE TYPE addr (street text);
ALTER TYPE addr ADD zip int;
ALTER TYPE addr RENAME zip TO postal;
CREATE TABLE u (id int PRIMARY KEY, home frozen<addr>);
";
    ok(
        schema,
        "-- name: A :one\nSELECT id, a, c, d, e FROM t WHERE id = ?;",
    );
    assert_eq!(
        codes(&run(
            schema,
            "-- name: B :one\nSELECT b FROM t WHERE id = ?;"
        )),
        ["CQL102"]
    );
    assert_eq!(
        codes(&run(
            schema,
            "-- name: C :one\nSELECT k FROM gone WHERE k = ?;"
        )),
        ["CQL101"]
    );
    ok(
        schema,
        "-- name: H :one\nSELECT home.postal AS p FROM u WHERE id = ?;",
    );
    // dropping a key column is an error
    let c = compile(
        &[Input::new(
            "s.cql",
            "CREATE TABLE t (id int PRIMARY KEY, a int); ALTER TABLE t DROP id;",
        )],
        &[],
    );
    assert!(c.has_errors());
}

#[test]
fn user_defined_functions_and_aggregates() {
    let schema = "
CREATE TABLE t (id int PRIMARY KEY, a int, b text);
CREATE OR REPLACE FUNCTION double_it (x int) CALLED ON NULL INPUT RETURNS int LANGUAGE java AS 'return x * 2;';
CREATE FUNCTION IF NOT EXISTS shout (s text) RETURNS NULL ON NULL INPUT RETURNS text LANGUAGE java AS $$ return s.toUpperCase(); $$;
CREATE FUNCTION state_sum (state int, v int) CALLED ON NULL INPUT RETURNS int LANGUAGE java AS 'return state + v;';
CREATE FUNCTION fin (state int) CALLED ON NULL INPUT RETURNS double LANGUAGE java AS 'return (double) state;';
CREATE AGGREGATE total (int) SFUNC state_sum STYPE int FINALFUNC fin INITCOND 0;
";
    let ty = |expr: &str| {
        let q = &ok(
            schema,
            &format!("-- name: Q :many\nSELECT {expr} AS x FROM t WHERE id = ?;"),
        )[0];
        q.columns[0].cql_type.clone()
    };
    assert_eq!(ty("double_it(a)"), CqlType::Int);
    assert_eq!(ty("shout(b)"), CqlType::Text);
    assert_eq!(ty("total(a)"), CqlType::Double);
    assert_eq!(
        codes(&run(
            schema,
            "-- name: Q :many\nSELECT double_it(a, a) AS x FROM t WHERE id = ?;"
        )),
        ["CQL108"]
    );
    // dropped functions disappear
    let dropped = format!("{schema} DROP FUNCTION double_it;");
    assert_eq!(
        codes(&run(
            &dropped,
            "-- name: Q :many\nSELECT double_it(a) AS x FROM t WHERE id = ?;"
        )),
        ["CQL113"]
    );
}

#[test]
fn statements_without_effect_on_types_are_accepted_in_schema_files() {
    let schema = "
USE shop;
CREATE KEYSPACE IF NOT EXISTS shop WITH replication = {'class': 'NetworkTopologyStrategy', 'dc1': 3} AND durable_writes = true;
CREATE ROLE IF NOT EXISTS app WITH PASSWORD = 'x' AND LOGIN = true;
GRANT SELECT ON KEYSPACE shop TO app;
CREATE TABLE t (id int PRIMARY KEY, v int);
CREATE TRIGGER trg ON t USING 'x.Trigger';
TRUNCATE t;
ALTER KEYSPACE shop WITH replication = {'class': 'SimpleStrategy', 'replication_factor': 1};
";
    ok(schema, "-- name: Q :one\nSELECT v FROM t WHERE id = ?;");
}

#[test]
fn keys_entries_and_full_indexes_validate_their_column() {
    let c = compile(
        &[Input::new(
            "s.cql",
            "CREATE TABLE t (id int PRIMARY KEY, n int); CREATE INDEX ON t(KEYS(n));",
        )],
        &[],
    );
    assert!(c.has_errors());
    let c = compile(
        &[Input::new(
            "s.cql",
            "CREATE TABLE t (id int PRIMARY KEY, l list<int>); CREATE INDEX ON t(FULL(l));",
        )],
        &[],
    );
    assert!(c.has_errors(), "FULL needs a frozen collection");
    let c = compile(
        &[Input::new(
            "s.cql",
            "CREATE TABLE t (id int PRIMARY KEY, l frozen<list<int>>); CREATE INDEX ON t(FULL(l));",
        )],
        &[],
    );
    assert!(!c.has_errors());
}
