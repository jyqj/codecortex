# P8 completion work, 2026-10-09 identifier

This directory preserves the original engineering records for the current PR
and TODO work. The identifier is a run name; timestamps inside each receipt
record when its observation actually happened.

## Engineering originals

`engineering-originals.tar.gz` contains 679 unchanged files: full recovery,
actual schema 25/24 binary rollback, lifecycle smoke attempts and fixes, the
original scale tests, the 60-file smoke, and the unsuccessful first 1k run.
It also includes the task branches as a small incremental Git bundle whose
only prerequisite is the existing base
`7354db236c9d9850a75f31672697ae9eab44565e`.

Archive SHA-256:
`b2e77d94d5a43ef3fee09f9462f8024fd3b518cec5849727e6135123f356ab61`.
The archive's internal `manifest.json` records every original path, size,
SHA-256 and evidence classification. `SHA256SUMS` covers the manifest and
all preserved members. The adjacent delivery manifest records the archive
inventory and review limitations.

Each record retains its actual source and observer identity. Unavailable
early observer bytes are explicitly documented. Later fixes do not relabel
those attempts as executions of the final PR head. The initial lifecycle
32 KiB SHM accounting omission and the original 1k resolution failure remain
in this archive. Large executable and Cargo target bytes are excluded; their
original receipts remain unchanged.

These engineering records do not certify the final candidate, complete a
roadmap task, or establish release approval. Final frozen-source matrix runs
and their independent reviews are recorded separately.

`scale-preflight-originals.tar.gz` separately preserves 29 original files from
the fixed `803773e4c0f3364364800e07ac6f484a41891d51` build and preflights.
The same original 8/128 plan passed all 14 groups at 1k, while the 10k run
finished with exit 1: cold/no-op passed, and all seven mutation groups remained
incomplete after 129 actual builds. The adjacent JSON records the archive
digest and its exact scope. Neither run is relabeled as a new-capacity result.

## Disposable hosted-runner capacity

The scale-capacity profile registers `256 KiB × files + 4 GiB` of available
space on the filesystem that will own its fixtures. The scale runner checks
this requirement again immediately before execution. This is a capacity
estimate based on the 10k observations, not a measured 100k peak or an
assurance that all 100k phases will finish.

GitHub's [standard hosted-runner specification](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)
lists 14 GB SSD for the Ubuntu labels and a fresh VM for each standard job.
Therefore the workflow does not assume that the large profile fits. Its
explicit preparation step first measures actual free space. Only on this
repository's fresh GitHub-hosted Ubuntu 24 VM, with the fixed source verified
and both activation controls present, it may remove the two unused preinstalled
SDK directories `/usr/local/lib/android` and `/usr/share/dotnet`, stopping as
soon as the registered requirement is available. This reclaims existing VM
space; it does not purchase or allocate a larger runner.

The helper refuses self-hosted machines, the shared workspace, symlinked
ancestors, caller-supplied deletion paths, and overlap with source, temporary
evidence or toolchains. Each fixed command runs under a same-privilege timeout,
with its original argv journaled before execution. If the outer supervisor
cannot confirm cleanup, the failed receipt explicitly marks the artifacts
unsealed. The preparation cannot turn insufficient capacity into a passing
measurement. All capacity receipts are uploaded separately from native shards.

## PR inventory

`pr-audit.json` records the read-only inspection of the 43 existing draft PRs.
Historic checks alone did not establish a safe merge or redundant closure.
The original task baseline remains 192 total, 163 complete, 29 outstanding
until the requested full acceptance evidence is available.
`pr-triage-applied.json` records the subsequent exact-head recheck and additive
labels: 43 `needs-review`, 11 `needs-ci-fix`, and one `needs-rebase`.
