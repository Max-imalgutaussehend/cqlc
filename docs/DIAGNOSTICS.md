# Diagnostic codes

Codes are stable. `CQL0xx` syntax, `CQL1xx` errors, `CQL2xx` warnings.

| Code | Severity | Meaning |
| --- | --- | --- |
| CQL001 | error | Syntax error (including lexer errors such as unterminated strings) |
| CQL002 | error | Valid CQL that cqlc does not support yet |
| CQL101 | error | Unknown or ambiguous table |
| CQL102 | error | Unknown column |
| CQL103 | error | Unknown type |
| CQL104 | error | Duplicate definition (table, type, column, query, result column) |
| CQL105 | error | Literal does not fit the column type, or NULL where it is not allowed |
| CQL106 | error | Bind marker whose type cannot be inferred (e.g. in the selection list) |
| CQL107 | error | Missing or invalid `-- name:` annotation, or kind does not fit the statement |
| CQL108 | error | Wrong number of values, or more than 16 bind markers |
| CQL109 | error | Invalid primary key definition |
| CQL110 | error | Type has no supported Rust mapping (UDTs, unorderable set/map keys) |
| CQL111 | error | Statement does not address the full primary key it needs |
| CQL112 | error | Restriction, assignment or expression Cassandra rejects (range on partition key, updating key columns, `!=` in `WHERE`, aggregates outside the selection, ...) |
| CQL113 | error | Unknown or unsupported function |
| CQL114 | error | Expression in the selection list needs an `AS` alias |
| CQL115 | error | Reserved CQL keyword used as an unquoted name |
| CQL201 | warning | Partition key not fully restricted; query may scan many partitions |
| CQL202 | warning | `ALLOW FILTERING` is used |
| CQL203 | warning | Restriction on a non-key column (needs an index or `ALLOW FILTERING`) |
| CQL204 | warning | Clustering columns are not restricted left to right |
| CQL205 | warning | `ORDER BY` does not follow the clustering order or its exact reverse |

Warnings never block generation (use `--deny-warnings` to change that). An intended warning can be
silenced for one query with a comment before it, e.g. `-- allow: CQL201, CQL202`; only the warning
codes CQL201 to CQL205 can be allowed. A query with
an error is not generated; other queries are still analyzed so one run reports as many
problems as possible.
