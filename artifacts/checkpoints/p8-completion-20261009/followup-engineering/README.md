# P8 follow-up engineering originals

This archive preserves the actual engineering originals behind the three local follow-up commits. It does not assert that a product workload, new-source CI, TODO, or release has passed. Original files and their embedded paths, source identities, statuses and errors have not been rewritten.

| Identity | Exact value |
| --- | --- |
| Public prerequisite G | `bb9a96d71622458c39a143055360cc97f0d11d78` |
| Backfill flag repair | `88d447390fa8a7dd04d9a6679fe90c1477bfa280` |
| Runtime repair | `d5d4d570d97b59cda29a55251db875235ff6e507` |
| Collector repair / local tip | `d0fe84a3e3f556c4f5907cc62190b20902a9f84d` |
| Published source P3 | `b11addeed93b02c7b840bd3b61a452ce0e671287` |
| Whole tree shared by local tip and P3 | `eaabe503cf1439894326c94ab0f964a1e3fad7f8` |

`engineering-originals.tar.gz` contains the complete selected directories below `evidence/`. Relative paths after that prefix reproduce the original scratch-root layout. `manifest.json` records every original file's bytes, SHA-256, mode and category, plus all directories including empty outputs. Absolute paths inside old receipts remain historical references. `archive-verification.json` records the packager's complete read-back verification and recheck of the untouched originals. `DELIVERABLES.json` identifies every other delivery file; its own hash is supplied by the publishing commit or external delivery receipt, avoiding recursive self-hashing.

Verify the complete compressed archive, all members, and all listed delivery files without extracting or executing evidence:

```sh
python3 verify_archive.py --directory .
```

An existing Git repository containing public G can additionally verify the source bundle prerequisite and objects:

```sh
python3 verify_archive.py --directory . --git-repository /path/to/codecortex
git -C /path/to/codecortex bundle verify /absolute/path/followup-local-commits.bundle
git -C /path/to/codecortex fetch /absolute/path/followup-local-commits.bundle refs/heads/task/p8-runtime-followup-20261009:refs/remotes/followup-archive/source
```

The bundle retains exactly the three local commits listed above, excludes history reachable from G, and has G as its only prerequisite. It preserves actual local author/parent/tree provenance despite the published source being a separate whole-tree-equal commit. No source or guard approval is inferred merely from importing this bundle.

The runtime records include the original three failing controls, fixed-control red reproduction against immutable original code, successful protocol tests, and the final 356-test Python result. Earlier sparse-checkout fixture failures remain present. The thread tests use explicit protocol fakes with actual executor threads; they are not product load measurements. Raw-budget exhaustion can omit raw terminal rows. A worker whose termination could not be confirmed remains explicitly unsealed. These limitations are part of the evidence.

The collector records include the original real CLI empty-input refusal with an empty newly created output directory, the fixed CLI refusal with a retained invalid matrix receipt, and 58 strict platform controls. The earlier uncommitted draft's `not_run` counting was rejected in review; its log remains labeled superseded. Final failure counts are unknown (`null`), not inferred producer results. Existing directories and symlinks are never written.

Both independent-review directories include their original frozen script exports, command/results, audit files, and retained thread fixtures. Their reviewed source identities are d5/d0. The actual Cargo 1.95 feature-label comparison retains its explicit dependency-free fixture, commands, original Cargo logs, and two small original test executables. It proves `[default, semantic]` versus `[semantic]` selection; it is not a product build or completed backfill run. Re-running its script should use a fresh separate directory, because original logs and targets must remain untouched.

The c709 source-approval records retain all three original commands. The original full suite remains **failed** after 139 passing methods and three setup errors caused by exhausted disk space. The first missing-42 attempt remains **not run** for capacity. The second attempt ran the missing original methods in three processes (21, 7, 14), and the combined coverage report accounts for 181 distinct original method IDs. This is not a rewritten successful single full-suite invocation. Selected v15 and historical-gate results retain exact c709 identity; none are relabeled as P3 execution.

The scale input review proves unchanged source inputs and acceptance rules between G and d0. It does not substitute for the original G run's full raw/capacity verification, change its source identity, or certify a P3 run. Original G scale build/shards must be replayed from exact G.

Signed artifact URLs, unrelated peer PR metadata/downloads, heldout corpus bodies, and regenerable Cargo caches are excluded. The two retained Cargo protocol executables are the only selected files inside their old target directories. New formal G recovery evidence and future P3 runs belong in separate records.
