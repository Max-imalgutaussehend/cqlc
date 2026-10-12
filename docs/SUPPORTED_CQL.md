# Supported CQL (0.1)

A feature is supported only if the compiler analyzes it, the generator emits correct
code and tests cover it. Everything else fails with `CQL002` ("not supported yet").

## Schema files

| Construct | Notes |
| --- | --- |
| `CREATE TABLE [IF NOT EXISTS] [ks.]t (...)` | inline `PRIMARY KEY`, `PRIMARY KEY ((a, b), c, d)`, `STATIC` |
| `WITH CLUSTERING ORDER BY (...)` | recorded in the schema |
| other `WITH` options | skipped; they do not influence types |
| `CREATE TYPE [IF NOT EXISTS] [ks.]t (...)` | parsed and validated; **not usable in queries yet** (`CQL110`) |
| `CREATE KEYSPACE ...` | accepted, options ignored |
| everything else (`USE`, `CREATE INDEX`, `ALTER`, ...) | `CQL002` |

Types: all primitive types (`ascii bigint blob boolean counter date decimal double
duration float inet int smallint text|varchar time timestamp timeuuid tinyint uuid
varint`), `list<T>`, `set<T>`, `map<K,V>`, `tuple<...>`, `frozen<T>`.

## Query files

Every statement needs a `-- name: Name :one|:many|:exec` comment directly before it.

| Statement | Supported | Rejected (`CQL002`) |
| --- | --- | --- |
| `SELECT` | columns, `*`, `AS` aliases, `WHERE` with `AND`, `ORDER BY`, `LIMIT`, `ALLOW FILTERING` | functions/aggregates, `JSON`, `DISTINCT`, `GROUP BY`, `PER PARTITION LIMIT`, field/element selection |
| `INSERT` | `INTO t (cols) VALUES (...)`, `USING TTL/TIMESTAMP` | `IF NOT EXISTS`, `JSON` |
| `UPDATE` | `SET col = term`, `USING TTL/TIMESTAMP` | `+=`, `col = col + ?`, element/field assignment, `IF ...` |
| `DELETE` | whole row or listed columns, `USING TIMESTAMP` | element deletion, `IF ...` |

Relations: `=`, `<`, `<=`, `>`, `>=`, `IN ?`, `IN (a, b, ...)`. Rejected: `!=`,
`CONTAINS`, `LIKE`, tuple relations, `token(...)`.

Terms: positional `?` markers and plain literals (strings, numbers, booleans, uuids,
blobs, `null`). Not supported: named markers, function calls such as `now()`, collection
literals. A `?` in the selection list is an error (`CQL106`): its type cannot be inferred.

Lightweight transactions (`IF ...`) are rejected rather than modeled half-way: a plain
`Result<(), _>` cannot express the `[applied]` result.

Limits: at most 16 bind markers per query (driver tuple limit).

## Rust type mapping

| CQL | Rust |
| --- | --- |
| `ascii`, `text` | `String` |
| `tinyint`, `smallint`, `int`, `bigint` | `i8`, `i16`, `i32`, `i64` |
| `float`, `double` | `f32`, `f64` |
| `boolean` | `bool` |
| `blob` | `Vec<u8>` |
| `uuid` | `uuid::Uuid` |
| `timeuuid` | `scylla::value::CqlTimeuuid` |
| `inet` | `std::net::IpAddr` |
| `decimal`, `varint`, `duration`, `counter` | `CqlDecimal`, `CqlVarint`, `CqlDuration`, `Counter` (`scylla::value`) |
| `timestamp`, `date`, `time` | native: `CqlTimestamp`, `CqlDate`, `CqlTime`; chrono: `DateTime<Utc>`, `NaiveDate`, `NaiveTime` |
| `list<T>` | `Vec<T>` |
| `set<T>` | `BTreeSet<T>` |
| `map<K,V>` | `BTreeMap<K,V>` |
| `tuple<A,B>` | `(A, B)` |

Set elements and map keys must have a total order in Rust; `set<double>` and similar
are reported as `CQL110` instead of silently using another type.
