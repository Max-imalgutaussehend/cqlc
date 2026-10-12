# Contributing

Thanks for helping. Bug reports with a minimal schema and query are the most useful
contribution.

## Setup

```bash
git clone https://github.com/Max-imalgutaussehend/cqlc && cd cqlc
cargo test --workspace
```

Before opening a pull request:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The full list of checks, including end-to-end runs and fuzzing, is in
[docs/TESTING.md](docs/TESTING.md). End-to-end tests need Docker: `docker compose up -d --wait`,
then `cargo run -p cqlc-example-shop`.

## Adding CQL support

A feature is supported only when parser, checker, IR, generator and tests agree. In one pull
request:

1. extend the parser and the AST (`crates/cqlc-core/src/parser.rs`, `ast.rs`);
2. type it in the checker, with a diagnostic for every way it can be wrong;
3. extend the IR and the generator if the result changes;
4. add tests in the cheapest layer: parser tests for syntax, checker tests for types and
   diagnostics, golden tests for generator output;
5. add a query to `examples/shop` if the feature needs a real server to be trusted, and run it;
6. update [docs/SUPPORTED_CQL.md](docs/SUPPORTED_CQL.md) and [CHANGELOG.md](CHANGELOG.md).

Unsupported syntax must fail with `CQL002`, never be accepted loosely.

## Regenerating committed output

If generator output changes, regenerate and review the diff:

```bash
for p in examples/basic examples/shop tests/compile-types; do
  cargo run -p cqlc -- -c $p/cqlc.toml generate
done
```

## Conduct and licensing

Be kind; see [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md). Contributions are licensed under
MIT OR Apache-2.0, like the project.
