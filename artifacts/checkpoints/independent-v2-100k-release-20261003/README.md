# Independent status v2 fixed 100k release observation

Product outcome: **failed**. One formal 100k sample; full V20 and semantic quality are not claimed. Independent correctness review is separate.

- Production: `11af963c33cfa68cc9497e355464c1d6d058adac`; final source/build: `29b03a0fec6bfde92aac9c41dce996ebb2d08cc7`. No production code difference between these commits.
- Fresh target release build: 236.413s, exit 0, opt-level 3, no debug assertions; semantic + semantic-http.
- Binary SHA256: `c7e6bdf2edf2925759fac2d8319b8e88eda500c3b63caa90f4fb054596225a60`.
- Cold index: 29.935471s; 100000 scanned/parsed/added, no skipped/parse errors.
- Status observations: 1347; status errors: 0. Status latency p50/p95/max (ms): {'count': 1347, 'min_ms': 7.33087, 'p50_ms': 20.524604, 'p95_ms': 33.1998, 'max_ms': 169.447645}.
- Root sampled RSS maximum: 1981968384 bytes (1890.15 MiB). Children files unavailable: full tree RSS **unknown**, never zero-filled.
- Normal process exits: [{'exit_code': 0, 'forced': False, 'wall_seconds': 6.6346351130000585}].

No accepted ready observation within 300s. Failure: [{'phase': 'backfill-drain', 'type': 'TimeoutError', 'error': 'backfill did not become ready in 300s'}].
Boundary read-only snapshot began at 300.000820922s; counts: {'files': 100000, 'symbols': 100000, 'chunks': 100000, 'document_manifest': 100000, 'semantic_manifest': 29082}. This is an actual timestamped near-boundary observation, not asserted to be an exact 300.000000s count.
Cleanup-tail inspection completed at 310.3038797610001s: counts {'files': 100000, 'symbols': 100000, 'chunks': 100000, 'document_manifest': 100000, 'semantic_manifest': 29696, 'edge_tables': {'call_edges': 0, 'test_edges': 0, 'data_flow_edges': 0, 'co_change_edges': 0, 'http_call_edges': 0, 'semantic_edges': 200000, 'infra_edges': 0}, 'outbox_states': {'done': 29696, 'pending': 70304}, 'attempt_count': 29696}; integrity ok, FK errors 0. Tail is outside the deadline and does not pass readiness.
Not run: ['ready_manifest_integrity_fk', 'bounded_concurrency', 'normal_exit', 'reopen_stability']. Failure cleanup normal EOF is separate from the gated post-ready normal-exit test.

Fixed protocol: 100000 source files, source(i,value=0), 128 dimensions, fake loopback HTTP, batch16, semantic concurrency4/per-project2, parse4, 200ms poll gap, 300s readiness, original RPC/overall/resource limits. Raw RPC, HTTP and 20ms resource logs are retained compressed. V2 observations explicitly consume point-in-time generation/identity/service scopes, not response-time latest or query authorization.

HTTP counts split by monotonic deadline: {'within300s': {'entered': {'requests': 29082, 'inputs': 29082}, 'returned': {'requests': 29082, 'inputs': 29082}}, 'cleanup_tail': {'entered': {'requests': 614, 'inputs': 614}, 'returned': {'requests': 614, 'inputs': 614}}}. HTTP returned inputs do not imply committed manifests. Configured max_batch_items is 16; every actual request contained 1 input in this run.

Original PR101 failure at `a8452be903e1a88c935e267f5d76879ce7ee2716` remains failed: cold 21.638s, not ready within 300s, cleanup semantic_manifest 29696. Neither author 1k/5k in-process AB nor this single observation certifies MCP quality/full V20 or proves relative speed across environments.

Real provider calls: 0. No heldout, private source transfer, GC/WAL fault or kill experiment. Dummy bearer is synthetic and loopback-only. HOME unchanged. Default rustup read-only path error and gh CLI Forbidden are recorded; rejected action was not retried. GitHub App identity lookup succeeded as jyqj (noUsername=false).

Protocol and evidence:

- `protocol.json`, `protocol-diff.patch`, `baseline-run.py`, `run.py`: frozen scope and derivation.
- `build-receipt.json`, compressed Cargo/build logs: build identity.
- `preregistration.json`, `summary.json.gz`, `environment.json`: run outcome and timing.
- `live/n100000/*.jsonl.gz`: input manifest, original RPC/HTTP/resource observations.
- `live/n100000/deadline-300s-db.json`, `cleanup-tail-db.json`: separately timed counts.
- `resource-analysis.json`, `rpc-http-analysis.json`, `verification.json`, `analyze.py`: replayable evidence checks. These verify evidence retention/identity, not independent product correctness.
- `retained-local-data.json`: local runtime/corpus/database retention. Generated corpus, database/cache and build tree are excluded from Git.
