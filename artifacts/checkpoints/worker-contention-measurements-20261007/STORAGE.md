# Lossless P7-015 evidence storage

Six original request arrays and two raw Cargo logs contain 1,895,684 bytes.
They are preserved in the 53,795-byte deterministic archive
`raw-observations.tar.gz`, with original relative member paths and byte-for-byte
content. All per-seed protocols/queue snapshots/summaries, original lifecycle
observations, small logs, source maps and receipts remain readable on disk.

`checkpoint-files.json` covers 34 original files: 8
archive members and 26 direct originals. Its SHA-256 is
`609f808324eca0335d61a891aa8b46be5e527ea696702d22cc085ef730e86bf2`. The archive SHA-256 is `84e6ae2c27c22db99addc8fd3bfd3e13c7d00419496086cc8ff669761aac7c0b`.

```sh
python3 verify_storage.py
```

The verifier checks pinned manifest and compressed bytes before parsing the
same archive byte buffer, exact safe paths and membership, sorted unique plain
regular USTAR members, canonical zero-time gzip/tar metadata, per-file and
total byte bounds, and streaming SHA-256. Disk paths reject symlinks in every
relative component. It never extracts to filesystem paths or executes payloads.
Any present original raw mirror must match its stored hash. Such mirrors are
not tracked; a clean checkout obtains those originals from the archive.

The index, archive, verifier, this note and storage receipt are metadata outside
the original-file index to avoid self-hash cycles; the evidence commit binds
their bytes. This verification reuses the previously independently reviewed
GC/BM25 verifier with only the six fixed manifest/archive/count constants changed.
Storage verification is independent of test acceptance: the original tail
events, unknown resource attribution and incomplete P7-015 scope are retained.
