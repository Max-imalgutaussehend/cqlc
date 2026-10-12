# cqlc

A static CQL analyzer and Rust code generator for Cassandra-compatible databases,
in the spirit of [sqlc](https://sqlc.dev).

You write your schema and named queries as plain `.cql` files. `cqlc` parses and
type-checks them against the schema, reports mistakes before runtime, and generates
typed async Rust functions on top of the
[ScyllaDB Rust driver](https://crates.io/crates/scylla).

```sql
-- schema/users.cql
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
let user: Option<GetUserByIdRow> = queries::get_user_by_id(&session, user_id).await?;
// user.username: Option<String>   (regular columns may be absent)
// user.user_id:  Uuid             (primary key columns never are)
```

`cqlc` is **not** a driver, an ORM, a query builder or a migration tool, and it needs
no database connection to generate code.

## Status

`0.1` is a vertical slice of a deliberately small CQL subset: everything it accepts is
analyzed and generated correctly, everything else is rejected with an explicit
diagnostic instead of being guessed. See [docs/SUPPORTED_CQL.md](docs/SUPPORTED_CQL.md).

## Usage

```bash
cargo install --path crates/cqlc-cli      # installs the `cqlc` binary

cqlc generate            # reads ./cqlc.toml, writes the Rust module
cqlc generate --check    # CI: fail if the generated file is out of date
cqlc check               # analyze only (add --deny-warnings to fail on warnings)
```

`cqlc.toml` (see [cqlc.toml.example](cqlc.toml.example)):

```toml
schema = "schema"        # directory or file(s) with CREATE statements
queries = "queries"      # directory or file(s) with annotated queries

[rust]
out = "src/queries.rs"
time_library = "native"  # or "chrono"
```

Declare the generated file as `#[rustfmt::skip] mod queries;` and depend on
`scylla` (and `uuid`, plus `chrono` if configured). Exit codes: `0` ok, `1`
diagnostics errors or stale output, `2` configuration / I/O problems.

### Query annotations

```sql
-- name: GetUsersByStatus :many
```

| Kind    | Statement             | Generated return type                  |
| ------- | --------------------- | -------------------------------------- |
| `:one`  | `SELECT`              | `Result<Option<Row>, CqlcError>`       |
| `:many` | `SELECT`              | `Result<Vec<Row>, CqlcError>` (unpaged) |
| `:exec` | `INSERT/UPDATE/DELETE` | `Result<(), CqlcError>`                |

`:one` assumes the query matches at most one row and returns the first row otherwise;
cqlc does not verify cardinality.

A single bind parameter becomes a plain argument, several become a
`<Name>Params` struct. Result columns become a `<Name>Row` struct.

## Null, unset and absent values

Two separate questions, deliberately modeled separately:

- **Can the database cell be absent?** Primary key columns are never null, all other
  columns are `Option<T>` in result structs.
- **Can the caller pass NULL?** Regular columns written by `INSERT`/`UPDATE` take
  `Option<T>`, where `None` binds **NULL** (which writes a tombstone). There is no
  "leave unset" yet; omit the column from the statement instead.

## Diagnostics

Every problem has a stable code, a source location and, where possible, a hint.

```
warning[CQL201]: query does not restrict partition key column `tenant_id` and may scan many partitions
 --> queries/events.cql:8:1
  |
8 | SELECT payload FROM events WHERE event_time >= ?
  | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  |
  = help: consider modeling a table around this access pattern
```

The full list is in [docs/DIAGNOSTICS.md](docs/DIAGNOSTICS.md). Warnings are advice,
not proof of a full scan: what Cassandra accepts also depends on indexes and server
configuration.

## Layout

| Path                     | Purpose                                                        |
| ------------------------ | -------------------------------------------------------------- |
| `crates/cqlc-core`       | lexer, parser, schema resolver, type checker, typed IR (pure)   |
| `crates/cqlc-codegen`    | Rust generator working on the IR only                          |
| `crates/cqlc-cli`        | `cqlc` binary and `cqlc.toml` handling                         |
| `examples/basic`         | runnable end-to-end example with committed generated code      |
| `tests/compile-types`    | generated code for every supported type must compile           |

Architecture notes: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Development

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

# end-to-end against a real node
docker compose up -d --wait
cargo run -p cqlc-example-basic
```

## License

MIT, see [LICENSE](LICENSE).
