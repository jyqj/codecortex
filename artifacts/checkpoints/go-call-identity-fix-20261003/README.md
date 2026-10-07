# Go nested call identity repair

Base PR92 fixed `ace2bc7983be2955831c9384e44d1bdd0749c909`; diagnosis PR99 `b25723458a77edd2207ed1a52dceb0ea1009cb93` describes the same conflict at execution source `78b0ce52cb2ff4c53c53893b9f7dd469269806b8`. This repair changes only `go.rs::extract_single_call`, that file's tests, and this exclusive evidence directory. Global IDs/validation, shared versions, Cargo, ledgers, type_atoms, resolution_name_keys and corpus are untouched.

Nested selectors have overlapping function-expression starts: in `a.B().C()`, both previously used `a` for the ID anchor. The fix anchors selector calls and their refs to the field token (`B` or `C`) and records that token's exact line/byte-column end. Direct identifier calls retain their previous anchors/IDs. Receiver expressions, callee names, argument counts, dispatch/resolution metadata and extraction traversal remain unchanged. Both calls are retained. No Gin-specific logic, deduplication, ignored validation failure or reduced source domain is introduced.

Four new local tests cover repeated same-leaf selectors at three depths, nested arguments/direct controls, multiline selectors with Unicode receiver/callee byte spans, and generic receiver/method calls. All 15 Go module tests pass. Repeated parsing produces identical full parse outcomes; each call has a distinct edge/ref identity and its ref shares the callee-token span. New tests retain six extracted calls in the generic/method fixture, rather than treating generic syntax as an unsupported blanket case. Existing extraction limits on other complex/parenthesized function expressions remain unchanged.

## Real pipeline negative and positive evidence

A standalone driver links actual `cc-index`, `cc-db`, `cc-parsers`, `cc-model` and serde_json cargo artifacts and invokes `Indexer::build_index(full=true)` in owned temporary projects. This exercises the production writer's strict resolution-manifest validation; no alternate validator or test hook is used. The old build is exact base SHA in a detached worktree. Before the final old build, only the isolated target's cc-parsers/cc-index artifacts were cleaned to avoid shared-target freshness across checkout paths; logs prove both crates freshly compiled from the old worktree.

| Input | Old strict index | Fixed strict index | Calls/refs retained |
|---|---|---|---|
| `a.B().C()` | duplicate call site rejected | passed, zero duplicate IDs | 2 / 2 |
| `a.B().B().B()` | duplicate symbol_ref site rejected | passed, zero duplicate IDs | 3 / 3 |
| fixed Gin `context.go` | 13 duplicate call-ID groups; rejected | passed, zero duplicate IDs | 256 / 256 |

Fixed nested-argument, Unicode/multiline, and generic/method controls also index successfully (6, 2 and 6 calls respectively). All fixed call spans select the actual callee bytes. Parsed source hashes and counts are recorded in results. Fixed Gin source is `gin-gonic/gin@43fe48e8a0f44af783116cdb010725e6bb50255f`; context hash `b2336e5768f9175f2855c7df91bb48de0bbb533d74f6a6e7c3af5576491524ce`; its locked MIT LICENSE is retained. Only this public source file and LICENSE are archived. This validates context.go alone, not an entire Gin project or public corpus run. No queries/ranking, provider or private data are used.

Fixed pipeline evidence has six index calls; final old evidence has three. A preliminary old driver ran two calls before stopping on the same-leaf fixture because it expected only a call-site error; strict validation instead correctly rejected a symbol_ref conflict. The corrected old driver accepts exactly these two conflict classes, with the unchanged error printed. Those preliminary calls are not silently counted as retained runs. An earlier fixed driver compilation had the wrong dependency search directory and performed no index calls. The original fixed driver source is archived separately because this later correction affects only its old-error assertion. Neither issue was a production defect or required production changes.

## Integration requirement and limits

**Main integration must bump the appropriate Go parser/cache version and invalidate persisted parse-cache outcomes.** Every selector's ID/ref anchor changes from receiver-expression start to callee token, so cached old outcomes can still contain collisions. This author does not edit lib.rs/shared version constants or cache gates. Fresh temporary DBs used here do not certify migration of old cached records; release integration must handle that before adoption. Direct identifier IDs are preserved; source-position IDs still change when the callee token moves.

Go module tests (15/0), strict `cc-parsers --lib` clippy, scoped rustfmt and evidence/hash checks pass. This does not close full Gin indexing, all-language/release quality gates, cache migration, V20 or any corpus status. No merge, deployment or shared task changes.

## Reproduction

```sh
CARGO_TARGET_DIR=/tmp/go-call-fix-target CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=1 cargo test --locked --offline -p cc-parsers --lib go::tests -- --nocapture
CARGO_TARGET_DIR=/tmp/go-call-fix-target CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=1 cargo build --locked --offline -p cc-index --message-format=json > /tmp/go-call-fix-build.jsonl
python3 artifacts/checkpoints/go-call-identity-fix-20261003/run_probes.py --build-json /tmp/go-call-fix-build.jsonl --output /tmp/go-call-fix-probes-new --mode fixed
python3 artifacts/checkpoints/go-call-identity-fix-20261003/verify.py
```

For old reproduction use a separate target or clean the owned parser/index artifacts before compiling the exact base. `run_probes.py` refuses overwriting evidence and only creates/deletes its own temporary projects. Binaries remain local; receipts preserve exact artifact/binary/source hashes and commands.
