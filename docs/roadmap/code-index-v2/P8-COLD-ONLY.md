# Independent P8-005 cold-only study

This is a new, explicitly selected study for P8-005. The existing full-stage
scale workflow, its 150 shards/1500 samples, and any currently running study
remain unchanged. No historical full-stage prefix supplies a cold-only sample.
P8-006 incremental/fanout work and P8-008 query measurements receive no credit
from this study. Passing a measurement aggregate does not change task status or
certify a release/G8.

The native plan adds optional `stage_scope: cold_only_v1` and `cold_study`
(run ID and attempt). Both are absent in default full-stage serialization.
The worker executes the existing synthetic generation, two independent fresh
projects with empty index/parse caches, both complete cold builds, all fifteen
canonical table comparisons, physical counts, and independent configuration
fact. Only then does the explicit cold branch return its normal sample summary.
The supervisor still must exit successfully with complete stderr and evidence;
a timeout with a cold prefix is a failure. OS page cache is not cleared.

The release protocol fixes five scales (1000, 5000, 10000, 50000, 100000), seed
12648430, 30 repetitions per scale, one repetition per shard, capacity profile
`scale_capacity_v1`, 200/1024 work settings, 18,000,000 ms native deadline and
512 MiB output budget. Each shard retains both cold builds and full parity.
The preflight job is repetition zero, counted once. All five successes release
the remaining 145 jobs at max-parallel 10; job timeout remains 350 minutes.
The independent aggregate requires exactly all 150 scale/repetition slots.

`scripts/p8_cold_matrix.py` and `.github/workflows/p8-cold.yml` use separate
`p8-cold-*` artifact names including run ID and attempt. The outer build/shard
receipts bind those identities, the exact source inventory and release binary,
and the complete observer closure (cold driver, shared full validator, build
identity helper, capacity helper and new workflow). The original full validator
continues to demand its complete original stage population and rejects the cold
plan. The cold validator reuses its exact build, count, fifteen-table and byte
budget checks; it does not accept a passed summary in place of original records.

Both worker process resource snapshots are retained as observations, never as
peak/process-tree proofs. Every sample and host/CPU/kernel/environment stratum
is retained; heterogeneous results are not pooled into a stable tail claim.
Vectors remain explicitly disabled with a null count. Input digests must agree
across repetitions at the same scale. The native cold header records the
seed-cache environment input, its actual usize parse result and effective
capacity from the production function, plus Rust flags and temporary-directory
inputs; this observes settings without changing cache capacity. The existing
input event retains the actual engine configuration. Artifact inventories and
native hashes seal all raw evidence, including unsuccessful attempts.

The workflow starts only through its distinct `p8-cold-run` PR label or an
explicit dispatch. A caller must pin a new candidate before starting it. This
change itself neither dispatches a study nor replaces the existing full study.
