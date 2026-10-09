# Isolated candidate formatting

This job formats exactly 15 fixed Rust candidate files in new, owned copies. It is an engineering preparation step and does not compile code, run tests, execute a product, measure performance, or certify earlier CI.

The immutable input bundle contains 13 product files from author handoff d7782d331842dd6f08fd4d090a1a95ec32c51cf2, the optional statistics file bc1106abc587b9ada81fbc36ca4461d987f0af87, and the backfill test a4728eb6bb33e10c42a502dcc3a36361215aa6b2. All 15 original UTF-8 bodies, Git blob IDs, byte lengths, and SHA-256 values were independently fetched and checked before publication.

The original workspace edition is 2021 and all five affected crates inherit it. The source root and complete crates tree contain no rustfmt configuration. The runner passes an explicit empty TOML in its owned directory to prevent unrelated home configuration from affecting results. It uses the fixed 1.95.0 toolchain, records the actual rustfmt executable digest and both version outputs, and sends one file at a time through stdin. Text input avoids traversal of missing out-of-line modules; no unstable skip-children flag is needed.

Only the fresh RUNNER_TEMP directory is written. The original checkout, fixed candidate bundle, materialized inputs, and formatter executable are compared before/after. Formatter children receive a short environment allowlist containing no GitHub token, loader hook, or arbitrary Rust flag. Installation may access the normal toolchain distribution service; no claim of physical network isolation is made.

Every attempted file keeps its original identity, actual stdout bytes (base64 plus UTF-8 when valid), stderr, argv, timing, and exit code. A parse failure is retained, the remaining files may still be formatted, and the overall receipt fails. No code is silently repaired. Formatted copies are proposed bytes for later review, not an installed product source.

Six exact output files use schema p8-candidate-rustfmt-frame-v1: formatted-sources.json, commands.json, stderr.log, source-before.json, source-after.json, receipt.json. Frames retain 1,024 raw bytes per chunk, full per-chunk/file hashes and exact order, with 2,048-byte physical JSON line, 4 MiB file, and 12 MiB total raw bounds. A complete uploaded artifact additionally retains all input and output copies. Final acceptance requires the actual process/job exit 0, final passed receipt, all 15 successful formats, unchanged inputs, and complete raw delivery; an early formatted entry alone is insufficient. Hard termination can prevent final frames or artifact upload and must remain a failure/incomplete observation.

Publication parent is the already published R23 evidence commit 94f6e1c36583bfed1d3b8e3c76591bc2a0b26ee5. Only this isolated prefix and the new exact-branch push workflow are added. No active source, guard, original study, task ledger, or existing workflow is modified. No branch/ref/PR was created by the draft author.

The candidate still requires final source review and normal CI after formatted bytes are installed. No TODO is closed: 29 remain.
