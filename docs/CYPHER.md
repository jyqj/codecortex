# Cypher Subset

`graph_query` supports a deliberately small, read-only Cypher-like subset over
CodeCortex's SQLite graph schema.

## Supported clauses

- `MATCH` with node labels and relationship types.
- `OPTIONAL MATCH` for a single-hop optional relationship.
- `WHERE` with `=`, `<>`, comparison operators, `AND`, `OR`, `CONTAINS`,
  `STARTS WITH`, `ENDS WITH`, and restricted `=~` regex.
- Variable-length relationships: `*`, `*N`, `*1..N`, `*..N`.
- `RETURN`, aliases via `AS`, `DISTINCT`.
- Aggregates: `COUNT`, `SUM`, `AVG`, `MIN`, `MAX`, `COLLECT`, including
  `DISTINCT` variants where implemented.
- `ORDER BY` and `LIMIT`.
- `UNION` / `UNION ALL` when all branches return the same column count.

## Supported labels and relationships

Use `status(aspect="schema")` for the authoritative runtime list. Common labels
include `File`, `Function`, `Class`, `Method`, `Route`, and `Package`.
Common relationships include `CALLS`, `DEFINES`, `DEFINES_METHOD`,
`CONTAINS_FILE`, `CONTAINS_MODULE`, and HTTP route/call edges.

## Intentional limits

- Read-only only: no `CREATE`, `MERGE`, `DELETE`, `SET`, `WITH`, or `UNWIND`.
- `LIMIT` defaults to 50 when omitted.
- Complex regex is rejected. The `=~` operator only supports patterns that can
  be safely approximated by SQL `LIKE` (`.*`, `.+`, and `.` wildcards).
- `OPTIONAL MATCH` applies to the first single-hop pattern only.
- Mixed-edge multi-hop chains are not supported.
