# Security policy

## Reporting a vulnerability

Please report suspected vulnerabilities privately through GitHub's
["Report a vulnerability"](https://github.com/Max-imalgutaussehend/cqlc/security/advisories/new)
form on this repository, not in a public issue. Include the schema, queries and `cqlc` version
that reproduce the problem. You can expect an acknowledgement within a few days.

## Scope

cqlc reads local `.cql` and `.toml` files and writes one Rust file; it does not connect to a
database or the network. Relevant issues are, for example:

* a crash, hang or unbounded memory use on crafted input;
* generated code that executes a different statement than the one written in the query file
  (the CQL text is passed to the driver as a parameterized statement; values are always bound,
  never interpolated);
* path handling that writes outside the configured output file.

## Supported versions

Security fixes are released for the latest minor version of the current major release.
