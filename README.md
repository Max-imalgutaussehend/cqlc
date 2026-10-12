# cqlc

[![CI](https://github.com/Max-imalgutaussehend/cqlc/actions/workflows/ci.yml/badge.svg)](https://github.com/Max-imalgutaussehend/cqlc/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/cqlc.svg)](https://crates.io/crates/cqlc)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

A static CQL analyzer and Rust code generator for Cassandra-compatible databases, in the spirit of
[sqlc](https://sqlc.dev).

You write your schema and named queries as plain `.cql` files. `cqlc` parses and type-checks them
against the schema, reports mistakes before runtime, and generates typed async Rust functions on
top of the [ScyllaDB Rust driver](https://crates.io/crates/scylla). It works with Apache Cassandra
and ScyllaDB, and needs no database connection to generate code.

```sql
-- schema/schema.cql
CREATE TABLE users_by_id (
    user_id uuid PRIMARY KEY,
    username text,
    email text,
    created_at timestamp
);

-- queries/users.cql
-- name: GetUserById :one
SELECT user_id, username, email, created_at
FROM users_by_id
WHERE user_id = ?;
```

```rust
let user: Option<GetUserByIdRow> = queries.get_user_by_id(user_id).await?;
// user.username: Option<String>   (regular columns may be absent)
// user.user_id:  Uuid             (primary key columns never are)
```

`cqlc` is **not** a driver, an ORM, a query builder or a migration tool.

## Why

* **Wrong column, wrong type or wrong key: caught at build time.** Unknown tables and columns,
  literals that do not fit, writes that miss the primary key, ranges on partition keys, UDT fields
  that do not exist, `token` used as a column name: all are diagnostics with a code, a location and
  a hint.
* **Cassandra-aware warnings.** Queries that do not restrict the partition key, use
  `ALLOW FILTERING`, skip a clustering column or order against the clustering order are flagged.
* **Typed everything.** Parameters and results get exact types, with null and absent values kept
  separate. UDTs become structs, lightweight transactions return an `applied` outcome, large
  results stream.
* **Deterministic and offline.** Same input, same output. CI can verify it with
  `cqlc generate --check`.

## Install

```bash
cargo install cqlc
```

Requires Rust 1.88 or later.

## Quick start

```bash
cqlc init        # cqlc.toml, schema/schema.cql, queries/users.cql
cqlc generate    # writes src/queries.rs
```

Add the dependencies of the generated code to your `Cargo.toml`:

```toml
scylla = "1.9"
uuid = "1"
futures = "0.3"                                  # for the stream_ functions
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

and declare the module:

```rust
#[rustfmt::skip]
mod queries;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let session = scylla::client::session_builder::SessionBuilder::new()
        .known_node("127.0.0.1:9042")
        .build()
        .await?;
    session.use_keyspace("my_keyspace", false).await?;

    // Prepares every statement once.
    let queries = queries::Queries::new(std::sync::Arc::new(session)).await?;
    let user = queries.get_user(uuid::Uuid::new_v4()).await?;
    println!("{user:?}");
    Ok(())
}
```

A larger, runnable project that exercises every kind of statement is in
[examples/shop](examples/shop).

## Commands

```bash
cqlc init                     # create a starter project
cqlc check                    # analyze only
cqlc generate                 # analyze and write the Rust module
cqlc generate --check         # CI: fail if the generated file is out of date
cqlc check --deny-warnings    # CI: fail on warnings, too
cqlc check --message-format json    # one JSON object per diagnostic, for editors and CI
```

Exit codes: `0` ok, `1` diagnostics errors or stale output, `2` configuration or I/O problem.

`cqlc.toml`:

```toml
version = 1
schema = "schema"        # directory or file(s) with CREATE statements
queries = "queries"      # directory or file(s) with annotated queries

[rust]
out = "src/queries.rs"
time_library = "native"  # or "chrono"
streams = true           # generate stream_ functions (needs the futures crate)
```

## Queries

Every query is a statement with a name and a result kind:

| Annotation | Statement | Generated result |
| --- | --- | --- |
| `-- name: X :one` | `SELECT` | `Option<XRow>` |
| `-- name: X :many` | `SELECT` | `Vec<XRow>`, and `stream_x` for large results |
| `-- name: X :exec` | `INSERT`, `UPDATE`, `DELETE` | `()` |
| `-- name: X :lwt` | any write with `IF` | `XOutcome { applied, .. }` |

```sql
-- allow: CQL201
-- name: CountUsers :one
SELECT count(*) AS total FROM users_by_id;

-- name: AddTag :exec
UPDATE users SET tags = tags + ? WHERE user_id = ?;

-- name: ClaimUsername :lwt
INSERT INTO usernames (username, user_id) VALUES (?, ?) IF NOT EXISTS;
```

The supported language is documented precisely in [docs/SUPPORTED_CQL.md](docs/SUPPORTED_CQL.md):
functions and aggregates, casts, collection and UDT literals, tuple and token relations,
`CONTAINS`, collection and counter updates, element and field assignment, `JSON`, `DISTINCT`,
`GROUP BY`, lightweight transactions and more. Whatever is not listed is rejected with an
explicit diagnostic.

## Diagnostics

```
warning[CQL201]: query does not restrict partition key column `tenant_id` and may scan many partitions
 --> queries/events.cql:8:1
  |
8 | SELECT payload FROM events WHERE event_time >= ?
  | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  |
  = help: consider modeling a table around this access pattern
```

All codes are listed in [docs/DIAGNOSTICS.md](docs/DIAGNOSTICS.md). Warnings are advice, not
proof: what Cassandra accepts also depends on indexes and server settings. Silence an intended
one with `-- allow: CQL201`.

## Documentation

* [Supported CQL](docs/SUPPORTED_CQL.md): language subset and type mapping
* [The generated code](docs/GENERATED_CODE.md): API, errors, nulls, streams, LWT
* [Diagnostics](docs/DIAGNOSTICS.md): every error and warning code
* [Compatibility](docs/COMPATIBILITY.md): tested servers and driver versions
* [Stability](docs/STABILITY.md): semver policy and migration rules
* [Architecture](docs/ARCHITECTURE.md) and [Testing](docs/TESTING.md)
* [Changelog](CHANGELOG.md), [Contributing](CONTRIBUTING.md), [Security](SECURITY.md)

## Layout

| Path | Purpose |
| --- | --- |
| `crates/cqlc-core` | lexer, parser, schema resolver, type checker, typed IR (pure) |
| `crates/cqlc-codegen` | Rust generator working on the IR only |
| `crates/cqlc-cli` | the `cqlc` binary |
| `examples/basic`, `examples/shop` | runnable projects with committed generated code |
| `tests/compile-types` | generated code for every supported type must compile |
| `fuzz` | `cargo fuzz` targets |

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at
your option. Unless you explicitly state otherwise, any contribution intentionally submitted for
inclusion in the work by you shall be dual licensed as above, without any additional terms or
conditions.
