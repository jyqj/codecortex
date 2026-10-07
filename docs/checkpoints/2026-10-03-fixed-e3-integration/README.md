# Fixed e3 local attempt width four integration

The parent accepted only the fixed local-attempt candidate: explicit per-project >= 2 enables width min(4, max_batch); default 0/1 remains serial. Production identity is **e3c04fed903c4d0e3c26b5d7cf0e055a6f7c5207**. HTTP global/project caps 4/2, claim budget 16, DB lease token contract, cache layout, fsync and publication behavior are unchanged. PR127 DB group facade is excluded; its successful CI does not establish wiring or rollback failure recovery.

## Identities and exact imports

`production-identity.json` contains every original Git blob, SHA256 and byte count for **706 crates paths plus Cargo.toml/Cargo.lock (708 total)**, including tests/fixtures. Integration verification compares the actual path inventory and bytes against this complete fixed source manifest. The only diff from PR123 comparator `513a98c9a94b15ec77153df41af26fa3c8c0b5e8` in crates is five paths: queue.rs, attempt_lease_contract.rs, bounded_parallel.rs, semantic_runtime.rs and shared_provider_gate_tests.rs; queue.rs is the production behavior change, the others add/update tests. The source reference is `11f5b76273a16b520e6b6e01ad32a0a6743173fb`.

The final integration begins at e3 and imports exact files, never whole branches or their different production ancestors. `imported-identity.json` binds these **58** files to their original commits:

| Source | Authorized extraction | Paths |
|---|---|---:|
| PR125 `b6b163918d3ddf1b217a4f5d4f604285b5528e8f` | docs/reviews/20261003-independent-e3 | 4 |
| PR128 `800d32d50bfe158cf86366fb913aabef73f8d965` | scripts/resource_harness, tests/resource_harness, docs/harness | 17 |
| PR129 `1be640ecb6fe093e6c45eaca74617c901c87a847` | candidate-independent-100k-gate-20261003 and candidate-independent-100k-recovery-20261003 | 20 |
| PR130 `186ac53e121fe9ec770a6a293e4291be3a6e36b1` | baseline-cloud-20261003-build-blocked and baseline-cloud-20261003-resumed-once | 17 |

The two newly authorized consumer tasks each have failure and success directories; all four are preserved. The old localwidth4-100k-paired directory, old restricted 42 files, private localdiag, corpus, cache, DB, binaries and build targets are not imported. New consumer archives were inspected without extracting or executing their contents: no source corpus, Rust source, DB, cache, binary or target content is present. Candidate published hashes and all 84 archived file hashes passed; baseline published file/archive hashes passed (110 archive entries). Imported files are unchanged, including historical failed/unrun statements; this report records the current acceptance without rewriting them.

Production identity and final docs/tooling identity are separate. `final-tooling-identity.json` inventories every staged docs/scripts/tests/.github file with Git blob/mode/SHA256, excluding only itself to avoid self-reference. Its own identity is provided by the final published commit. The published integration commit/tree identifies the final docs/tooling, which includes this report, regenerated plan, scope-limited CI and read-only verifier. The measured product source remains e3, not the integration commit or PR128 ancestor.

## Accepted measurement subgate

These are the two existing, newly authorized independent cloud reports. This integration does **not** rerun 100k.

| Variant | Formal runs | Drain to ready (s) | Cold index (s) | Result |
|---|---:|---:|---:|---|
| candidate e3 | 1 | 182.999904792 | 22.086386410 | passed_declared_100k_local_scope |
| baseline PR123 | 1 | 257.929186553 | 22.218803684 | passed_declared_100k_local_scope |

Each completed 100000 exact counts, integrity/FK, coherent ready, serial query references, complete C4 repeated/distinct hits, normal EOF exit0 without force, reopen counts/hits and second normal EOF. Fixed protocol remains 128 dimensions, parse4, HTTP4/2, claim16, original deadlines/guards and loopback synthetic model. Per-consumer preliminary n32 caches were independent. The boundary observer stopped after early ready; cleanup-tail reports do not substitute for deadline acceptance.

Two independent cloud environments have unknown cgroup relation/independence and incomplete process-tree coverage. Cgroup memory includes build/cache/runner and overlapping anon/file/slab fields; root RSS is not whole-tree memory/PSS. There is no strict paired causal speedup or statistical significance conclusion. HTTP interval reconstruction does not independently prove permit occupancy; queue/service time attribution is unknown. No live provider, network-effect quality or heldout claim is made.

PR124/126 failed readiness/memory observations remain failures. Initial env-i DNS preparation failures remain in the new failure directories. Recovery retained platform standard proxy/certificate environment, fetched the original lock from official crates.io, and did not change safety configuration. Old EROFS runtime failure, GC/WAL recovery/concurrency gaps, full scale series and overall quality remain open.

## Verification and CI scope

The commands and actual results are recorded in `validation.json` and this directory's `logs/`: fmt, strict Clippy (default and semantic-http), all default targets compiled, default workspace excluding semantic 2047 passed/61 ignored, semantic library 232 passed, eleven semantic normal-path targets 83 passed/1 retained ignored assertion, driver 26 passed, bounded HTTP/status/stdio 17 passed, two isolated smoke leaves 2 passed, eval-http all-target check and architecture/identity/TODO checks passed. No failures occurred in the executed nonempty scope. Code/docs/import whitespace checking passes with untouched raw command log files excluded; their original blank EOF lines are retained and ordinary --check flags them. The empty-discovery attempts and initial tool entry failure are separately preserved. The installed official direct Rust1.95.0 was used with the original lock and normal official fetch. No env-i, mirrors, custom proxy, safety configuration, credentials or permissions were changed. Initial cargo fmt/clippy tool entry tried the read-only default rustup home before the installed /workspace/.rustup tool directory was selected; no compilation or test had run in that failed invocation.

The new normal cache/publication smoke attaches the exact PR125 fixture in an isolated e3 source copy. Only its queue width/claim16/retry publication test and real HTTP/runtime/cache/serial/two-project/close test are selected. It is not a whole fixture suite, synthetic performance rerun, old EROFS runtime test, GC/WAL/fault test or 100k rerun. The delivery crates are unchanged. The first smoke selections reused the main build target and returned zero tests (350 filtered out), so they were rejected as empty discovery and their logs retained. A separate owned target, with debug information and incremental output disabled only for this smoke build, is used for the actual nonempty smoke. The assertions/deadlines and product source are unchanged.

CI compiles every default test target, runs the default workspace excluding cc-semantic plus its library and eleven explicit normal-path integration targets, and retains existing bounded semantic HTTP status/stdio checks. It excludes process-kill/staging/GC/WAL integration execution and never invokes the old semantic_runtime recoverable-retry cases or a semantic runtime broad suite. The driver AST comparator fetches the pinned PR126 git object read-only; it does not check out, run or republish that old driver/evidence. No omitted target is reported as passed. The 26 harmless driver tests, TODO consistency and complete product/import identity checks are added to CI. Remote CI starts only after draft publication; the parent owns CI follow-up.

`tasks.json` remains the TODO authority. `code_index_plan.py --write` regenerated 05-TODO.md and a subsequent check passed: 192 tasks, 150 done, 41 todo, one in_progress. P7-014 remains in_progress; P7-015/016/018/019/020 and P8-005 remain todo. Only this fixed local protocol's candidate/baseline/count/FK/query/C4/EOF/reopen subgates are marked closed_declared_local_scope. The original 37 remaining-gate rows, V19 open status, live authorization, heldout custody, old EROFS, GC/WAL and quality gaps are retained.

No merge or deployment is authorized or performed. Repository/ancestor AGENTS.md, /workspace/.agents and /workspace/.codex supplied no local instructions or skill files; the available skill catalog has no workflow required for this repository-only integration.
