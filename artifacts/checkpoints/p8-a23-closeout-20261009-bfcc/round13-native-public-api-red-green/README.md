# Native public snapshot dependency regression and repair

The original public API compatibility regression was reproduced in real Rust after successful compilation: the new test failed with64 committed dependency keys where the original API preserved70. The frozen two-file production repair made the same test pass. The first control run passed7 integration,14 unit and6 snapshot tests, then stopped on a real format failure and missing original artifact compile input.

The v2 patch changes only the reported test-line formatting. Two unchanged Rust files referenced by existing server tests were restored from exact3ff. On v2, all27 related tests, complete workspace format check and strict cc-db Clippy passed with actual zero exits. Every attempt retains its own source map, stdout, stderr and receipt.

These are ordinary native engineering checks on Rust1.95.0/macOS SDK15.4, using an owned APFS-cloned cache. The original domains contain1242 files; v2 adds the two explicitly listed original artifact inputs, yielding1244. No whole-repository checkout, cold-build, performance or current-main certification is claimed.

The63-member native archive was created and fully CRC/member-checked on the runner. Receiver-side independent raw review follows this immutable publication; it is not asserted here. The v2 source archive already includes non-author whitespace-only review. See publication-manifest.json and the actual controllers for exact scope.

Original TODO count remains29. The separate original Gc8 study and its identities are unchanged.
