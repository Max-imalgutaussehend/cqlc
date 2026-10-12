# cqlc

A static CQL analyzer and Rust code generator for Cassandra-compatible databases, in the spirit
of [sqlc](https://sqlc.dev). Write your schema and named queries as `.cql` files; `cqlc` checks
them against the schema and generates typed async Rust on top of the
[ScyllaDB Rust driver](https://crates.io/crates/scylla).

```bash
cargo install cqlc
cqlc init        # starter cqlc.toml, schema and queries
cqlc generate    # writes src/queries.rs
```

Documentation: <https://github.com/Max-imalgutaussehend/cqlc#readme>

Licensed under MIT or Apache-2.0, at your option.
