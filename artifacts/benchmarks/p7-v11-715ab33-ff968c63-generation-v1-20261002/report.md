# V11 production generation contract — p7-v11-715ab33-ff968c63-generation-v1-20261002

Exact test source `715ab33e83ecb6c65228c18ed55e0fa6604ca0a6`; production remains c8c20b5, all379 reviewed production/Cargo fingerprints unchanged. Five warm-build executions:10 passed,0 failed,0 ignored. Actual built product binary and test binary hashes are in manifest. No live-model, cold-build, quality holdout or full gate claim.

L3 search and context each warm a local envelope containing731, hold cold query HTTP, perform a real parsed rebuild to947 and status through the product's public stdio with a one-connection read pool, and only then release a successful HTTP response. The pending RPC returns exact -32603 retryable=true generation-conflict after1 attempt and no envelope. Stable same-query semantic and local calls return947, never731; the semantic vector remains valid across document generations without another POST. These are client requests, not internal retry attempts.

L2 calls the production generation fence directly while real rebuilds change every attempt, with successful and failed work: exact3-attempt exhaustion; then a one-change window accepts only the second attempt's generation. Cancellation, deadline and stable corruption each execute once. This is an explicit production-fence entry test, not a claim of forcing three whole public requests or three provider attempts.

Compiler/test raw logs stay local by filename/hash. Two early preflight assertions incorrectly expected the word generation rather than the documented index-changed message, and source text in spans rather than machine_pack hits. They were corrected to exact public contracts without production edits; their log hashes are retained in integration receipt. Full V11 cache axes and formal row consolidation, V05 all lanes/DSL/BM25 at required levels remain pending. V16 memory/review belong to independent owners. D1/D2/live V19 restrictions remain.

Replay: checkout exact source, build this target with semantic-http --locked --offline --no-run --message-format=json; run replay.py with P7_REPOSITORY_ROOT, P7_CARGO_ARTIFACTS and a fresh P7_REPLAY_OUTPUT. The command needs synthetic loopback only.
