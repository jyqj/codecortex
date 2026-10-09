# Local E guard execution history

This is a lossless evidence package for execution `a23bb72d3c954f385b99fe81ce9189885c208557`, product `254009277688d64677361a0ca33e5dea73f295ef` and review `3a11f30f9f00a89fe3cd481b3b7609066728baad`. Packaging did not run the guard again and did not change any source, task, index, ref or remote.

`payload-manifest.json` records the 60 exact source members, their sizes, SHA-256 and Git blob SHA-1 values, source modes, source paths and verification. `selection-inventory.json` is the pre-packaging inventory. `runtime-local-E-guard-history.tar.gz` preserves 11,775,147 bytes of original records, including all three local attempt directories, all three wrapper versions, the 7,052,231-byte original Git trace, original stdout/stderr, before/after source and observer snapshots, preparation records and official product/review/execution bindings. `prepare_payload.py` is the packaging recipe; it refuses to overwrite an existing archive.

The first two wrapper attempts failed in their own import/preflight work and invoked the original guard zero times. The third wrapper invoked the guard once. Its original stdout says `passed`, but the CLI integer exit was not persisted before a later wrapper cleanup assertion failed. Its recorded CLI exit remains **null**, and its outer wrapper exit remains **1**. These facts must not be rewritten as a local recorded exit zero. A later successful normal GitHub CI gate is a separate execution and cannot change this history.

The local view was a required-file projection, including the complete product/validation inputs and historical guard read closure. It was not a complete main checkout or a native runtime result. This package contains no private Git object database, derived historical checkout, original measurement ZIP, binary library or signed download reference. All original source records remain in place.

The archive uses sorted regular USTAR members, mode `0644`, zero UID/GID/mtime, empty owner names, and gzip compression level 9 with empty filename and mtime zero. Every member was reopened and compared byte-for-byte with its original, and a second archive generation was compared byte-for-byte without writing another copy. Source file bytes and modes were checked again after packaging.

This package does not complete a TODO. The recorded ledger remains **163 completed / 29 remaining**, and `formal_task_completion` is false.
