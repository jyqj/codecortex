# Registered local 1k study: terminal failure retained

The original registered 30-cell local cohort stopped with **7 accepted cells (0–6), 1 failed cell (7), and 22 not-run cells (8–29)**. Its primary-origin registration is unchanged. This study is failed and incomplete; an eventual GitHub result for the same cell cannot replace the chosen failed local result.

Cell 7 completed the workload with worker exit 0 and all 14 measurement groups passing. The original supervisor then received `Directory not empty (os error 39)` from `workspace.close()`. Its unchanged cleanup gate correctly returned exit 2 and `measurement_failed`; the Python CLI returned 1, and the wrapper and sequential executor stopped. A non-author independently replayed the entire original raw protocol and also confirmed that the original `validate_shard` rejects this cell with `shard execution failed or is missing`. A successful raw workload does not override the overall failure.

The original stderr is complete and empty, source and binary identities are unchanged, and both disk checks passed. No timeout, raw output overflow, or parity mismatch was reported. The deeper cause of the cleanup OS error is unproven. Earlier cleanup-success receipts and externally visible residual directories are both retained without substituting residual directory contents for the original final measurement evidence.

Every original file from cells 1–7 and their terminal controls is retained in its numbered archive. Cell 0 was already saved in the parent checkpoint. `terminal-reviews.tar.gz` retains the stopped sequence's original launch and progress, independent failure and storage inspections, and the explicitly suspended, unreviewed, unexecuted cohort-validator draft. Temporary fixtures remain untouched locally and are not part of these archives. The original build remains identified by its Actions artifact and exact digests in the manifest.

Run `python3 verify.py` here to check all eight archives, complete member inventories, exact bytes and modes. The packager additionally compared every archived member against its original local file after packaging. The package helper is the unchanged `../package.py` from the parent checkpoint.

No native workload was rerun after the failure, no original gate or source file was changed, and no successful full-cohort seal was generated. The separate original GitHub-only study continues unchanged. **Original TODO count: 163 done / 29 remaining; this round adds 0 completed TODOs.**
