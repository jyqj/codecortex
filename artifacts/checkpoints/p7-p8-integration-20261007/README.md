# P7 / P8 integration verification — 2026-10-07 work batch

This receipt records the final integration source check before updating PR #145.
The actual checked-out commit was `1557ce8d1ea023d280f5d696ba7d2da18fc4d957`.
The attached command output and JSON receipt record a passing direct source guard
(`exit_code=0`, 67.078 seconds); they are source-integrity evidence only.

## Immutable source and review

- Latest main integrated: `b951f27d3ed50b7755bc2456c6425355f753ec17` (PR #144).
- Complete product source: `d77a2143cdb82e722b1d1c62851298c43e707b65`,
  containing 785 protected inputs.
- Isolated joint P7 strategy / P8 engineering source:
  `3359647e81ea73b9ee98a56d6753f1cfa741b3d2`, containing 781 protected inputs
  against its own base, with exactly 22 reviewed changed paths.
- Independent joint review: `468a4f7f79bc7c933b6030eb3f27c68674afb2a7`;
  review JSON SHA256:
  `1e10f8153fb8bd4ca7a6e959fe6b0060103e8a6b311f16a708257bfc1bd9e4ce`.
- Active source selector: `p7-p8-engineering-20261007-v10`.
- Registry SHA256:
  `195fa37b20130e53f3417f8076eb608f18e3b3a98c9206907ba0ce9ab31b3d8a`.

The registry retains five approved groups from main without changes and replaces
the overlapping strategy/P8 groups with their independently reviewed union.
The six active groups cover 39 distinct changed paths and reconstruct the full
785-input product. The isolated 781-input source is not the complete product.
Historical reviews remain preserved. The explicit-base, before-byte, disjoint
path and full-product checks remain enforced.

A separate workspace agent reviewed the exact final source/helper/CI bytes on
`1557ce8d`: the five existing groups, joint review pins, private binary paths,
historical-v2 wrapper migration, and original CI steps/negative controls were
preserved. The CI additions are the P8 release-evidence tests, P8 facts check,
and the v10 source selector. This review did not run Cargo or GitHub CI.

## Task accounting

The merged task ledger has 192 tasks: 152 done, 12 in progress, 27 todo and
1 blocked, leaving 40 unfinished. Its SHA256 is
`f0758315964411384e8802cbde907c2eb374ea79c87d5b3650870d71be10d786`.
The ten P8 tasks advanced by this batch remain in progress; the two additional
done tasks came from PR #144. Original acceptance criteria and dependencies
were preserved. The four generated task views and P8 facts checks passed.

## Runtime validation provenance

The original combined 33-test pass and scoped strict Clippy pass apply to
`853385b7ccb2780818f9f8e8a83791f1c197efcf`, before the compatibility fix and
latest-main integration. They must not be relabeled as tests of this final
product source.

The first PR #145 CI run (37656213598) failed strict Clippy because Rust 1.99
deprecated `AtomicU64::fetch_update` in the new load harness. The replacement
uses a checked compare-exchange loop without suppressing warnings or raising
the MSRV. A local retest on `d911a0573e5a53ac20073f050f55503c5b2874c3` passed
both new reservation tests. Its existing load suite had eight passes and one
failure in the worker executable-digest assertion; the failure's cause remains
unconfirmed, and the full raw evidence is retained separately. No test was
skipped or relaxed. Workspace formatting passed after the main integration.

The new GitHub CI and P7 engineering workflows are pending at the time of this
receipt. This receipt makes no claim of a full-scale run, sustained soak, real
provider benchmark, held-out evaluation, installation certification or release
certification. Later workflow results must retain their own actual source and
run identifiers.
