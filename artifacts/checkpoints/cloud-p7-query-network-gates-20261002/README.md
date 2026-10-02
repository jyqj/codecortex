# Independent query-network authorization acceptance — wiring still pending

Product baseline: `codex/cloud-p7-query-optin-integration@00e8c6d667198b1c0a1eb2b4a8fd69ca3fd71c4b` (fetched and inspected). Config/status is present; the actual public producer is not attached. This is a new-feature integration gap, not a violation of the existing default cold-cache contract. No production file is modified here.

Owned source commits:
- `10303e76be11cc85c1e47e2a1a697d16f477fc4f`: append five tests and loopback observation helper to the existing V18 test file; preserve all frozen PR33 test bodies.
- `13d63d3828ae310297258fb9ea0e2854fdb143a9`: require a genuinely authorized live encoder before treating local/empty-scope zero traffic as success. This changes only the positive-path precondition.

## Verified denial paths

`negative-matrix/summary.json` binds the exact first source commit, baseline, commands, actual product and runner binary digests, raw JSON and log hashes. Profiles execute sequentially because Cargo shares the product binary path. Five repetitions per profile, 25 command runs, **45 passed / 0 failed / 0 ignored**, 455 raw records; all recorded query POST counts are zero. HTTP requests are synthetic and 127.0.0.1 only. Default D1/D2 remain unchanged.

Coverage: default false and omitted false; explicit booleans; rejection of five non-boolean typed config inputs; each missing feature/config gate; existing network_opt_in alone; actual public search/context cold-cache unavailable receipts where a semantic port exists; all fourteen MCP schemas without a bypass switch; forbidden MCP allow_query_network parameters rejected before encoding. HTTP document publication is observed separately from unique user query/task markers. Document counts in individual probe records are cumulative within a test; the per-run receipt sums these observations and must not be interpreted as a distinct document-call total.

`cargo clippy -p cc-server --test p7_v18_parameter_contract --features semantic-http --locked --offline --no-deps -- -D warnings` passed (`clippy.log`). Rustfmt and diff checks passed. Frozen PR33's 105-pass evidence is retained unchanged and is not re-counted.

## Executed counterexample and remaining dependency

Full new HTTP filter on source `13d63d3` was executed, not ignored: **3 passed / 2 failed / 0 ignored**, exit 101 (`unwired-counterexample.log` and raw receipts). Both failures observe actual `/retrieval/query_encoding.network_authorized=false` and reason `query_encoder_not_attached` after successful document publication. They require true before they can claim anything about the live query encoder. The evidence is retained as an expected incomplete-feature counterexample, not green acceptance.

Actual baseline code: `service_factory.rs` stores the optional query lifecycle; `capability_status.rs` reports its absence; `semantic_wiring.rs` clears it and does not install the new encoder. The designated main/service owners must compose the new service into the real query path (`query_handle.rs`/`engine_query.rs` as appropriate) and publish the agreed integration commit. Those files are outside this task's ownership. Service branch `codex/cloud-p7-query-encoding-service@eda4df9f84943c5ec86f7d03f508c3fffedcc012` contains a service module but is not that integrated product baseline.

After that dependency arrives, run the complete new `query_network_contract` filter: healthy all-gates search and context must each encode their distinct marker, return actual semantic candidates and source spans, then hit cache without another POST. Local, empty hard scope and public invalid arguments must send zero query POST with a live encoder; a new product session loading revoked config must also send zero. There is no manual cache insertion, injected receipt, widened timeout, removed assertion, ignore, or test skip. Hot reload and transport fault/cancellation matrices remain with their designated owners. The public file loader's fail-closed behavior is not claimed as a new fatal startup error; this task verifies typed config parsing only.

**Nonempty public semantic acceptance and formal P7-014 remain pending.** This draft is not merge-ready until the real producer path passes and is independently reviewed.
