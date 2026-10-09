# Independent fresh-profile P8-006 study

This is a new registered study. It does not repair, resume, relabel or reuse any
part of a prior all-stage or cold-only study. The original default full runner,
`scale_capacity_v1` artifacts, cold-only study and their outcomes remain intact.
The machine-readable registration is `p8-profile-isolated-v1.json` in this directory.
Neither this protocol nor a single successful slice closes a task automatically.

The original P8-006 asks for no-op, body, API, config, batch and over-budget closure
measurements, attributable phase counts, and complete full parity after closure.
The original benchmark §8 starts each mutation with the same pristine A/B pair;
§9 separates profiles and retains N>=30. This protocol registers exactly N30 for
all eight mutation profiles on each of the original five scales: 1,200 cells.
The separate fanout population is five values (1,4,16,64,128) x30 =150 cells,
each with **actual N+1 files**. It is not multiplied by the five corpus scales.
The fanout plan's `files=1000` is solely the existing conservative disk-reservation
field; it does not claim a 1k fanout fixture.

Every mutation cell generates two fresh projects, with empty index and parse
cache, the same seed and original config augmentation. Both initial full builds,
the complete initial 15-table comparison, one selected mutation, every original
incremental/resume build, full-control rebuild and final complete 15-table parity
are inside that worker's original five-hour deadline. Dirty/resume limits remain
200/1024; fanout retains 8/128 and original handwritten call-edge assertions.
The fanout evaluator additionally retains the already executed initial complete
canonical comparison and original initial/full-control reports. Its existing
ordinary entry's wire and correctness decisions remain unchanged.

A fresh batch has not executed a preceding config mutation: its SQL config
witness must still resolve target0. Only the config profile expects target1.
The witness is retained and checked, not removed. Partial closure, parse errors,
missing tables, failed native termination and truncated raw evidence cannot be
accepted. The aggregate also retains the original requirement to observe an
actual over-budget incomplete first fanout build that later completes.

This explicitly changes history and the whole-study budget. It allows 1,350
workers x5h =6,750 worker-hours, versus the original 150 sequential workers x5h
=750 hours. It is **not an equivalent partition of that old budget or warm/cache
history**. Each of the eight main profiles has a fresh setup; setup costs are
retained, not taken outside the deadline. The 1,200 setup pairs carry no automatic
P8-005 credit, and there is no P8-008 query credit. OS page cache is retained.

The optional native `profile_study` and `profile_isolated_v1` scope are absent
from old plans, preserving their serialized fields. New artifacts use only
`p8-profile-*-${run_id}-${run_attempt}` names, with profile/scale-or-fanout/repetition
slots. Old public full/cold validators still reject this scope; the new strict
validator rejects full/cold slices and does not salvage failed prefixes.
All 1,350 slots must bind one source, release binary, complete driver closure,
build receipt, run ID and attempt. All raw rows, initial/final comparisons,
phase counts, original worker resource snapshots, effective seed-cache cap,
engine/runtime settings and temporary filesystem identities are retained.
Snapshots are not peaks or process-tree coverage. Host/CPU/kernel/environment
strata remain explicit; no pooled stable tail inference is added.

The independent workflow is `.github/workflows/p8-profile.yml`, started only by
its distinct `p8-profile-run` label event or explicit dispatch. It builds one
release executable, then runs nine dependent matrices of 150 jobs each (eight
mutation profiles and one fanout matrix). Each matrix is bounded to ten jobs;
only one matrix is active at a time. Failure stops dependent matrices and the
always-run aggregate rejects missing cells. There is no automatic new label,
retry, source change or scheduler in the driver. Job timeout remains 350 minutes,
while each supervised worker retains the original 5h and 512 MiB output budget.

Build and intake commands use `scripts/p8_profile_matrix.py` with `build`, `run`
and `aggregate` subcommands. A run requires the explicit `--mutation-profile`,
`--scale`, `--shard-index`, `--run-id` and `--attempt`; fanout additionally requires
`--fanout`. The normal build/hash/parity/source primitives are reused, not a new
weaker comparison algorithm. The shared raw event validator checks every consumed
build and the new independent population layer verifies every registered slot.

The committed Python tests are synthetic protocol controls. Added Rust tests
exercise fresh batch and actual small fanout contracts through the native worker.
They are not release measurements. Candidate preparation does not execute or
schedule this study; actual source admission, regression and explicit study
execution remain separate, accurately attributed records.
