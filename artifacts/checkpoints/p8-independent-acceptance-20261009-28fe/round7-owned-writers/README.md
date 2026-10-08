# Runtime owned-writer sealing correction — round 7

Fixed product: `362a8216537dd1df614077abc6b6e2f98ef8a054`. The only changed implementation/test paths are `scripts/p8_runtime.py` and `scripts/tests/test_p8_runtime.py`.

Independent real-process controls reproduced seven premature-sealing cases in M: a live sampler, a live reader or exit watcher on either product, and a construction receipt failure after either real child started. Six cases changed RPC logs after `run()` returned. The corrected controller leaves all seven cases unsealed, keeps the raw stream open for surviving owned writers, and preserves the existing unfinished-worker diagnostic. Normal completion and a stopped process with a nonzero exit still retain a valid sealed archive.

The final integrated P8 control suite passed 381/381 in 69.178 seconds. `receipt.json` records the original execution HEAD and separate before/after hashes; it does not claim that execution originally ran at the later product commit. The two fixed blobs were independently matched to the published product.

`raw-controls.tar.xz` retains 4,797 original files with exact bytes, including before/after release observations and source snapshots. `raw-controls-index.json` binds every member; archive round-trip verification passed. `execution-provenance-appendix.json` explains the temporary test-definition snapshots and identifies its helper provenance as a retrospective statement. The original reproducer, observations, failure results and independent reports remain unchanged.

Constructor pending flags prevent a false complete archive; they do not claim to recover a Product handle lost during failed construction. The counterexample harness owns and stops its synthetic children after observing the controller return. These are controller tests, not measured product workload evidence.

No original roadmap task changes status here: 192 total, 163 done, 29 remaining, session formal completion 0/10. Full scale and original task dependencies remain required.
