# Independent query encoding review

Candidate: PR36 `eda4df9f84943c5ec86f7d03f508c3fffedcc012`.
Candidate file SHA256: `e038f59ebf2794dfc74f552ffca158a29ba7066f6d2bf1f96e3cd9bf3adb416e`.
PR35 checkpoint `00e8c6d667198b1c0a1eb2b4a8fd69ca3fd71c4b` is an ancestor of this candidate.

## Scope and method

The candidate is **not registered** in lib.rs or main.rs at this checkpoint. The runner verifies exact candidate bytes against the frozen commit, copies them unchanged to a temporary directory, and appends a separate independent test module. An ephemeral integration-test harness loads that copy and the real service_factory.rs, including its actual QueryServices/QueryPin implementation. No production file or registration changes. The runner removes its harness even on test failure.

Only `independent_review` tests run. The 15 author tests compile but are filtered out and contribute no claimed independent evidence. The existing candidate `before_publish` test hook is used solely to hold execution after provider encoding and context checks, immediately before the lifecycle/control/abandonment publication locks. Assertions and fixtures are independently authored.

Five independent tests in each of `semantic` and `semantic-http` cover:

- Four deterministic final-publication faults: future drop, explicit shared-control cancel, child deadline expiration, lifecycle retirement. Each blocks after encoding but before publication; all leave the cache empty. Capacity stays held after waiting future exit until blocking work exits. Drop and child deadline preserve usable shared control for local fallback.
- Real root `ensure_query_vector`, blocked synthetic provider call, real QueryPin: abort cancels only the child token, keeps parent token/shared QueryControl usable, retains slot and pin until physical blocking exit, suppresses late cache publication.
- Cold-cache then warm-cache requests: exactly one factory invocation and one synthetic provider call overall; warm hit succeeds even with the direct capacity semaphore exhausted.
- Raw transport seam captures min(provider timeout, absolute remaining query budget), preserves shorter provider timeouts, rejects cancelled child before IO and cancellation raised during IO after return, without cancelling the parent token.
- Expired deadline and parent cancellation prevent all raw transport calls.

No real provider, credentials, HTTP socket, or paid API was used. The HTTP wrapper tests use an in-memory transport with a synthetic loopback URL; they prove wrapper policy, **not** reqwest's wall-clock deadline enforcement or socket cancellation. Fake provider vectors and queries are synthetic. No source was sent to a provider.

## Static findings and limits

No counterexample found within these boundaries. Final cache publication acquires lifecycle, request-control publication, abandonment-control publication, then cache mutex; checks occur after lifecycle acquisition. Drop cancellation linearizes via the independent abandonment control. The blocking closure owns its service, slot and keep-alive pin. `ensure_query_vector` gives factories a child token and drops its guard on return/drop. This review does not establish starvation freedom, exhaustive scheduling correctness, or hard real-time deadline semantics during an already accepted short cache write.

Main integration must still register the module; enforce semantic-http/enabled/network/query-text opt-ins plus nonlocal/nonempty scope; use the same control for exact scan; install the transport wrapper and supplied child token in the real factory; retire the shared lifecycle fence before cancelling project token; and align foreground/background limits with the provider gate. Those conditions are documented by the candidate but are not enforceable or proved by this unregistered harness. No production wiring/config, author test, shared ledger or TODO changed. Full V18, full workspace checks and actual executable integration were not run.

## Reproduction

From the candidate checkout with Rust 1.95:

```sh
python3 scripts/review/p7_query_encoding_review.py --features semantic
python3 scripts/review/p7_query_encoding_review.py --features semantic-http
```

Both commands run `cargo test -p cc-server --features <feature> --locked --offline --test p7_query_encoding_review_harness independent_review -- --nocapture`.
Cloud environment additionally set PATH=/workspace/.cargo/bin:$PATH, RUSTUP_HOME=/workspace/.rustup, CARGO_HOME=/workspace/.cargo, CARGO_TARGET_DIR=/tmp/p7-017-build, CARGO_INCREMENTAL=0, CARGO_PROFILE_DEV_DEBUG=0 and CARGO_PROFILE_TEST_DEBUG=0.

The harness compiles unused production/author interfaces, yielding dead-code warnings; no `-D warnings` claim is made for this artificial registration context. Review case file was rustfmt formatted. CI/workspace clippy and full workspace test were not run.

## Ledger suggestion for integrator

Record this as bounded independent service review evidence, 5 passing independent tests per feature, with unregistered/main-integration and real HTTP enforcement limitations. Keep the query-network integration/full V18 gate open until the composition root is implemented and independently exercised. Do not close P7-014/P7-017 or claim full semantic acceptance from this result.
