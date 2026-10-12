# Supported CQL

A feature is supported only if the compiler analyzes it, the generator emits correct code,
and tests cover it. Everything else fails with `CQL002` ("not supported by cqlc yet") instead of
being guessed.

## Schema files

| Construct | Notes |
| --- | --- |
| `CREATE TABLE [IF NOT EXISTS] [ks.]t (...)` | inline `PRIMARY KEY`, `PRIMARY KEY ((a, b), c, d)`, `STATIC` |
| `WITH CLUSTERING ORDER BY (...)` | recorded in the schema; used to check `ORDER BY` |
| other `WITH` options | skipped; they do not influence types |
| `CREATE TYPE [IF NOT EXISTS] [ks.]t (...)` | becomes a Rust struct when a query uses it |
| `CREATE KEYSPACE ...` | accepted, options ignored |
| anything else (`USE`, `CREATE INDEX`, `ALTER`, ...) | `CQL002` |

Types: `ascii bigint blob boolean counter date decimal double duration float inet int smallint
text|varchar time timestamp timeuuid tinyint uuid varint`, `list<T>`, `set<T>`, `map<K,V>`,
`tuple<...>`, `frozen<T>` and user-defined types. Using a reserved keyword such as `token` as an
unquoted name is an error (`CQL115`), as in Cassandra.

## Query files

Every statement needs a `-- name: Name :kind` comment directly before it. Optional
`-- allow: CQL201, CQL202` comments silence those warnings for the next statement.

| Kind | Statement | Returns |
| --- | --- | --- |
| `:one` | `SELECT` | `Option<Row>` |
| `:many` | `SELECT` | `Vec<Row>`, plus a `stream_` variant |
| `:exec` | `INSERT`, `UPDATE`, `DELETE` | `()` |
| `:lwt` | any write with `IF` | an outcome with `applied` |

### SELECT

`SELECT [JSON | DISTINCT] <selection> FROM t [WHERE ...] [GROUP BY ...] [ORDER BY ...]
[PER PARTITION LIMIT n] [LIMIT n] [ALLOW FILTERING]`

* Selection: `*`, columns, `AS` aliases, UDT field access (`addr.city`), element access
  (`m['k']`, `l[0]`), casts and the functions below. **Anything that is not a plain column needs an
  alias** (`CQL114`), because the alias names the Rust field.
* `LIMIT ?` and `PER PARTITION LIMIT ?` take a bind marker.

### Relations

`=`, `<`, `<=`, `>`, `>=`, `IN ?`, `IN (a, b)`, `CONTAINS`, `CONTAINS KEY`, element
restrictions (`m[?] = ?`), `token(a, b) > token(?, ?)` and tuple relations
(`(c1, c2) > (?, ?)`, `(c1, c2) IN ((?, ?), (?, ?))`, `(c1, c2) > ?`).

Rejected: `LIKE`, `!=` in `WHERE` (valid only in `IF`), restrictions on UDT fields.

### Functions

| Function | Result |
| --- | --- |
| `now()`, `currentTimeUUID()` | `timeuuid` |
| `uuid()` | `uuid` |
| `currentTimestamp()`, `currentDate()`, `currentTime()` | `timestamp`, `date`, `time` |
| `toTimestamp(x)`, `toDate(x)`, `toUnixTimestamp(x)` | `timestamp`, `date`, `bigint` |
| `minTimeuuid(ts)`, `maxTimeuuid(ts)` | `timeuuid` |
| `token(pk...)` | `bigint` |
| `writetime(col)`, `ttl(col)` | `bigint`, `int` (nullable) |
| `count(*)`, `count(col)` | `bigint` |
| `sum(col)`, `avg(col)`, `min(col)`, `max(col)` | the column's type |
| `cast(x AS type)` | the target type |

Unknown functions are `CQL113`. User-defined functions and aggregates are not supported.
Arithmetic (`a + b`) in selections is not supported.

### INSERT

`INSERT INTO t (cols) VALUES (...)` and `INSERT INTO t JSON ?`, with `IF NOT EXISTS` and
`USING TTL ? AND TIMESTAMP ?`. Values may be `?`, literals, collection/tuple/UDT literals and
functions such as `now()`.

### UPDATE

`UPDATE t [USING ...] SET ... WHERE ... [IF ...]`. Assignments:

| Form | Meaning |
| --- | --- |
| `col = value` | replace |
| `col = col + ?`, `col += ?` | add to a list/set/map, increment a counter |
| `col = col - ?`, `col -= ?` | remove from a list/set, remove map keys, decrement a counter |
| `col = ? + col` | prepend to a list |
| `col[?] = value` | set a map entry or list element (non-frozen) |
| `udt.field = value` | set a field of a non-frozen UDT |

### DELETE

`DELETE [col, col[?], udt.field] FROM t [USING TIMESTAMP ?] WHERE ... [IF ...]`.

### Lightweight transactions

`IF NOT EXISTS` (INSERT), `IF EXISTS` and `IF col <op> value [AND ...]` (UPDATE, DELETE) with
`=`, `!=`, `<`, `<=`, `>`, `>=` and `IN`. Not supported: conditions on collection elements or UDT
fields, and `IF NOT EXISTS` on UPDATE/DELETE. Conditions on primary key columns are rejected
(Cassandra rejects them too).

### Limits

* At most 16 bind markers per query (the driver serializes tuples up to 16).
* Named markers (`:name`) are not supported; use `?`.
* Batches, `TRUNCATE`, `USE` and DDL in query files are not supported.

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
| user-defined type | generated struct, every field `Option<T>` |

Set elements and map keys need a total order in Rust. `set<double>` or a set of a UDT with a
`double` field is reported as `CQL110` instead of silently using another type.
