# Independent real-product query verification — fixed c8c20b5

Canonical product baseline: **`c8c20b5b7d416372ee06ed5e248663c48912064e`** (updated PR41). New source commit: **`153ced0bf83a844f83211e41e2841d9a265786a7`**. This branch adds only a new test and this evidence directory. No production/config/dependency/CI/ledger or prior frozen assertion edit.

PR37's full `p7_v18_parameter_contract.rs` is byte-identical to frozen head `c8ac032b7e84989bceedd81bdd197150688d6953`. Exact-byte comparison is enforced by the replay script. The complete frozen suite runs without a test-name filter; no removed assertions, ignore or skips. Previously retained 105-pass and 60-pass evidence is not relabelled as this checkpoint's result.

## Canonical repeated result

20 command runs, **155 passed / 0 failed / 0 ignored**, five repetitions of each complete profile and the new HTTP supplement:

| Profile | Frozen suite per run | New HTTP suite per run | Repetitions | Total passes |
| --- | ---: | ---: | ---: | ---: |
| default | 8 | — | 5 | 40 |
| semantic | 9 | — | 5 | 45 |
| semantic-http | 12 | 2 | 5 | 70 |

Profiles run sequentially because Cargo shares the product binary path. `matrix/summary.json` and every run receipt bind baseline/source SHAs, command, feature, actual product/test binary and source-file SHA-256, raw JSON and log digests. All **1,573 canonical raw records** were checked against receipts. Strict clippy for both test targets with semantic-http and `-D warnings`, rustfmt and diff checks pass.

Complete frozen coverage includes default/typed config rejection, all required feature and authorization gates, old network_opt_in not expanding query authority, forbidden MCP bypass input, local/empty hard scope/reloaded revoked config zero-query POST, actual search/context encoding and nonempty semantic candidates/source spans, and warm cache zero additional POST.

The five frozen HTTP runs contain **20 verified semantic source responses** for distinct `v18_query_search_marker` and `v18_query_context_marker`, with no lexical overlap with the source fixture. Every response has a complete actual semantic lane, positive candidate count and a `needle.py` source span. The loopback endpoint records **10 query POSTs** for these positive sessions: one per distinct search/context marker; repeated requests use the real product cache and add none. No manual cache priming or injected semantic receipt.

## Real HTTP deadline and cancellation supplement

The new test starts the actual built `codecortex mcp` subprocess and uses stdio MCP. Only synthetic source/query, a 127.0.0.1 HTTP endpoint and a public dummy authorization marker are used. Documents publish through the normal worker before queries. The endpoint distinguishes query from document inputs and records actual requests and socket EOF.

For public search and context, stall either before response headers or after headers with an incomplete body. A configured **150ms semantic child** within an **8s total** produces a timeout/semantic_deadline lane with zero semantic candidates while auto still returns actual local `needle.py` source. Observed socket EOF is **147–149ms**, for both header and body stalls. One POST occurs, with no retry. A healthy retry posts again, produces complete semantic candidates/source, and a repeat makes no extra POST. This proves the unsuccessful encoding did not populate the query cache.

For public search and context, send an actual MCP request cancellation notification after query HTTP dispatch. A later distinct request using the same toy query retains auto local fallback; original and later stalled HTTP calls close within their already-clamped **2s child** budget (observed **1,997–1,999ms**). Healthy recovery posts again and produces complete semantic candidates/source. There are 30 supplement session records, 70 query POSTs and 40 observed EOFs across five runs, all replay-bound.

**Limit:** cancellation does not promise immediate physical socket abort. The current blocking HTTP contract fences publication and permits in-flight I/O until its bounded deadline. The cancelled request itself is not reported as a successful fallback response; the later public auto request is checked. These tests do not inspect private cancellation tokens or independently exercise the successful-late-response publication barrier; that service-level barrier remains covered by its designated review. No timeout or production behavior was altered.

## Earlier fixed tree and retained failure history

All three preliminary runs used **`bfaf45ead49469a5ba7bd5ea732a71fc0b6e247d`**, before canonical source was frozen:
- `preliminary.log`: 1 passed, 1 failed. The newly authored cancellation test incorrectly required socket EOF within 1s under a documented 2s blocking deadline. The failure is retained.
- `preliminary-corrected.log`: 2 passed. Corrected only that new test assumption to check the actual bounded I/O contract and later auto fallback.
- `preliminary-complete.log`: 2 passed after adding body-stage deadline and semantic source-span checks.

These scaffolding runs are not counted in the 155 canonical passes. `baseline-update.diff` records the two production files changed by the main owner from bfaf45e to c8c20b5; this task edited neither. Remote `codex/cloud-p7-model-transition@a9f5b7c13e2a4d79c25a017e3a8cbfb47e9037db` was inspected after canonical verification: its additional commit is documentation/evidence only, with no crates/Cargo/scripts diff. The executed conclusion is still explicitly c8c20b5; no test run is relabelled a9f5b7c.

D1/D2 remain unchanged. No real provider, credential, source egress or provider-quality certification. Full workspace regression, CI receipt and formal P7 gate decisions remain with the main owner; this is independent scoped public-query evidence.

## Final review baseline

Per the main owner, final PR is based on `a9f5b7c13e2a4d79c25a017e3a8cbfb47e9037db`. All executed runs remain labelled c8c20b5 / frozen source 153ced0. `production-byte-equivalence.json` proves exact equality of tracked production src and Cargo blobs between executed c8c20b5, source 153ced0, a9f5b7c and this final branch, with SHA-256 for every module. New and old test bytes also match the executed frozen sources. No metadata-only update caused a redundant rerun or relabelling.
