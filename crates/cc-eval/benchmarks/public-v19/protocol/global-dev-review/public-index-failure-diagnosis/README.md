# Public index failure diagnosis at fixed 78b0

Both four-repository DEV index blockers reproduce on a single synthetic file using the actual frozen default `codecortex` binary. Nine retained index-only cases match their expected errors/success controls. This diagnoses product-generated invalid evidence; it does not repair production or complete the failed baseline.

Execution source: `78b0ce52cb2ff4c53c53893b9f7dd469269806b8`.
Product binary SHA256: `4e2f101137591c5bbc3bdc6d660cd7c2989e0b72fbacc0bb89d1a79b9ba8fe77`.
The compiler/build receipt and default feature evidence remain in `../development-baseline/build/`. Before and after diagnostics, all 375 materialized production/Cargo files matched that receipt. Source is unchanged. The standalone Rust probe links the exact cc-model/cc-parsers artifacts named by that compiler receipt, with artifact and probe hashes recorded in evidence/receipt.json; it does not rebuild or edit the production workspace.

## Requests: ellipsis becomes a name dependency with an empty key

Minimal independently authored Python input (no imports/configuration required):

```python
def f() -> tuple[int, ...]:
    return ()
```

Actual index returns JSON-RPC `-32602`, `case.py: invalid resolution manifest: invalid dependency key`. Replacing ellipsis with `str`, or using a scalar return annotation, indexes successfully. Indexing only the admitted original `src/requests/exceptions.py` reproduces its original error; the parser observes its ellipsis return annotation on line 55.

Responsibility chain, fixed source:

- `crates/cc-index/src/resolver/helpers.rs:227`, `type_atoms`: preserves the `...` punctuation token because it is neither a delimiter, a primitive, a single uppercase parameter nor an empty string.
- `crates/cc-index/src/resolver/type_edges.rs:48`, `SymbolCatalog::derive_uses_type_edges`: emits a USES_TYPE edge for each such atom.
- `crates/cc-index/src/resolver/evidence.rs:178`, `seal_resolution_manifest`, and `record_names`: convert the generated semantic target into name-bucket dependencies.
- `crates/cc-model/src/resolution.rs:366`, `resolution_name_keys`: the ellipsis yields a nonempty exact key plus an empty leaf after the last dot. The standalone probe directly exercises this actual function and demonstrates that feeding its result into the actual manifest fails validation.
- `ResolutionManifest::dependency` accepts that empty key; `ResolutionManifest::validate` rejects it at lines 233–239 as `CcError::InvalidParams`.

This is legal Python syntax and a producer/schema inconsistency, not bad benchmark input. The end-to-end repro plus direct model probe and static producer chain isolate the cause; no instrumentation was added to production. Suggested repair ownership: `cc-index/src/resolver/helpers.rs` (exclude punctuation/ellipsis from type atoms), and `cc-model/src/resolution.rs` (never emit empty leaf keys). Related tests should preserve valid qualified names and meaningful variadic tuple type edges. Filtering the empty leaf alone would leave a bogus punctuation type edge; weakening validation or deleting the annotation is not a repair. Any broader empty-key API policy requires explicit tests and ownership review.

## Gin: two legitimate nested calls share one call identity

Minimal independently authored Go input:

```go
package p
func f() { a.B().C() }
```

Actual index returns JSON-RPC `-32602`, `case.go: invalid resolution manifest: conflicting duplicate call site call:219091e8d474ecda`. The parser produces two distinct calls, two references, and one duplicate call-ID group. Three nested selectors also fail. Separate selector statements and nested argument calls index successfully. The fixture need not execute or typecheck: like normal indexing with unresolved external names, it is legal Go parse syntax. The original admitted `context.go` alone reproduces the baseline ID `call:385a3223e8cfb7ea`; parser output contains 256 calls and 13 duplicate-ID groups.

Responsibility chain, fixed source:

- `crates/cc-parsers/src/go.rs:612`, `walk_calls`, visits inner and outer call AST nodes legitimately.
- `GoParser::extract_single_call`, lines 700–702, anchors both IDs to the entire function expression's starting row/column. Nested selector expressions have the same start and different ends/callees.
- Line 747 calls `StableId::edge_id("call", file_path, line_no, start_col)`. That actual function in `cc-model/src/id.rs:45` hashes kind/path/start only; it cannot distinguish these two sites.
- `seal_resolution_manifest` records both distinct callee queries under that same call site. Normalization correctly retains different records; `ResolutionManifest::validate`, lines 263–273, correctly rejects the conflict as `CcError::InvalidParams`.

Suggested repair ownership: `crates/cc-parsers/src/go.rs::extract_single_call` and its local tests. Use a source anchor that distinguishes the actual callee tokens (or an explicitly versioned range-based identity) while retaining both nested calls, stable IDs and coherent spans/refs. Prefer the localized parser fix to changing the global shared edge ID function, which affects other languages and persisted identities. Do not deduplicate away one call, swallow the manifest error or reduce the indexed source domain. Downstream model validation should remain strict.

## Evidence, limits and reproduction

`diagnose.py` executes nine retained cases against the actual binary through MCP initialize + index only, in disposable isolated projects; it records full RPC response/stderr and component observations. It verifies the two original source hashes against the frozen admission-backed input map before reading them. No question/gold/heldout/historical body is read. Original source bodies are not duplicated here; the existing development-baseline retained license/NOTICE archive covers the identifier/span observations from those two admitted files. Synthetic fixtures are original test material.

There were nine preliminary exploratory index calls before this retained nine-case run (one cleanup timeout followed by two four-case probes); total diagnostic index calls are 18. Those calls are not retrieval ranking or the original baseline. The initial probe cleanup timeout is corrected by terminating the disposable child with kill/wait; no running child remains. Preserved preliminary four-case output is in evidence/exploratory-last-four.txt. No search calls, no result-driven ranking parameter changes, no provider calls and no gold/scorer changes occurred.

```sh
PATH=/workspace/.cargo/bin:$PATH RUSTUP_HOME=/workspace/.rustup CARGO_HOME=/workspace/.cargo \
python3 <this-directory>/diagnose.py --output /tmp/new-public-index-diagnosis
python3 <this-directory>/verify_evidence.py
```

Paths can be overridden with `--binary`, `--source`, `--deps`, `--inputs`. The fixed binary hash/source/admitted source hashes are enforced. A new compiler may produce different bytes; do not substitute an unreceipted binary or current production source. The supplied Rust component probe asserts the known bad behavior at the frozen version, rather than claiming repair regression success. The future fix owner should invert/add the corresponding correctness tests at a newly frozen fix SHA.

This PR only adds `public-index-failure-diagnosis/`. No production/Cargo/shared ledger changes, no full Rust suite/clippy/release run, no baseline reranking, merge or deployment. Old 78b0 baseline raw and receipts remain untouched. Integration should ledger both reproduced producer bugs and allocate the above non-resource-matrix files for repair; a new product SHA must precede the later complete baseline rerun.
