# Independent fixed D0 full scale study

This prospectively registered study executes all 150 original scale cells on
`d0cb69c601e530dcef738af0c2dffb8d3b8bcf28`. The registration was frozen after
the original engineering run [37846370300](https://github.com/jyqj/codecortex/actions/runs/37846370300)
completed successfully and independent reviewers replayed its original release
build, twelve controls, and complete 1k and 10k diagnostic raw through the original
validators. Those two diagnostics are admission evidence only; every primary
cell in this new study is a new measurement.

The measured checkout and original compiled source path remain D0. The separate
controller commit is recorded as the controller identity; it is never substituted
for the measured source identity. The unchanged original build is downloaded by
its fixed artifact ID and checked against its original receipt, full source
inventory, binary SHA256/BLAKE3, and original Cargo provenance.

## Fixed execution

- Five scales: 1000, 5000, 10000, 50000, and 100000 files.
- Thirty new repetitions at each scale, for 150 cells and 1500 group samples.
- Original seed 12648430, `scale_capacity_v1`, all phases, full 15-table oracle,
  fanout, raw inventory, source checks, and strict complete-matrix aggregate.
- Original native 300-minute, Python 302-minute, and job 350-minute limits;
  512 MiB raw output limit per cell.
- Maximum 20 concurrent cells on separate disposable hosted VMs. This is an
  explicit scheduling choice; no per-cell capacity or timeout is increased.
- Only workflow attempt 1 is admitted. Failed or missing cells remain failures
  or missing; a later attempt cannot replace them.

The dedicated workflow first receives and replays the fixed upstream originals.
Any prerequisite failure prevents new measurement. It preserves failed
preparation, failed measurements, and the original aggregate outcome as Actions
artifacts. The original G study and the failed local G study remain separate,
with all their original successes, failures, and missing measurements retained.

## Evidence and acceptance limits

`registration.json` fixes every original input SHA, original plan, upstream job,
artifact ID, ZIP digest, and prerequisite receipt. `admit.py` and `workflow.yml`
are the reviewed control implementation. The repository workflow is installed at
`.github/workflows/p8-fixed-d0-scale.yml`; the colocated copy is the exact same
file. `controller-review-history.tar.gz` retains the reviewed earlier templates,
resolved findings, actual attempt-guard controls, and the final independent
registration review. `D0-engineering-originals.tar.gz` and its manifest retain
the actual engineering controls, both diagnostic ZIPs, capacity observations,
build provenance, full original validation, and five source-change reviews.

This registration does not report N=30 completion, a P5 measurement, a P5 full-CI
pass, a release-lock approval, or a TODO closure. A future P5 may cite D0 results
only together with a separately proven full production/scale-observer byte
equivalence; the raw measured identity remains D0. The original TODO count at
registration is 163 done and 29 remaining out of 192.
