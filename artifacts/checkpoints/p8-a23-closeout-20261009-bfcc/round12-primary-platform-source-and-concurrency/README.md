# Round 12: original source admission, recovery, platform evidence and concurrency

This is an evidence-only checkpoint for source G `c8be5afaac568ffd40ef86d3795423c3b73c9f39` (P `35a7b3412e5fdee9e8ff88951735c06b0e371465`, R `a9706a90a72226cb59c3bfd374ef9c1e3d66cbcb`). The original 192-task ledger remains 163 done and 29 remaining; this commit closes no TODO.

## Included evidence

- Original CI job 113726259355 finished successfully. Its actual 181/181 test summary and selected v15 P/R result are retained with an independent audit and peer review. Actual checkout was synthetic merge `39ae6437c417236f40110f0d18f29aa1ac3db3de`; the complete relevant product and validation domains match G. Other repository paths differ, so this is not a whole-repository equality claim.
- Original mixed C8 and C4 each contain 900 operations. Observed peak concurrency was 7 and 4 respectively. Full raw stdio, resource, 15-table parity and distribution reviews are included. Configured concurrency and achieved peaks are kept distinct.
- The frozen recovery and first seven platform-cell packet includes the original fault, stdio, schema rollback and historical-source identity evidence. The separately observed eighth cell and original collector will be published in an incremental packet; this commit does not substitute seven cells for the required eight-cell matrix.
- Historical source 277f704 is bound by 704 original product paths, including 613 identical Git blobs and 91 independently fetched differing blobs. Earlier reviewer metadata assertion failures and their corrections are preserved.
- Additional original gates transport and independent peer records are included. No new native gate run was performed for this publication.

## Read and verify

`publication-manifest.json` lists the exact public ZIP/member layout, byte count and SHA-256 of every selected input. Unzip each review bundle with normal safe archive handling. The C8 and C4 `.tar.gz` containers are stored directly at the paths listed in `direct_files`; these are new lossless containers of selected original members, distinct from the official GitHub artifact ZIPs. Each selected member is bound back to the original ZIP inventory; three original ELF files per mixed artifact remain referenced at their original artifact locations.

The initial C8 local intake stopped before extraction because its destination had no free capacity. The subsequent RAM intake used the same verified original ZIP and succeeded. Both local attempt records remain visible, and the GitHub workload was not rerun.

All original source identities, task dependencies, repetition counts, deadlines and budgets remain unchanged. Full scale-study and one-hour soak results are still pending. Stage resource samples are not continuous process-tree peaks; unavailable provider costs remain null.
