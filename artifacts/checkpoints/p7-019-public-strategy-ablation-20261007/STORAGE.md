# Observation storage

301 original observation files (6,028,848 bytes) are stored losslessly in `raw-observations.tar.gz` (1,483,800 bytes). The archive was built twice with identical bytes. Each path, original length and SHA-256 is listed in `checkpoint-files.json`, together with the two directly stored summaries.

The archive preserves every included original file byte, including failed/early logs, raw JSON, source maps and local execution receipts. Large copied executables and exact source snapshots are excluded; their hashes, compiler artifacts, complete input manifests and immutable Git source pins remain recorded. Copied source is reproducible from the recorded commit; product hashes do not promise a byte-reproducible rebuild.

Run `python3 verify_storage.py` for read-only verification of the fixed archive, member paths/types/metadata, lengths and hashes, plus direct originals. It does not extract files, execute payloads, rerun tests or upgrade any historical observation. Optional disk mirrors must match the same original bytes.
