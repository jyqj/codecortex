# Local compatibility-fix retest: pass and failure retained

This bundle preserves the actual follow-up on source `d911a0573e5a53ac20073f050f55503c5b2874c3`, whose `p8_load.rs` is byte-identical to root fix `0ea1879417775ffb09c0fb5b2aae4f55c680b7ae`. Rust 1.95.0 ran the two new artifact-budget unit tests successfully (exit 0). The load integration group finished with 8 passes and 1 failure (exit 101). This is **not** all-green validation or proof of Rust 1.99 compatibility.

The failed test expected child stderr to contain `binary binding mismatch`; its assertion did not print or persist that stderr. The preceding check of child exit code 2 succeeded. The cause remains unconfirmed. A post-execution digest found the CLI file identical to the earlier 853385b7 binary, while the checked-out module had changed. Worker checks its compiled module before the intentionally incorrect executable digest; mixed build artifacts are a candidate explanation. This digest is not a before/after execution binding.

Raw Cargo logs and exit receipts are copied byte-for-byte. The unit executable digest was captured earlier after successful execution; integration and CLI digests were captured when recovering the completed run. No repeated test run, skipped test, softened assertion, source change, or Actions rerun was performed in response to this failure. Root will use an independent clean GitHub run for stable-toolchain validation.

The machine had no available disk space when this result was recovered. That observation does not prove why the assertion failed. After preserving digests and confirming both completed exit receipts, this agent removed only its own `target-load` cache; immediate disk observation still reported zero available bytes. No binary, SQLite fixture, or other session cache is included here.

`receipt.json` records scope and limits; `source-binary-binding.json` records post-execution digests. `manifest.json` and `SHA256SUMS` cover persisted files.
