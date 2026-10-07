# V19 offline evidence and checklist recommendation

This independent evidence block is based on published integration `d6462c163abe6ca92d2ae0a2cf967b1ad2433e22` plus PR14 input `58e20d08285303fc9e12ed8cdfa21d6afb84910b`. Dataset/arm plan was committed as `05f4853` before measurement; the build receipt binds the exact identical engine `crates` tree and both default-feature binary SHA256 values. The local offline build succeeded. No production source, dependencies, shared tasks/TODO, provider settings or D1/D2 authorization changed.

Read [runs-v1/REPORT.md](runs-v1/REPORT.md), [runs-v1/aggregate.json](runs-v1/aggregate.json), [runs-v1/audit.json](runs-v1/audit.json), and [plan.json](plan.json). The admitted snapshot contains exactly nine real public CodeCortex Rust source files from the fixed input SHA. Query/gold/plan documents remain outside the indexed source. Native gold answers and annotations are preserved verbatim; compat gold is an explicitly declared file-only projection, not an upstream OCE import.

Seven Python guard tests pass: family leakage, gold-module leakage, fixed quotas, original native gold preservation, single-factor/corpus identity, independent bootstrap family sample count, and frozen input corruption rejection. The official runner froze new authoring manifests before execution, then ran ten cells through real MCP stdio or literal rg, three repetitions each, and replayed every cell with raw hash verification. There are 420 retained rows: 312 Partial, 72 no_match, 36 success; all 312 Partial have raw packing.partial=true. Eight MCP gates remain failed, two rg cells are baseline observations. No case was excluded and the normal 16KB output budget was retained.

Actual observations: both graph scoring single-factor controls and the explicitly composite lexical vote control have identical per-query Top1/nDCG to the baseline in both profiles. Native baseline micro nDCG=0.857143; derived compat=0.847123. Retrospective holdout nDCG=0.833333 in both. These are descriptive path-gold scores with failed completeness gates, not a quality improvement or release certificate. Full per-partition micro/macro repo/category/family statistics and paired family bootstrap confidence intervals are in aggregate.json. There is one repo, seven total families and three holdout families; bootstrap intervals are inconclusive. Repetitions and scoring profiles are never pooled as independent quality samples.

**Gold freshness limit:** ten queries have source digests different from the original 2026-09-27 annotations. The preserved R09 `compute_fingerprint_for_unit` anchor is absent in the current gold file; all three repetitions return no_match in each MCP arm. Audit retains both old and new hashes. The old annotations are provenance, not a claim of fresh independent review. Other current anchors and schema/closure/scope concepts can be inspected in the frozen source, but no external human certification is claimed. Every query was already used as dev; the deterministic 4-dev-family/3-holdout-family partition is retrospective and cannot become an unseen D5 holdout merely by renaming its split.

The audit actually mines deterministic token-overlap distractor candidates from the admitted non-gold documents. They are explicitly unreviewed candidates, not certified hard-negative gold. None enters scoring. Independent hard-negative review, fresh facet/span annotation, genuine unseen holdout, the 6-repo/600-query corpus, and upstream 200-query compat remain blocked. Real dense-only/local+dense semantic quality remains blocked under the existing D1/D2 limits. Selector-off cannot be represented by public configuration; production edits are outside this block. Graph controls suppress votes or rerank weight rather than disabling traversal; the lexical control is composite and retains selector/overlap/exact bonuses and physical lanes. The rg baseline is whole-query fixed-string matching, not BM25. No fake transport or synthetic vectors were used to claim retrieval quality.

Checklist recommendation: attach this evidence to V19 as **offline mechanism subitems executed / formal L3-L4 blocked**; leave V19 and P7 semantic quality closure unpassed. P7-013 offline transport evidence remains separately scoped in PR14. Leave P7-014 and live authorization decisions with their independent owners. Shared checklist files were not edited.

Reproduction (from repository root):

```sh
export CARGO_HOME=/workspace/.cargo
export RUSTUP_HOME=/workspace/.rustup
export PATH=/workspace/.cargo/bin:$PATH
export CARGO_BUILD_JOBS=5
cargo build -p cc-server --bin codecortex -p cc-eval --bin cc-eval --locked --offline
python3 -m unittest discover -s artifacts/benchmarks/p7-v19-offline-20261002 -p 'test_*.py'
python3 artifacts/benchmarks/p7-v19-offline-20261002/experiment.py verify
python3 artifacts/benchmarks/p7-v19-offline-20261002/experiment.py run --output /tmp/p7-v19-new-run
python3 artifacts/benchmarks/p7-v19-offline-20261002/audit.py /tmp/p7-v19-new-run
```

Output must be a new directory. Authoring `prepare` deliberately rejects the frozen existing plan. Measurement does not refresh locks. `audit.py` verifies paired corpus/query identity, retains all rows, mines labeled candidates, and checks byte-identical aggregate regeneration from official scores. No separate nDCG formula exists in these scripts. Raw timings are observations, not L4 performance certification. A new integration head requires a separate exact-source build and minimal rerun; the integration remote was still d646 at completion.
