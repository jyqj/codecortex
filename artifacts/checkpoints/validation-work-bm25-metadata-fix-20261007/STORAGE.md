# Lossless evidence storage

This new checkpoint archives 44 original raw files (5,947,134 bytes) into `raw-observations.tar.gz` (379,746 bytes). The remaining 20 original evidence files stay directly readable. Every original path, byte count and SHA256 is fixed in `checkpoint-files.json`; no original payload was rewritten. Old checkpoints are untouched.

- Index SHA256: `aed15930f5bc069ec4d21d2c709414a87e08e034fed194246267e7bef4e64345`
- Archive SHA256: `23f668c88921a53b8d7c0fcd6b131c4bf4717afc873dd86d53c1afb64204510a`
- Archive format: sorted USTAR regular-file members; mode 0644; uid/gid/mtime 0; empty owner/group names; no links, sparse records or extension headers. Gzip has mtime 0, no filename, compression level 9.
- Raw groups: all five baseline/instrumented/repaired observation directories, three temporary diagnostic source copies, and every original command/CI log, including empty/tiny logs and their original blank lines/trailing spaces. Failed baseline, failed default attempts, instrumentation and repaired output are preserved equally.

Run the read-only byte check from the repository root:

```sh
python artifacts/checkpoints/validation-work-bm25-metadata-fix-20261007/verify_storage.py
```

The script pins the index and compressed bytes before parsing, checks safe relative paths and every disk path component for symlinks, requires exact declared sizes and hashes, rejects extra/missing/duplicate/noncanonical tar members, and streams all archived payloads. It never extracts a member to a filesystem path or executes its content. The execution environment may retain matching raw mirrors in the isolated worktree after deletion; such mirrors are independently checked and explicitly excluded from the Git commit. The final committed evidence layout is 44 archive members plus 20 direct original files.

`storage-verification.json` records the observed local check, including any matching mirrors. Storage verification first checked every original on disk against the archive before any mirror removal. A storage success is solely an integrity result; the four default test failures remain failures in the validation receipts.

`checkpoint-files.json` indexes the completed original evidence snapshot; this note, verifier, storage receipt and index itself are storage metadata added afterward. The verifier pins the index SHA to prevent the index and archived contents from drifting together. These metadata files and the archive are included in the final Git evidence commit.
