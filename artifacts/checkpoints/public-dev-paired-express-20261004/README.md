# Fixed same-input Express public DEV paired verification

**Terminal measurement: complete replayable schedule; quality FAIL / not certified.**
Each arm ran 387/387 scheduled rows once, missing 0, timeout/tool/protocol error 0,
387 Partial, Success/NoMatch 0. Four run/replay exits are 1, zero changed bytes.
Every suite actually prepared/indexed all seven source files and reached Ready
before queries. No retry selected a better result; no product or knob was tuned.

| Arm | Frozen profile | Scheduled / executed | Missing / error | Partial | Top1 | nDCG10 | Gate |
|---|---|---:|---:|---:|---:|---:|---|
| baseline | native | 210 / 210 | 0 / 0 | 210 | 0.084745763 | 0.097281741 | FAIL |
| candidate | native | 210 / 210 | 0 / 0 | 210 | 0.084745763 | 0.097281741 | FAIL |
| baseline | compat | 177 / 177 | 0 / 0 | 177 | 0.813559322 | 0.783961285 | FAIL |
| candidate | compat | 177 / 177 | 0 / 0 | 177 | 0.813559322 | 0.783961285 | FAIL |

Native and compat are different frozen scoring projections, not evidence that one
product improved over the other. Same-profile paired Top1/nDCG deltas are zero;
all-zero percentile intervals are degenerate/inconclusive, never a certified pass.
Native Recall5/10 (0.102259887), MRR10 (0.084745763), span recall (0.375259926)
are unchanged. Native span precision falls 0.160939462 -> 0.157016978:
query-micro delta -0.003922484; family-balanced delta -0.004060115, descriptive
Express-only 95% cluster quantiles [-0.009408250, -0.000438863]. Every supported
metric, category cell, denominator and per-case delta is in analysis. Compat
Recall/MRR/span are unavailable in the original scorer and remain null.
No confirmatory semantic-gain, noninferiority or global six-repo claim is made.

Strict native no-answer is 11 queries x 3 repetitions: both arms 0/33 correct,
33 incorrect Partial, zero missing. Source evidence: baseline 1578 returned hits,
candidate 1623, explicit invalid 0 and unverified 0. All 774 originating retrieval
work receipts are retained; per-field availability/count/sum/mean, lane status,
truncation, packing/generation/adapter/fixture identity and each case stage are
in summary/original-output archive. Originating work is not current cache work.
Observed elapsed times are retained and do not establish performance causality.

## Exact identity and preconditions

Baseline `88f2cf099c8b81f3acef485fd5ac9b01c63ce790`; candidate
`37dd042eaa1209a86e0cafdcd92ae77e036e76f5`. Candidate production src/Cargo/lock
matches `90858afae647a513537bf118932a7ba5020ee98b` (review tests differ).
Original JS execution reference `535ff1b13b841af8021346a660c83c57919525e2`
schedules 783 for Express + TypeScript; this task executes only Express's full
210 native + 177 compat schedule per arm. Plan commit `93e55e5d` was pushed before
ranking; execution/binary freeze commit `4cc03b83` preceded the first retrieval.
Prior public DEV aggregate exposure is disclosed, not backdated as unseen.

Exact original PR91 admission `5385f5a7a2a875c6d5cbd049bdde039bf71bbf32`, SHA256
`b4388ca60612f6734719b348bf30e05dd0752b7b754f47cc078b590e35c1e425`.
Source author `465e9e0bd435e2e30c08de8702f78a0d10c49c8e`, Express upstream
`7ef98448f8b38099ab1ded55e458538ad47a51e7`. Eleven original input files,
15 admitted source/review/suite/query/license pins, source SHA256/size/Git blobs
and MIT license bytes were checked. Native/compat suite/query/config/source
bytes are unchanged in both independent input roots; PYGO taxonomy is excluded.
Repetitions 3, warmup 0, seed 20261003, top_k 10, timeout 30000,
original MCP hybrid adapter/config and cc-eval-public-v7. Exact evaluator scorer,
normalizer, adapter, manifest, runner, schema and schedule source hashes match
between arms; no field filling or gold identity injection.

Two clean checkouts and independent targets/binaries/cache/index/fixtures.
Official Rust 1.95.0, original lock, default local features, dev opt0/debug
assertions/debuginfo0, incremental0. First offline builds failed ordinary reqwest
cache miss; diagnostics remain. Official locked Cargo fetch with normal inherited
proxy succeeded, then each actual isolated build succeeded. No retrieval preceded
successful builds/validate/prepare/index/readiness. Build/binary SHA256 and profiles
are in execution-freeze.json and actual compiler-artifact build receipts.
No AGENTS.md or .agents/skills was present; instructions receipt lists reads.

## Evidence, verification and boundaries

`paired-express-original-output.tar.gz` contains complete newly executed raw and
original outputs, derived stages/analysis, both initial/intermediate analysis
diagnostics, execution logs and retained license: 871 byte-verified members.
SHA256 `6de0aafab18fa49830c93d276856ce20a9ffea14e665a7feaa81e522af18d587`.
No full corpus source tree, binary or database is exported. All four original
cc-eval replays returned the same exit and changed zero files. Derived final
analysis also replays byte-for-byte in a separate directory without searches.
Initial/intermediate analysis diagnostics are preserved; the final labels
zero-width intervals degenerate/inconclusive and derives no-answer from rows.

To reproduce, create NEW clean baseline/candidate Git checkouts at exact SHAs,
independent target/binary/runtime/output roots, and fetch the fixed author objects.
Extract the original JS `plan.json` into the selftools prepare reference path
from commit 535ff1b, and the protocol subtree from
`5d3d9e101198cebdfa1a959f74ad6ef5da4a064c` into the protocol reference path.
Use selftools/build.py per exact source with distinct target/output/binaries;
selftools/prepare.py verifies original admission/input bytes, then run.py issues
each original suite once and replays it. Keep existing directories immutable.
Analyze retained results using analyze.py [NEW_OUTPUT], and package using
package_raw.py; these commands issue zero new retrievals. Paths in receipts are
execution identities, not a requirement to copy or publish binaries.

Only this evidence prefix is owned. Central tasks and old evidence remain open
and untouched; old1671 allPartial/qualityFAIL is preserved. Public DEV 301 native /
256 compat / 280 correlated groups is not 600 independent samples or holdout;
clean holdout = 0. Required facet coverage/graph correctness/Recall20/
SymbolAccuracy/DuplicationRate are not implemented; full freshness/semantic
ablation/release performance not_run. No excluded old suite, GC/WAL/kill/staging
fault/private42 export/100k scaling test, protected future holdout, live provider,
merge/deploy or new CI success declaration was run. Publication uses normal Git
push and one draft PR attempt; a Forbidden response stops that action.
