# Releasing

1. Update `version` in the root `Cargo.toml` (`[workspace.package]` and `[workspace.dependencies]`)
   and move the entries of `CHANGELOG.md` under the new version and date.
2. `cargo test --workspace`, then the checks listed in [docs/TESTING.md](docs/TESTING.md),
   including the end-to-end examples against Cassandra 4.1, 5.0 and ScyllaDB.
3. Check the packages: `cargo package -p cqlc-core --list` and `cargo publish --dry-run -p cqlc-core`.
   (`cqlc-codegen` and `cqlc` can only be dry-run after the crates they depend on are on
   crates.io.)
4. Commit, then tag and push: `git tag v1.2.3 && git push origin v1.2.3`.
5. The `Release` workflow verifies tag, version and changelog, builds binaries for Linux, macOS
   and Windows, publishes `cqlc-core`, `cqlc-codegen` and `cqlc` to crates.io in that order, and
   creates the GitHub release from the changelog. Without the repository secret
   `CARGO_REGISTRY_TOKEN` it skips crates.io (publish locally with `cargo publish -p cqlc-core`,
   then `cqlc-codegen`, then `cqlc`, waiting for the index in between). The `release`
   environment can be used to require approval.

A published version cannot be deleted from crates.io, only yanked, so do not skip step 2.
