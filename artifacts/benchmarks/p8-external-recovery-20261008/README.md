# P8-003 external-input recovery and engineering execution

This package is prepared for a new ordinary GitHub Actions run. It is not an execution receipt and does not close P8-003 or its P8-002 dependency.

It reuses the fixed original Rust importer, scorer and scripts/p8_compat.py from the accepted ffdc6f0f97db78cc25a6c026904e7c2adde05d14 default artifact. It neither rebuilds nor edits the 795 measured source inputs. Product and evaluator remain the actually observed dev builds. No hosted coding task, model service, OCE upload service or holdout input is involved.

## What is recoverable from the disconnected local session

historical-recovery.json records the actually observed six successful old 9ebe import/freeze/validate commands, their two public target commits, aggregate counts and previously observed digest. The original local receipts, original exact argv/suite bytes and uncommitted script are currently unavailable after executor disconnection. This package does not reconstruct them as original raw.

The old query-drift controls never started successfully and remain not_run. Native gold review and actual ranking were not performed in that historical preparation. New results, including any failure, must be recorded separately.

## New fixed inputs

manifest.json binds the original default artifact ZIP, the two actual binary members, original build receipts, three existing Python modules, ten existing Rust scorer files and the original comparison policy. The script verifies all before invoking them.

The new ordinary execution obtains only these public commits in fresh private directories:

- OCE reference: d4f10554a18e31599d1e46d5d56da6588d4aa86c.
- farion1231/cc-switch: 40cac1a68edf8c9e7b3a89125cf40bb93a348404.
- pallets/flask: 22d924701a6ae2e4cd01e9a15bbaf3946094af65.

It checks full Git HEAD/tree, all 1451 tracked blob bytes and modes, clean status, no submodules, and the common uploader/cc-eval input domain. The expected common inventory is 1030 + 229 files. All 269 expected-file patterns must match this domain without overlaps. These facts do not establish the semantic correctness of 200 answers.

The OCE question and answer corpus is temporary external input only. No redistribution permission is inferred from public availability or the inspected absence of a license declaration. The package contains no corpus body.

## Exact execution entry

Copy workflow-template.yml to a separate branch's .github/workflows/p8-external-recovery.yml and invoke its ordinary workflow_dispatch. The template has read-only repository/action permissions. It downloads artifact 11529447454 and uses a separate private work directory and public evidence directory.

Equivalent command after retrieving that exact ZIP:

    python3 artifacts/benchmarks/p8-external-recovery-20261008/recover_external.py \
      --manifest artifacts/benchmarks/p8-external-recovery-20261008/manifest.json \
      --archive /absolute/p8-default-ffdc.zip \
      --work /absolute/new-private-work \
      --public-output /absolute/new-public-evidence

Both output directories must be new and disjoint. Do not upload the private work directory.

The script uses the original CLI forms, resolved from the fixed current source:

    cc-eval import-oce --queries EXTERNAL_JSONL --metadata EXTERNAL_METADATA --output NEW_COMPAT_JSONL
    cc-eval freeze --suite NEW_SUITE_JSON
    cc-eval validate --suite NEW_SUITE_JSON
    python ORIGINAL_FIXED_SOURCE/scripts/p8_compat.py run --lock LOCK_JSON --output NEW_RUN

The last line is invoked through the original run_locked entry inside that unchanged module; its validate/run/replay subprocesses, 600-second command limit, bounded logs/raw, gate handling and zero-exit-without-raw rejection remain intact. It runs actual MCP stdio against the pinned default product.

Budgets are explicitly shared by both profiles and both repositories: top-k 10, repetitions 3, warmup 0, query timeout 30000 ms, seed 20261003 and only {"auto_index":{"enabled":false}}. The wrapper uses its original smoke measurement profile and process-probe setting. There are 1200 planned measured requests overall; actual denominators and failures must be read from the resulting receipts. This is not a tail-latency or release profile.

Preparation now plans ten successful authoring/validation commands: two imports and four freeze/validate pairs. Four additional controls mutate queries in new sibling copies, preserve the original locked query bytes and require the original Rust validator's exact lock-drift error with exit 2. These are new controls, not retroactive claims that the disconnected historical controls ran.

## Mechanical native side report

The compatible imported rows are preserved unchanged. The native side is a separately frozen mechanical projection of original expected-file ordering: first pattern primary grade 2, later patterns secondary grade 1, each glob's actual matching paths represented as alternatives in one group. It has the same source lock, budget and engine config.

Native answer intent, completeness, facets, symbols, spans and query-family independence have not received an independent human gold review. The annotation and public summary preserve that limitation. A measured native score here is a mechanical side report, not P8-002 admission, holdout evidence or certified native quality.

## Actual results and preservation

The original wrapper produces full raw measurement files and runs its unchanged Rust replay before the packaging step. Its actual statuses are retained: zero exit only means a baseline was recorded, and failed/inconclusive/invalid measurements remain non-green. The script does not change the scorer, gate, comparison policy, query budget or raw files.

The public directory contains actual command receipts/logs, build identities, source inventories, suite/import locks, per-profile aggregate results and complete original-file hash inventories. It never copies query JSONL bodies into that directory.

The external-reference-package is a lossless reference package, not the original raw ZIP:

- Each queries.jsonl is an external query-snapshot reference, regenerated from the fixed imported/projection rows using the actual Rust Query field order. The script checks those regenerated bytes against the actual runner snapshot.
- Other files are split only at exact full-question UTF-8/JSON-encoded byte occurrences. Those spans are query-ID/encoding references; the remaining actual bytes are stored as SHA256-addressed payloads.
- The package immediately reconstructs every file into a private directory on the same runner, verifies its recorded SHA256/length and compares the bytes to the untouched original run. The public package is accepted only if all files match.
- The original OCE input and query/gold bodies remain external. Payloads are checked against all exact question encodings used for factoring. No transformed or partial-string corpus reconstruction is claimed.

To restore later, first run the preparation command with --prepare-only into a fresh private directory. This recreates the fixed external rows without issuing benchmark queries. Then use:

    python3 artifacts/benchmarks/p8-external-recovery-20261008/recover_external.py \
      --manifest artifacts/benchmarks/p8-external-recovery-20261008/manifest.json \
      --work /absolute/new-private-work \
      --restore-package /absolute/external-reference-package \
      --restore-output /absolute/new-private-restored-raw

The original pinned evaluator can replay a restored per-profile directory directly. Keep restored raw private because it contains the externally supplied corpus. Restoration is not a new measurement; it must reproduce the original recorded hashes.

If input, measurement, gate or package validation fails, retain that result and fix the actual cause. Do not replace a failed run, edit expected hashes to green it, or count this prepared script itself as completion.
