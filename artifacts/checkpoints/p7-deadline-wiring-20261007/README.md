# P7-013 / P7-014 current evidence (2026-10-07)

This checkpoint binds local author execution to source
`5af7ac0089ee7522e78ff2ce2468f881c8cf70f2` and preserves every captured attempt.
The only new production change is checked GC retention conversion after P7-012.

| Original task | Current result | Closure |
| --- | --- | --- |
| P7-013 | Existing deadline/fault/cache/lock matrix: 57 passed, 1 failed; one separate two-function diagnostic passed | Open pending actual normal-schedule integrated CI review. |
| P7-014 | Four new GC regression functions pass; parameter/runtime/status/default stdio matrix passes; 13-state actual lifecycle passes | Bounded fix accepted; full original scheduling/conditional-owner reconciliation remains open. |

Read the separate [P7-013 receipt](p7-013-receipt.json),
[acceptance map](P7-013-ACCEPTANCE.md), [P7-014 receipt](p7-014-receipt.json), and
[acceptance map](P7-014-ACCEPTANCE.md). The
[independent review](p7-014-retention-independent-review.json) covers only the GC
source/fixture/docs and is explicitly not an independent Rust replay.

`raw-artifacts.tar.gz` contains original byte-for-byte logs, per-run source maps,
stdio/HTTP traces, and the original baseline wiring source. The index distinguishes
the extra git-extracted baseline blob from captured run outputs. Initial shared-cache
misbuild/E0432, canonical red, public deadline failure, and isolated diagnostic all
remain available. No failed attempt is overwritten or omitted.

`source-identity.json` records the 770 tracked crate/Cargo inputs, ten additional
literal repository inputs, source dependency and changed paths. Cargo JSON logs
retain compiler-artifact events; per-run receipts retain actual test/product
executable hashes. These records do not turn a shared target into a hermetic build
or certify its contents after another worker takes the execution slot.

Run `python3 verify-evidence.py` to verify archived/direct bytes and rederive local
counts. Add `--source-root /path/to/checkout` only for a checkout with these exact
fixed inputs. The verifier does not execute Rust or any product binary and does
not approve a later source. The original `verify-lifecycle.py` remains unchanged
as the executed verifier; its historical workspace path is recorded in the script.
The portable evidence verifier rechecks the archived lifecycle rows directly.
