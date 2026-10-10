# Round 31: terminal originals, bounded observations and PR199 publication

Current main remains `09d4454fa45455dc5a8bfdfec67e31bb7ed162c7`: **192 original TODOs, 164 done, 28 remaining; one newly completed in the active task (P8-005)**. This checkpoint adds zero original TODO completion credit.

## GW4 is terminal and incomplete

Original source `008f1ec0ae16b9b6aff9f20d033b17b2219168f8`, run 37982220020, attempt 1 failed. The original 100k rep0 job 114000097090 has no uploaded measurement artifact. Its terminal job API still leaves the execution step in progress and the upload step pending. The complete job-log endpoint returned 404 BlobNotFound; annotation content is unavailable through the supported connector. The exact cause and native child exit remain unknown.

The original aggregate job 114077867196 exited 1. Its unchanged failed matrix lists exactly 146 missing slots: five sizes × reps 1–29, plus 100k rep0. The JSON has no sample_count field. Accepted results stay **4/150 shards and 41/1500 observations**. The complete 580-byte original failed-matrix ZIP, its sole original member, full aggregate log, official API/access responses and an independent terminal-boundary review are retained in the sibling GW4 directories. No original study was restarted or combined with another source.

## PR191 finite equal-lifetime observation

Run 38007966044 / attempt 1 at source `1ebb6208f2f5f8fe8a5bf237c00a3d963f44bb08` completed successfully. The frozen nonauthor reader actually exited 0 once. It accepted all 15 original ZIP members, fixed source/toolchain/producer identities, the retained actual ELF bytes, and all 80 pairing records. Complete original archive bytes are stored as five ordered binary chunks with an exact reconstruction manifest; the original full job log is retained.

All 80 pairs and all 17 slower pairs remain visible. Ratios are paired retained/original nanoseconds:

| Case | All-pair median | Original first | Retained first | Slower pairs |
| --- | ---: | ---: | ---: | ---: |
| single_plain | 0.987809 | 0.906428 | 1.133664 | 10/20 |
| short_plain_6 | 0.923640 | 0.912892 | 0.976580 | 2/20 |
| mixed_32 | 0.922615 | 0.910464 | 0.926241 | 3/20 |
| compressed_64 | 0.986978 | 0.984446 | 0.990976 | 2/20 |

Every single_plain retained-first pair was slower. Default adoption remains held; there is no universal-speedup, whole-build or scale completion claim. Typed equality is supported by the original fixed Rust full assertions and actual successful child, plus one reference digest and two assertion booleans; the archive does not contain each side's full typed-row dump or independent output digests. Original v1 observations remain separate.

## PR199 actual main integration

PR199 was actually opened against main `09d4454…` at head `4786c1457b47c5baf4c09d32b96048795e863111`. It composes the exact borrowed oracle implementation/correctness controls with the minimal V18 mock-request/status barrier fix. Original source and task-plan admission succeeded with unchanged input identities; those full records were published in the preceding checkpoint.

Actual test-merge `d51e4d4ad298b2f883225fd847c5fe213cc53bc7` has parents main09 and candidate4786 and the exact full candidate tree. Publication and actual Git-identity records are retained here.

The finite 00:20 and 00:27 UTC observations remain historical observations. At 00:27, 27 checks comprised 14 success, 2 conditional skips and 11 running, with zero failures. The closeout V18-containing step had actually succeeded, but the complete closeout and main jobs were not terminal; the one-hour runtime soak was also running. This snapshot grants no future CI or merge success.

## Next source work

The root has authorized preparation of a separate 100k diagnostic using unchanged native workload and population settings, with bounded uploads of immutable original progress prefixes during execution. This work has not yet been published or executed. It will not replace either original failed study or count diagnostic samples as completion of the registered 150-shard cohort.
