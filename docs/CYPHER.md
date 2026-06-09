# Cypher Subset

`graph_query` supports a deliberately small, read-only Cypher-like subset over
CodeCortex's SQLite graph schema.

## Supported clauses

- `MATCH` with node labels and relationship types.
- `OPTIONAL MATCH` for a single-hop optional relationship, including the
  anchored two-clause form `MATCH (f:Label) OPTIONAL MATCH (f)-[:R]->(g)` where
  the source node is preserved even when no target matches (NULL target columns).
- `WHERE` with `=`, `<>`, comparison operators, `AND`, `OR`, `CONTAINS`,
  `STARTS WITH`, `ENDS WITH`, and `=~` regex (SQLite REGEXP via Rust `regex` crate).
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

## Regex (`=~`)

The `=~` operator compiles patterns with the Rust `regex` crate and executes
them via a custom SQLite `REGEXP` function. Any syntax accepted by
[`regex::Regex`](https://docs.rs/regex/latest/regex/) works: character classes,
alternation, anchors, quantifiers, etc.

Invalid patterns produce an explicit SQL error, not silent false results.

## Intentional limits

- Read-only only: no `CREATE`, `MERGE`, `DELETE`, `SET`, `WITH`, or `UNWIND`.
- `LIMIT` defaults to 50 when omitted. `graph_query` returns an envelope
  `{ results, row_count, truncated, truncated_reason, limit_applied }`: when the
  default limit may have clipped rows it sets `truncated: true` with
  `truncated_reason: "default_limit"` (or `"output_budget"` when a server-side
  item budget truncated them), so callers can tell a full result set from a
  capped one.
- `OPTIONAL MATCH` supports a single-hop optional relationship, either standalone
  or as the second clause anchored on a preceding single-node `MATCH` that shares
  the source variable. Chains of multiple optional clauses are not supported.
- Variable-length paths (`*1..N`) are clamped to a maximum of 32 hops and only
  support `CALLS`, `DEFINES`, `DEFINES_METHOD`, `CONTAINS_FILE`,
  `CONTAINS_MODULE` edge types. Multi-hop chains with different edge types are
  not supported.
- Variable-length traversal uses **reachability** semantics: it returns the set
  of nodes reachable within the hop range, deduplicated, and does not enumerate
  distinct paths. Path multiplicity is therefore not preserved — an aggregate
  such as `COUNT(*)` over a variable-length match counts reachable nodes, not
  paths.
- Use `status(aspect="schema")` for the authoritative list of node labels,
  relationship types, and their properties.
