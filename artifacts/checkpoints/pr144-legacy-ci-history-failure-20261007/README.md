# PR144 initial legacy CI failure, preserved

This checkpoint preserves the complete observations already captured for
[CI run 37653732411, attempt 1](https://github.com/jyqj/codecortex/actions/runs/37653732411).
The APIs and logs were not fetched again for packaging. All 21 original files
and the original `evidence-sha256.json` retain their exact bytes.

## Observed result and identities

| Item | Observed value |
|---|---|
| Run head | `149aa04f24ddcfd02c3aa5626a59343e88d74866` |
| Three jobs' checkout | `1d73d87a602c214ca227ed896fa9abc9ba83c5b3` |
| Checkout tree | `c19cf91f6455472806b0d08a5408016e262ab5ab` |
| Complete Git crate/Cargo input map | 776 entries; SHA256 `baf2bd238706a0fd88b91c80357d1780415afd5b917ef56d4bf283f3932e8ee6` |
| check, job 112903426046 | failure |
| msrv, job 112903426232 | success |
| security, job 112903426485 | success |

The checkout and run head have identical complete trees. The source map was
independently derived from those Git blobs. The artifact API returned no
artifacts; no runner executable or full build receipt was downloaded.

The failed step was **Fixed resource driver harmless fixtures and TODO
consistency**. Its preceding resource tests (46), plan check, source tests
(65), and v8 source-integrity gate (776 inputs) passed. The original failure
then occurred in `verify_fixed_e3_integration.py:23`, calling
`verify_packing_integration.py:62`. That assertion compared the current task
states with a frozen historical task-state map. The exact four mismatches are
preserved in `historical-gate-task-diff.json`; the frozen manifest still matches
its own original integration's Git task snapshot.

## Executed and skipped scopes

Format, Clippy, compilation of every default target and the default regression
step passed before the historical assertion failed.

| Default regression scope | Targets | Passed | Failed | Ignored |
|---|---:|---:|---:|---:|
| Workspace excluding cc-semantic | 171 | 2258 | 0 | 64 |
| cc-semantic lib | 1 | 233 | 0 | 0 |
| Explicit semantic normal targets | 11 | 83 | 0 | 1 |
| Total | 183 | 2574 | 0 | 65 |

`default-rust-target-results.json` retains each target's original log lines,
counts and ignored reasons. Every target's observed named rows match its
summary. These counts belong only to the executed default step of this run.

The default workspace already executed `cc-server/tests/mcp_stdio.rs` with
**9 passed, 0 failed, 0 ignored**, using the real product stdio transport.
Those nine tests are included in the table above. The later semantic-http
17-test step and both explicit P7-017 default/semantic builders and oracles
were **skipped**. In the earlier default benchmark adapter target, its two
explicit-product tests were ignored. The original local isolated P7-017
failures are not superseded by this run. `stdio-execution-scope.json` keeps
these distinct execution scopes and the observed names.

## Contents

The ten direct original files retain analysis, the trace excerpt, target
results, stdio scope, Git identity and the complete source map. Eleven raw
API/log files are stored losslessly in `raw-observations.tar.gz` with their
original relative filenames. See `STORAGE.md` for the fixed archive identity
and read-only verification command. Storage verification checks preserved
bytes; the initial CI result remains failure. No Rust, product, benchmark or
CI job was run while making this checkpoint.
