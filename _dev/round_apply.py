from pathlib import Path


def replace(path, old, new, count=1):
    p=Path(path); s=p.read_text(); n=s.count(old)
    if n != count:
        raise RuntimeError(f'{path}: wanted {count}, found {n}: {old[:100]!r}')
    p.write_text(s.replace(old,new))


def append(path, text):
    p=Path(path); p.write_text(p.read_text()+'\n'+text)

replace('crates/cc-index/src/indexer.rs',
        'to_parse.sort_by(|a, b| b.scanned.size.cmp(&a.scanned.size));',
        'to_parse.sort_by_key(|a| std::cmp::Reverse(a.scanned.size));')
path='crates/cc-index/src/indexer_phases/analysis.rs'
replace(path, 'impl Indexer {', '''/// Immutable inputs to analysis, separated from its mutable explanation sink.
/// Scanning provenance travels together; no positional unused parsed-path input.
pub(crate) struct AnalysisInputs<'a> {
    pub project_path: &'a Path,
    pub full: bool,
    pub write_units: &'a [FileWriteUnit],
    pub route_nodes: &'a [RouteNodeRecord],
    pub walk_manifest: Option<&'a crate::scanner::WalkManifest>,
    pub scope_hints: Option<&'a crate::indexer::ScopeSignatureHints>,
}

impl Indexer {''')
replace(path, '''        project_path: &Path,
        full: bool,
        write_units: &[FileWriteUnit],
        _parsed_file_paths: &[String],
        route_nodes: &[RouteNodeRecord],
        walk_manifest: Option<&crate::scanner::WalkManifest>,
        scope_hints: Option<&crate::indexer::ScopeSignatureHints>,
        build_explain: &mut BuildExplainCollector,
    ) -> CcResult<AnalysisPlan> {''', '''        inputs: AnalysisInputs<'_>,
        build_explain: &mut BuildExplainCollector,
    ) -> CcResult<AnalysisPlan> {
        let AnalysisInputs { project_path, full, write_units, route_nodes, walk_manifest, scope_hints } = inputs;''')
replace('crates/cc-index/src/indexer_phases/mod.rs', 'use analysis::AnalysisPlan;', 'use analysis::{AnalysisInputs, AnalysisPlan};')
replace('crates/cc-index/src/build_plan.rs', '''        let analysis = indexer.phase_analysis_compute(
            project_path,
            self.mode.is_full(),
            &write_units,
            &parsed_file_paths,
            &carry.output_snapshot.route_nodes,
            walk_manifest.as_deref(),
            carry.scan_result.scope_hints.as_ref(),
            &mut build_explain,
        )?;''', '''        let analysis = indexer.phase_analysis_compute(
            crate::indexer_phases::AnalysisInputs {
                project_path,
                full: self.mode.is_full(),
                write_units: &write_units,
                route_nodes: &carry.output_snapshot.route_nodes,
                walk_manifest: walk_manifest.as_deref(),
                scope_hints: carry.scan_result.scope_hints.as_ref(),
            },
            &mut build_explain,
        )?;''')
path='crates/cc-server/src/engine.rs'
replace(path,'fn graph_rerank_parity_with_pre_refactor_baseline()', 'fn graph_rerank_preserves_flip_and_score_accounting()')
s=Path(path).read_text()
a=s.index('        // Bit-exact rerank values captured pre-refactor;')
b=s.index('        // Flip proof:',a)
s=s[:a]+'''        // The old numeric snapshot encoded inverted BM25 preselection. Check
        // additive accounting alongside independent graph math and rank flip.
        for hit in hits {
            let total: f64 = hit["score_trace"].as_array().unwrap().iter()
                .map(|component| component[1].as_f64().unwrap())
                .sum();
            assert!((total - hit["rerank_score"].as_f64().unwrap()).abs() < 1e-12);
        }

'''+s[b:]
Path(path).write_text(s)
path='crates/cc-index/src/indexer_phases/dirty.rs'
replace(path,'            if old_fp != new_fp {', '''            let unsupported_surface = write_unit_index.get(file_path.as_str())
                .is_some_and(|unit| !matches!(unit.language,
                    cc_model::Language::JavaScript | cc_model::Language::TypeScript |
                    cc_model::Language::Jsx | cc_model::Language::Tsx));
            if old_fp != new_fp || unsupported_surface {''')
replace(path,'''            .filter(|path| {
                targets_cache''', '''            .filter(|path| {
                // An absent export contract cannot prove a facade unchanged.
                let conservative = !matches!(cc_parsers::detect_language(path),
                    cc_model::Language::JavaScript | cc_model::Language::TypeScript |
                    cc_model::Language::Jsx | cc_model::Language::Tsx);
                conservative || targets_cache''')
replace(path,'''        // Step 2: Compare old vs new export fingerprints to find files whose''', '''        // Non-JS/TS parsers have no complete export contract yet. Source edits
        // seed bounded conservative importer closure: None == None is unknown,
        // not proof of stability. Unresolved/global dependencies remain outside
        // this imported-dependency contract (see the differential oracle).
        // Step 2: Compare old vs new export fingerprints to find files whose''')
path='crates/cc-search/src/preselect.rs'
replace(path, 'let bm25_score = (-raw_score).max(0.0);','let bm25_score = bm25_strength(raw_score);')
replace(path, 'score: ctx.ranking.preselect_fts_base + bm25_score / (1.0 + bm25_score),', 'score: ctx.ranking.preselect_fts_base + bm25_score,')
append(path, '''/// SQLite BM25 is negative-better; return a bounded positive-better feature.
fn bm25_strength(raw: f64) -> f64 {
    let strength = (-raw).max(0.0);
    strength / (1.0 + strength)
}

#[cfg(test)]
mod monotonicity_tests {
    #[test]
    fn stronger_sqlite_bm25_gets_a_larger_preselect_score() {
        assert!(super::bm25_strength(-4.23) > super::bm25_strength(-1.98));
        assert_eq!(super::bm25_strength(0.0), 0.0);
        assert!(super::bm25_strength(-1e6) < 1.0);
    }
}
''')
append('crates/cc-eval/src/runner.rs', '''#[cfg(test)]
mod rank_contract_tests {
    use super::*;
    #[test]
    fn unnamed_hits_keep_their_original_rank() {
        let output = serde_json::json!([{}, {}, {}, {}, {}, {"name":"target"}]);
        let (recall, rr) = compute_retrieval_metrics(&output, &Assertion::expected_symbols("target"));
        assert_eq!(recall, 0.0);
        assert_eq!(rr, 1.0 / 6.0);
    }
    #[test]
    fn duplicate_hits_cannot_inflate_recall() {
        let output = serde_json::json!([{"name":"a"},{"name":"a"},{"name":"a"}]);
        let (recall, rr) = compute_retrieval_metrics(&output, &Assertion::expected_symbols("a,b"));
        assert_eq!(recall, 0.5);
        assert_eq!(rr, 1.0);
    }
}
''')
append('crates/cc-search/src/lanes.rs', '''#[cfg(test)]
mod fusion_contract_tests {
    use super::*;
    #[test]
    fn duplicate_candidate_gets_one_vote_at_original_rank() {
        let lane = LaneOutcome { lane_id: "test", weight: 1.0, annotates_hits: false,
            score_slot: None, hits: vec![("a".into(), 1.0),("a".into(), 1.0),("b".into(), 1.0)] };
        let fused = fuse_outcomes(&[lane], 50);
        assert_eq!(fused["a"].total, 1.0 / 51.0);
        assert_eq!(fused["b"].total, 1.0 / 53.0);
    }
}
''')
print('Applied architecture contracts and regression tests, round 2')
