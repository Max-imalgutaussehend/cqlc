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
| `CREATE [CUSTOM] INDEX [name] ON t (col \| KEYS(col) \| VALUES(col) \| ENTRIES(col) \| FULL(col)) [USING 'class']` | legacy, SAI (`'sai'`) and SASI indexes are recorded; they decide which restrictions are legal and are required by `ANN OF` and `LIKE` |
| `CREATE MATERIALIZED VIEW v AS SELECT ... PRIMARY KEY (...)` | becomes a read-only table; writes to it are rejected |
| `CREATE FUNCTION` / `CREATE AGGREGATE` | callable from queries with the declared types (the body is not analysed) |
| `ALTER TABLE ... ADD / DROP / RENAME / WITH`, `ALTER TYPE ... ADD / RENAME` | applied to the schema in file order |
| `DROP TABLE / TYPE / INDEX / MATERIALIZED VIEW / KEYSPACE / FUNCTION / AGGREGATE` | applied in file order |
| `USE`, `TRUNCATE`, `GRANT`, `REVOKE`, `LIST`, `CREATE/ALTER/DROP ROLE/USER/TRIGGER`, `ALTER KEYSPACE` | accepted and ignored: they do not influence types |
| `MASKED WITH ...` on a column (Cassandra 5.0), `CHECK ...` constraints (Cassandra 6.0) | parsed; no effect on the generated types |

Types: `ascii bigint blob boolean counter date decimal double duration float inet int smallint
text|varchar time timestamp timeuuid tinyint uuid varint`, `list<T>`, `set<T>`, `map<K,V>`,
`tuple<...>`, `frozen<T>`, `vector<T, N>` (Cassandra 5.0, generated as `Vec<T>`) and user-defined types. Using a reserved keyword such as `token` as an
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

`LIKE` (needs a text column; the index decides whether Cassandra accepts it). Rejected: `!=` in `WHERE` (valid only in `IF`), restrictions on UDT fields.

Restrictions on non-key columns are legal when an index serves the operator: legacy indexes serve `=`, `CONTAINS` and `CONTAINS KEY`; SAI adds `<`, `<=`, `>`, `>=`; SASI adds ranges and `LIKE`. Otherwise `CQL203` applies.

### Vector search (Cassandra 5.0)

`ORDER BY col ANN OF <vector> LIMIT n` needs a `vector<T, N>` column with a SAI index, a `LIMIT` of at most 1000 and no other `ORDER BY` column. The vector can be a bind marker (`Vec<f32>`) or a literal `[0.1, 0.2, ...]` of the right dimension.

### Arithmetic (Cassandra 4.0)

`+ - * / %` and unary `-` in the selection list, in relations and in values. The result type follows Cassandra's `OperationFcts`: numbers take the wider type (`int + bigint` is `bigint`, `float + bigint` is `double`, anything with `decimal` or `varint` is `decimal`/`varint`), `timestamp` and `date` plus or minus a `duration` (`1d`, `3h30m` literals or a marker), and `text + text`. `timestamp - timestamp` does not exist in Cassandra and is an error. In `UPDATE ... SET` only `col = col +/- v` and `col = v + col` are supported.

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
| `maxwritetime(col)` (5.0) | `bigint` (nullable; also on collections) |
| `abs`, `exp`, `log`, `log10`, `round` (5.0) | the argument's type |
| `map_keys(m)`, `map_values(m)` (5.0) | `set<K>`, `list<V>` |
| `collection_count(c)`, `collection_min/max/sum/avg(c)` (5.0) | `int`, the element type |
| `similarity_cosine / euclidean / dot_product(v1, v2)` (5.0) | `float` (two `vector<float, N>` of the same dimension) |
| `mask_null`, `mask_default`, `mask_replace`, `mask_inner`, `mask_outer`, `mask_hash` (5.0) | the argument's type, `text`, `blob` for `mask_hash` |
| `toJson(x)` | `text` |
| `<type>AsBlob(x)`, `blobAs<type>(b)` | `blob`, the type |
| `dateOf(timeuuid)`, `unixTimestampOf(timeuuid)` | `timestamp`, `bigint` |
| functions and aggregates from `CREATE FUNCTION` / `CREATE AGGREGATE` | their declared result type |
| `count(*)`, `count(col)` | `bigint` |
| `sum(col)`, `avg(col)`, `min(col)`, `max(col)` | the column's type |
| `cast(x AS type)` | the target type |

Unknown functions are `CQL113`.

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
* Batches, `BEGIN TRANSACTION` (Cassandra 6.0 Accord), `TRUNCATE`, `USE` and DDL in query files are not supported.

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
| `vector<T, N>` | `Vec<T>` |
| `decimal`, `varint`, `duration`, `counter` | `CqlDecimal`, `CqlVarint`, `CqlDuration`, `Counter` (`scylla::value`) |
| `timestamp`, `date`, `time` | native: `CqlTimestamp`, `CqlDate`, `CqlTime`; chrono: `DateTime<Utc>`, `NaiveDate`, `NaiveTime` |
| `list<T>` | `Vec<T>` |
| `set<T>` | `BTreeSet<T>` |
| `map<K,V>` | `BTreeMap<K,V>` |
| `tuple<A,B>` | `(A, B)` |
| user-defined type | generated struct, every field `Option<T>` |

Set elements and map keys need a total order in Rust. `set<double>` or a set of a UDT with a
`double` field is reported as `CQL110` instead of silently using another type.
