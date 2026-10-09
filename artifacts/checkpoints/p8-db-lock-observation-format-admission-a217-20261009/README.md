# Actual source admission for the LG formatter repair

The original v15 command executed once at G2 `4d18dcdb34b5d7a277566b57801cc882d8b1eb61`, product P2 `b79f7749eb8f0a6c728493adfb073ca7db5563be`, review R2 `87c697343f9d5a465cac7527f2e16933410ccd26`.

It ran from **2026-10-09 12:23:37.117668 to 12:27:57.171114 UTC**, returned **0** in **260.052307088 seconds**, and did not hit the original 3600-second outer limit. The original historical v14 proof actually executed. All **1851 materialized required input files**, including the **187 protected historical files**, had identical bytes and modes before and after. The complete admitted source remains **1094 product inputs / 142 validation inputs / 60 historical BASE differences**.

The existing diagnostic branch was then advanced by a normal expected-head fast-forward from original LG `18255c53b7fa153bb71c96f57f1b97ca139e427f` to G2 at **12:29:42 UTC**. A fresh ref read confirmed the exact target. The original failed LG run `37919399759` remains retained; it was not rerun. The new [diagnostic workflow run 37930392164](https://github.com/jyqj/codecortex/actions/runs/37930392164) is a separate push/attempt1 run and its initial observed state is queued. Repaired-source Rust formatting, compilation, controls and four native mixed loads are not claimed passed here.

## Original records

- [Actual R2/G2 composition proof](actual-G2-composition-proof.json): R2 adds only27 review artifacts; G2 changes only the two original binding files and their four original assignments.
- [Actual command receipt](original-v15-execution.json), [complete stdout](original-v15.stdout.txt), and [complete stderr](original-v15.stderr.txt).
- [Normal branch update and fresh-ref receipt](LG2-diagnostic-ref-update-receipt.json).
- [Fixed source review, original failure log and both repair reviews](https://github.com/jyqj/codecortex/tree/4d18dcdb34b5d7a277566b57801cc882d8b1eb61/artifacts/checkpoints/p8-db-lock-observation-format-repair-a217-20261009).

The larger before/after input inventories are identified by exact hashes in the original execution receipt and retained separately; they are not claimed to be contained in this small appendix. No original task status, acceptance requirement, benchmark protocol or original C study has changed. **0 original TODOs newly complete; 163 done, 29 remaining.**
