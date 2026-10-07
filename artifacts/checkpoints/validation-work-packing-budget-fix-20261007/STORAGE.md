# Lossless storage of raw observations

This storage-only follow-up moves the bulky JSON observation directories from
checkpoint commit `c4a3a27b61dc3077b2503c5d0dacba5f1021d8a2` into
`raw-observations.tar.gz`. Every member keeps its original path relative to
this checkpoint and its exact original bytes, including all failed-candidate
observations. It does not change the source, tests, conclusions or original
checkpoint manifest.

| Original directory | Files | Original content bytes |
| --- | ---: | ---: |
| `baseline-stages/` | 52 | 1154116 |
| `candidate-boundary/` | 1 | 2040511 |
| `candidate2-boundary/` | 12 | 2904567 |
| `combined-check/original-boundary/` | 12 | 2904756 |
| `combined-check/packing-scope/` | 1 | 27239 |
| **Total** | **78** | **9031189** |

The archive is **312827 bytes**, a 96.5% reduction relative to those original
file contents. The remaining 28 files named by the original manifest stay on
disk unchanged, including the top-level README, logs, source manifests and
independent review, plus the combined check's small logs, runner, receipt and
768-input source manifest. `checkpoint-sha256.json` itself stays unchanged.
Paths in the original README and manifests remain the logical member paths;
this document records the storage relocation.

## Integrity

Original `checkpoint-sha256.json` SHA-256:

```text
8e10787d4d7f78eff350ed62f2aed9f6fc2a1fb483121509f1e3f3f1c2b884b4
```

`raw-observations.tar.gz` SHA-256:

```text
5de52b3bd07103902c7b983f1564a0dab63a729eb59fd2497475433da83e8483
```

Run the read-only checker from the repository root:

```sh
python artifacts/checkpoints/validation-work-packing-budget-fix-20261007/verify_storage.py
```

The checker first binds the original manifest and complete archive bytes to
these hashes, then streams tar member contents without extracting them to the
filesystem. It checks all **106 original file hashes**, using disk files or
archive members. If both representations exist, it requires both to match.
After relocation it reports 78 archive members, 28 disk files and zero mirrored
files. Before removing the raw disk copies, the same verification also passed
with all 106 disk files and their 78 archived mirrors present.

The checker rejects unsafe relative paths, duplicate/extra/missing members,
non-regular, linked, sparse or extended members, oversized payloads and changed
canonical tar metadata. It rejects symlinks at every component of an original
disk path. Archive parsing uses the same bounded in-memory bytes whose archive
hash was checked. It never executes JSON or other stored payloads and never
writes or extracts a member to a filesystem destination.

## Deterministic archive construction

The archive contains only regular file entries in lexicographic member-path
order. Python's standard-library `tarfile` writer uses `USTAR_FORMAT`, mode
`0644`, UID/GID zero, empty owner/group names and modification time zero. It
adds no directory, link or extended-header entries. The `gzip.GzipFile` writer
uses compression level 9, an empty original filename and modification time
zero. Two complete constructions in the same environment produced identical
archive bytes before any raw disk files were removed.

Original content hashes were checked before construction, against every
archive member before relocation, and again by `verify_storage.py` after
relocation. The archived bytes remain recoverable from this single archive;
the original unarchived observations also remain in the fixed Git commit named
above. No Rust tests were repeated for this storage-only change.
