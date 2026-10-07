# Independent controls frozen before production edits

Base: `37dd042eaa1209a86e0cafdcd92ae77e036e76f5`.
Self-authored 28-row matrix and Go/Python/Rust source; no public query/gold used.
Expected models describe boost eligibility, not guaranteed retrieval/rank. Actual
engine/MCP and unmodified normalizer/source verification will be retained for both arms.
No evaluator gold or scoring changes. Original public metrics remain FAIL.

Explicit model: DSL name wins; whole-query qualified member targets the final identifier;
whole-query `class/interface/type/function/method NAME` optionally `on CONTAINER`
selects the named kind; `methods on CONTAINER` identifies context without a named target.
`type` follows product type_alias taxonomy (Go structs are class). Unrecognized prose
uses legacy token matching and can still suffer the diagnosed broad-prose regression.
These are boost hints, not hard kind/container constraints or a receiver resolver.
Split fragments inherit the same boost eligibility as their accurate symbol identity.
