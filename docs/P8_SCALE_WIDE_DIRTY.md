# Explicit wide dirty scale configuration

`scale_wide_dirty_v1` is an independent work configuration. It raises the main
scale dirty propagation budget from 200 to 4096 and retains 1024 maximum resume
builds. The separate fanout workload still uses a dirty budget of 8 and at most
128 resume builds; its actual incomplete-first-build and full fact comparison
requirements remain in force.

The original `scale_capacity_v1` name continues to mean 200/1024. The unregistered
benchmark CLI default remains 8/128; production indexing defaults are unchanged.
Neither old profile, its results, nor its running study is
changed or relabeled by selecting the new configuration. Wide dirty results
cannot be combined with either old profile, another source, or another binary.

## Work identity and physical oracle identity

The native plan, run registration, shard record, and aggregate contract use
`capacity_profile: scale_wide_dirty_v1` as the work identity. The wide contract
also records `oracle_capacity_profile: scale_capacity_v1`. Every small or large
table parity record retains that existing physical oracle identity. The existing
large-table entry point and physical limits are unchanged: 5,000,000 rows per
table, 16 GiB canonical bytes, 8 GiB scratch bytes, 1 MiB per row, 2048 KiB sorting
cache, and the existing `without_rowid_value_ordinal_v1` layout. The capacity
preparation observer also retains its physical `scale_capacity_v1` label; its
record is not the source of the work configuration's dirty budget.

## Independent workflow and retained population

`.github/workflows/p8-scale-wide-dirty.yml` runs only through its own dispatch or
the `p8-scale-wide-dirty-run` pull request label. It has separate
`p8-scale-wide-dirty-*` artifact and output names. The original workflow and its
`p8-scale-run` label are unchanged. Each execution binds one actual source and
fresh release build. It first executes the five original repetition-zero shards;
only success of all five admits the remaining 145 shards. Failure is retained,
never replaced by another source, repetition, or profile.

Before that release build, the new workflow runs `cargo fmt --all -- --check`
and `cargo test --locked -p cc-eval --test p8_scale` once. These protocol controls
use a separate target directory; their compiled outputs do not populate the
fresh release producer target. Their engineering runtime is outside every
native measurement's five-hour clock. A failed command stops the build job and
prevents the scale shards from starting. These commands do not assert scale
performance or replace the original raw validators.

The workflow retains seed 12648430, all five scales (1k, 5k, 10k, 50k, 100k), N=30,
one repetition per shard, and the complete continuous history of cold, no-op,
body, API, configuration, and 1/10/100/1000-file batches. Every checkpoint still
requires complete fifteen-table equality and original counters. Fanout
1/4/16/64/128 runs only at 1k, once for each repetition. Complete coverage is
150 shards and 1500 samples; no phase or repetition is removed because it is
slow or failed. The original five-hour native deadline, 512 MiB native output
budget, disk capacity admission, source/build seals, and raw replay remain.
Heterogeneous machines retain explicit strata; the aggregate does not pool
latency or certify a speedup, stable tails, quality, or release readiness.

The explicit commands for a previously bound build are:

```sh
python3 scripts/p8_scale_matrix.py run --root "$GITHUB_WORKSPACE" \
  --build "$RUNNER_TEMP/p8-scale-wide-dirty-build" \
  --scale 100000 --shard-index 0 --shard-count 30 --repetitions 30 \
  --capacity-profile scale_wide_dirty_v1 \
  --output "$RUNNER_TEMP/p8-scale-wide-dirty-shard-100000-0"
python3 scripts/p8_scale_matrix.py aggregate \
  --build "$RUNNER_TEMP/p8-scale-wide-dirty-build" \
  --inputs "$RUNNER_TEMP"/p8-scale-wide-dirty-shards/p8-scale-wide-dirty-shard-* \
  --output "$RUNNER_TEMP/p8-scale-wide-dirty-matrix" \
  --repetitions 30 --shard-count 30 --capacity-profile scale_wide_dirty_v1
```

Selecting this configuration is not evidence that a workload completed. Native
engineering controls and synthetic Python protocol tests are distinct from an
actual source-bound scale study. No earlier failure becomes a success, and no
TODO status changes merely because this configuration is available.
