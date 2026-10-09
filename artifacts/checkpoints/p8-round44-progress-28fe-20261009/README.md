# P8 round 44 progress

Frozen authoritative state:

- main: `379099a4a7b09ebe9928c190017bae53d8d1edf6`
- tasks.json blob: `adb44ee70a4c129bfb443d4ecf678fb934c8ea6b`
- ledger: 192 total / 164 done / 15 in progress / 12 todo / 1 blocked
- remaining original TODOs: 28
- newly completed in this round: `P8-005`
- cumulative progress against the 163-done baseline: 1/10

## P8-005 admission

PR #197 changed only the allowed P8-005 progress fields and the generated plan views. The fixed source `044c008c...` cold study completed five scales times 30 repetitions (150 pairs), its aggregate and complete 150-slot custody ledger were independently checked, and PR #197 exact-head CI finished 19/19 jobs successfully.

The historical scoped-acceptance checklist at commit `51138c236cf2ed47858e2306f9c7bfd3600aaef9`, blob `3792f29446472d98b79ad98eb7a561069998af9a`, explicitly permits the honest retained limitations `strict_paired_causal_performance=not_established` and `whole_tree_memory_peak=unknown` for this scoped task. Those values are not upgraded. Completion is limited to P8-005 fixed-source cold scale coverage and is not V20/G8, semantic, provider, causal-performance, stable-tail, or whole-tree RSS certification.

## Remaining blockers

The original dirty200 run `37962416564` failed its 100k preflight and aggregate; its measurement job was skipped. Therefore P8-006 remains open, and its dependent chain P8-007 through P8-013 plus P8-016 cannot close. The dirty4096 research `37982220020` remains profile-distinct and its 100k preflight was still running at freeze time.

PR #195 has 7/7 successful exact-head workflows and 26/26 successful jobs, including the corrected sequential spool and public nine-field controls. It remains Draft/HOLD because its test merge predates current main/PR #197; it must integrate the fresh baseline and rerun the required binding/CI before admission. It receives no original TODO or performance credit.

## Independent reviews

- cold aggregate/pairing: unattached blob `541dd1cf814f742e4c32bbffea180564f8b5a132`
- custody/ledger: unattached blob `298b555f0f7fe84a16d1d31c7571af583b044606`
- PR #195 admission: unattached blob `138b6517688b47e169e475ecb0a7275fe0de9872`

No workflow was dispatched, retried, or cancelled. No product branch was merged by this round.
