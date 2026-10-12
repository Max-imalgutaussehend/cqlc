#![no_main]
use cqlc_codegen::{Options, generate_rust};
use cqlc_core::{Input, compile};
use libfuzzer_sys::fuzz_target;

const SCHEMA: &str = "
CREATE TYPE address (street text, zip int);
CREATE TABLE t (
    id uuid, ck int, name text, tags set<text>, m map<text, int>, home frozen<address>, n counter,
    PRIMARY KEY ((id), ck)
);
CREATE TABLE c (id uuid PRIMARY KEY, n counter);
";

// Whatever the query text is, the whole pipeline must not panic, and
// diagnostics must render.
fuzz_target!(|query: &str| {
    let c = compile(
        &[Input::new("s.cql", SCHEMA)],
        &[Input::new("q.cql", format!("-- name: Q :one\n{query}"))],
    );
    for d in &c.diagnostics {
        let _ = d.render(&c.sources);
    }
    if !c.has_errors() {
        let _ = generate_rust(&c.ir, &Options::default());
    }
});
