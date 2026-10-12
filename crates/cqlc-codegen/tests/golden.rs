//! The committed generated module of `examples/basic` is the golden file.
//! Update it with `cqlc generate -c examples/basic/cqlc.toml`.

use cqlc_codegen::{Options, TimeLibrary, generate_rust};
use cqlc_core::{Input, compile};
use std::path::{Path, PathBuf};

fn example_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/basic")
}

fn inputs(dir: &Path) -> Vec<Input> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "cql"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|p| Input {
            name: p.display().to_string(),
            text: std::fs::read_to_string(&p).unwrap(),
        })
        .collect()
}

fn generate(schema: &[Input], queries: &[Input], opts: &Options) -> String {
    let c = compile(schema, queries);
    assert!(!c.has_errors(), "{:?}", c.diagnostics);
    generate_rust(&c.ir, opts).expect("generation succeeds")
}

#[test]
fn example_matches_committed_output() {
    let dir = example_dir();
    let opts = Options {
        time_library: TimeLibrary::Chrono,
    };
    let out = generate(
        &inputs(&dir.join("schema")),
        &inputs(&dir.join("queries")),
        &opts,
    );
    let expected = std::fs::read_to_string(dir.join("src/queries.rs")).unwrap();
    assert_eq!(
        out, expected,
        "run `cqlc generate -c examples/basic/cqlc.toml`"
    );
}

#[test]
fn output_is_deterministic() {
    let dir = example_dir();
    let (s, q) = (inputs(&dir.join("schema")), inputs(&dir.join("queries")));
    let opts = Options::default();
    assert_eq!(generate(&s, &q, &opts), generate(&s, &q, &opts));
}

fn one(schema: &str, query: &str, opts: &Options) -> Result<String, Vec<String>> {
    let c = compile(
        &[Input {
            name: "s.cql".into(),
            text: schema.into(),
        }],
        &[Input {
            name: "q.cql".into(),
            text: query.into(),
        }],
    );
    assert!(!c.has_errors(), "{:?}", c.diagnostics);
    generate_rust(&c.ir, opts).map_err(|d| d.into_iter().map(|d| d.message).collect())
}

#[test]
fn single_parameter_is_a_plain_argument_multiple_use_a_struct() {
    let schema = "CREATE TABLE t (k int PRIMARY KEY, v text);";
    let out = one(
        schema,
        "-- name: A :one\nSELECT v FROM t WHERE k = ?;",
        &Options::default(),
    )
    .unwrap();
    assert!(out.contains("pub async fn a(session: &::scylla::client::session::Session, k: i32)"));
    assert!(!out.contains("AParams"));
    let out = one(
        schema,
        "-- name: B :exec\nINSERT INTO t (k, v) VALUES (?, ?);",
        &Options::default(),
    )
    .unwrap();
    assert!(out.contains("pub struct BParams"));
    assert!(out.contains("params: BParams"));
}

#[test]
fn keyword_columns_and_aliases_are_renamed_for_the_driver() {
    let schema = "CREATE TABLE t (k int PRIMARY KEY, \"type\" text, \"userId\" text);";
    let out = one(
        schema,
        "-- name: A :one\nSELECT \"type\", \"userId\" FROM t WHERE k = ?;",
        &Options::default(),
    )
    .unwrap();
    assert!(
        out.contains("#[scylla(rename = \"type\")]\n    pub type_:"),
        "{out}"
    );
    assert!(
        out.contains("#[scylla(rename = \"userId\")]\n    pub user_id:"),
        "{out}"
    );
}

#[test]
fn time_library_switches_types() {
    let schema = "CREATE TABLE t (k int PRIMARY KEY, ts timestamp, d date);";
    let q = "-- name: A :one\nSELECT ts, d FROM t WHERE k = ?;";
    let native = one(schema, q, &Options::default()).unwrap();
    assert!(native.contains("::scylla::value::CqlTimestamp"));
    assert!(native.contains("::scylla::value::CqlDate"));
    let chrono = one(
        schema,
        q,
        &Options {
            time_library: TimeLibrary::Chrono,
        },
    )
    .unwrap();
    assert!(chrono.contains("::chrono::DateTime<::chrono::Utc>"));
    assert!(chrono.contains("::chrono::NaiveDate"));
}

#[test]
fn collection_type_mapping() {
    let schema = "CREATE TABLE t (k int PRIMARY KEY, a list<int>, b set<text>, c map<text, bigint>, d tuple<int, text>);";
    let out = one(
        schema,
        "-- name: A :one\nSELECT a, b, c, d FROM t WHERE k = ?;",
        &Options::default(),
    )
    .unwrap();
    assert!(out.contains("Option<::std::vec::Vec<i32>>"));
    assert!(out.contains("Option<::std::collections::BTreeSet<::std::string::String>>"));
    assert!(out.contains("Option<::std::collections::BTreeMap<::std::string::String, i64>>"));
    assert!(out.contains("Option<(i32, ::std::string::String,)>"));
}

#[test]
fn unorderable_set_elements_are_a_compile_error_not_a_wrong_type() {
    let schema = "CREATE TABLE t (k int PRIMARY KEY, s set<double>);";
    let err = one(
        schema,
        "-- name: A :one\nSELECT s FROM t WHERE k = ?;",
        &Options::default(),
    )
    .unwrap_err();
    assert!(err[0].contains("no total order"), "{err:?}");
}

#[test]
fn colliding_rust_names_are_rejected() {
    let schema = "CREATE TABLE t (k int PRIMARY KEY, v text);";
    let q = "-- name: GetV :one\nSELECT v FROM t WHERE k = ?;\n-- name: get_v :one\nSELECT v FROM t WHERE k = ?;";
    let err = one(schema, q, &Options::default()).unwrap_err();
    assert!(err[0].contains("both map to the Rust name"), "{err:?}");
}

#[test]
fn nullable_binds_are_documented() {
    let schema = "CREATE TABLE t (k int PRIMARY KEY, v text);";
    let out = one(
        schema,
        "-- name: B :exec\nINSERT INTO t (k, v) VALUES (?, ?);",
        &Options::default(),
    )
    .unwrap();
    assert!(out.contains("`None` binds NULL"));
}

#[test]
fn type_matrix_matches_committed_output() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/compile-types");
    let out = generate(
        &inputs(&dir.join("schema")),
        &inputs(&dir.join("queries")),
        &Options::default(),
    );
    let expected = std::fs::read_to_string(dir.join("src/queries.rs")).unwrap();
    assert_eq!(
        out, expected,
        "run `cqlc generate -c tests/compile-types/cqlc.toml`"
    );
}
