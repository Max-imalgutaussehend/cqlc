# cqlc-core

The compiler front end of [cqlc](https://github.com/Max-imalgutaussehend/cqlc), a static CQL
analyzer and Rust code generator for Cassandra-compatible databases.

It contains the lexer, parser, schema resolver, type checker and the typed intermediate
representation (IR). It is pure: no I/O and no database access.

```rust
use cqlc_core::{compile, Input};

let c = compile(
    &[Input::new("schema.cql", "CREATE TABLE t (k uuid PRIMARY KEY, v text);")],
    &[Input::new("q.cql", "-- name: Get :one\nSELECT v FROM t WHERE k = ?;")],
);
assert!(c.diagnostics.is_empty());
assert_eq!(c.ir.queries[0].name, "Get");
```

Most users want the `cqlc` command line tool instead. See the
[repository](https://github.com/Max-imalgutaussehend/cqlc) for documentation and the stability
policy: `compile` and the `ir`, `schema`, `types`, `diagnostic` and `span` modules are covered by
semver; the compiler internals are not.

Licensed under the [Apache License, Version 2.0](LICENSE).
