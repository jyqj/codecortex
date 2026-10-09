# Exact-E runtime/lifecycle original reports

These are 55 selected original JSON members copied without parsing/reserializing or recalculating measurements. They cover workflow run 37871838957: mixed C1/C4/C8/C16, the existing one-hour soak, and the separate fake-provider backfill; and lifecycle run37871838975. Every member's original artifact ID, ZIP and member SHA256/size/mode, fixed E source, and intended repository path are in delivery-manifest.json and each report-member-manifest.json.

The selected JSON files are not a full sealed native directory. Full raw, SQLite/WAL/SHM, compiler logs, source snapshots and original executables remain together in complete original ZIPs. The parts map distinguishes paths actually published at immutable checkpoint a9d21e731a92c520aaf27258977fa8bead53264d from pending paths. Currently 19/44 required parts are bound to that published tree. PUBLICATION-GUARD.json remains blocked until all original parts and these benchmark files are actually published and independently read back. A planned locator or uploaded unreachable blob does not satisfy that requirement.

No original outcome is rewritten. Mixed configured concurrency is distinct from observed overlap/peak; the soak cache protocol intentionally serializes workload calls and reports maximum1. Its accepted original raw covers3601 successes over3600.053467347s, all2400 compound reads/9600RPC, actual completion-time quarters, and original finite-window RSS/queue rules. Backfill is the seeded fake-provider worker scenario, not paid/live-provider coverage. Lifecycle resource attribution and page-cache/unknown boundaries remain those in its original receipts. Task ledger remains163 completed/29 remaining; all ten tasks remain formally unclosed until their original dependencies and final gates pass.

## Lossless restoration and original checks

1. Obtain each part from its fixed published commit and repository path. Verify byte length, SHA256 and Git blobSHA1 against the reviewed full manifest. Concatenate by ordinal with no separators into the original artifact ZIP; verify whole official ZIP size/SHA256.
2. Extract each complete ZIP safely to a new directory, keeping original member paths and bytes; reject absolute/parent traversal/symlink/special members. Do not extract only this selected JSON subset and claim it has a complete seal. Preserve the original ZIP and one read-only original extraction.
3. From an exact E source checkout, the original seal commands for the restored actual member roots are:

```sh
python3 -B scripts/p8_runtime.py verify --output "$RESTORED/p8-runtime" --build-output "$RESTORED/p8-build"
python3 -B scripts/p8_backfill.py verify --output "$RESTORED"
python3 -B scripts/p8_lifecycle.py verify --output-dir "$RESTORED/measurement"
```

`RESTORED` is a separate artifact root for each command. These are seal checks; they do not manufacture a fresh build identity when the original private Cargo target is absent.

The already-executed independent runtime audit used the original retained p8-runtime-statistics with original plan.json/raw.jsonl, and p8-oracle with copies of original project/ and fresh-full/. It made only the replay binary copies executable, never repaired or rebuilt the incremental database, and compared all15 original table results. Those copies and outputs are separate from read-only originals. Exact commands and actual exit0 are preserved in the fixed helper and audit package SHA256598735634e87e640d25e2906dea1da138a75ba8a81469ad479a127a6677f2291. Lifecycle replay likewise used only a derived input whose1200 source_root paths were relocated to the exact E checkout; original input bytes remain in the full ZIP and the reproducible derivative proof/actual report/exit are retained. No replay or benchmark is performed by this delivery preparation.

The generic future filenames in roadmap09-BENCHMARK section11 are not invented here. The mapping preserves the actual P8 schemas and actual original values, including null/not_run/false limitations. Original task definitions, P8-005 historical subgates, previous readiness snapshots and failure evidence are unchanged.
