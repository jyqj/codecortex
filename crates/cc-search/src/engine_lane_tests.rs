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
    fuse_outcomes, run_lanes, FusedScore, GraphLane, GrepLane, LaneContext, LaneOutcome,
    LexicalLane, RetrievalLane, ScoreSlot, LANE_GRAPH, LANE_GREP, LANE_LEXICAL,
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
    assert_eq!(hits, vec![("chunk:src/alpha.rs".to_string(), 1.0)]);
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
    assert!(cache.get("chunk:src/g.rs").is_some());
    assert!(
        cache.get("chunk:src/other.rs").is_none(),
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
    let make_chunk = |chunk_id: &str, index: i64, start: u32, end: u32| ChunkRecord {
        chunk_id: chunk_id.to_string(),
        file_path: "src/wide.rs".to_string(),
        language: Language::Rust,
        chunk_index: index as u32,
        start_line: start,
        end_line: end,
        breadcrumb: "root".to_string(),
        text: "fn narrow_fn() {}".to_string(),
        symbol_name: None,
        symbol_kind: None,
        token_estimate: 8,
        parser_tier: ParserTier::TreeSitter,
        parser_confidence: 1.0,
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
    assert_eq!(ids, vec![LANE_LEXICAL, LANE_GREP, LANE_GRAPH]);
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
    let outcomes = run_lanes(&lanes, &context).unwrap();

    let fused = fuse_outcomes(&outcomes, 50);
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
        LaneOutcome {
            lane_id: "fake-a",
            weight: 1.0,
            annotates_hits: false,
            score_slot: None,
            hits: vec![("x".to_string(), 1.0), ("y".to_string(), 0.5)],
        },
        LaneOutcome {
            lane_id: "fake-b",
            weight: 0.5,
            annotates_hits: false,
            score_slot: None,
            hits: vec![("y".to_string(), 1.0)],
        },
    ];
    let fused = fuse_outcomes(&outcomes, 50);

    // score(d) = sum over lanes of weight / (k + rank)
    assert!((fused["x"].total - 1.0 / 51.0).abs() < 1e-12);
    assert!((fused["y"].total - (1.0 / 52.0 + 0.5 / 51.0)).abs() < 1e-12);

    // Per-lane breakdown is preserved in lane-accumulation order and
    // sums (left-to-right) to the fused total bit-for-bit.
    assert_eq!(fused["x"].by_lane, vec![("fake-a", 1.0 / 51.0)]);
    assert_eq!(
        fused["y"].by_lane,
        vec![("fake-a", 1.0 / 52.0), ("fake-b", 0.5 / 51.0)]
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
