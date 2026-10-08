# Public DEV recovery checkpoint

This is a **new recovery package**, not a recreation of missing historical bytes under old names or hashes. It preserves the exact known author/reviewer pins and honestly separates prior reported checks from work actually repeated after the executor disconnected.

The local author commits remain unavailable through the repository's current GitHub refs. In particular, exact queries/gold spans, the 238-file selected subset list, author generator and 37b global-component review bodies are **not present in this package**. The old fixed objects may still survive in the disconnected workspace. No public candidate or review is fabricated here.

## What has actually been recovered

- Frozen Serde and Vite upstream Git commit metadata and their complete recursive tree inventories. Every subtree and the root Git SHA1 were reconstructed from modes, names and child object IDs.
- Five exact upstream license blobs, checked against original Git blob IDs and SHA256. Serde is MIT OR Apache-2.0. The create-vite license includes distinct CC0-1.0 template scope; the templates must not be described as MIT-only.
- Three unchanged original protocol files from `5385f5a7a2a875c6d5cbd049bdde039bf71bbf32`, matching the previously recorded hashes.
- The deterministic first-20 reservation calculation: 14 Serde DEV families and 12 Vite DEV families. The remaining 6+8 slots stay reserved, unread and undrafted.
- A restoration entrypoint that uses only Python's standard library and Git unless explicit original format-check replay is requested.

This entrypoint does not execute recovered source, contact a provider, run benchmark rankings, launch a cloud coding session, or access holdout bodies.

## Run the metadata check in ordinary Actions

From this package, with a new output directory:

```sh
python3 recover_public_dev.py --output "$RUNNER_TEMP/p8-public-metadata-recovery"
```

This verifies current packaged tree/license/protocol bytes and reservation metadata. A successful result explicitly says that the original public payload is still pending; it grants no source/gold/family admission and closes no task. Use the actual command exit status and archive stdout, stderr and `recovery-result.json`.

## Recover the original public bytes if the old Git objects return

Point at the original Git repository that actually contains both fixed author `b3565a61cea32a67e96963902aa89caa48e35f86` and reviewed import `f61b33a2ff2ccfb412f92b9aba70ac1b6673ee8d`:

```sh
python3 recover_public_dev.py \
  --original-git /path/to/recovered/codecortex \
  --output /path/to/new-public-recovery
```

This is read-only against Git. It reads only the fixed public intake prefixes and specifically pinned review files, refuses protected/non-DEV names and unknown query shards before reading them, and writes recovered bytes to a separate new directory. It validates the original 26 native/26 compat DEV family populations, 224 indexed sources +5 licenses +9 excluded sources, 238 stored blobs, 2,555,289 stored bytes, all 32 revised gold byte ranges, exact source/gold review hash and the disclosed three mode differences. Original files remain untouched. The recovery copies bytes and records original Git mode metadata; it does not chmod copied files and does not promise a mode-complete checkout. It does not silently fetch, broaden the original source selection, apply new global annotations or mark author rows accepted.

If original global review commit `37b0a99402750651caa84e171e71cccb1d47daa2` is available, its three files are recovered only if their original SHA256 values match. Otherwise the report preserves that gap. The 305-component map is a prior independently reviewed proposal, not a regenerated substitute.

For the exact previous format-only check, explicitly install `jsonschema==4.23.0` in the Python environment and add `--check-original-format`. The bundled original checker runs over the four recovered revision-2 public shards. Its unchanged result remains `format_checks_passed_not_source_gold_review`; no `--custodian` path is provided. This does not substitute for the still-unexecuted 12-shard canonical projection or independent family/source/holdout admission.

## Scope that remains open

The 26 candidates and 32 spans were previously source/gold reviewed, and 327 combined public DEV rows were previously reviewed into a proposed 305 correlated components. Those fixed historical checks are recorded as claims with their original hashes until the exact old files can be restored. Twenty-six new family labels form 25 new components because the two manifest-location tasks merge; this is not a claim of statistical independence.

The earlier canonical-projection patch/run never returned a result before the executor went offline. Its completion is unknown; there is no verified projection commit or original full-checker run for it. The nine native and seven compat annotation changes are recorded as a pending exact plan, while IDs/questions/gold/split remain unchanged.

The source snapshots are text subsets, not executable clean upstream checkouts: three Vite files retain original author mode 100644 although upstream is 100755. A genuine later checkout must preserve upstream modes and dependency closure. The package does not execute these files or repair the historical snapshot in place.

This work does not complete P8-002, certify the original frozen family/admission protocol or its approximately-600 coverage plan, access or manufacture holdout custody, or change the current P7 candidate's 795 inputs.
