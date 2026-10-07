# Lossless storage for the initial legacy CI observations

The original index `evidence-sha256.json` is unchanged:

`83865419fcc4d2bb71bfe86551e27850c378073ae0e6bdcd14fb36d111c74eb1`

It records 21 original files totaling **1,682,463 bytes**. Ten original files
remain directly readable. Eleven complete decoded job logs and API responses,
totaling **1,407,477 bytes**, are stored in `raw-observations.tar.gz`:

| Property | Value |
|---|---|
| Archive bytes | 261547 |
| Archive SHA256 | `3d1f1f0c9e05829b6743a59529a46105e99e8f9d6fe01d6eea99a999dfebe373` |
| Member filenames | Original relative paths, sorted and unique |
| Tar encoding | Plain USTAR regular files, mode 0644, uid/gid/mtime 0, empty user/group names |
| Gzip encoding | Compression level 9, mtime 0, no stored filename |

Archived original names:

- `artifacts-api.json`
- `check-job-logs-api.json`
- `check-job.log`
- `initial-jobs-api.json`
- `msrv-job-logs-api.json`
- `msrv-job.log`
- `run-metadata-api.json`
- `run-payload.json`
- `security-job-logs-api.json`
- `security-job.log`
- `terminal-jobs-api.json`

Run:

```sh
python3 -B artifacts/checkpoints/pr144-legacy-ci-history-failure-20261007/verify_storage.py
```

The verifier first pins the unchanged original manifest and the same archive
bytes which it subsequently parses. It checks every original SHA and byte
count, exact archive membership, ordering, plain member type, metadata and
total payload size. It rejects symlinks in disk path components, unsafe paths,
missing or extra archive members, and mismatched optional disk mirrors.
It never extracts a member to a filesystem destination or executes a payload.

The verifier derives from the already reviewed P7-017 storage verifier at
`72efd2ff65b71a7e9633e8ffc6ddf88cb1ad0a7a`. This checkpoint changes its fixed
pins/counts and adapts manifest decoding to the original `{schema_version,
files}` wrapper; archive placement is an explicit fixed eleven-name list.
No original file, timestamped observation, failed result, ignored result or
skipped result was rewritten during packaging. The raw job files preserve
the complete decoded connector strings re-encoded as UTF-8, including their
initial BOM; API files preserve the complete serialized tool responses.
