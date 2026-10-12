# Compatibility

## Tested matrix

Every pull request runs the end-to-end examples (`examples/basic`, `examples/shop`) against:

| Server | Versions |
| --- | --- |
| Apache Cassandra | 4.1, 5.0 |
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

## Server differences cqlc handles

* A failed conditional write returns only the tested columns on Cassandra, but the whole row on
  ScyllaDB. Both shapes are decoded.
* `token(...)` is typed `bigint`, which is correct for Murmur3Partitioner, the default of both
  systems.

## Known limits

* Semantics beyond the CQL subset in [SUPPORTED_CQL.md](SUPPORTED_CQL.md) are rejected, not
  guessed.
* cqlc does not talk to a cluster. It cannot know about secondary indexes, so warnings
  `CQL201` to `CQL204` are advice: Cassandra may still accept such a query.
* Cassandra-version-specific syntax (for example SAI `:` operators or vector types) is not
  supported.
* The `scylla` driver's own minimum Rust version applies to code that uses the generated module;
  `cqlc` itself builds on 1.88.

## Changing the matrix

Adding a server version to the matrix is a minor release. Dropping one is announced in the
changelog one minor release ahead.
