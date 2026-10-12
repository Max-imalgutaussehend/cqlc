# Stability and versioning

cqlc follows [semantic versioning](https://semver.org). Starting with 1.0, the following are the
public contract.

## What is covered

1. **The command line.** Subcommands, flags, exit codes (`0` ok, `1` diagnostics errors or stale
   output, `2` configuration or I/O problem) and the JSON message format (fields are only added).
2. **`cqlc.toml`.** Keys and their meaning. `version = 1` is the only config version.
3. **The accepted language.** Every query that compiles today keeps compiling with the same
   types, parameter names and result fields throughout 1.x.
4. **Diagnostic codes.** A code keeps its meaning. New checks get new codes. A new *warning* may
   appear in a minor release; a new *error* for previously accepted input only in a major one
   (unless it is a bug fix, see below).
5. **The generated code.** The layout is versioned by the output format (see
   [GENERATED_CODE.md](GENERATED_CODE.md)). Generated files do not change between releases of the
   same major version unless your queries or schema changed.
6. **The library API.** `cqlc_core::compile` with `Input` and `Compilation`, the modules `ir`,
   `schema`, `types`, `diagnostic` and `span`, and `cqlc_codegen::generate_rust` with `Options`.
   Open enums and structs are `#[non_exhaustive]`: matching them needs a wildcard arm, and they
   gain variants and fields in minor releases.

## What is not covered

* The compiler internals: `cqlc_core::{ast, lexer, parser, resolver, checker}`. They are public
  for tooling and tests, hidden in the documentation, and may change in any release.
* Exact wording of diagnostic messages and help texts (codes and locations are covered).
* The order of diagnostics between different files.

## Bug fixes that may reject queries

If cqlc accepted a query that Cassandra rejects, or typed it incorrectly, fixing that is a bug fix
and may turn the query into an error in a minor release. Such fixes are listed in the changelog
under "Fixed" with the affected code.

## Supported versions

| Dependency | Policy |
| --- | --- |
| Rust | The MSRV (1.88) rises only in a minor release and is announced in the changelog. |
| `scylla` driver | Generated code supports one driver major version. Moving to a new driver major is a new cqlc major. |
| Cassandra / ScyllaDB | See [COMPATIBILITY.md](COMPATIBILITY.md). |

## Migration rules

* **Patch and minor releases** need no action. Run `cqlc generate`; the output stays identical
  unless you changed queries. Add `cqlc generate --check` to CI to be told when it does not.
* **Major releases** list every breaking change and its migration in the changelog. When the
  output format changes, `cqlc generate` rewrites the module; fix compile errors in the code that
  calls it, which the changelog describes.
* **Deprecations** are announced in a minor release, warned about by the CLI where possible and
  removed no earlier than the next major release.
