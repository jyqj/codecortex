# Independent production fairness and model-transition review

Executed final candidate: PR41 `c8c20b5b7d416372ee06ed5e248663c48912064e`, including PR38 `4654b9c37e4cb2c58539a29c8c8e023c03989bf4`.
Earlier independently observed candidate: `bfaf45ead49469a5ba7bd5ea732a71fc0b6e247d`.
PR41 remote head observed during PR preparation: `a9f5b7c13e2a4d79c25a017e3a8cbfb47e9037db`; its delta from c8c20b5 contains only author evidence and ledger/docs (five files), no source/config/test changes. This review's executed candidate remains **c8c20b5**; no claim that remote a9f5b7c was separately tested.

## Independent production harness

Only new `crates/cc-server/tests/p7_production_fairness_model_review.rs` and this evidence directory are owned by this review. No production file, existing author test, config implementation, Cargo configuration, task ledger or TODO was edited.

Unlike the earlier unregistered PR36 review, these tests use the actual registered production modules: CodeIndex configuration loading/set_project/reopen/close; real indexing handlers and caller-driven worker; real configured provider factory, foreground/background semaphores, process provider FIFO gate, blocking reqwest transport, cancellation/publish fences, and search/context handlers. No copied author fixture or assertion is used. MCP JSON dispatch is outside this harness; typed production search/context handlers are called directly.

All indexed files and query text are synthetic. The sole endpoint is an in-process HTTP server bound to 127.0.0.1 with synthetic file credentials and isolated temporary cache. It parses one request, records only model and query/document classification, blocks responses under explicit test control, and returns synthetic two-dimensional vectors. No real/paid provider or source egress. Child handler/worker tasks share actual process gates. Tests serialize only to protect process-singleton initialization and environment/cache isolation.

## Six independent passing contracts

1. Hold two foreground HTTP calls (search and context), enqueue two actual background jobs in measured provider-gate order, release one foreground call, then introduce a later foreground request. Both earlier background requests enter HTTP before the later foreground request. CPU in-flight is zero while waiting, a raw `BEGIN IMMEDIATE; ROLLBACK` succeeds with a 100 ms busy bound, and local search succeeds within one second despite the single read-pool connection and network saturation.
2. Hold two background HTTP calls, enqueue search then context in measured gate order, and schedule a third project's background job. Both foreground requests enter HTTP before the new background provider admission. The third background job cannot enter the provider gate while both background class slots are occupied.
3. Load model A, edit on-disk configuration to B, and perform a real indexing trigger: the old runtime still calls A and does not switch space. Explicit set_project installs B; while A is still active, the capability projection reports backfilling/dense_published=0 rather than ready. The retired A owner cannot schedule. B's explicit schedule commits an audited A-revoked/B-active transition with configured-doc-spec revision and pinned=false. Close while both A/B provider replies are blocked, then release and wait for real pin/gate exit: neither can publish the current document. Reopen B recovers ready under B.
4. Real configured model B worker encounters an external SQLite `BEGIN IMMEDIATE` blocker; close completes within 500 ms without waiting for that blocker. After rollback and actual pin exit, active model remains A, no HTTP call occurs, and the retired owner cannot schedule. The test observes a worker pin and allows 100 ms scheduling lead-in; it does not instrument the exact internal instruction when SQLite starts waiting.
5. Abort two actual foreground queries while reqwest responses are blocked: both gate permits, foreground slots and two actual QueryPins stay held. A third cold semantic search promptly returns `unavailable/semantic_capacity` with no HTTP call. Once the original HTTP calls physically exit, pins/permits release; abandoned results never populate the query cache, and a new query can call and cache normally.
6. Close one of two background owners while its HTTP response is blocked, then schedule a third background project. The closed owner retains its actual pin and gate permit; the third job cannot enter either HTTP or provider-gate admission until the closed owner's blocking call really exits. After release, third-project publication and remaining live worker converge.

Every independent progress wait retains a five-second upper bound; response barriers also have a five-second fail bound. No author timeout/assertion was changed. Suite result on c8c20b5: **6 passed, 0 failed, 0 ignored** (0.95 seconds test execution). Targeted cc-server semantic-http clippy with `-D warnings` passed; the new file's rustfmt check passed.

## Static boundary findings

Foreground is a process-wide two-permit direct semaphore held by the spawn_blocking encoding closure. Background is a separate process-wide two-permit semaphore held by the blocking finite-job closure. Each provider attempt additionally acquires the same process-wide FIFO provider gate through AdmittedProvider; later arrivals cannot bypass existing eligible waiters. Network factory/call/drop and gate waits are outside query CPU execution and SQLite transactions. Bounded fixtures establish progress under competing arrivals; they do not establish mathematical starvation freedom for all infinite arrival/cancellation/timeout schedules.

Model transition authority is stored only at explicit composition-root installation/set_project. Ordinary indexing rounds consume that authority once and subsequently reject active/configured space mismatch. `IndexDb::prepare_semantic_configured_space` locks the writer and obtains `BEGIN IMMEDIATE` **before** entering LifecycleFence; its permit remains in scope through registration, active switch, generation bump and COMMIT/ROLLBACK. Incarnation is checked inside that transaction. Runtime close retires the same lifecycle fence before token cancellation. These source findings complement the production blocker/close test; no test hook was inserted in the COMMIT path.

The c8c20b5 delta moved semantic health callback and scope probe onto a bounded CPU worker and rechecks control there. No network call is made in that CPU closure. Runtime test diagnostic changes in the delta do not change these production contracts.

## Preserved old observations

`baseline-bfaf45e/initial-expectation-failure.log`: original independently authored four-case suite exited 101, with 2 pass/2 fail. Both failures were an erroneous unconditional successful-result assertion after intentional concurrent publication; production returned exactly `RetrievalChanged { attempts: 1 }`. This is permitted generation-fence refusal, not a production starvation/publication counterexample. The fixture and assertions were corrected to accept only successful completion or this exact documented refusal; timeout/cancel/busy/other errors remain rejected. The original five-second bound was retained.

`baseline-bfaf45e/corrected-contract.log`: corrected four-case suite passed 4/4 on the same bfaf45e production tree. Subsequent query/background physical-exit tests and stronger audit/pin-exit assertions were added; final six-case proof is only claimed for c8c20b5. Initial five-case c8c20b5 result also passed, but final evidence is the checked-in six-case log.

## Reproduction and limits

```sh
cargo test -p cc-server --features semantic-http --locked --offline --test p7_production_fairness_model_review -- --nocapture
cargo clippy -p cc-server --features semantic-http --locked --offline --test p7_production_fairness_model_review -- -D warnings
rustfmt --edition 2021 --check crates/cc-server/tests/p7_production_fairness_model_review.rs
```

Rust 1.95.0; cloud PATH=/workspace/.cargo/bin:$PATH, RUSTUP_HOME=/workspace/.rustup, CARGO_HOME=/workspace/.cargo, CARGO_TARGET_DIR=/tmp/p7-017-build, CARGO_INCREMENTAL=0, CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0.

No counterexample found in this bounded production review. Full workspace test/clippy, full V18, remote CI, all HTTP fault/parameter matrices, infinite-load fairness, and the author's 1100-document cross-job/reopen claim were not independently rerun here. No merge/deploy/force push.

Ledger suggestion: add this as independent real-production FIFO, physical-exit and configured-model transaction/close evidence at c8c20b5, preserving the old expectation-failure history and the full-V18/remote-CI limitations. Integrator owns the authoritative ledger update; this PR does not close any shared gate.
