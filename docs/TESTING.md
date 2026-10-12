# Testing and quality assurance

A compiler can be tested stage by stage, and cqlc is.

| Layer | What it checks | Where |
| --- | --- | --- |
| Unit and table tests | lexer (strings, comments, escapes, uuids), parser (AST shapes, error recovery), checker (types, nullability, every diagnostic code), naming | `crates/*/tests`, `crates/*/src` |
| Property tests | the whole front end never panics on arbitrary text or CQL-like token soup | `crates/cqlc-core/tests/robustness.rs` |
| Fuzzing | `cargo fuzz` targets for the lexer, both parsers and the whole pipeline including code generation | `fuzz/` |
| Golden tests | generated code of the examples is compared byte for byte with the committed files, so every generator change shows up in a diff | `crates/cqlc-codegen/tests/golden.rs` |
| Compile tests | the generated code for every supported type, both time libraries and all statement kinds is compiled against the real driver | `tests/compile-types`, `examples/*` |
| End-to-end tests | `examples/basic` and `examples/shop` run 43 queries against real servers and assert on the results | CI matrix |
| CLI tests | exit codes, `--check`, `init`, JSON output, config validation | `crates/cqlc-cli/tests` |
| Doc tests | the examples in the API documentation | `cargo test --doc` |

## Running everything locally

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# generated files are current and warning-free
for p in examples/basic examples/shop tests/compile-types; do
  cargo run -q -p cqlc -- -c $p/cqlc.toml generate --check
done

# end-to-end
docker compose up -d --wait
cargo run -p cqlc-example-basic
cargo run -p cqlc-example-shop

# fuzzing (nightly)
cargo install cargo-fuzz
cd fuzz && cargo +nightly fuzz run compile -- -max_total_time=60
```

## Principles

* A bug found by fuzzing or by an end-to-end run gets a regression test in the cheapest layer
  that can reproduce it.
* Anything that is accepted must be analyzed and generated correctly; anything else must be
  rejected with a diagnostic. Tests for new syntax cover both directions.
* The generator is deterministic, and the golden files enforce it.
