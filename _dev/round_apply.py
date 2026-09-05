from pathlib import Path


def replace(path, old, new, count=1):
    p = Path(path)
    s = p.read_text()
    n = s.count(old)
    if n != count:
        raise RuntimeError(f'{path}: expected {count} occurrences, found {n}: {old[:100]!r}')
    p.write_text(s.replace(old, new))


def append(path, text):
    p = Path(path)
    p.write_text(p.read_text() + '\n' + text)


replace('crates/cc-search/src/plan.rs', '''        if !preselect.files.is_empty() && request.file_paths.is_none() {
            request.file_paths = Some(preselect.files.clone());
        }
''', '''        // Preserve caller scope. Heuristic preselection is a ranking/scan hint,
        // not authorization to exclude globally matching lexical/graph candidates.
''')
replace('crates/cc-search/src/plan.rs', '''    pub(crate) fn has_file_scope(&self) -> bool {
        self.filters.has_file_scope()
    }
''', '''    pub(crate) fn has_file_scope(&self) -> bool {
        self.grep_scope().file_paths.is_some_and(|files| !files.is_empty())
    }

    /// Grep is a bounded scan, so use informative preselect hints for its work
    /// scope. Fallback/recency-only files must not hide rare global literals.
    /// Lexical and graph retrieval always use the caller's hard scope instead.
    pub(crate) fn grep_scope(&self) -> cc_db::ChunkScope {
        let mut scope = self.chunk_scope();
        if scope.file_paths.is_none()
            && !self.preselect.lane_stats.used_fallback
            && !self.preselect.files.is_empty()
        {
            scope.file_paths = Some(self.preselect.files.clone());
        }
        scope
    }
''')
replace('crates/cc-search/src/plan.rs', '''    /// Whether the request carries an explicit file-paths scope (which
    /// bounds the grep scan's cardinality).
    pub(crate) fn has_file_scope(&self) -> bool {
        self.file_paths
            .as_ref()
            .map(|files| !files.is_empty())
            .unwrap_or(false)
    }

''', '')
replace('crates/cc-search/src/plan.rs', '''    /// Whether the request carries an explicit file-paths scope (which
    /// bounds the grep scan's cardinality).''', '    /// Whether the effective grep scan has a bounded file-path scope.')
replace('crates/cc-search/src/plan.rs', '''            "stage_a_files_considered": self.preselect.files.len(),''', '''            "stage_a_files_considered": self.preselect.files.len(),
            "scope_policy": "hard-scope-with-preselect-hints-v1",''')
replace('crates/cc-search/src/preselect.rs', 'let bm25_score = raw_score.abs();', 'let bm25_score = (-raw_score).max(0.0);')
replace('crates/cc-search/src/preselect.rs', 'ctx.ranking.preselect_fts_base + (1.0 / (1.0 + bm25_score))', 'ctx.ranking.preselect_fts_base + bm25_score / (1.0 + bm25_score)')
replace('crates/cc-search/src/preselect.rs', '1.4 + 1.0 / (1.0 + |score|)', '1.4 + strength / (1.0 + strength), strength = max(-bm25, 0)')
replace('crates/cc-model/src/config.rs', 'FTS summary layer: score is `base + 1 / (1 + |bm25|)`.', 'FTS summary layer: score is `base + strength / (1 + strength)`, strength = max(-bm25, 0).')
for path in ['crates/cc-search/src/preselect.rs', 'crates/cc-search/src/lanes.rs']:
    p = Path(path)
    s = p.read_text()
    old = 'b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)'
    assert old in s
    p.write_text(s.replace(old, old + '.then_with(|| a.0.cmp(&b.0))'))
replace('crates/cc-db/src/index_db_retrieval.rs', 'ORDER BY score LIMIT ?3', 'ORDER BY score, files.file_path LIMIT ?3')
replace('crates/cc-db/src/index_db_retrieval.rs', 'ORDER BY score LIMIT ?2', 'ORDER BY score, files.file_path LIMIT ?2')
replace('crates/cc-db/src/index_db_retrieval.rs', 'sql.push_str(" ORDER BY score LIMIT ");', 'sql.push_str(" ORDER BY score, chunks.chunk_id LIMIT ");')
replace('crates/cc-db/src/index_db_retrieval.rs', 'bm25(files_fts, 1.8, 1.0)', 'bm25(files_fts, 0.0, 1.0)', 2)
p = Path('crates/cc-search/src/lanes.rs')
s = p.read_text()
a = s.index('impl RetrievalLane for GrepLane')
b = s.index('/// Graph retrieval lane:', a)
s = s[:a] + s[a:b].replace('&plan.chunk_scope()', '&plan.grep_scope()') + s[b:]
p.write_text(s)
replace('crates/cc-search/src/lanes.rs', '''            // Smallest containing chunk, matching the old per-symbol query.
            let cid = chunks_by_file.get(file).and_then(|spans| {
                spans
                    .iter()
                    .filter(|(_, cs, ce)| *cs <= start && *ce >= end)
                    .min_by_key(|(_, cs, ce)| ce - cs)
                    .map(|(cid, _, _)| cid.clone())
            });
            if let Some(cid) = cid {
                best_per_chunk
                    .entry(cid)
                    .and_modify(|s| *s = s.max(score))
                    .or_insert(score);
            }''', '''            if let Some(spans) = chunks_by_file.get(file) {
                for cid in crate::symbol_chunks::project_symbol_chunks(spans, start, end) {
                    best_per_chunk
                        .entry(cid.to_string())
                        .and_modify(|s| *s = s.max(score))
                        .or_insert(score);
                }
            }''')
replace('crates/cc-search/src/lanes.rs', 'mapped back to the smallest containing chunks.', 'mapped to the smallest containing chunk, or the chunks of a split symbol.')
replace('crates/cc-search/src/lanes.rs', '''    for outcome in outcomes {
        for (rank, (id, _)) in outcome.hits.iter().enumerate() {
            let score = outcome.weight / (rrf_k + rank + 1) as f64;''', '''    for outcome in outcomes {
        let mut seen = HashSet::new();
        for (rank, (id, _)) in outcome.hits.iter().enumerate() {
            // Duplicate candidates consume their original rank but cannot vote
            // twice for the same document within one lane.
            if !seen.insert(id) {
                continue;
            }
            let score = outcome.weight / (rrf_k + rank + 1) as f64;''')
append('crates/cc-search/src/lib.rs', 'mod symbol_chunks;\n')
Path('crates/cc-search/src/symbol_chunks.rs').write_text('''//! Symbol-to-source projection independent of graph retrieval and SQL.
//!
//! A short symbol selects its smallest containing chunk. A split symbol has
//! no containing chunk: return intersecting pieces in source order instead
//! of silently losing a symbol already found in the graph.

pub(crate) fn project_symbol_chunks(
    spans: &[(String, u32, u32)],
    start: u32,
    end: u32,
) -> Vec<&str> {
    if start > end {
        return Vec::new();
    }
    if let Some((id, _, _)) = spans
        .iter()
        .filter(|(_, cs, ce)| *cs <= start && *ce >= end)
        .min_by(|a, b| (a.2 - a.1).cmp(&(b.2 - b.1)).then_with(|| a.0.cmp(&b.0)))
    {
        return vec![id.as_str()];
    }
    let mut pieces: Vec<_> = spans
        .iter()
        .filter(|(_, cs, ce)| *cs <= *ce && *cs <= end && *ce >= start)
        .collect();
    pieces.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.2.cmp(&b.2)).then_with(|| a.0.cmp(&b.0)));
    pieces.into_iter().map(|(id, _, _)| id.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn long_symbol_survives_chunk_boundaries() {
        let spans = vec![("second".into(), 81, 160), ("first".into(), 1, 80)];
        assert_eq!(project_symbol_chunks(&spans, 1, 160), ["first", "second"]);
        assert_eq!(project_symbol_chunks(&spans, 79, 82), ["first", "second"]);
    }
    #[test]
    fn smallest_container_and_stable_ties() {
        let spans = vec![("large".into(), 1, 200), ("b".into(), 10, 20), ("a".into(), 10, 20)];
        assert_eq!(project_symbol_chunks(&spans, 11, 19), ["a"]);
        assert!(project_symbol_chunks(&spans, 20, 10).is_empty());
        assert!(project_symbol_chunks(&spans, 300, 400).is_empty());
    }
}
''')
replace('crates/cc-eval/src/runner.rs', '''    let result_names: Vec<String> = arr
        .iter()
        .filter_map(|item| {''', '''    let result_names: Vec<String> = arr
        .iter()
        .map(|item| {''')
replace('crates/cc-eval/src/runner.rs', '                    return Some(s.clone());', '                    return s.clone();')
replace('crates/cc-eval/src/runner.rs', '''            None
        })
        .collect();

    // Recall@5''', '''            String::new()
        })
        .collect();

    // Recall@5''')
replace('crates/cc-eval/src/runner.rs', '''    let mut recall_at_5: Option<f64> = None;
    let mut mrr: Option<f64> = None;''', '''    // Retrieval errors remain zero-valued observations, not missing values
    // silently excluded from the report's denominator.
    let measures_retrieval = case.assertions.iter().any(|a| a.kind == "expected_symbols");
    let mut recall_at_5: Option<f64> = measures_retrieval.then_some(0.0);
    let mut mrr: Option<f64> = measures_retrieval.then_some(0.0);''')
replace('crates/cc-eval/src/bench.rs', '    warm_best_us: u64,', '    warm_us: Vec<u64>,')
replace('crates/cc-eval/src/bench.rs', '    let warm_best_us = durations.iter().copied().min().unwrap_or(0);\n', '')
replace('crates/cc-eval/src/bench.rs', '        warm_best_us,', '        warm_us: durations,')
replace('crates/cc-eval/src/bench.rs', 'group.iter().map(|m| m.warm_best_us).collect()', 'group.iter().flat_map(|m| m.warm_us.iter().copied()).collect()')
replace('crates/cc-eval/src/bench.rs', '1 warmup + 2 measured calls on the shared session, best of the 2.', '1 warmup + 2 measured calls on the shared session, retaining both samples.')
replace('crates/cc-eval/src/bench.rs', '    pub warm_max_us: u64,', '    pub warm_max_us: u64,\n    pub warm_samples: usize,')
replace('crates/cc-eval/src/bench.rs', '                warm_max_us: warm.last().copied().unwrap_or(0),', '                warm_max_us: warm.last().copied().unwrap_or(0),\n                warm_samples: warm.len(),')
replace('crates/cc-index/src/memory_budget.rs', '''    /// Byte cap for file content carried from the scan/diff phase into
    /// parse/enrichment (the single-read pipeline). One eighth of the total
    /// budget: the carried set peaks alongside parse allocations (tree-sitter
    /// trees, symbol/chunk vectors), which the remaining budget must absorb.
    /// Files past this cap simply fall back to a disk re-read in parse.
    pub fn content_carry_budget(&self) -> u64 {
        self.total_budget / 8
    }

''', '')
print('Applied code-index correctness round 1; no embedding dependencies added.')
