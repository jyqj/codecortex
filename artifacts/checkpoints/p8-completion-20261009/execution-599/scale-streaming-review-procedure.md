# Fixed 599 P8 scale review

This directory contains an independent replay of the original scale evidence for
GitHub run **37835810882**, fixed source
**599a7050e7d52b5b7b93975c419138e175b3f754**. It does not change task status.

The authoritative task mapping is `original-acceptance-mapping.json`. Its original
P8-005 and P8-006 steps, acceptance clauses and dependencies were checked against
the exact 599 Git blobs. Both tasks remain `in_progress`; the fixed task table has
29 unfinished tasks. Do not count the older 6e3 measurements as samples of this run.

## Fixed inputs

The shared, read-only source is
`/dev/shm/a217aaae3bde/codecortex-round5-frozen`. The original
`scripts/p8_scale_matrix.py` and `scripts/p7_build_identity.py` are loaded from this
checkout. Every replay checks the exact Git HEAD, complete current crate/Cargo
inventory, committed blobs, driver identities and retained build receipt again.

The full source inventory has 1,087 inputs and manifest SHA256
`ac7421155638a64b3690cb63d5434e562f1b0645f0145d298e9594ef62175a59`.
The original build receipt SHA256 is
`4e21749f58726491bce7c33fcff5d14dc97810059e9f62ed91eed2dbba7aac0c`.
The executable SHA256 is
`45229090024cbcb1aafc52acb38e8532a3eb5ab17915a7aa38b01a679d7317cc`;
its BLAKE3 is
`0b5534ee0c5a1686f651bce534158d38cd966f49a1331ad4331a9595a656232b`.
`validated/state.json` binds these inputs and this helper's bytes. Do not edit the
helper after initialization; a changed helper is rejected by subsequent steps.

`validated/initialization-review.json` records successful original
`validate_build` replay, including the strict producer checkout, Cargo target,
pre-copy/post-copy hash and executable-size checks. Only a derived executable's
permission bits were restored. No workload was executed locally.

The independent raw-engine comparison hashes every 738 `.rs`/`.toml` input under
`crates` using the retained executable's `--hash-file` operation. It follows the
original Rust `runner::engine_provenance` tuple ordering and JSON representation.
The expected source BLAKE3 is
`486b60985a3e80bf8ea13b52c1413f8727c9c2d7acb6b2335af8137a4aad9b91`.
Cargo.lock BLAKE3 is
`add600c6653b09a8e966f2edda1667267d82aeb6e9985d0a36d441f53e71f782`.

## One artifact at a time

Root obtains each GitHub original artifact ZIP and independently fetched metadata.
The metadata must contain the artifact ID, exact name, ZIP size, SHA256 digest and
the workflow run's ID and full source SHA. The helper does not fetch or follow
download URLs.

For each original shard, use its actual paths:

```sh
python3 /dev/shm/a217aaae3bde/scale-round5-review/review_scale_fixed_source.py shard \
  --state /dev/shm/a217aaae3bde/scale-round5-review/validated \
  --archive /path/to/original-shard.zip \
  --metadata /path/to/artifact-metadata.json
```

The helper verifies ZIP size and SHA256, rejects unsafe/duplicate/special paths,
checks extraction capacity and CRC, then calls the **unchanged original
`validate_shard`** on the complete native evidence. It additionally compares the
entire original source inventory and raw engine source/Cargo.lock digests with
the fixed source. A failed native report or failed replay remains a failure.

`validated/shards/<artifact-id>/validated-shard.json` is exactly the original
validator's returned object, including every compact measurement. `review.json`
records its hash, original receipt hash and native report. Only the derived
extraction is removed, after rechecking the original ZIP. The ZIP is never removed
by this helper. Root must durably archive every original ZIP and its metadata
before reclaiming any local original file; compact summaries do not replace raw.

This keeps local expansion bounded by one shard, with a 600 MiB ZIP expansion
safety ceiling and the unchanged native 512 MiB evidence budget. The build and all
compact records remain available. Failed extraction cleanup is recorded; any
original-byte mismatch keeps the derived files for investigation.

## Complete population

The original matrix requires five scales, 30 repetitions per group, 150 unique
shards and 1,500 samples across 50 groups. Each scale has cold, no-op, body, API,
configuration and batches 1/10/100/1000. The 1k scale additionally owns fanouts
1/4/16/64/128. Rep0 is the original full first repetition; its five jobs contain
50 measurements. After all five rep0 jobs pass, the remaining 145 shards contain
the other 1,450 measurements. No extra sample or lowered budget is introduced by
that scheduling boundary.

Only after all 150 original shard reviews pass:

```sh
python3 /dev/shm/a217aaae3bde/scale-round5-review/review_scale_fixed_source.py combine \
  --state /dev/shm/a217aaae3bde/scale-round5-review/validated \
  --output /dev/shm/a217aaae3bde/scale-round5-review/final-matrix-review
```

This checks all compact seals and source/build bindings, then runs **unchanged
original `combine`** with 30 repetitions, 30 shards per scale and the registered
capacity profile. Missing/duplicate shards, missing groups, missing global sample
IDs and mixed sources fail. It writes `matrix.json`, complete original artifact
lineage and a separate review receipt. This is an original per-raw validation
followed by original aggregation, rather than a claim that the entire raw matrix
was simultaneously expanded and re-read in one invocation.

The CI aggregate should be independently compared after it is archived. Evidence
directory prefixes differ on different machines and should be normalized only in
that comparison; original native outputs and original compact values stay intact.

## Scope and controls

`streaming-review-controls.json` contains 24 small admission controls, including
wrong source/run/name/size/digest, traversal, duplicate files, symlinks, collisions,
CRC corruption and invalid JSON. A serialization control compares the helper's
digest calculation against already verified **6e3** Rust raw and is explicitly
only an old-source utility control. It contributes zero new scale samples.

After the first actual 599 shard passed, `partial-matrix-control.json` also checked
the real CLI against that incomplete population. It exited 1, wrote no matrix,
reported that exactly 150 reviews are required, and retained the unchanged raw ZIP.

Local hash records establish integrity relative to retained inputs. They are not
an independently signed execution attestation or a hermetic-build claim. The
original acceptance still needs actual complete measurements and relevant current
regressions. Heterogeneous environment strata, disabled semantic/vector scope,
OS page cache, snapshot-only resources, `release_certification=not_run` and
`G8=not_evaluated` remain as stated by the original protocol.
