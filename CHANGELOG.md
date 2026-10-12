# Changelog

All notable changes are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres to
[Semantic Versioning](https://semver.org); see [docs/STABILITY.md](docs/STABILITY.md).

## [1.0.0] - 2026-10-12

First stable release. The CLI, `cqlc.toml`, the accepted language, diagnostic codes, the layout of
the generated code (output format 1) and the library API described in
[docs/STABILITY.md](docs/STABILITY.md) are now covered by semver.

### Added

* Expression language: functions (`now`, `uuid`, `toTimestamp`, `minTimeuuid`, `writetime`,
  `ttl`, `token`, ...), aggregates (`count`, `sum`, `avg`, `min`, `max`), `cast`, collection,
  tuple and UDT literals, UDT field and collection element access in selections.
* Relations: `token(...)`, tuple relations, `CONTAINS`, `CONTAINS KEY`, element restrictions.
* `SELECT JSON`, `DISTINCT`, `GROUP BY`, `PER PARTITION LIMIT`, `INSERT ... JSON`.
* Writes: collection and counter updates (`+`, `-`, prepend), element and UDT field assignment,
  element and field deletes.
* Lightweight transactions with the `:lwt` kind and an `applied` outcome, working with both the
  Cassandra and the ScyllaDB result shape.
* User-defined types as generated structs.
* `stream_*` functions for `:many` queries and a `Queries` struct that prepares every statement
  once.
* Diagnostics `CQL113` (unknown function), `CQL114` (missing alias), `CQL115` (reserved word),
  `CQL205` (`ORDER BY`), and `-- allow: CQL2xx` to silence intended warnings.
* `cqlc init`, `--message-format json`, `--deny-warnings`, `version` in `cqlc.toml`.
* Fuzz targets, property tests, an end-to-end suite against Cassandra 4.1, 5.0 and ScyllaDB, and
  a compile test for every supported type.
* Documentation: supported CQL, generated code, compatibility, stability, testing.

### Changed

* The generated header carries an output format version instead of the cqlc version, so patch
  releases do not change generated files.
* The CLI crate is published as `cqlc`; the library crates are `cqlc-core` and `cqlc-codegen`.
* Public enums and structs of the library crates are `#[non_exhaustive]`; the compiler internals
  are hidden from the documentation and outside the semver promise.
* Licensed under MIT OR Apache-2.0.

### Fixed

* `Diagnostic::render` panicked when a span started inside trailing `\r` characters.

## [0.1.0]

Vertical slice: schema and query parsing, type inference, Rust generation for `SELECT`,
`INSERT`, `UPDATE` and `DELETE` with primitive types and collections, diagnostics and the
`generate --check` workflow.

[1.0.0]: https://github.com/Max-imalgutaussehend/cqlc/releases/tag/v1.0.0
