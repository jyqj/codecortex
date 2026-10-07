# v9 CI entry and current PR management review

## Fixed entry migration

`entry-review.json` independently reviews source commit `5636f0dbbdffed00a76cf22c55bf754dda0f4465`. Its accepted scope is the four-file CI entry migration only. The replacement `verify_historical_integrations_v2.py` is absent from that commit; its implementation and complete gate still require separate review and execution. No complete v9 historical gate or repair of the legacy CI failure is claimed here.

The guard keeps the same PRODUCT `7b1650c1f6475843d568b2649ff84ec9bdd0e613`, six reviewed deltas and 776 crate/Cargo inputs. Registry JSON changes only `source_version`. Guard literals change only VERSION and the resulting registry hash. CI bytes independently reconstruct exactly from fixed BASE `886f90a542a6174a037c79eebbb4f74848fb1f53`, with one selector migration and one historical entry migration; the prior 19 default-product path substitutions remain strictly enforced.

All nine historical helper/registry files match BASE bytes. All 9,705 previously tracked entries under `artifacts/checkpoints` and `docs/checkpoints` at PR head `149aa04f24ddcfd02c3aa5626a59343e88d74866` keep their Git blob/type/mode identity. Existing v4-v8 records remain unchanged. Static controls reject fallback to the old entry and replacing the new entry with `true`; this reviewer did not rerun the coordinator's source-integrity tests.

## PR snapshot, 2026-10-07 17:05 UTC

The authoritative all-author REST open collection contains **44 PRs: 43 retained old PRs plus #144**. The earlier cleanup snapshot had 46 retained old PRs. #10, #11 and #111 are now closed without merge, at 15:32:20, 15:32:22 and 15:32:24 UTC respectively. The snapshot records this later state without rewriting the earlier cleanup evidence or attributing its actor.

#144 remains open, non-draft and mergeable against main at head `149aa04f24ddcfd02c3aa5626a59343e88d74866`. Main remains `6d02d77f018a5965a6f289b0b43558ed4b9f8322`; #142 and #143 are merged. No new external PR, retained-head change or new main commit was observed.

Across all 46 formerly retained heads, the returned first-page PR-triggered Actions run IDs/statuses/conclusions are unchanged from the prior audit. The only mergeability transition is #76 from `False` to `True`; #3 remains the sole open PR currently reported unmergeable. Mergeability is relative to each PR's own base, often a stacked branch, and does not approve adoption into main. #127/#133/#137/#3/#4 remain open; no independent feature was adopted by this review.

`pr-management-snapshot.json` preserves the complete 46-row comparison, 43-current-old/open set, CI run IDs and exact head/base identity. It is an observation snapshot, not a promise of future state. The reviewer made no remote changes, requested no reviews, and did not push, close, merge or rerun CI.

## Independent P7 evidence-storage review

`p7-ci-storage-independent-review.json` preserves `/root/todo_audit`'s original scoped storage review of evidence-only commit `517d28e251b19ed8223da52f48e0cc4bf64634e1`. It confirms seven Git files, 47 original objects (45 archived + two direct), unchanged original GitHub ZIP and unchanged 7,708-byte P7 execution review. The independent verifier ran once; no Rust, product or 58-case rerun occurred. The existing P7 checkpoint and its manifest are not modified by this addendum.

The P7 engineering success remains separate from the reported legacy CI failure. At this snapshot the coordinator retains **40 unfinished original TODOs and P7-013 unaccepted**. Entry or storage review does not change those task states.

## Exact report identities

| File | SHA256 |
|---|---|
| `entry-review.json` | `4855379da75678b80eb0c3b0292a7825b9cee04b0420d952960b5baac3fe47c4` |
| `pr-management-snapshot.json` | `5157ed4d3162f997cabcba86132ab782be3ad347705fdb5f9b31ed0548a72fa8` |
| `p7-ci-storage-independent-review.json` | `03877c118d2b2450094591bd9d93fbb960782ec03a48757c7ae7e5fa19aba60f` |

This commit contains evidence only. It changes no product, CI, helper, registry, original assertion, budget, historical evidence or task ledger.
