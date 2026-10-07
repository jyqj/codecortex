# Observation storage

41 original observation files (1,722,453 bytes) are stored losslessly in `raw-observations.tar.gz` (387,326 bytes). Two directly stored summaries make 43 original objects in the size/SHA manifest. The archive was built twice with identical bytes.

Original success and failure logs, receipts, probes and helper sources are preserved without rewriting. Copied product/test-runner executables and disposable Cargo intermediate directories are excluded; their recorded identities remain. No raw mirror is committed.

`python3 verify_storage.py` checks exact manifest/archive pins before parsing and validates all member paths, types, canonical metadata, sizes and hashes. It rejects changed or unsafe disk mirrors and does not extract or execute payloads. A successful storage check does not alter P7-017's failed isolated gate result.
