# cqlc-codegen

The Rust code generator of [cqlc](https://github.com/Max-imalgutaussehend/cqlc). It consumes the
typed IR from [`cqlc-core`](https://crates.io/crates/cqlc-core) and emits a module of typed async
functions on top of the [ScyllaDB Rust driver](https://crates.io/crates/scylla).

```rust
use cqlc_codegen::{generate_rust, Options};
use cqlc_core::{compile, Input};

let c = compile(
    &[Input::new("s.cql", "CREATE TABLE t (k uuid PRIMARY KEY, v text);")],
    &[Input::new("q.cql", "-- name: Get :one\nSELECT v FROM t WHERE k = ?;")],
);
let code = generate_rust(&c.ir, &Options::new()).unwrap();
assert!(code.contains("pub async fn get("));
```

Output is deterministic, and its layout is versioned by `OUTPUT_FORMAT`.

Licensed under MIT or Apache-2.0, at your option.
