# Round 14: fixed-source 50k evidence and original DB-wait contract correction

This is an append-only evidence publication. It does not change product source, the original task ledger, any workload, or any earlier pass/failure record. The original task count remains **163 done / 29 remaining (192 total), zero newly closed here**.

## Correction to previous completion summaries

This README and the retained correction report supersede the **“only the complete scale study remains”** summary in the previous round13 soak README and task maps. The old files remain preserved as historical statements. The unchanged original `09-BENCHMARK.md` §9, line 175 requires DB lock wait observation in the actual C1/4/8/16 mixed path. **P8-007 still lacks that actual acquisition-wait evidence**, in addition to its unchanged dependencies.

The retained mixed/backfill/lifecycle/soak component validations remain passed in their demonstrated scope. Client dispatch/queue/service/end-to-end timings do not isolate internal DB acquisition wait. The original backfill once-per-seed DB availability probe ran before the held-query waves; its pool checkout and combined BEGIN/ROLLBACK timing does not replace actual mixed-path acquisition measurements. No new full-backend tracing, latency threshold, four-C one-hour workload, or formal150 gate is introduced.

## Fixed original scale evidence

`scale50k-selected-originals.zip` contains the exact **32 selected files (12,668,696 bytes)** plus the original **`third-shard-50k-selection.json`**. It is a new transport container of retained files, **not the original GitHub artifact ZIP**. Its paths retain the `scale-intake-primary/` layout. The original source is **Gc8 `c8be5afaac568ffd40ef86d3795423c3b73c9f39`**, run **37902429727**, artifact **11610565832**, 50k repetition 0. The original owner has accepted **4/150 shards and 41/1500 samples** across that fixed study. This package adds no samples, does not rerun its original validator, and does not mark the complete study accepted.

## Reviews and unchanged existing ZIPs

`original-contract-and-independent-reviews.zip` holds the exact 5-file original-contract audit, 9-file public implementation discovery, 3-file task-map correction, and 2-file existing native peer review, plus all four original selections: **23 members**. Group layouts are `pr-triage/db-lock-contract-review/`, `pr-triage/db-lock-implementation-followup-round14/`, and `workspace/scale-intake-primary/...`. The original selections retain their original bytes and target paths; this packaging manifest explicitly maps them into this new publication container.

`todo-audit-pr180-safe-integration-plan.zip` and `official-current-merge-trees.zip` are reused byte for byte, not extracted into or repacked inside another ZIP. Their SHA256, Git blob and complete CRC/member-read receipts are in the manifest. The public discovery report records that the a217 DB-observation workstream had not yet supplied a locatable published implementation commit in its checked scope; it is not a review or acceptance of that implementation.

## New product/native preparation is a pending snapshot

`root-round14/product-and-native-preparation.json` is the exact frozen **2026-10-09 10:46:04 UTC** observation. It records actual P **`c92eb5ac7ece70d1f62271d7e2dacac513c285b5`**, tree **`9ae248f8e0c42a8a7389e26e3f44710dfb029640`**, with ordered parents **4652cad11dde4b41126544a38fddf25eb2fb7474** and **55aa2bcf355441585bcf980e1d6f4fab8eebe59d**. P preserves current main and the public snapshot API-prefix repair; it is **not** the DB-observation implementation. Native preparation actually exited 0. Native controls were only started in that frozen record: **no terminal controls result, R/G acceptance, cold-build, performance, or task-closure credit is claimed here**. Any later result must be archived separately with its actual source and identity.

## Byte-preserving packaging

Every selected input was checked against its original length/SHA256, all new and reused ZIP members were fully read for CRC and SHA256, and selected inputs were rechecked unchanged after packaging. `manifest.json` holds the complete member/selection identities. `upload-index.json` lists exact repository paths, bytes, SHA256 and Git blob IDs. Each upload item is at most 8 MiB; if splitting was necessary, the manifest lists ordered offsets and verified reconstruction. This preparation performed no Git mutation, workload, validator replay or sampling.
