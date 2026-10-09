# PR181 round 9: actual G4 source-binding verification

The original v15 CLI ran on fixed **G4 `2b60ae7bff2b4b2bf52257bb5cde600fc61c3858`**, tree `3fa58c9bb30371dd7542e080ce3301121d8c71ef`, and **exited 0 in 186.683156 seconds**. The receipt records actual start `2026-10-09T10:55:17.313090+00:00` and finish `2026-10-09T10:58:23.996612+00:00`. Complete before/after HEAD, tree, registry and verifier records are equal; the tracked checkout was clean.

The command was the original `python3 -B scripts/verify_reviewed_source_v15.py --source-version p8-completion-source-20261009-v15`, with the original 3,600-second execution bound. The exact interpreter path and argv are in `G4-v15-cli/receipt.json`. Raw output, the supervisor and its independent result review are included. This records the CLI's actual pass; it does not claim a native unittest run, current-head GitHub CI, unfiltered workspace test, scale study, task completion or release certification.

## Immutable chain

| Record | Commit | Scope |
|---|---|---|
| P4 | `31a42daeb12da6936695abb08eb3912d4f3c6064` | Actual product composed from B4 and main55/#185 |
| R4 | `20efd9664f1be2ca4705bec757505bcb530e7f48` | 32 added archive files; 31 manifest payloads; product, validation and guard unchanged |
| G4 | `2b60ae7bff2b4b2bf52257bb5cde600fc61c3858` | Registry and exactly four verifier binding constant lines |

P4's independent canonical SHA-256 is `79b3830e7ddb7a23074a25ef9e6c267621d0314bb12632c9dc5c2bcff050e11e`, at `artifacts/checkpoints/p8-empty-input-main-55-integration-20261009-50c/independent-source-review.json` in R4. It binds 1,092 product inputs, 139 validation inputs and 53 original-BASE differences. Actual P4 engineering evidence is separately archived in R4: all nine commands passed, with 954 passed test executions and three existing ignored cases across the selected Rust targets.

G4 registry SHA-256: `2770014b30bfce1f8cfde89d661ca43072205c5e942dec071053bee08d9b4a60`. Verifier SHA-256: `5d98075e2a2b99236bd95ed16da891a6a091b19f230006116a1ce846f67f4bba`. Original BASE, source version, static registry identity, validation input map, historical checks, exclusions, frozen files and CI entry remain unchanged. The independent binding review verifies actual immutable P4/R4/G4 objects and the exact closed archive manifest; it does not use a moving branch as the source of truth.

This post-binding directory is added after G4. R4's manifest remains closed and unmodified. No source, guard, validation, task definition, task state, acceptance requirement or prior evidence is changed by this publication.

## Original TODO and study status

**192 total = 163 done + 16 in progress + 12 todo + 1 blocked. Remaining 29; newly fully completed this session 0.** Rounds 1–9 each have 29 remaining and zero original closures. The requested minimum of ten original completions remains unmet.

`latest-study-and-todo-state.json` is a separately timestamped read of existing owners' study metadata and original task requirements. Metadata success alone is not an independently accepted shard or a full study. It does not combine sources, manufacture a new study, cancel/restart owners' work or relabel one source's observations onto P4. Original full-scale and downstream acceptance remain open. The round-8 correction to the overstrong instrumentation interpretation remains in force; no new backend instrumentation architecture is imposed as a completion gate.

Earlier raw failures and scoped successes, including P2's unfiltered workspace exit 101 and original G's CI, remain in their prior archives with their actual source identities. This file does not replace those historical results.

`archive-manifest.json` records every payload file in this directory except itself.
