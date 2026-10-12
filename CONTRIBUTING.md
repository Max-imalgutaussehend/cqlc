# Contributing

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Adding a CQL feature means changing, in one PR: parser, checker (and diagnostics), IR if
needed, generator, tests, and `docs/SUPPORTED_CQL.md`. A feature is not supported until
all of them agree.

If generator output changes, regenerate the committed files and review the diff:

```bash
cargo run -p cqlc -- -c examples/basic/cqlc.toml generate
cargo run -p cqlc -- -c tests/compile-types/cqlc.toml generate
```

CI also runs `examples/basic` against a real Cassandra node (`docker compose up -d --wait`
and `cargo run -p cqlc-example-basic` locally).
