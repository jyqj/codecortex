# Remaining offline acceptance work

Authority: docs/roadmap/code-index-v2/06-VALIDATION.md; task state remains authoritative in tasks.json.

| Gate | New or independently integrated evidence | Minimal remaining evidence |
| --- | --- | --- |
| V16 L1/L2 | Independent literal hand-cosine gold, ties, filter-before-top-k, scope intersection/empty, deletion and cross-space rejection through production worker/query; five repetitions | Bounded-memory measurement under a growing document corpus, with reproducible resource sampling; complete formal row consolidation |
| V05 L1–L3 | New L2 scope and hydration subset; prior parameter cases preserved | Full BM25 monotonic, DSL, language/path intersection and all lanes/final hydration matrix at the prescribed levels, including actual product stdio |
| V11 L2/L3 | PR43 physical capacity/FIFO/lock progress six cases; PR44 actual stdio header/body deadline/cancellation plus frozen parameter matrix 155/0/0 | Mixed-generation rejection and finite retry proof; complete cache-key axis matrix; consolidate full formal row |

PR44 cancellation can leave blocking HTTP alive until its shortened deadline. No immediate physical abort or public successful-late-response publication barrier claim. V19 live quality remains blocked by D1/D2 authorization; no paid provider, private-source egress or old development corpus relabeled as holdout.

P7-014 remains in_progress; P7-011/013/016 remain todo. Independent bounded subreviews now have evidence; full V05/V11/V16/V18 and dependent task acceptance are not inferred.

Reproduction: checkout source 2d48f2628ae7c745fcab1a21dd784eae4582a193 in an isolated worktree; build the p7_v16_exact_oracle target with semantic-http, --locked --offline, and capture Cargo --message-format=json. Extract its compiler-artifact executable/features/fresh into a build receipt and SHA256 the executable, matching the included build-receipt schema. Run replay.py with P7_REPOSITORY_ROOT pointing to that checkout, P7_BUILD_RECEIPT pointing to the new receipt, and P7_REPLAY_OUTPUT pointing to a fresh directory. Five runs require only loopback transport. Original raw logs remain local by filename/hash; the earlier 1642718 preflight had a shared production scope predicate and is not the independent scope oracle.
