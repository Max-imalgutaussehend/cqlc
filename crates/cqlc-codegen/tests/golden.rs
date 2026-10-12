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
        .map(|p| {
            Input::new(
                p.display().to_string(),
                std::fs::read_to_string(&p).unwrap(),
            )
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
    let opts = Options::new().time_library(TimeLibrary::Chrono);
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
        &[Input::new("s.cql", schema)],
        &[Input::new("q.cql", query)],
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
    let chrono = one(schema, q, &Options::new().time_library(TimeLibrary::Chrono)).unwrap();
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

#[test]
fn shop_example_matches_committed_output() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/shop");
    let opts = Options::new().time_library(TimeLibrary::Chrono);
    let out = generate(
        &inputs(&dir.join("schema")),
        &inputs(&dir.join("queries")),
        &opts,
    );
    let expected = std::fs::read_to_string(dir.join("src/queries.rs")).unwrap();
    assert_eq!(
        out, expected,
        "run `cqlc generate -c examples/shop/cqlc.toml`"
    );
}

const UDT_SCHEMA: &str = "
CREATE TYPE address (street text, zip int);
CREATE TYPE geo (lat double, lon double);
CREATE TABLE t (
    k int PRIMARY KEY, v text, home frozen<address>, spot frozen<geo>,
    homes set<frozen<address>>, spots set<frozen<geo>>
);";

#[test]
fn udts_become_structs_and_only_used_ones_are_emitted() {
    let out = one(
        UDT_SCHEMA,
        "-- name: A :one\nSELECT home FROM t WHERE k = ?;",
        &Options::default(),
    )
    .unwrap();
    assert!(out.contains("pub struct Address {"), "{out}");
    assert!(out.contains("pub street: ::std::option::Option<::std::string::String>,"));
    assert!(out.contains("::scylla::SerializeValue, ::scylla::DeserializeValue"));
    assert!(
        !out.contains("pub struct Geo"),
        "unused UDTs are not generated"
    );
    // Fully ordered fields: the struct can be a set element or map key.
    assert!(out.contains("PartialEq, Eq, PartialOrd, Ord"));
}

#[test]
fn udts_with_floats_are_unorderable() {
    let out = one(
        UDT_SCHEMA,
        "-- name: A :one\nSELECT spot FROM t WHERE k = ?;",
        &Options::default(),
    )
    .unwrap();
    assert!(out.contains("pub struct Geo"));
    assert!(!out.contains("Eq, PartialOrd, Ord"));
    let err = one(
        UDT_SCHEMA,
        "-- name: A :one\nSELECT spots FROM t WHERE k = ?;",
        &Options::default(),
    )
    .unwrap_err();
    assert!(err[0].contains("no total order"), "{err:?}");
    // A set of an orderable UDT is fine.
    one(
        UDT_SCHEMA,
        "-- name: A :one\nSELECT homes FROM t WHERE k = ?;",
        &Options::default(),
    )
    .unwrap();
}

#[test]
fn lwt_outcome_shape() {
    let schema = "CREATE TABLE t (k int PRIMARY KEY, v text, applied boolean);";
    let out = one(
        schema,
        "-- name: Cas :lwt\nUPDATE t SET v = ? WHERE k = ? IF applied = true;",
        &Options::default(),
    )
    .unwrap();
    assert!(out.contains("pub struct CasOutcome"));
    assert!(out.contains("pub applied: bool,"));
    // A column called `applied` must not clash with the outcome flag.
    assert!(out.contains("pub applied_:"), "{out}");
    assert!(out.contains("#[scylla(rename = \"[applied]\")]"));
    assert!(out.contains("if specs.len() <= 1"));
    // ScyllaDB reports the whole row for a failed write; Cassandra only the tested column.
    assert!(out.contains("struct CasFull") && out.contains("struct CasCurrent"));
    let out = one(
        schema,
        "-- name: Del :lwt\nDELETE FROM t WHERE k = ? IF EXISTS;",
        &Options::default(),
    )
    .unwrap();
    assert!(!out.contains("DelCurrent"), "IF EXISTS tests no columns");
    assert!(
        out.contains("struct DelFull"),
        "but ScyllaDB returns the whole row"
    );
}

#[test]
fn streams_can_be_switched_off() {
    let schema = "CREATE TABLE t (k int, c int, v text, PRIMARY KEY (k, c));";
    let q = "-- name: Many :many\nSELECT v FROM t WHERE k = ?;";
    let on = one(schema, q, &Options::default()).unwrap();
    assert!(on.contains("pub async fn stream_many("));
    assert!(on.contains("::futures::stream::BoxStream<'static"));
    let off = one(schema, q, &Options::new().streams(false)).unwrap();
    assert!(!off.contains("stream_many") && !off.contains("futures"));
}

#[test]
fn queries_struct_prepares_every_statement_once() {
    let schema = "CREATE TABLE t (k int PRIMARY KEY, v text);";
    let out = one(
        schema,
        "-- name: A :one\nSELECT v FROM t WHERE k = ?;\n-- name: B :exec\nDELETE FROM t WHERE k = ?;",
        &Options::default(),
    )
    .unwrap();
    assert!(out.contains("pub struct Queries {"));
    assert!(out.contains("pub async fn new(session: ::std::sync::Arc<"));
    assert!(out.contains("let a = session.prepare(A_CQL)"));
    assert!(out.contains("pub async fn b(&self, k: i32)"));
    assert!(out.contains("b_with(&self.session, &self.b, k)"));
}

#[test]
fn generated_names_must_not_clash() {
    let schema =
        "CREATE TYPE foo_row (a int); CREATE TABLE t (k int PRIMARY KEY, f frozen<foo_row>);";
    let err = one(
        schema,
        "-- name: Foo :one\nSELECT f FROM t WHERE k = ?;",
        &Options::default(),
    )
    .unwrap_err();
    assert!(err[0].contains("FooRow"), "{err:?}");
    let schema = "CREATE TABLE t (k int PRIMARY KEY, v text);";
    let err = one(
        schema,
        "-- name: Session :one\nSELECT v FROM t WHERE k = ?;",
        &Options::default(),
    )
    .unwrap_err();
    assert!(err[0].contains("reserved"), "{err:?}");
}

#[test]
fn output_header_has_a_format_version_instead_of_the_crate_version() {
    let out = one(
        "CREATE TABLE t (k int PRIMARY KEY);",
        "-- name: A :exec\nDELETE FROM t WHERE k = ?;",
        &Options::default(),
    )
    .unwrap();
    assert!(
        out.starts_with("// @generated by cqlc (output format 1)."),
        "{}",
        &out[..80]
    );
}

/// Query text, names and column names end up in `///` comments of the generated
/// file. A line break there would turn the rest of the text into code.
#[test]
fn text_from_the_cql_files_cannot_escape_doc_comments() {
    let schema = "CREATE TABLE t (k int PRIMARY KEY, v text, \"we\nird\" text);";
    let query = "-- name: Q :exec\nINSERT INTO t (k, v, \"we\nird\") VALUES (?, 'line1\nline2 fn evil() {}\r\npub fn also_evil() {}', ?);";
    let out = one(schema, query, &Options::default()).unwrap();
    // Every line inside the CQL doc block is a comment.
    let mut inside = false;
    for line in out.lines() {
        if line.trim() == "/// ```text" {
            inside = true;
            continue;
        }
        if line.trim() == "/// ```" {
            inside = false;
        }
        if inside {
            assert!(
                line.trim_start().starts_with("///"),
                "not a comment: {line:?}"
            );
        }
    }
    assert!(!out.lines().any(|l| l.trim_start().starts_with("line2")));
    assert!(!out.lines().any(|l| l.contains("fn evil")
        && !l.trim_start().starts_with("//")
        && !l.contains("pub const")));
    // The multi-line column name is documented on one line, too.
    for line in out.lines().filter(|l| l.contains("we\\nird")) {
        assert!(
            line.trim_start().starts_with("///")
                || line.contains("rename")
                || line.contains("pub const"),
            "{line:?}"
        );
    }
}

#[test]
fn bidirectional_and_control_characters_are_escaped() {
    let schema = "CREATE TABLE t (k int PRIMARY KEY, v text);";
    let query = "-- name: Q :exec\nINSERT INTO t (k, v) VALUES (?, 'a\u{202e}b\u{2066}c\u{0}d');";
    let out = one(schema, query, &Options::default()).unwrap();
    for bad in ['\u{202e}', '\u{2066}', '\u{0}'] {
        assert!(
            !out.contains(bad),
            "U+{:04X} leaked into the output",
            bad as u32
        );
    }
    assert!(out.contains("\\u{202e}"));
}
