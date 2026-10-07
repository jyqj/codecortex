# Lossless evidence storage

The two raw Cargo logs contain 162,940 bytes and are preserved in a
12,557-byte deterministic `raw-observations.tar.gz` archive. Member names
are their original checkpoint-relative paths. Contents, including blank lines
and compiler JSON, are unchanged. All small observations and receipts remain
directly readable.

`checkpoint-files.json` covers 33 original files: 2
archive members and 31 disk originals. Its SHA-256 is
`abaf329b6cff036c1e592faaff712aabc1be79f66d3bee0438191d16549a7c38`. The archive SHA-256 is `c7f7c705306f1a3b6a1452651bbde99e350a93252a5ed0381f1576d9d1bb1218`.

```sh
python3 verify_storage.py
```

The verifier pins the index and exact compressed bytes before parsing, rejects
unsafe or duplicate paths and non-plain members, verifies canonical sorted
USTAR headers and zero-time gzip metadata, enforces exact per-file and total
lengths, and streams all payload hashes. Disk paths are checked component by
component for symlinks. No filesystem extraction or payload execution occurs.
Any present raw-log mirror must match its original hash; such mirrors are not
tracked. A clean Git checkout has only the archived copies of these two logs.

The manifest, archive, verifier, this storage note and verification receipt
are storage metadata outside the original-file index to avoid self-hash cycles.
Their exact bytes are bound by the evidence commit. The storage receipt does
not replace or upgrade behavioral results, including the expected five red
baseline failures and the deliberately incomplete overall P7-016 scope.
