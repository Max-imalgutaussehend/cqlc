# Changelog

All notable changes are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres to
[Semantic Versioning](https://semver.org); see [docs/STABILITY.md](docs/STABILITY.md).

## [1.0.1] - 2026-10-12

### Security

* Text from `.cql` files could break out of the `///` doc comments of the generated module: a
  multi-line string literal or a quoted identifier containing a line break put the rest of the
  text into the generated file as code. All such text is now escaped, and control and
  bidirectional-formatting characters are written as `\u{..}` escapes. If you generate code from
  `.cql` files you do not trust, upgrade.

### Fixed

* A deeply nested expression or type (for example 100,000 opening brackets) crashed `cqlc` with a
  stack overflow. Nesting is limited to 64 levels and reported as `CQL001`.
* A UTF-8 byte order mark at the start of a `.cql` file, as written by some Windows editors, is
  accepted instead of reported as an unexpected character.
* Diagnostics on very long lines show a window around the error instead of the whole line.
* `cqlc generate` writes the module through a temporary file and a rename, so an interrupted run
  cannot leave a half-written file behind.

### Changed

* The license is now Apache-2.0 only (before: MIT OR Apache-2.0). Versions published earlier
  keep the terms they were released under. A `NOTICE` file was added. The README states that
  generated code is not covered by the license.

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
* Licensed under MIT OR Apache-2.0 (changed to Apache-2.0 in 1.0.1).

### Fixed

* `Diagnostic::render` panicked when a span started inside trailing `\r` characters.

## [0.1.0]

Vertical slice: schema and query parsing, type inference, Rust generation for `SELECT`,
`INSERT`, `UPDATE` and `DELETE` with primitive types and collections, diagnostics and the
`generate --check` workflow.

[1.0.1]: https://github.com/Max-imalgutaussehend/cqlc/releases/tag/v1.0.1
[1.0.0]: https://github.com/Max-imalgutaussehend/cqlc/releases/tag/v1.0.0
