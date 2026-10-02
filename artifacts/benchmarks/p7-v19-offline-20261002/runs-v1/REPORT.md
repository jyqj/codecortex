# P7 V19 offline retrospective experiment

V19 remains **blocked**. These are real local MCP and rg observations on historical development gold; no real-model quality claim.

Inputs: integration `d6462c163abe6ca92d2ae0a2cf967b1ad2433e22` + PR14 `58e20d08285303fc9e12ed8cdfa21d6afb84910b`. Seed `2719013`, 9 source files, 14 queries / 7 families, 8 dev / 6 retrospective holdout queries. Three repetitions, 10 cells, 420 rows. Repetitions are not independent quality samples.

Compat is derived file-only gold, not upstream OCE. Gold answers and original annotations are unchanged in native; only split is re-authored. Gold-module sets are disjoint, but all questions were previously development questions. Incidental distractors are not certified hard negatives.

| Profile/arm | All nDCG | Holdout nDCG | Holdout Top1 | Status counts (all) |
|---|---:|---:|---:|---|
| native/baseline | 0.857143 | 0.833333 | 0.833333 | {'partial': 39, 'no_match': 3} |
| native/graph_vote_off | 0.857143 | 0.833333 | 0.833333 | {'partial': 39, 'no_match': 3} |
| native/graph_rerank_off | 0.857143 | 0.833333 | 0.833333 | {'partial': 39, 'no_match': 3} |
| native/lexical_votes_only | 0.857143 | 0.833333 | 0.833333 | {'partial': 39, 'no_match': 3} |
| native/rg_literal | 0.390952 | 0.266059 | 0.222222 | {'no_match': 24, 'success': 18} |
| compat/baseline | 0.847123 | 0.833333 | 0.833333 | {'partial': 39, 'no_match': 3} |
| compat/graph_vote_off | 0.847123 | 0.833333 | 0.833333 | {'partial': 39, 'no_match': 3} |
| compat/graph_rerank_off | 0.847123 | 0.833333 | 0.833333 | {'partial': 39, 'no_match': 3} |
| compat/lexical_votes_only | 0.847123 | 0.833333 | 0.833333 | {'partial': 39, 'no_match': 3} |
| compat/rg_literal | 0.375847 | 0.271822 | 0.166667 | {'no_match': 24, 'success': 18} |

Official cc-eval scores and gates are retained for every cell, including Partial/failed cases. aggregate.json contains micro, macro repo/category/family, strata, and paired family bootstrap CIs. Only 3 holdout families: intervals are descriptive and marked inconclusive.

graph_vote_off and graph_rerank_off each change one public scoring knob; traversal still executes. lexical_votes_only suppresses multiple votes and graph reranking, while selector, overlap, exact bonuses and physical lanes remain. rg is the official literal whole-query baseline. This does not implement dense-only or selector-off.

## Remaining blockers

- Unseen D5 holdout
- 6-repo/600-query D2 corpus
- Upstream 200-query OCE compat
- Independently reviewed hard negatives and facet/span gold
- Real dense-only/local+dense and semantic quality (D1/D2 authorization unchanged)
- Selector-off: no public config switch, production edits forbidden
- L4 performance certification: smoke sample count only

## Reproduce

From repository root, set CARGO_HOME=/workspace/.cargo, RUSTUP_HOME=/workspace/.rustup and PATH=/workspace/.cargo/bin:$PATH. Build with the receipt command. Run `python3 artifacts/benchmarks/p7-v19-offline-20261002/experiment.py verify`, then `.../experiment.py run --output /tmp/p7-v19-new-run` (new directory only). Report regeneration: `.../experiment.py report --output <run>`. No freeze occurs during run. Each cell is replayed by official cc-eval with raw digest verification.
