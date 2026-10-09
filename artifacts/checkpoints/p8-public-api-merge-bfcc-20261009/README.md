# Public snapshot API compatibility repair and current-main integration

Product P is [c92eb5ac7ece70d1f62271d7e2dacac513c285b5](https://github.com/jyqj/codecortex/commit/c92eb5ac7ece70d1f62271d7e2dacac513c285b5), tree `9ae248f8e0c42a8a7389e26e3f44710dfb029640`. Its ordered parents are PR180 `4652cad11dde4b41126544a38fddf25eb2fb7474` and main `55aa2bcf355441585bcf980e1d6f4fab8eebe59d`.

## Why this change is needed

The existing public `SnapshotWriteTxn::write_file_data` can return an error to a caller that catches it and commits the borrowed transaction. Applying dependency batching transitively to that old API changed the surviving dependency prefix: the actual Rust regression observed 64 rows where the original per-row path kept 70.

The repair separates dependency batching from snapshot leaf batching. The old public method retains the original per-row error prefix. The explicit rebuild path retains batching and its contract that the owner abandons failed staging and prevents publication. Existing row/byte limits, statement order and schema fallbacks remain intact. The previous actual Rust RED101 → GREEN0 and controls stay preserved at [cd10beb3](https://github.com/jyqj/codecortex/commit/cd10beb3c2452d6a3269c2c818730d00a026696c), with their original 3ff-plus-patch identity.

This product also preserves the seven product/build changes from main55, including the Codex TOML preservation fix and installer CLI tests, all current documentation/task evidence, and both sides' historical artifacts.

## Independent source review

[The canonical review](independent-source-review.json) independently binds all **1093 product inputs, 139 validation inputs and 59 original-BASE deltas** to actual P. The reviewer did not author the three-file fix. The [complete source-audit archive](independent-source-audit.zip) retains 31 selected originals and their exact selection, including official Git records, tree/commit reconstruction and direct path reviews.

## Actual native validation at P

The [native archive](native-public-api-merge-P-c92e-controls.zip) contains 52 members: 51 selected controller, input, stdout/stderr and actual-exit records plus the manifest. Every command directly used the detached P checkout; all **1245 selected inputs** were checked before and after execution.

| Command scope | Actual result |
| --- | --- |
| cc-db resolution-store integration tests | 7 passed |
| cc-db snapshot/binder/dependency unit tests | 14 passed; 193 filtered |
| cc-index snapshot-leaf owner controls | 6 passed |
| cc-server installer CLI integration tests | 2 passed |
| Workspace formatting check | exit 0 |
| cc-db all-targets strict Clippy | exit 0 |

All 29 executed tests passed, with no failed or ignored test in these selected runs. This was ordinary engineering on macOS arm64, Rust 1.95.0 and SDK 15.4 using an owned APFS clone of a cached target. It does not claim a fresh/cold build, full workspace test run or workspace-wide Clippy.

[The independent execution review](native-execution-peer-review.json), its retained script and actual exit-0 receipt verify all ZIP/member identities, original Git commit bytes, command exits, nonzero test counts and the 12 before/after input maps. [The installation manifest](archive-installation-manifest.json) records exact paths and hashes, including explicit mapping from the unchanged original peer selection.

## Remaining original task evidence

The original ledger remains **163 done / 29 remaining**, with no newly closed original task here. The fixed Gc8 study remains separate at 4/150 accepted shards and 41/1500 samples. P8-007 also still requires original actual mixed-path DB acquisition-wait observation. The [append-only correction](https://github.com/jyqj/codecortex/tree/cd27b4a944a84627dec143888800a2a9967bd41e/artifacts/checkpoints/p8-a23-closeout-20261009-bfcc/round14-scale50k-and-original-contract-correction) preserves all earlier component passes and corrects the earlier only-scale-remains summary.

R records this scoped review and evidence. The existing v15 identity rebind and any final publication merge are separate commits; their actual checks must retain their own checkout identity.
