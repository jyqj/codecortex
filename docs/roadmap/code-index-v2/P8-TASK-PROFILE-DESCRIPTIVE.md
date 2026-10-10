# P8 task profile descriptive population

This separately registered population supplies descriptive P8-006 evidence. Its
schema is `p8-profile-task-descriptive-v1` and native scope is
`profile_task_descriptive_v1`. It has one observation per cell, with **45 cells
and 85 retained records**. It is neither the sequential 150-shard study nor the
1,350-cell `profile_isolated_v1` study. It changes no existing study, accepted
sample, release condition, or task status.

## Fixed population and history

The eight mutation profiles are `no_op`, `body`, `api`, `config`, `batch_1`,
`batch_10`, `batch_100`, and `batch_1000`. Each runs once at 1,000, 5,000, 10,000,
50,000, and 100,000 generated source files. Each of these 40 cells creates fresh
A/B inputs, retains its cold setup pair and full fifteen-table comparison,
applies only its selected mutation, completes the original incremental closure,
then performs its full control and full fifteen-table comparison. A batch cell
starts with the fresh target-zero configuration; a config cell retains its
independent target-one witness. The original fixture also reports its auxiliary
visible configuration file separately from requested source-file capacity.

The five additional cells use the original fanouts 1, 4, 16, 64, and 128. Their
actual inputs contain **N+1 files**. Their `files: [1000]` plan field reserves
capacity; it is not the fanout's measured file count. They retain the original
initial full builds, initial fifteen-table comparison, mutation, dirty-budget
closure, final full build/comparison, and independent call-edge assertions.
First-build-incomplete remains an observation; aggregate success requires actual
over-budget closure coverage in these original fanout results.

The five cold-curve records are prescribed **before execution**: the setup from
the `no_op` cell at each scale. They are a subset of the 40 setup records. The
other 35 setups remain in full; none is selected or discarded according to time.
The output counts 40 setups + 40 mutations + 5 fanouts = 85 records, not 90.

Every cell uses seed `0xc0ffee`, N=1, shard index 0/count 1, main dirty budget
200/resume limit 1024, and the independent fanout dirty budget 8/resume limit 128.
The physical oracle remains `scale_capacity_v1`; all fifteen tables, canonical
rows, duplicate semantics, limits and parity predicates stay unchanged. Each
worker's five-hour deadline includes setup, mutation, closure, full control and
parity. Its original native output budget is 512 MiB. Forty-five workers allow
225 worker-hours in total; this is a distinct population and history, not an
equivalent resharding of the existing studies. Actual runner scheduling may take
longer than native time and is not a performance measurement.

## Registration, source and original evidence

The standalone build command performs one native release build. Before cells
start, `build/task-registry.json` records all 45 exact plans and the actual
source commit, source manifest, binary SHA-256/BLAKE3, native build receipt,
observer manifest, run ID and attempt. `build/task-build.json` seals that registry
and its complete file inventory. No placeholder identity is accepted at run or
aggregate time. The observer closure includes this driver, the shared native
driver/helper, runner capacity observer, this protocol and its document, the new
capture wrapper and both tests, and the dedicated workflow.

The build layout is `build/native-build/p8-scale`, with the unchanged original
`native-build/build.json` and original source records. Each cell writes
`task-shard.json`, `native-shard/shard.json`, and the original
`native-shard/native/{plan.json,raw.jsonl,worker.stderr,worker-summary.json,report.json}`.
The task receipt includes the exact build and registry digests. The native
validator still verifies the original release producer, source before/after,
binary, complete raw EOF, native and worker exits, closure, counts, full control,
parity and independent witnesses. Failure never becomes an accepted prefix.

The workflow attempts all 45 declared cells after a successful build, with
`max-parallel: 10` and `fail-fast: false`. It retains every successful or failed
cell's original output. Build failure starts no native cells and remains failure.
The distinct `p8-task-*` artifact namespace includes run, attempt and cell
identity. Bounded capture uploads preserve original byte ranges during execution;
they report custody faults separately from native outcome. Prefixes do not
replace complete shard receipts and cannot fill missing cells.

The aggregator calls the original validators for the supplied files, records
each input's success or exact failure, and retains all supplied original task
receipts and file digests. It rejects missing, duplicate or extra cells, changed
plans and mixed source, binary, driver, build, registry, run or attempt. It only
sets `passed: true` when all 45 cells satisfy the original predicates. Missing
or failed originals stay explicit in `task-matrix.json`; no old study or diagnostic
sample may substitute for them. Preserve the original shard artifacts alongside
the matrix, including failures.

## Descriptive interpretation

The matrix retains every raw timing, build and parity record, its original host
and environment, and each available resource snapshot. It computes no pooled
latency distribution, tail percentile, stable performance claim, speedup, causal
cross-host comparison, or release certification. Nested elapsed measurements
must not be added as disjoint costs. Missing resource peaks remain unknown;
snapshot observations do not prove process-tree maxima. Vectors remain disabled
or absent as recorded by the original protocol.

Completion of this artifact is evidence for a later human task review. It does
not edit `tasks.json`, waive dependencies or original task acceptance, certify
P8-005, P8-008 or P8-020, or mark the N30 populations complete. All 45 results,
including failures, must remain available regardless of the later review.

## Commands

Run only on the finally admitted fixed source with the actual workflow identity:

```sh
python3 scripts/p8_task_profile_matrix.py build --root . --output build --run-id RUN_ID --attempt ATTEMPT
python3 scripts/p8_task_profile_matrix.py run --root . --build build --output cell --run-id RUN_ID --attempt ATTEMPT --scale 100000 --shard-index 0 --mutation-profile batch_10
python3 scripts/p8_task_profile_matrix.py run --root . --build build --output fanout-cell --run-id RUN_ID --attempt ATTEMPT --scale 1000 --shard-index 0 --mutation-profile fanout --fanout 16
python3 scripts/p8_task_profile_matrix.py aggregate --root . --build build --output aggregate --inputs CELL_DIRECTORIES
```

The workflow alone controls the full fixed matrix and its independent
`p8-task-profile-run` label. These example commands do not authorize a local
experiment or silently create a new registration.
