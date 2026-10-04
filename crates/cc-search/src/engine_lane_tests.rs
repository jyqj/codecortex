//! Engine-side tests of the retrieval-lane seam (`RetrievalLane` trait,
//! lane adapters, `run_lanes`/`fuse_outcomes`, lane-rank annotation), moved
//! out of `engine.rs`'s test module unchanged.  They exercise the lanes
//! through `SearchEngine`'s plan/context types, which is why they live here
//! rather than in `lanes.rs`.

use std::sync::Arc;

use cc_db::index_db::{FileWriteUnit, IndexDb};
use cc_model::config::{ProjectConfig, SearchConfig};
use cc_model::search::SearchRequest;
use cc_model::{
    CallEdgeRecord, CcResult, ChunkRecord, Language, ParseOutcome, ParserTier, SymbolRecord,
};

use crate::engine::SearchEngine;
use crate::engine_test_support::{insert_chunk_file, insert_graph_file, scoped_test_engine};
use crate::lanes::{
    fuse_outcomes, materialize_test_outcomes, run_lanes, test_lane_outcome, FusedScore, GraphLane,
    GrepLane, LaneContext, LexicalLane, RetrievalLane, ScoreSlot, LANE_EXACT_SYMBOL, LANE_GRAPH,
    LANE_GREP, LANE_LEXICAL, LANE_PATH,
};
use crate::plan::{CandidateChunk, SearchPlan};

#[test]
fn graph_search_returns_empty_for_no_symbols() {
    let (engine, _tmp) = scoped_test_engine();
    insert_chunk_file(
        &engine,
        "src/alpha.rs",
        Language::Rust,
        "fn alpha_handler() { process() }",
    );

    // GraphLane::search on an empty symbol table should return empty, not error
    let plan = SearchPlan::build(
        &engine.db,
        &engine.config,
        &engine.ranking,
        &SearchRequest {
            query: "alpha".to_string(),
            top_k: 5,
            include_grep: false,
            ..Default::default()
        },
        None,
    )
    .unwrap();
    let graph_hits = GraphLane::search(&engine.db, &plan, plan.query_tokens(), 12);
    // Should succeed (possibly empty — no symbols indexed yet)
    assert!(graph_hits.is_ok());
}

// ── Lane seam (RetrievalLane trait) ────────────────────────

fn build_plan(engine: &SearchEngine, request: &SearchRequest) -> SearchPlan {
    SearchPlan::build(&engine.db, &engine.config, &engine.ranking, request, None).unwrap()
}

fn engine_with(
    search: SearchConfig,
    ranking: cc_model::config::RankingConfig,
) -> (SearchEngine, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let db = IndexDb::open(&tmp.path().join("index.sqlite3")).unwrap().0;
    let config = ProjectConfig {
        search,
        ranking,
        ..Default::default()
    };
    (SearchEngine::new(Arc::new(db), &config, None), tmp)
}

#[test]
fn exact_symbol_lane_ignores_soft_preselect_and_exposes_lane_contract() {
    let (engine, _tmp) = engine_with(
        SearchConfig {
            lexical_weight: 0.0,
            exact_symbol_weight: 1.0,
            path_weight: 0.0,
            grep_weight: 0.0,
            graph_weight: 0.0,
            ..Default::default()
        },
        Default::default(),
    );
    insert_graph_file(
        &engine,
        "src/target.rs",
        "fn sought_symbol() {}",
        "sought_symbol",
        "uid:target",
        Vec::new(),
    );
    insert_graph_file(
        &engine,
        "src/noise.rs",
        "fn unrelated() {}",
        "unrelated",
        "uid:noise",
        Vec::new(),
    );

    let result = engine
        .search_with_diagnostics(&SearchRequest {
            query: "sought_symbol".into(),
            top_k: 1,
            include_grep: false,
            pinned_file_paths: Some(vec!["src/noise.rs".into()]),
            file_preselect_limit: Some(1),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(result.hits[0].file_path, "src/target.rs");
    assert!(result.hits[0].reasons.contains(&"exact-target".into()));
    assert_eq!(result.lanes.len(), 5);
    let exact = result
        .lanes
        .iter()
        .find(|lane| lane.lane_id == LANE_EXACT_SYMBOL)
        .unwrap();
    assert_eq!(exact.status, cc_model::retrieval::LaneStatus::Complete);
    assert_eq!(exact.candidate_count, 1);
    assert!(exact.candidates[0].exact_identity);
    assert_eq!(
        result
            .lanes
            .iter()
            .find(|lane| lane.lane_id == LANE_PATH)
            .unwrap()
            .status,
        cc_model::retrieval::LaneStatus::Disabled
    );
}

#[test]
fn exact_symbol_lane_retains_ambiguity_in_deterministic_order() {
    let (engine, _tmp) = engine_with(
        SearchConfig {
            lexical_weight: 0.0,
            exact_symbol_weight: 1.0,
            path_weight: 0.0,
            grep_weight: 0.0,
            graph_weight: 0.0,
            ..Default::default()
        },
        Default::default(),
    );
    for path in ["src/a.rs", "src/b.rs"] {
        insert_graph_file(
            &engine,
            path,
            "fn duplicate_name() {}",
            "duplicate_name",
            &format!("uid:{path}"),
            Vec::new(),
        );
    }
    let result = engine
        .search_with_diagnostics(&SearchRequest {
            query: "duplicate_name".into(),
            top_k: 2,
            include_grep: false,
            ..Default::default()
        })
        .unwrap();
    let paths: Vec<_> = result
        .hits
        .iter()
        .map(|hit| hit.file_path.as_str())
        .collect();
    assert_eq!(paths, vec!["src/a.rs", "src/b.rs"]);
    let exact = result
        .lanes
        .iter()
        .find(|lane| lane.lane_id == LANE_EXACT_SYMBOL)
        .unwrap();
    assert_eq!(exact.candidate_count, 2);
    assert!(exact
        .candidates
        .iter()
        .all(|candidate| candidate.exact_identity));
}

#[test]
fn path_lane_recalls_exact_path_but_never_widens_hard_scope() {
    let (engine, _tmp) = engine_with(
        SearchConfig {
            lexical_weight: 0.0,
            exact_symbol_weight: 0.0,
            path_weight: 1.0,
            grep_weight: 0.0,
            graph_weight: 0.0,
            ..Default::default()
        },
        Default::default(),
    );
    insert_chunk_file(
        &engine,
        "src/deep/target.rs",
        Language::Rust,
        "fn target() {}",
    );
    insert_chunk_file(&engine, "tests/noise.rs", Language::Rust, "fn noise() {}");

    let found = engine
        .search_with_diagnostics(&SearchRequest {
            query: "src/deep/target.rs".into(),
            top_k: 1,
            include_grep: false,
            pinned_file_paths: Some(vec!["tests/noise.rs".into()]),
            file_preselect_limit: Some(1),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(found.hits[0].file_path, "src/deep/target.rs");
    let path = found
        .lanes
        .iter()
        .find(|lane| lane.lane_id == LANE_PATH)
        .unwrap();
    assert!(path.candidates[0].exact_identity);

    let scoped_out = engine
        .search_with_diagnostics(&SearchRequest {
            query: "src/deep/target.rs".into(),
            top_k: 5,
            include_grep: false,
            path_prefix: Some("tests/".into()),
            ..Default::default()
        })
        .unwrap();
    assert!(scoped_out
        .hits
        .iter()
        .all(|hit| hit.file_path.starts_with("tests/")));
    assert!(scoped_out
        .lanes
        .iter()
        .flat_map(|lane| &lane.candidates)
        .all(|candidate| candidate.legacy_chunk_id != "chunk:src/deep/target.rs"));
}

#[test]
fn non_finite_search_or_ranking_configuration_fails_before_scoring() {
    let search = SearchConfig {
        path_weight: f64::NAN,
        ..Default::default()
    };
    let (engine, _tmp) = engine_with(search, Default::default());
    assert!(engine
        .search_with_diagnostics(&SearchRequest {
            query: "anything".into(),
            ..Default::default()
        })
        .is_err());

    let ranking = cc_model::config::RankingConfig {
        overlap_weight: f64::INFINITY,
        ..Default::default()
    };
    let (engine, _tmp) = engine_with(SearchConfig::default(), ranking);
    assert!(engine
        .search_with_diagnostics(&SearchRequest {
            query: "anything".into(),
            ..Default::default()
        })
        .is_err());
}

#[test]
fn lexical_lane_adapter_matches_inline_ranking() {
    let (engine, _tmp) = scoped_test_engine();
    insert_chunk_file(
        &engine,
        "src/alpha.rs",
        Language::Rust,
        "alphatoken appears here",
    );

    let request = SearchRequest {
        query: "alphatoken".to_string(),
        top_k: 5,
        include_grep: false,
        file_paths: Some(vec!["src/alpha.rs".to_string()]),
        ..Default::default()
    };
    let plan = build_plan(&engine, &request);
    let context = LaneContext {
        plan: &plan,
        db: &engine.db,
        config: &engine.config,
        chunk_text_cache: &engine.chunk_text_cache,
        cache_epoch: 0,
    };

    let lane = LexicalLane;
    assert_eq!(lane.lane_id(), LANE_LEXICAL);
    assert!(lane.is_enabled(&context), "lexical lane always runs");
    assert_eq!(
        lane.weight(&engine.config),
        engine.config.lexical_weight,
        "lexical lane weight comes from lexical_weight"
    );

    let hits = lane.run(&context).unwrap();
    // Native diagnostic score is now separate from the legacy rank slot.
    assert!(hits.iter().all(|(_, raw)| raw.is_finite() && *raw < 0.0));
    let ranks: Vec<_> = hits
        .iter()
        .enumerate()
        .map(|(rank, (id, _))| (id.clone(), 1.0 / (rank + 1) as f64))
        .collect();
    assert_eq!(ranks, vec![("chunk:src/alpha.rs".to_string(), 1.0)]);
}

#[test]
fn grep_lane_adapter_ranks_matches_and_caches_only_hits() {
    let (engine, _tmp) = scoped_test_engine();
    insert_chunk_file(
        &engine,
        "src/g.rs",
        Language::Rust,
        "the needle is right here",
    );
    insert_chunk_file(&engine, "src/other.rs", Language::Rust, "nothing relevant");

    let request = SearchRequest {
        query: "needle".to_string(),
        top_k: 5,
        include_grep: true,
        file_paths: Some(vec!["src/g.rs".to_string(), "src/other.rs".to_string()]),
        ..Default::default()
    };
    let plan = build_plan(&engine, &request);
    let context = LaneContext {
        plan: &plan,
        db: &engine.db,
        config: &engine.config,
        chunk_text_cache: &engine.chunk_text_cache,
        cache_epoch: 0,
    };

    let lane = GrepLane;
    assert_eq!(lane.lane_id(), LANE_GREP);
    assert!(lane.is_enabled(&context), "include_grep=true enables grep");
    assert_eq!(lane.weight(&engine.config), engine.config.grep_weight);

    let hits = lane.run(&context).unwrap();
    assert_eq!(hits, vec![("chunk:src/g.rs".to_string(), 1.0)]);

    // Side effect: only *matched* chunks land in the engine's chunk
    // text cache.  Scan-only rows stay out so a cold scan over a large
    // scope can't rotate the LRU and evict hot entries.
    let mut cache = engine.chunk_text_cache.lock().unwrap();
    assert!(cache.get(&(0, "chunk:src/g.rs".into())).is_some());
    assert!(
        cache.get(&(0, "chunk:src/other.rs".into())).is_none(),
        "non-matching scanned chunk must not enter the text cache"
    );
}

#[test]
fn grep_lane_scan_budget_truncates_recency_first_and_deterministically() {
    let tmp = tempfile::tempdir().unwrap();
    let db = IndexDb::open(&tmp.path().join("index.sqlite3")).unwrap().0;
    let config = ProjectConfig {
        search: SearchConfig {
            lexical_top_k: 3,
            grep_top_k: 10,
            grep_scan_cap: 2,
            lexical_weight: 1.0,
            grep_weight: 0.8,
            ..Default::default()
        },
        ..Default::default()
    };
    let engine = SearchEngine::new(Arc::new(db), &config, None);

    // Four matching files, inserted oldest → newest.  With the scan
    // budget at 2, the unscoped recency-ordered scan only reaches the
    // two most recently indexed files.
    for name in ["a", "b", "c", "d"] {
        insert_chunk_file(
            &engine,
            &format!("src/{name}.rs"),
            Language::Rust,
            "the scanneedle is here",
        );
    }

    // file_preselect_limit=0 empties preselect, so no file scope is
    // materialized and grep takes the unscoped (budgeted) path.
    let request = SearchRequest {
        query: "scanneedle".to_string(),
        top_k: 10,
        include_grep: true,
        file_preselect_limit: Some(0),
        ..Default::default()
    };
    let plan = build_plan(&engine, &request);
    let context = LaneContext {
        plan: &plan,
        db: &engine.db,
        config: &engine.config,
        chunk_text_cache: &engine.chunk_text_cache,
        cache_epoch: 0,
    };

    let first = GrepLane.run(&context).unwrap();
    assert_eq!(
        first,
        vec![
            ("chunk:src/d.rs".to_string(), 1.0),
            ("chunk:src/c.rs".to_string(), 0.5),
        ],
        "budget of 2 must cover exactly the two most recently indexed files"
    );

    // Determinism: same index + same config + same query → same result.
    let second = GrepLane.run(&context).unwrap();
    assert_eq!(first, second);
}

#[test]
fn grep_lane_disabled_when_request_excludes_grep() {
    let (engine, _tmp) = scoped_test_engine();
    let request = SearchRequest {
        query: "needle".to_string(),
        top_k: 5,
        include_grep: false,
        ..Default::default()
    };
    let plan = build_plan(&engine, &request);
    let context = LaneContext {
        plan: &plan,
        db: &engine.db,
        config: &engine.config,
        chunk_text_cache: &engine.chunk_text_cache,
        cache_epoch: 0,
    };
    assert!(!GrepLane.is_enabled(&context));
}

#[test]
fn graph_lane_adapter_ranks_seed_above_one_hop_neighbor() {
    let (engine, _tmp) = scoped_test_engine();
    let process_to_helper = CallEdgeRecord {
        edge_id: "edge:process->helper".to_string(),
        file_path: "src/a.rs".to_string(),
        caller_symbol: Some("process".to_string()),
        callee_symbol: "helper".to_string(),
        line: 1,
        caller_symbol_uid: Some("uid:process".to_string()),
        callee_symbol_uid: Some("uid:helper".to_string()),
        ..Default::default()
    };
    insert_graph_file(
        &engine,
        "src/a.rs",
        "fn process() { helper() }",
        "process",
        "uid:process",
        vec![process_to_helper],
    );
    insert_graph_file(
        &engine,
        "src/b.rs",
        "fn helper() {}",
        "helper",
        "uid:helper",
        vec![],
    );

    let request = SearchRequest {
        query: "process".to_string(),
        top_k: 5,
        include_grep: false,
        file_paths: Some(vec!["src/a.rs".to_string(), "src/b.rs".to_string()]),
        ..Default::default()
    };
    let plan = build_plan(&engine, &request);
    let context = LaneContext {
        plan: &plan,
        db: &engine.db,
        config: &engine.config,
        chunk_text_cache: &engine.chunk_text_cache,
        cache_epoch: 0,
    };

    let lane = GraphLane;
    assert_eq!(lane.lane_id(), LANE_GRAPH);
    assert_eq!(lane.weight(&engine.config), engine.config.graph_weight);

    let hits = lane.run(&context).unwrap();
    assert_eq!(
        hits,
        vec![
            ("chunk:src/a.rs".to_string(), 1.0),
            ("chunk:src/b.rs".to_string(), 0.5),
        ],
        "seed symbol chunk first, 1-hop callee chunk at half score"
    );
}

#[test]
fn graph_lane_respects_languages_filter_by_file_extension() {
    let (engine, _tmp) = scoped_test_engine();
    insert_graph_file(
        &engine,
        "src/a.rs",
        "fn process() {}",
        "process",
        "uid:process",
        vec![],
    );

    let request = SearchRequest {
        query: "process".to_string(),
        top_k: 5,
        include_grep: false,
        languages: Some(vec![Language::Rust]),
        file_paths: Some(vec!["src/a.rs".to_string()]),
        ..Default::default()
    };
    let plan = build_plan(&engine, &request);

    // Symbols live in a .rs file, the filter asks for Rust — the graph
    // lane must keep the seed hit instead of misclassifying the file as
    // Language::Unknown (regression: a file *path* was passed where a
    // language *name* was expected).
    let hits = GraphLane::search(&engine.db, &plan, plan.query_tokens(), 12).unwrap();
    assert_eq!(
        hits,
        vec![("chunk:src/a.rs".to_string(), 1.0)],
        "languages=[Rust] must not drop the Rust seed symbol's chunk"
    );
}

#[test]
fn graph_lane_disabled_when_weight_is_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let db = IndexDb::open(&tmp.path().join("index.sqlite3")).unwrap().0;
    let config = ProjectConfig {
        search: SearchConfig {
            graph_weight: 0.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let engine = SearchEngine::new(Arc::new(db), &config, None);
    let request = SearchRequest {
        query: "anything".to_string(),
        top_k: 5,
        include_grep: false,
        ..Default::default()
    };
    let plan = build_plan(&engine, &request);
    let context = LaneContext {
        plan: &plan,
        db: &engine.db,
        config: &engine.config,
        chunk_text_cache: &engine.chunk_text_cache,
        cache_epoch: 0,
    };
    assert!(
        !GraphLane.is_enabled(&context),
        "graph lane must short-circuit before any work when weight is 0"
    );
}

#[test]
fn graph_lane_expands_caller_direction_at_decay() {
    // Mirror of the callee-direction golden test: the query matches the
    // CALLEE, and the caller must be pulled in at graph_neighbor_decay.
    let (engine, _tmp) = scoped_test_engine();
    let process_to_helper = CallEdgeRecord {
        edge_id: "edge:process->helper".to_string(),
        file_path: "src/a.rs".to_string(),
        caller_symbol: Some("process".to_string()),
        callee_symbol: "helper".to_string(),
        line: 1,
        caller_symbol_uid: Some("uid:process".to_string()),
        callee_symbol_uid: Some("uid:helper".to_string()),
        ..Default::default()
    };
    insert_graph_file(
        &engine,
        "src/a.rs",
        "fn process() { helper() }",
        "process",
        "uid:process",
        vec![process_to_helper],
    );
    insert_graph_file(
        &engine,
        "src/b.rs",
        "fn helper() {}",
        "helper",
        "uid:helper",
        vec![],
    );

    let request = SearchRequest {
        query: "helper".to_string(),
        top_k: 5,
        include_grep: false,
        ..Default::default()
    };
    let plan = build_plan(&engine, &request);
    let hits = GraphLane::search(&engine.db, &plan, plan.query_tokens(), 12).unwrap();
    assert_eq!(
        hits,
        vec![
            ("chunk:src/b.rs".to_string(), 1.0),
            ("chunk:src/a.rs".to_string(), 0.5),
        ],
        "seed (callee) chunk first, 1-hop caller chunk at decay score"
    );
}

#[test]
fn graph_lane_scores_fuzzy_seed_below_exact_seed() {
    // Exact name match seeds at graph_seed_exact_score (1.0); a substring
    // match seeds at graph_seed_fuzzy_score (0.5).
    let (engine, _tmp) = scoped_test_engine();
    insert_graph_file(
        &engine,
        "src/exact.rs",
        "fn process() {}",
        "process",
        "uid:process",
        vec![],
    );
    insert_graph_file(
        &engine,
        "src/fuzzy.rs",
        "fn process_batch() {}",
        "process_batch",
        "uid:process_batch",
        vec![],
    );

    let request = SearchRequest {
        query: "process".to_string(),
        top_k: 5,
        include_grep: false,
        ..Default::default()
    };
    let plan = build_plan(&engine, &request);
    let hits = GraphLane::search(&engine.db, &plan, plan.query_tokens(), 12).unwrap();
    assert_eq!(
        hits,
        vec![
            ("chunk:src/exact.rs".to_string(), 1.0),
            ("chunk:src/fuzzy.rs".to_string(), 0.5),
        ],
        "exact-name seed must outrank substring seed"
    );
}

#[test]
fn graph_lane_seeds_short_tokens_by_exact_name() {
    // Tokens under 3 chars cannot use the trigram table; they seed via
    // exact-name equality (both common casings) instead of being dropped.
    let (engine, _tmp) = scoped_test_engine();
    insert_graph_file(&engine, "src/ok.rs", "fn ok() {}", "ok", "uid:ok", vec![]);

    let request = SearchRequest {
        query: "ok".to_string(),
        top_k: 5,
        include_grep: false,
        ..Default::default()
    };
    let plan = build_plan(&engine, &request);
    let hits = GraphLane::search(&engine.db, &plan, plan.query_tokens(), 12).unwrap();
    assert_eq!(
        hits,
        vec![("chunk:src/ok.rs".to_string(), 1.0)],
        "a 2-char exact symbol name must still seed the graph lane"
    );
}

#[test]
fn graph_lane_maps_symbol_to_smallest_containing_chunk() {
    // A file with a wide chunk and a narrow chunk that both contain the
    // symbol span: the lane must pick the narrowest container.
    let (engine, _tmp) = scoped_test_engine();
    let source_text = format!("\nfn narrow_fn() {{\n}}\n{}", "// padding\n".repeat(47));
    let source = cc_model::source::SourceSnapshot::new(source_text.as_bytes());
    let make_chunk = |chunk_id: &str, index: i64, start: u32, end: u32| {
        let span = source.line_range(start as usize, end as usize).unwrap();
        ChunkRecord {
            source: Some(cc_model::source::ChunkSource {
                source: source.identity().clone(),
                span,
                slice_digest: source.slice_digest(span).unwrap(),
                boundary: "graph-fixture".into(),
                owner: Some(source.line_range(2, 3).unwrap()),
                signature: Some(source.line_range(2, 2).unwrap()),
            }),
            chunk_id: chunk_id.to_string(),
            file_path: "src/wide.rs".to_string(),
            language: Language::Rust,
            chunk_index: index as u32,
            start_line: start,
            end_line: end,
            breadcrumb: "root".to_string(),
            text: source.slice(span).unwrap().to_string(),
            symbol_name: None,
            symbol_kind: None,
            token_estimate: 8,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 1.0,
        }
    };
    let symbol = SymbolRecord {
        symbol_id: "sym:src/wide.rs:narrow_fn".to_string(),
        file_path: "src/wide.rs".to_string(),
        name: "narrow_fn".to_string(),
        kind: cc_model::SymbolKind::Function,
        container: None,
        start_line: 2,
        end_line: 3,
        start_col: 0,
        end_col: 0,
        signature: None,
        doc: None,
        parser_tier: ParserTier::TreeSitter,
        parser_confidence: 1.0,
        qname: None,
        parent_symbol_id: None,
        scope_id: None,
        export_name: None,
        is_default_export: false,
        symbol_uid: Some("uid:narrow_fn".to_string()),
        framework_role: None,
        receiver_type: None,
        param_types: None,
        return_type: None,
        param_count: None,
        base_types: None,
        implements: None,
    };
    let outcome = ParseOutcome {
        summary: "fixture".to_string(),
        chunks: vec![
            make_chunk("chunk:wide", 0, 1, 50),
            make_chunk("chunk:narrow", 1, 1, 5),
        ],
        symbols: vec![symbol],
        parser_tier: ParserTier::TreeSitter,
        parser_confidence: 1.0,
        ..Default::default()
    };
    let conn = crate::test_seed::seed_conn(&engine.db);
    IndexDb::insert_file_data(
        &conn,
        &FileWriteUnit {
            rel_path: "src/wide.rs".to_string(),
            language: Language::Rust,
            content_hash: "hash-wide".to_string(),
            mtime: 0.0,
            size: 10,
            outcome,
        },
    )
    .unwrap();
    drop(conn);

    let request = SearchRequest {
        query: "narrow_fn".to_string(),
        top_k: 5,
        include_grep: false,
        ..Default::default()
    };
    let plan = build_plan(&engine, &request);
    let hits = GraphLane::search(&engine.db, &plan, plan.query_tokens(), 12).unwrap();
    assert_eq!(
        hits,
        vec![("chunk:narrow".to_string(), 1.0)],
        "the smallest chunk containing the symbol span must win"
    );
}

/// Synthetic lane for exercising the generic lane loop.
struct FakeLane {
    id: &'static str,
    enabled: bool,
    lane_weight: f64,
    annotates: bool,
    hits: Vec<(String, f64)>,
    ran: std::sync::atomic::AtomicBool,
}

impl RetrievalLane for FakeLane {
    fn lane_id(&self) -> &'static str {
        self.id
    }
    fn weight(&self, _config: &SearchConfig) -> f64 {
        self.lane_weight
    }
    fn is_enabled(&self, _context: &LaneContext<'_>) -> bool {
        self.enabled
    }
    fn annotates_hits(&self) -> bool {
        self.annotates
    }
    fn run(&self, _context: &LaneContext<'_>) -> CcResult<Vec<(String, f64)>> {
        self.ran.store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(self.hits.clone())
    }
}

fn fake_candidate_chunk() -> CandidateChunk {
    CandidateChunk {
        qname: None,
        document: None,
        source_evidence: None,
        chunk_id: "chunk:src/x.rs".to_string(),
        file_path: "src/x.rs".to_string(),
        language_name: "rust".to_string(),
        start_line: 1,
        end_line: 2,
        breadcrumb: "root".to_string(),
        symbol_name: None,
        symbol_kind: None,
        text: "fn x() {}".to_string(),
    }
}

#[test]
fn new_lane_opting_into_annotation_gets_generic_hit_reasons() {
    // Extensibility guarantee: a brand-new lane that opts into per-hit
    // annotation (`annotates_hits() == true`) must surface `{lane_id}@{rank}`
    // reasons WITHOUT any edits to plan.rs lane-id whitelists.
    let (engine, _tmp) = scoped_test_engine();
    let request = SearchRequest {
        query: "anything".to_string(),
        top_k: 5,
        include_grep: false,
        ..Default::default()
    };
    let plan = build_plan(&engine, &request);
    let context = LaneContext {
        plan: &plan,
        db: &engine.db,
        config: &engine.config,
        chunk_text_cache: &engine.chunk_text_cache,
        cache_epoch: 0,
    };

    let fourth = FakeLane {
        id: "fourth",
        enabled: true,
        lane_weight: 1.0,
        annotates: true,
        hits: vec![("chunk:src/x.rs".to_string(), 1.0)],
        ran: std::sync::atomic::AtomicBool::new(false),
    };
    let fifth = FakeLane {
        id: "fifth",
        enabled: true,
        lane_weight: 1.0,
        annotates: true,
        hits: vec![
            ("chunk:other".to_string(), 1.0),
            ("chunk:src/x.rs".to_string(), 0.5),
        ],
        ran: std::sync::atomic::AtomicBool::new(false),
    };

    let lanes: [&dyn RetrievalLane; 2] = [&fourth, &fifth];
    let outcomes = run_lanes(&lanes, &context).unwrap();
    let lane_ranks = plan.lane_ranks(&outcomes);

    let hit = plan
        .hit_from_chunk(
            fake_candidate_chunk(),
            &FusedScore {
                total: 0.5,
                by_lane: vec![],
                exact_identity: false,
            },
            &lane_ranks,
        )
        .unwrap();

    assert_eq!(
        hit.reasons,
        vec!["fourth@1".to_string(), "fifth@2".to_string()],
        "annotating lanes must contribute {{lane_id}}@{{rank}} reasons in lane order"
    );
    // Lanes without a declared ScoreSlot have no dedicated score field
    // in SearchHit (cc-model fields are fixed); built-ins stay 0.0.
    assert_eq!(hit.lexical_score, 0.0);
    assert_eq!(hit.grep_score, 0.0);
    assert_eq!(hit.graph_score, 0.0);
}

#[test]
fn new_lane_declaring_score_slot_projects_without_plan_edits() {
    // Extensibility guarantee: a brand-new lane that declares an
    // existing ScoreSlot gets its rank-derived score projected into the
    // matching SearchHit field purely via trait impl + registration —
    // no lane-id match arm anywhere in plan.rs or engine.rs.
    struct SlottedLane;
    impl RetrievalLane for SlottedLane {
        fn lane_id(&self) -> &'static str {
            "semantic"
        }
        fn weight(&self, _config: &SearchConfig) -> f64 {
            1.0
        }
        fn is_enabled(&self, _context: &LaneContext<'_>) -> bool {
            true
        }
        fn annotates_hits(&self) -> bool {
            true
        }
        fn score_slot(&self) -> Option<ScoreSlot> {
            Some(ScoreSlot::Graph)
        }
        fn run(&self, _context: &LaneContext<'_>) -> CcResult<Vec<(String, f64)>> {
            Ok(vec![
                ("chunk:other".to_string(), 1.0),
                ("chunk:src/x.rs".to_string(), 0.5),
            ])
        }
    }

    let (engine, _tmp) = scoped_test_engine();
    let request = SearchRequest {
        query: "anything".to_string(),
        top_k: 5,
        include_grep: false,
        ..Default::default()
    };
    let plan = build_plan(&engine, &request);
    let context = LaneContext {
        plan: &plan,
        db: &engine.db,
        config: &engine.config,
        chunk_text_cache: &engine.chunk_text_cache,
        cache_epoch: 0,
    };

    let slotted = SlottedLane;
    let lanes: [&dyn RetrievalLane; 1] = [&slotted];
    let outcomes = run_lanes(&lanes, &context).unwrap();
    let lane_ranks = plan.lane_ranks(&outcomes);

    let hit = plan
        .hit_from_chunk(
            fake_candidate_chunk(),
            &FusedScore {
                total: 0.5,
                by_lane: vec![],
                exact_identity: false,
            },
            &lane_ranks,
        )
        .unwrap();

    assert_eq!(hit.reasons, vec!["semantic@2".to_string()]);
    assert_eq!(
        hit.graph_score, 0.5,
        "declared slot must receive the lane's rank-derived score (1/rank)"
    );
    assert_eq!(hit.lexical_score, 0.0);
    assert_eq!(hit.grep_score, 0.0);
}

#[test]
fn default_lanes_registry_keeps_fusion_order() {
    // The registry is the single registration point; its order is the
    // deterministic RRF fusion order.
    let lanes = crate::lanes::default_lanes();
    let ids: Vec<&str> = lanes.iter().map(|lane| lane.lane_id()).collect();
    assert_eq!(
        ids,
        vec![
            LANE_EXACT_SYMBOL,
            LANE_PATH,
            LANE_LEXICAL,
            LANE_GREP,
            LANE_GRAPH
        ]
    );
}

#[test]
fn lane_opting_out_of_annotation_stays_fusion_only() {
    // A lane with annotates_hits() == false (like the graph lane) must
    // still feed RRF fusion but contribute no per-hit reason.
    let (engine, _tmp) = scoped_test_engine();
    let request = SearchRequest {
        query: "anything".to_string(),
        top_k: 5,
        include_grep: false,
        ..Default::default()
    };
    let plan = build_plan(&engine, &request);
    let context = LaneContext {
        plan: &plan,
        db: &engine.db,
        config: &engine.config,
        chunk_text_cache: &engine.chunk_text_cache,
        cache_epoch: 0,
    };

    let silent = FakeLane {
        id: "silent",
        enabled: true,
        lane_weight: 1.0,
        annotates: false,
        hits: vec![("chunk:src/x.rs".to_string(), 1.0)],
        ran: std::sync::atomic::AtomicBool::new(false),
    };

    let lanes: [&dyn RetrievalLane; 1] = [&silent];
    let mut outcomes = run_lanes(&lanes, &context).unwrap();
    materialize_test_outcomes(&mut outcomes);

    let fused = fuse_outcomes(&outcomes, 50).unwrap();
    assert!(
        fused.contains_key("chunk:src/x.rs"),
        "opted-out lane must still contribute to RRF fusion"
    );

    let lane_ranks = plan.lane_ranks(&outcomes);
    let hit = plan
        .hit_from_chunk(
            fake_candidate_chunk(),
            &fused["chunk:src/x.rs"],
            &lane_ranks,
        )
        .unwrap();
    assert!(
        hit.reasons.is_empty(),
        "fusion-only lane must produce no per-hit reasons, got {:?}",
        hit.reasons
    );
}

#[test]
fn run_lanes_iterates_collection_and_skips_disabled_lane_before_work() {
    let (engine, _tmp) = scoped_test_engine();
    let request = SearchRequest {
        query: "anything".to_string(),
        top_k: 5,
        include_grep: false,
        ..Default::default()
    };
    let plan = build_plan(&engine, &request);
    let context = LaneContext {
        plan: &plan,
        db: &engine.db,
        config: &engine.config,
        chunk_text_cache: &engine.chunk_text_cache,
        cache_epoch: 0,
    };

    let active = FakeLane {
        id: "fake-active",
        enabled: true,
        lane_weight: 1.0,
        annotates: false,
        hits: vec![("x".to_string(), 1.0), ("y".to_string(), 0.5)],
        ran: std::sync::atomic::AtomicBool::new(false),
    };
    let disabled = FakeLane {
        id: "fake-disabled",
        enabled: false,
        lane_weight: 0.0,
        annotates: false,
        hits: vec![("z".to_string(), 1.0)],
        ran: std::sync::atomic::AtomicBool::new(false),
    };

    let lanes: [&dyn RetrievalLane; 2] = [&active, &disabled];
    let outcomes = run_lanes(&lanes, &context).unwrap();

    assert!(
        active.ran.load(std::sync::atomic::Ordering::SeqCst),
        "enabled lane must run"
    );
    assert!(
        !disabled.ran.load(std::sync::atomic::Ordering::SeqCst),
        "disabled lane must be skipped before work"
    );
    assert_eq!(
        outcomes.iter().map(|o| o.lane_id).collect::<Vec<_>>(),
        vec!["fake-active", "fake-disabled"],
        "outcome order must follow lane collection order"
    );
    assert_eq!(outcomes[0].hits.len(), 2);
    assert!(
        outcomes[1].hits.is_empty(),
        "skipped lane contributes nothing"
    );
}

#[test]
fn fuse_outcomes_accumulates_rrf_generically() {
    let outcomes = vec![
        test_lane_outcome(
            "fake-a",
            1.0,
            vec![("x".to_string(), 1.0), ("y".to_string(), 0.5)],
        ),
        test_lane_outcome("fake-b", 0.5, vec![("y".to_string(), 1.0)]),
    ];
    let fused = fuse_outcomes(&outcomes, 50).unwrap();

    // score(d) = sum over lanes of weight / (k + rank)
    assert!((fused["x"].total - 1.0 / 51.0).abs() < 1e-12);
    assert!((fused["y"].total - (1.0 / 52.0 + 0.5 / 51.0)).abs() < 1e-12);

    // Per-lane breakdown is preserved in lane-accumulation order and
    // sums (left-to-right) to the fused total bit-for-bit.
    assert_eq!(fused["x"].by_lane, vec![("fake-a".to_string(), 1.0 / 51.0)]);
    assert_eq!(
        fused["y"].by_lane,
        vec![
            ("fake-a".to_string(), 1.0 / 52.0),
            ("fake-b".to_string(), 0.5 / 51.0)
        ]
    );
    for fused_score in fused.values() {
        let component_sum: f64 = fused_score.by_lane.iter().map(|(_, v)| v).sum();
        assert_eq!(component_sum, fused_score.total);
    }
}

#[test]
fn graph_lane_does_not_break_existing_search() {
    // Ensure enabling graph_weight > 0 doesn't change results when no
    // symbols exist: search should still return lexical-only results.
    let tmp = tempfile::tempdir().unwrap();
    let db = IndexDb::open(&tmp.path().join("index.sqlite3")).unwrap().0;
    let config = ProjectConfig {
        search: SearchConfig {
            lexical_top_k: 3,
            grep_top_k: 3,
            rrf_k: 50,
            lexical_weight: 1.0,
            grep_weight: 0.0,
            rerank_window: 3,
            graph_weight: 0.6,
            graph_top_k: 12,
            ..Default::default()
        },
        ..Default::default()
    };
    let engine = SearchEngine::new(Arc::new(db), &config, None);
    insert_chunk_file(
        &engine,
        "src/foo.rs",
        Language::Rust,
        "fn foo_handler() { do_work() }",
    );

    let request = SearchRequest {
        query: "foo".to_string(),
        top_k: 5,
        include_grep: false,
        ..Default::default()
    };
    let results = engine.search(&request).unwrap();
    assert!(!results.is_empty(), "lexical results should still work");
}

#[test]
fn query_target_gates_only_name_bonus_and_preserves_exact_identity() {
    let (engine, _tmp) = scoped_test_engine();
    let request = SearchRequest {
        query: "method Ignite on Lantern".into(),
        ..Default::default()
    };
    let plan = build_plan(&engine, &request);
    let outcomes = vec![];
    let ranks = plan.lane_ranks(&outcomes);
    let make_hit = |name: &str, kind: &str, exact_identity| {
        let mut chunk = fake_candidate_chunk();
        chunk.symbol_name = Some(name.into());
        chunk.symbol_kind = Some(kind.into());
        plan.hit_from_chunk(
            chunk,
            &FusedScore {
                total: 0.5,
                by_lane: vec![],
                exact_identity,
            },
            &ranks,
        )
        .unwrap()
    };
    let member = make_hit("Ignite", "method", false);
    let receiver = make_hit("Lantern", "class", false);
    let collision = make_hit("Ignite", "class", false);
    let exact = make_hit("Lantern", "class", true);
    for (hit, boost) in [
        (&member, true),
        (&receiver, false),
        (&collision, false),
        (&exact, true),
    ] {
        assert_eq!(hit.reasons.iter().any(|r| r == "symbol-exact"), boost);
        assert_eq!(
            hit.score_trace
                .iter()
                .any(|(key, _)| key == "boost:symbol-exact"),
            boost
        );
        assert_eq!(
            hit.score_trace
                .iter()
                .map(|(_, amount)| amount)
                .sum::<f64>(),
            hit.rerank_score
        );
    }
    // All non-name contributions are identical for the same receiver chunk.
    assert_eq!(
        receiver.score_trace,
        exact
            .score_trace
            .iter()
            .filter(|(key, _)| key != "boost:symbol-exact")
            .cloned()
            .collect::<Vec<_>>()
    );
    assert!(exact.reasons.iter().any(|r| r == "exact-target"));
    let mut hits = [member, exact];
    hits.sort_by(crate::plan::compare_hits);
    assert_eq!(hits[0].symbol_name.as_deref(), Some("Lantern"));
}

// Focused SearchPlan integration controls for the bounded owner hint.
#[test]
fn query_target_contextual_owner_preserves_exact_identity_and_non_name_trace() {
    let (engine, _tmp) = scoped_test_engine();
    let request = SearchRequest {
        query: "Which Lantern API changes the stored state with Ignite?".into(),
        ..Default::default()
    };
    let plan = build_plan(&engine, &request);
    let outcomes = vec![];
    let ranks = plan.lane_ranks(&outcomes);
    let make_hit = |name: &str, kind: Option<&str>, exact_identity| {
        let mut chunk = fake_candidate_chunk();
        chunk.symbol_name = Some(name.into());
        chunk.symbol_kind = kind.map(str::to_owned);
        plan.hit_from_chunk(
            chunk,
            &FusedScore {
                total: 0.5,
                by_lane: vec![],
                exact_identity,
            },
            &ranks,
        )
        .unwrap()
    };
    let member = make_hit("Ignite", Some("method"), false);
    let receiver = make_hit("Lantern", Some("class"), false);
    let other_type = make_hit("Ignite", Some("class"), false);
    let same_named_method = make_hit("Lantern", Some("method"), false);
    let unknown_kind = make_hit("Lantern", None, false);
    let exact = make_hit("Lantern", Some("class"), true);
    for (hit, boosted) in [
        (&member, true),
        (&receiver, false),
        (&other_type, true),
        (&same_named_method, true),
        (&unknown_kind, true),
        (&exact, true),
    ] {
        assert_eq!(hit.reasons.iter().any(|r| r == "symbol-exact"), boosted);
        assert_eq!(
            hit.score_trace
                .iter()
                .any(|(k, _)| k == "boost:symbol-exact"),
            boosted
        );
        assert_eq!(
            hit.score_trace.iter().map(|(_, value)| value).sum::<f64>(),
            hit.rerank_score
        );
    }
    assert_eq!(
        receiver.symbol_kind,
        Some(cc_model::symbol::SymbolKind::Class)
    );
    assert_eq!(
        receiver.score_trace,
        exact
            .score_trace
            .iter()
            .filter(|(k, _)| k != "boost:symbol-exact")
            .cloned()
            .collect::<Vec<_>>()
    );
    assert!(exact.reasons.iter().any(|r| r == "exact-target"));
    let mut hits = [member, exact];
    hits.sort_by(crate::plan::compare_hits);
    assert_eq!(hits[0].symbol_name.as_deref(), Some("Lantern"));
}

#[test]
fn query_target_contextual_owner_keeps_nonmatched_fallback_and_explicit_filters() {
    let (engine, _tmp) = scoped_test_engine();
    for (query, name, kind) in [
        ("class Lantern", "Lantern", "class"),
        ("interface Observer", "Observer", "interface"),
        ("type Label", "Label", "type_alias"),
        ("Lantern.Ignite", "Lantern", "class"),
        ("Lantern.spec.py", "Lantern", "class"),
        (
            "compare Lantern.Ignite and Shelf.Arrange",
            "Lantern",
            "class",
        ),
        (
            "Which Lantern API compares Ignite and Reserve?",
            "Lantern",
            "class",
        ),
        ("Which Lantern type stores the state?", "Lantern", "class"),
        (
            "name:Lantern Which Lantern API invokes Ignite?",
            "Lantern",
            "class",
        ),
        (
            "kind:class Which Lantern API invokes Ignite?",
            "Lantern",
            "class",
        ),
    ] {
        let request = SearchRequest {
            query: query.into(),
            ..Default::default()
        };
        let plan = build_plan(&engine, &request);
        let outcomes = vec![];
        let ranks = plan.lane_ranks(&outcomes);
        let mut chunk = fake_candidate_chunk();
        chunk.symbol_name = Some(name.into());
        chunk.symbol_kind = Some(kind.into());
        let hit = plan
            .hit_from_chunk(
                chunk,
                &FusedScore {
                    total: 0.5,
                    by_lane: vec![],
                    exact_identity: false,
                },
                &ranks,
            )
            .unwrap();
        assert!(hit.reasons.iter().any(|r| r == "symbol-exact"), "{query}");
    }
}

#[test]
fn query_target_contextual_owner_uses_primary_query_not_conversation_context() {
    let (engine, _tmp) = scoped_test_engine();
    for (primary, extra, expected_boost) in [
        ("Lantern Ignite", "Which Lantern API invokes Ignite?", true),
        ("Which Lantern API invokes Ignite?", "class Lantern", false),
        ("class Lantern", "Which Lantern API invokes Ignite?", true),
    ] {
        let request = SearchRequest {
            query: primary.into(),
            conversation_queries: Some(vec![extra.into()]),
            ..Default::default()
        };
        let plan = build_plan(&engine, &request);
        let outcomes = vec![];
        let ranks = plan.lane_ranks(&outcomes);
        let mut chunk = fake_candidate_chunk();
        chunk.symbol_name = Some("Lantern".into());
        chunk.symbol_kind = Some("class".into());
        let hit = plan
            .hit_from_chunk(
                chunk,
                &FusedScore {
                    total: 0.5,
                    by_lane: vec![],
                    exact_identity: false,
                },
                &ranks,
            )
            .unwrap();
        assert_eq!(
            hit.reasons.iter().any(|r| r == "symbol-exact"),
            expected_boost,
            "{primary}"
        );
    }
}

#[test]
fn query_target_comparison_phrase_preserves_bonus_trace_and_exact_identity() {
    let (engine, _tmp) = scoped_test_engine();
    for query in [
        "Which Beacon API rather than Sink accepts state?",
        "Which Beacon API RATHER THAN Sink accepts state?",
        "Which API on Beacon instead of Sink accepts state?",
    ] {
        let request = SearchRequest {
            query: query.into(),
            ..Default::default()
        };
        let plan = build_plan(&engine, &request);
        let outcomes = vec![];
        let ranks = plan.lane_ranks(&outcomes);
        for (name, kind, exact_identity) in [
            ("Beacon", "class", false),
            ("Beacon", "class", true),
            ("Sink", "interface", false),
            ("Beacon", "method", false),
        ] {
            let mut chunk = fake_candidate_chunk();
            chunk.symbol_name = Some(name.into());
            chunk.symbol_kind = Some(kind.into());
            let hit = plan
                .hit_from_chunk(
                    chunk,
                    &FusedScore {
                        total: 0.5,
                        by_lane: vec![],
                        exact_identity,
                    },
                    &ranks,
                )
                .unwrap();
            assert!(
                hit.reasons.iter().any(|r| r == "symbol-exact"),
                "{query} / {name}"
            );
            assert_eq!(
                hit.score_trace
                    .iter()
                    .filter(|(k, _)| k == "boost:symbol-exact")
                    .map(|(_, v)| *v)
                    .collect::<Vec<_>>(),
                vec![0.18]
            );
            assert_eq!(
                hit.score_trace.iter().map(|(_, v)| v).sum::<f64>(),
                hit.rerank_score
            );
            assert_eq!(
                hit.reasons.iter().any(|r| r == "exact-target"),
                exact_identity
            );
        }
    }
}

#[test]
fn query_target_code_reference_pairs_preserve_owner_suppression() {
    let (engine, _tmp) = scoped_test_engine();
    for query in [
        "Which Beacon API calls `rather()` `than()` with Commit?",
        "Which Beacon API calls $rather $than with Commit?",
    ] {
        let request = SearchRequest {
            query: query.into(),
            ..Default::default()
        };
        let plan = build_plan(&engine, &request);
        let outcomes = vec![];
        let ranks = plan.lane_ranks(&outcomes);
        for (name, kind, exact_identity, expected_boost) in [
            ("Beacon", "class", false, false),
            ("Beacon", "class", true, true),
            ("Beacon", "method", false, true),
            ("Commit", "method", false, true),
        ] {
            let mut chunk = fake_candidate_chunk();
            chunk.symbol_name = Some(name.into());
            chunk.symbol_kind = Some(kind.into());
            let hit = plan
                .hit_from_chunk(
                    chunk,
                    &FusedScore {
                        total: 0.5,
                        by_lane: vec![],
                        exact_identity,
                    },
                    &ranks,
                )
                .unwrap();
            assert_eq!(
                hit.reasons.iter().any(|r| r == "symbol-exact"),
                expected_boost,
                "{query} / {name}"
            );
            assert_eq!(
                hit.score_trace
                    .iter()
                    .any(|(k, _)| k == "boost:symbol-exact"),
                expected_boost
            );
            assert_eq!(
                hit.score_trace.iter().map(|(_, v)| v).sum::<f64>(),
                hit.rerank_score
            );
            assert_eq!(
                hit.reasons.iter().any(|r| r == "exact-target"),
                exact_identity
            );
        }
    }
}

#[test]
fn query_target_grouped_clause_preserves_bonus_and_exact_identity() {
    let (engine, _tmp) = scoped_test_engine();
    for query in [
        "Which Beacon API (rather than Sink) accepts state?",
        "Which Beacon API (instead of Sink) accepts state?",
        "Which Beacon API [(rather than the Sink)] accepts state?",
    ] {
        let request = SearchRequest { query: query.into(), ..Default::default() };
        let plan = build_plan(&engine, &request);
        let outcomes = vec![];
        let ranks = plan.lane_ranks(&outcomes);
        for (name, kind, exact_identity) in [("Beacon", "class", false), ("Beacon", "class", true), ("Sink", "interface", false)] {
            let mut chunk = fake_candidate_chunk();
            chunk.symbol_name = Some(name.into()); chunk.symbol_kind = Some(kind.into());
            let hit = plan.hit_from_chunk(chunk, &FusedScore { total: 0.5, by_lane: vec![], exact_identity }, &ranks).unwrap();
            assert!(hit.reasons.iter().any(|r| r == "symbol-exact"), "{query} / {name}");
            assert_eq!(hit.score_trace.iter().filter(|(k, _)| k == "boost:symbol-exact").map(|(_, v)| *v).collect::<Vec<_>>(), vec![0.18]);
            assert_eq!(hit.reasons.iter().any(|r| r == "exact-target"), exact_identity);
            assert_eq!(hit.score_trace.iter().map(|(_, v)| v).sum::<f64>(), hit.rerank_score);
        }
    }
}
