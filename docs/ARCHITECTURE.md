# Architecture

```
schema/*.cql  queries/*.cql  cqlc.toml
        |            |
     lexer  ->  parser (AST, spans)
        |            |
   schema resolver   |
        +-----> type checker / semantic analysis
                     |
                 typed IR  (cqlc-core, pure, no I/O)
                     |
              Rust generator (cqlc-codegen)
                     |
        generated module  ->  ScyllaDB driver
```

- **`cqlc-core`** has no I/O and no database access. `compile()` takes file contents and
  returns schema, IR and diagnostics. Lexer, parser, resolver and checker can each be
  tested in isolation.
- **The IR** (`ir.rs`) holds fully resolved queries: bind parameters with types and
  nullability in marker order, result columns, the normalized CQL text and source
  spans. Generators never look at the AST.
- **Diagnostics** carry a stable code, severity, span and optional help text. Errors
  reject a single query, warnings are advisory.
- **`cqlc-codegen`** is deterministic: identical input yields identical output, files are
  processed in sorted order. Output is plain text emitted with a small indentation-aware
  writer; the generated file is marked `@generated` and meant to be `rustfmt::skip`ped.
- **The runtime** is intentionally absent: the generated module is self-contained apart
  from the driver. It defines `CqlcError`, which wraps driver errors with the query name and
  failing stage, and `Queries`, which prepares every statement once. Connections, retries,
  load balancing and paging stay with the driver.

## Design decisions

- Unknown syntax is an error, never a guess. Adding syntax means touching parser, checker,
  IR, generator and tests together.
- Bind values and result cells have independent nullability.
- `SELECT *` is expanded at generation time; use `cqlc generate --check` in CI to notice
  schema changes.
- Each free function prepares its statement on every call; `Queries` prepares once. cqlc does
  not keep a second statement cache next to the driver's.
- Bind values and result cells have independent nullability. Failed lightweight transactions are
  results, not errors, and both server result shapes are decoded.

## Library API

`cqlc_core::compile` takes the file contents and returns schema, IR and diagnostics;
`cqlc_codegen::generate_rust` turns the IR into Rust. Other generators can be built on the IR
alone. See [STABILITY.md](STABILITY.md) for what is covered by semver.
