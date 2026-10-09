# Round 12: lifecycle, backfill, gates and preserved G2 failure

The current product source remains `c8be5afaac568ffd40ef86d3795423c3b73c9f39`. Original task states remain **163 done / 29 remaining / 0 newly completed**.

## Accepted scope in this checkpoint

| Evidence | Independently accepted observations |
| --- | --- |
| Gc8 lifecycle, run 37901930818 | 1,230/1,230 samples; 431 closed sessions; 1,261 resource entries; 30 cold builds and 400 samples each for reopen, uncached warm and result-cache hits. All original stdio associations and physical disk objects were checked. |
| Gc8 backfill, run 37901930897 | 768 original requests across three seeds, two phases and C1/4/8/16. Original query/progress/DB bounds, provider retirement and final drained state passed. |
| Gc8 gates, run 37901930843 | Six original Rust tests and seven deterministic replays using the retained original ELF. Actual exits were 1/1/1/1/2/2/2; invalid and inconclusive states stayed non-passing, and overwrite refusal preserved the existing report. |
| Original CI security, run 37901930820 | Original dependency audit passed on actual synthetic merge39ae6437, using the already checked relevant-source bridge. This is separate from source-integrity181/v15. |
| Old G2 diagnostic, run 37877604259 | **Failed**: original native status deadline_exceeded, native exit3 and driver/GitHub exit1. Complete retained raw evidence is preserved; incomplete stderr and unknown worker exit remain explicit. |

The old G2 source did not include the FTS changes present in Gc8. Its failure does not determine whether the independently running Gc8 100k primary succeeds. Partial old events are not accepted formal-study samples.

## Original bytes and archive layout

The manifest maps **155 selected input files** to their actual archive members or ordered binary parts. One selected input is a lossless tar.gz containing 2,299 original lifecycle JSON, stdio and fixture members. The five ZIPs are publication containers containing unchanged selected bytes. They are distinct from original GitHub artifact ZIPs.

The lifecycle tar.gz is stored as three ordered binary parts, each no larger than 8 MiB. The original direct upload was rejected by the transport's 16 MiB request-body limit; splitting changed only storage representation. Concatenation was checked byte-for-byte, and every part has its own Git blob ID and SHA-256. The reassembled container is exactly **21,176,927 bytes**, SHA-256:

`0467f68cf95bb9c589279b99a5cdbb6c8ae1383c75ba9d6cdd815312c09432cc`

After checking out this commit, run the adjacent `reassemble-lifecycle-evidence.py` with a new output filename. It resolves the committed part paths, verifies every part and the combined digest, and refuses to overwrite an existing file. `--parts-dir` also supports a directory containing the three downloaded part basenames.

Original GitHub ZIPs, ELF binaries and omitted database copies remain at the locators and identities recorded in the supplied selections. A locator entry does not imply that a large original is included here. Proposed expanded paths in the supplied selections are mapped to the actual archive layout by `publication-manifest.json`.

No source, protocol, primary population, budget or task state changed to create this checkpoint. Stage resource observations are not continuous process-tree peaks; unavailable and disabled-provider cost values remain null. Full scale, complete platform collection, original CI admission and hard task dependencies still govern task closure.
