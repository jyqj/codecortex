# PR180 public API failure-prefix review

Fixed reviewed head: `3ffcefc3b28ee1a4ed80caecebd7208a45c3e302`; product P: `b13ba8c1b738bc9c24b81cad1e88711fc9734283`.

The new resolution-dependency batching also reaches the old public `SnapshotWriteTxn::write_file_data` through `snapshot_leaf_batches=true`. Its byte-identical outer method therefore does not establish unchanged transitive failure behavior. With the PR's own table schema, trigger and73 input dependencies, a source-derived SQLite3.53.1 control actually returned exit0 and demonstrated70 retained rows for the original per-row SQL versus64 for the new64/8/1 statements. Both caught the same SQLITE_CONSTRAINT_TRIGGER, retained an active transaction and committed successfully.

This is a SQL-only semantic control plus a reviewed Rust call path, not a Rust product execution, bundled-SQLite test, benchmark result or current-Gc8 study failure. Existing rebuild-owner error propagation still protects publication; the missing regression is the general public caller that catches the error and commits its borrowed connection.

Required resolution:retain original per-dependency SQL on the old public route and put batching behind a separate explicit rebuild-only opt-in. Preserve abandon-on-error ownership and add a public catch-and-commit regression alongside the existing owner nonpublication control. Do not weaken the old interface contract to make this difference disappear.

The selection gives exact byte/SHA256 identities. Source files under `public-api/` reuse their already-fetched original Git blobs; the DDL/trigger in the executable control are extracted from those exact source blobs. For local replay, adjust the two filesystem roots in the script to the published source-byte directory, without changing SQL or test inputs. The full observed stdout and integer execution receipt are included.

All original task statuses remain unchanged:163 done/29 remaining; this review closes no original TODO.
