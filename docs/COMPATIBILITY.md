# Compatibility

## Tested matrix

Every pull request runs the end-to-end examples (`examples/basic`, `examples/shop`) against:

| Server | Versions |
| --- | --- |
| Apache Cassandra | 4.1, 5.0 (the 5.0 job also runs `examples/cassandra5`: vector search, SAI, masking) |
| ScyllaDB | current release (`scylladb/scylla:latest`) |

The `shop` example runs 36 queries covering every kind of statement and expression that cqlc
supports, including lightweight transactions, UDTs, counters, collections and streams.

| Component | Version |
| --- | --- |
| ScyllaDB Rust driver (`scylla`) | 1.9 (`>= 1.9, < 2`) |
| Rust (MSRV) | 1.88 |
| Rust (CI) | stable; the published crates are also checked with 1.88 |

Generated code uses only the driver's documented public API (`Session::prepare`,
`execute_unpaged`, `execute_iter`, the `DeserializeRow`, `SerializeValue` and `DeserializeValue`
derives and `QueryRowsResult`).

## Cassandra version coverage

| Version | CQL covered |
| --- | --- |
| 3.0, 3.11 | complete for the data-plane subset |
| 4.0, 4.1 | arithmetic operators, duration literals, `currentTimestamp()` & co., `ALTER`/`DROP`, views, UDFs |
| 5.0 (5.0.x) | `vector<T, N>`, SAI indexes and the SAI restriction rules, `ORDER BY ... ANN OF`, `similarity_*`, `maxwritetime`, math and collection functions, `MASKED WITH`, masking functions |
| 6.0 (alpha) | `CHECK` constraints parsed; transactions not yet |

The reference is the ANTLR grammar (`Parser.g`, `Lexer.g`) and the native function classes of apache/cassandra
branches `cassandra-4.0`, `cassandra-4.1`, `cassandra-5.0` and `trunk`. Note that **column `DEFAULT` values are not
CQL**: the `schema-v4.cql` and `schema-v5.cql` files of the KillrVideo sample use them, and Cassandra rejects them too.

## Server differences cqlc handles

* A failed conditional write returns only the tested columns on Cassandra, but the whole row on
  ScyllaDB. Both shapes are decoded.
* `token(...)` is typed `bigint`, which is correct for Murmur3Partitioner, the default of both
  systems.

## Known limits

* Semantics beyond the CQL subset in [SUPPORTED_CQL.md](SUPPORTED_CQL.md) are rejected, not
  guessed.
* cqlc does not talk to a cluster. It knows about indexes only from `CREATE INDEX` statements in your schema files; warnings
  `CQL201` to `CQL204` are advice: Cassandra may still accept such a query.
* Cassandra 6.0 (currently in alpha): `CHECK` constraints are parsed; `BEGIN TRANSACTION` (Accord) is not
  supported yet because its syntax may still change before general availability.
* The `scylla` driver's own minimum Rust version applies to code that uses the generated module;
  `cqlc` itself builds on 1.88.

## Changing the matrix

Adding a server version to the matrix is a minor release. Dropping one is announced in the
changelog one minor release ahead.
