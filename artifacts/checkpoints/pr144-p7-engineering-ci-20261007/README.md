# PR #144 P7 engineering CI evidence

This checkpoint preserves the successful P7 engineering job independently reviewed at its fixed source. The separate legacy CI failed at its resource-fixtures/TODO-consistency step, as reported by the coordinating reviewer; it is outside this checkpoint's execution scope. At packaging, the coordinator kept 40 original TODOs unfinished and P7-013 unaccepted. This success does not override that failure or certify full G7/live acceptance.

Run: https://github.com/jyqj/codecortex/actions/runs/37653732468  
Job: `112903425598` (`engineering`), all 14 steps successful.

## Verified executed scope

| Group | Passed | Failed | Ignored |
|---|---:|---:|---:|
| Coverage/configuration, hard scope and exact-oracle controls | 14 | 0 | 0 |
| Original deadline/cache/public matrix, 11 targets | 40 | 0 | 0 |
| Original query-encoding unit controls | 13 | 0 | 0 |
| Original absolute-deadline unit controls | 5 | 0 | 0 |
| P8 offline input-lock controls | 15 | 0 | 0 |
| New GC unlink controls + existing GC | 13 | 0 | 0 |
| Real worker contention + existing lifecycle + strategy protocol | 10 | 0 | 1 |
| **Selected total** | **110** | **0** | **1** |

The original 58 are exactly 40 + 13 + 5 distinct functions. All expected identities appear once, with no changed test scheduling, serial retry, assertion or budget. Their 13 source files are byte-identical to the earlier executed source `5af7ac0089ee7522e78ff2ce2468f881c8cf70f2`. The earlier normal-schedule 57/1 and separate serial diagnostic 2/0 stay historical; this fresh combined-source CI run does not establish the old failure's cause. The 58 cases mix unit/service and real product stdio/loopback HTTP coverage.

The single ignored `actual_stdio_local_and_auto_have_equal_effective_budget_and_verified_source` requires an explicit current product path/SHA256 and remains unexecuted here. The separate three-policy actual-stdio target is not selected. Unit logs also disclose 337 and 314 functions filtered outside the selected unit scopes.

The archive retains all six completed public deadline/cancellation observations, including the formerly failing recovery, plus 384 worker request rows and six lifecycle scenario receipts. Those rows/scenarios are observations inside three test functions and are not added to the 110 count. Worker resource attribution stays unknown and no performance, real-provider quality/cost, dense-only, P7-017 isolated-gate or P8 release acceptance is inferred.

## Fixed source and artifact

- PR head: `149aa04f24ddcfd02c3aa5626a59343e88d74866`.
- Actual checkout: merge `1d73d87a602c214ca227ed896fa9abc9ba83c5b3`.
- Both have tree `c19cf91f6455472806b0d08a5408016e262ab5ab`.
- Before/after snapshots are byte-identical; every one of the 776 crate/Cargo paths matches an independent SHA256 reconstruction from the fixed PR head.
- Source manifest: `baf2bd238706a0fd88b91c80357d1780415afd5b917ef56d4bf283f3932e8ee6`.
- Original GitHub ZIP artifact `11496843573`: 217,882 bytes, SHA256 `7a1d199876e6a5c24593096d5406c2195e6c03ac645c05041ad15df5ba2ad2a1`, matching the API digest. Its bytes are retained unchanged inside the archive.

The job used observed Rust 1.95.0 and a new private target/build directory with build-jobs 2, debug 0 and incremental 0. There is no new standalone product-binary SHA256 receipt; the checkpoint does not invent one. Python inputs belong to the immutable full checkout tree, outside the crate/Cargo-only manifest.

## Review and storage

`independent-review.json` is the original 7,708-byte independent review, SHA256 `6c3a2feff8c114bdcbb3d161c39eb15d57c3976926c029b26c5f537fa03f29bb`. `source-bindings.json` connects the reviewed source, run, artifact and narrowly scoped acceptance state. All raw API responses, full decoded job log, original ZIP, all 26 ZIP members, complete expected/actual function lists, source comparisons and the review method are preserved in the archive. `checkpoint-files.json` records exact sizes and SHA256 for every archived/direct original.

No source, original test, task ledger, CI workflow or old evidence was modified by this evidence-only commit. Execution belongs to GitHub Actions; independent review/storage belongs to `/root/pr_audit`. No Rust command, product or workflow was rerun locally.
