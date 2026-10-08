# Combined P8 repair publication

The independently reviewed repair supplement now includes the owner's fixed
`599a7050e7d52b5b7b93975c419138e175b3f754` candidate. The combined six Python and
control paths passed all **371 P8 Python tests**. The full inherited v15 source
proof then passed on the actual published admission commit in **200.856 seconds**.
Task-plan, declared-facts and working-diff checks also passed.

## Fixed publication identities

| Role | Commit | Tree |
| --- | --- | --- |
| Combined PRODUCT | `e52bf0b3c7abd9fca42e5384cdf88f4e33f96768` | `5173ebb5ffdb96ddc71bfcd9bac8099cd40efc05` |
| Independent REVIEW | `5b7fe1a980c130a73739a15105a0343392585cc8` | `20478d9e444df9ea018731bc102aa8c5bbd616b3` |
| Admission PINS | `ef6d772aff2756a94c129c2a69d982b4084cda49` | `e3b279def5ca0ea297a9870bb132cf554b06a289` |

PRODUCT has two ordered parents: the previously published supplement
`3f7da3bae78b5b5e1d804a722f215c56b24e2280` and owner candidate
`599a7050e7d52b5b7b93975c419138e175b3f754`. REVIEW is its direct child; PINS is
the direct child of REVIEW. This evidence archive is appended after PINS.

The main review SHA-256 is
`f85564c69fc8c8adb9b4cd137c19035d57ec6f22384e6aaab6bf0931da59b7b0`.
The registry SHA-256 is
`41c55b45d90a6629154ca6636443d19288a2a0bc63a04e5678ee9907954b6061`.

## Source and verification scope

All 1,087 Rust/Cargo inputs equal the owner candidate. The owner doc-key index,
scale checkout fixes, strict observer/producer contracts and existing source
reviews are retained. The supplement changes six existing validation inputs;
the full validation inventory remains 136 paths and the BASE-to-PRODUCT Rust
delta remains 30 paths.

The three conflicts were resolved in cold-build collection, rollback manifest
handling and platform controls. Collection retains strict selected-cell and
receipt/target ownership checks and writes explicit failure receipts. Rollback
retains strict map/list parsing, exact digest/count checks and hash/parse identity.
The runtime repair preserves owned terminal outcomes and drains work before
sealing evidence. Existing control cases from both parents remain present.

The [combined execution archive](../merge-599/) retains the original 59-test
platform attempt with 11 read-only-fixture permission errors, the passing
rollback controls, the fixture correction and the complete 371-test passing run.
The six recorded source hashes match PRODUCT by independent Git-byte readback.
Those original logs retain their actual pre-commit execution identity.

PINS changes only the registry and guard admission constants. The v15 version,
base, acceptance predicates, exclusions, historical proof and CI selector are
unchanged. The original review history and evidence indices remain available.

## Actual published-identity source proof

[source-proof/receipt.json](source-proof/receipt.json) records execution at PINS,
the three bound input hashes, all four commands, their exact exits and log hashes,
and unchanged inputs and HEAD after execution. The receipt SHA-256 is
`5164f537244685721abf439b3bfff314d33b8f532dfdcf416a6c9c27a446c611`.
The complete v15 log SHA-256 is
`a05b7d6bb041347ef9992008ec2aaac09ccf8a8e6c16970ad7199991f7c27c09`.

The independent metadata and published-chain reviews in this directory preserve
their original review times and scopes. The later proof readback is recorded
separately; static approval is not relabeled as a runtime test.

## Original task accounting

The task plan remains **192 total / 163 done / 29 unfinished**. This repair and
publication complete **zero original TODOs**. No original task status, dependency,
acceptance criterion, sampling population, deadline or performance threshold is
changed here. Full scale and the dependent runtime, recovery and release task
acceptance still require their own actual evidence. Each historical run retains
its original source; observations from different candidates are not pooled.
