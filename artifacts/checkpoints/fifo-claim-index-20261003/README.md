# FIFO claim physical index — 20261003

Base PR110 `5ffbadcf48e26523b2eb46beda0d187a2e2e29cd`; reviewed PR112
`199558c754aae7f8d1dba0ee73256a2ed11a4230`. Exact remote heads verified.
No applicable AGENTS.md or .agents/skills/SKILL.md was present in checkout or
workspace ancestors. Official Rust 1.95 toolchain and existing cargo registry/cache used.
Parent's later instruction authorizes only DEV source-lock/digest synchronization;
PR113 is for parent integration later, not a new experimental base.

## Correctness and scope

Only default ClaimFairness::Fifo gets a single-statement CASE EXISTS guard and
ordered partial nonunique index `(space_id,task_id,available_at) WHERE state='pending'`.
Both accesses explicitly bind their intended indexes. The ready probe uses the
existing `(state,available_at,space_id)` access path; the candidate preserves
`ORDER BY task_id ASC LIMIT 1`. The original UPDATE fields and RETURNING are retained:
random attempt token, attempt increment, lease, space and available_at fences.
Normal queue IMMEDIATE transaction, lifecycle admission and epoch handling are unchanged.
DocRoundRobin's full SQL literal remains byte-identical (`sql-source-verification.log`).

Schema **24 remains 24**. Fresh DDL adds the performance index. Matching-v24
normal writable open runs only the idempotent CREATE INDEX before the existing
incarnation ensure, not full-schema replay, row migration, rebuild or reparse.
An existing v24 file gets the index on first open. Repeated open is stable.
DDL errors propagate through the existing open seam; no permission/RO workaround.
This physical maintenance is separate from semantic migrations: versions 21/22/23
still return Mismatch and follow the existing rebuild contract.

`fifo_physical_index_preserves_existing_v24_logical_data` starts with an on-disk
pre-index v24 file, seeds unchanged files/chunks, document and semantic manifests,
pending work, active space and nonzero epochs, then compares every table including
FTS shadow tables and metadata before/after two real IndexDb opens. Rows, file hash,
manifest, incarnation and all epochs remain exactly equal; index is nonunique.
Finite-clock original/new full CAS comparison covers future prefix, other space,
done/claimed/superseded, exact readiness, ordinary retry/backoff and stale token.
Original/new two-connection normal IMMEDIATE CAS races both produce one winner,
one None, attempt=1 and one valid lease/token. Existing lease/fairness tests also pass.

## Complete production SQLite claim plans

`candidate.sql` is extracted from the actual production Rust SQL literal and
verified equal; `sql_probe.rs` invokes production claim_next_on for candidate timing
and uses that exact full UPDATE for EQP/VM steps. It compares the original full
UPDATE on a database without the new index. Production rusqlite SQLite **3.53.2**,
not a simplified SELECT or Python SQLite planner. 1k/5k/10k, six distributions.
Both selects and the mutation execute together; synthetic timing rolls back and
excludes commit, so it is not pipeline throughput evidence.

10k VM steps (`sql-results.json`):

| Distribution | Original | Candidate | Selected |
| --- | ---: | ---: | ---: |
| All ready | 90162 | 195 | 1 |
| All future | 36 | 37 | None |
| Oldest 5k future, remaining ready | 45163 | 25195 | 5001 |
| 9999 earlier available ready rows in other space, one active ready | 40167 | 40191 | 10000 |
| Only other-space ready | 40035 | 40036 | None |
| Mixed pending/future/other/done/claimed/superseded | 35163 | 205 | 8 |

Every candidate EQP uses `semantic_outbox_fifo_pending` with space equality;
Sort=0. Original full claim Sort=1. Oldest ready task agrees in every case.
All-future guard prevents the otherwise linear FIFO scan. **Future prefixes remain
linear; the ready probe can scan earlier-ready rows across spaces.** This is not a
universal asymptotic bound. New index has write/storage overhead: pending enqueue
and retry insert an entry; claim/supersede delete it. 10k all-ready synthetic page
count is 528→585 (4096-byte pages), not a logical-data change. Insert+commit timings
and page counts are retained, but single debug/in-memory timings are noisy.

## Finite normal pipeline paired AB

Same deterministic `source(i,value=0)` input hashes, fresh independent repositories
and durable caches, 128-dimensional loopback synthetic HTTP model, production MCP
stdio index→normal runtime/queue→input resolution→real HTTP provider→durable put→
readback→publish CAS. Fixture responds normally from first request; no provider hold.
Configuration is retained in `config.json`: claim16, concurrency4/per-project2,
parse4, poll .2s, termination ready or 90s, status `retrieval-capabilities-v2`.
No parallel builds during measured runs. Baseline and candidate use the **same**
PR112 temporary timing probes; no instrumentation lands in production.

Runs: baseline 1k/5k, candidate 1k/5k, then candidate-repeat 1k/5k followed by
baseline-repeat 1k/5k. Two finite pairs are insufficient for statistical claims.
`paired-results.json` retains cold/ready/claim/round/durable-put/CAS costs and
process lifetime CPU, HWM, logical syscall and physical IO counters. IO includes
startup, cold index, drain, status and diagnostics; read_bytes=0 means cached reads
are possible, not zero read work. JSONL resource samples and raw phase logs retained.
EOF exit0, all n rows done with attempts=n, n document/semantic manifests, no pending
or failed work, status v2 ready, integrity/FK OK required by `finalize.py`.

| Run | n | Ready s | Claim s | Physical writes MB | Logical read rchar MB |
| --- | ---: | ---: | ---: | ---: | ---: |
| baseline | 1000 | 4.708 | 0.253 | 129.97 | 60.26 |
| baseline | 5000 | 22.965 | 1.488 | 670.58 | 446.09 |
| candidate | 1000 | 4.449 | 0.093 | 135.55 | 60.19 |
| candidate | 5000 | 25.963 | 0.621 | 700.02 | 466.16 |
| candidate-repeat | 1000 | 4.652 | 0.084 | 135.54 | 60.68 |
| candidate-repeat | 5000 | 26.654 | 0.698 | 700.67 | 474.07 |
| baseline-repeat | 1000 | 4.471 | 0.120 | 129.97 | 59.89 |
| baseline-repeat | 5000 | 25.051 | 1.532 | 670.66 | 458.03 |

**No observed 5k end-to-end improvement:** claim fell from 1.488/1.532s to
0.621/0.698s, but ready time rose from 22.965/25.051s to 25.963/26.654s.
Physical writes increased from about 670.6MB to 700.0–700.7MB (+4.4%).
1k ready changes are small and reverse between pairs. These wall times are noisy;
cache put cost increased and dominates, so neither attributing all slowdown to
the index nor claiming overall speedup is justified.
 No formal 100k rerun; original
100k failure remains unresolved. Claim cost improvement alone does not establish
end-to-end improvement; durable cache and added index writes remain real costs.

## Limited validation and task TODO

- [x] Review runtime/queue/outbox/migration/DDL and exact PR112 diagnosis.
- [x] Implement only FIFO candidate/index, keep schema24 and DocRoundRobin SQL.
- [x] Real old-v24 open/reopen logical snapshots; existing mismatch contract tests.
- [x] Finite original/new retry, readiness, state and normal CAS competition tests.
- [x] Full production SQLite EQP/steps across six distributions and three scales.
- [x] Preserve future-prefix, cross-space ready-probe and index write/storage limits.
- [x] Source-reviewed DEV manifest and only R05/R06 digest annotations synchronized;
      all 14 queries/answers/scoring/config and other admitted sources unchanged.
- [x] Six-suite validation, source/query drift negative controls, benchmark_lock tests.
- [x] Finish two finite normal pipeline pairs and report reads/writes without overclaim.
- [x] Commit/push/draft PR and verify exact remote delivery; then stop.
      PR114 draft created on PR110 base; implementation commit c73128b500c2a19daa8a293caa52afb711b03516 verified remotely. Final evidence receipt commit is verified in the delivery response.

`correctness.log`: 20 outbox + 9 lease + 3 queue tests passed. `migration.log`:
5 migration tests passed. `benchmark-lock.log`: 10 tests passed. `six-suite.log`
and `dev-negative.log`: all six suites and bounded negative controls passed.
No CI workflow/central TODO, other suite/heldout, real provider, prior RO refusal,
GC/WAL kill/fault, permissions, merge, force push or deploy actions in this task.

Reproduce tests with official cargo and `--locked` using commands in build/test logs.
SQL probe: copy retained sql_probe.rs to a temporary cc-db example, cargo run example
with this evidence directory argument, then remove example. Pipeline: archive base
into scratch source, initialize its local git for instrumentation.patch generation,
run retained instrument.py there, release build with semantic-http and copy baseline
binary; overlay only three production files, build and copy candidate. Set FIFO_BINARY
and PHASE_CASE_PREFIX for pipeline.py. Large fixtures/cache and binaries stay local.

`instrumentation-verification.log` independently verifies the retained patch against
the actual five probe files in both build sources. The initial archive lacked .git:
instrument.py applied all spans, but trailing git-diff receipt generation failed;
the exact PR112 patch was then retained and verified rather than inventing a receipt.

Delivery: https://github.com/jyqj/codecortex/pull/114 (draft, stacked on PR110).
Implementation remote head c73128b500c2a19daa8a293caa52afb711b03516 was verified
against origin and connector PR metadata. No concrete Forbidden, Username error
or approval rejection occurred. Final receipt commit adds evidence only.
Independent review and any production integration decision belong to the parent;
this task ends after final remote verification, without further experiments.

## PR114 Clippy delivery repair

Frozen performance/review head remains `807f4710495a38bc6631549da2ab9623c83684b8`.
CI run 37122692030 / job 111201777748 reported `clippy::type_complexity` in the
finite-clock test's seven-field Vec return type. The repair adds only a local
`NormalizedLeaseRow` alias and its field-order comment. SQL, row mapping, all
seven fields, comparisons and assertions remain unchanged; no lint allowance.
Production performance remains unaccepted; no integration, merge or new experiment.

- [x] Add meaningful local test alias only (`semantic_outbox.rs` test diff).
- [x] Outbox tests: 20 passed (`clippy-fix/outbox-tests.log`).
- [x] cc-db all-target Clippy with -D warnings (`clippy-fix/clippy.log`).
- [x] Workspace fmt and diff checks (`clippy-fix/validation.json` and logs).
- [x] Frozen source/protocol audit (`clippy-fix/byte-identity.json`): three
      production files, DEV manifest/query locks and eight performance/probe/AB
      files byte-identical; the only crate change is the test alias.

The repair is delivered on the existing PR114 branch; exact new remote/PR head
verification is supplied in the final response. The frozen 807f471 measurements
and source identities remain valid for independent review and parent-owned AB.
