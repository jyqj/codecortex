## Why

Full snapshot config linking prepared complete symbol/path lookups even when the original scan and typed-config filtering produced no heuristic tokens. Interface dispatch also built full symbol/container maps before discovering there was no interface or implements consumer. These allocations sit on the P8-005 scale path.

## Changes

- Defer full snapshot symbol/path collection until nonempty filtered config tokens need it. Keep signature calculation, scanning, typed config resolution, empty-cache persistence, metadata, replacement semantics and the incremental path.
- Defer interface dispatch lookup maps until interface UIDs and implements pairs exist. Preserve the original CALL → symbols → implements typed-read/error order, prior CALL overlay, stale synthesized-edge cleanup and all positive dispatch behavior. This change saves unused maps; it does not remove the original SQL reads.
- Add two real full-index config controls and eight SQLite interface controls, including malformed-input ordering and cleanup. A follow-up fixes three strict-Clippy error assertion spellings without changing production logic.
- Preserve the original failing executions, complete independent source inventory and PR-management audit under [the checkpoint](https://github.com/jyqj/codecortex/tree/50d9b3bc7d1ecca1b6b1c3822a36c12b546b0529/artifacts/checkpoints/p8-empty-input-preparation-20261009-50c). Rebind the existing v15 registry by changing only its four identity constants and matching registry.

## Exact source and review

| Role | Commit |
| --- | --- |
| Main baseline | `bd4693351149a443bd0054a37508c53f74ed7b52` |
| Corrected product P | `b4fef72211e5967f4fba729d25d0ca2958094fd5` |
| Independent review R | `98fe910f22eb92f8d9f9c8a8043492ba45dc997c` |
| Guard binding G | `50d9b3bc7d1ecca1b6b1c3822a36c12b546b0529` |

Main → P contains exactly four Rust files. The independent canonical review covers all 1,091 product inputs, 139 validation inputs and 45 original-BASE deltas, with inherited-byte verification separately identified from this round's semantic review. R adds 37 evidence files. G changes only the v15 registry and PRODUCT / REVIEW / REVIEW_PATH / REGISTRY_SHA256; original BASE, VERSION, historical proof, exclusions and CI remain unchanged.

## Actual validation

On exact corrected P with Rust 1.95.0, four build jobs, incremental disabled and dev/test debug information disabled:

- `cargo fmt --all -- --check`: exit 0.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`: exit 0.
- `cargo test -p cc-index --all-targets --locked --offline`: exit 0; **590 passed, 0 failed, 3 existing ignored across 23 targets**.
- `cargo test -p cc-eval --locked --offline -- integration_fixtures_and_corpus`: exit 0; named corpus integration passed.
- `cargo test --workspace --locked --offline`: **exit 101**, retained. The unchanged cc-eval `live_child_snapshot_is_attributed_monotonic_and_disappears` test could not obtain its child snapshot. Its independent single-test diagnostic also failed. Local diagnostics observed different namespace PID and /proc views; this is not a proven causal attribution or a test waiver. Remaining workspace steps were not relabeled passed.
- The first product's strict-Clippy exit 101 is retained with its actual source identity; the corrective commit passed strict Clippy.
- The first original native source-integrity suite on G exited 1 (124 tests reported, four import/setup errors; 122 methods printed ok). The original v15 CLI also exited 1 with historical Git input unavailable. These immutable failures are retained. The original historical helpers prohibit lazy blob fetch. Two exact historical CI blobs were missing from the partial clone; they are being hydrated before an unchanged-entry retry. The v13 inventory error is a verified consequence of the historical adapter failing to install. Exact-head remote CI remains separate and pending.

## TODO accounting and coordination

Original ledger: **192 total / 163 done / 29 remaining**. Newly closed original TODOs: **0**. The user-requested ten full task completions remain open. These two engineering repairs and ten new regression tests are not counted as original TODO completions.

P8-005 still needs the complete same-source 1k–100k N30 study and raw acceptance; P8-006 through the downstream ten-task chain retain their original hard dependencies. No scale label, manual scale/soak dispatch, provider expense, cancellation or rerun was initiated here. This PR automatically queued its own seven existing workflows on exact G; their results stay separate from every other head. Existing G275, G9f/Gc8 and other branches retain their separate source/build evidence. Coordination is in #158.

Draft pending final original source checks, the failed full-workspace diagnostic and exact-head CI review.