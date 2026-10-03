//! Self-authored fixtures only: exhaustion must differ from a genuinely omitted item.
use super::*;
use crate::engine::SearchEngine;
use crate::engine_test_support::{insert_chunk_file, insert_graph_file, scoped_test_engine};
use cc_model::search::SearchRequest;
use cc_model::{CallEdgeRecord, Language};

fn plan(engine: &SearchEngine, query: &str, top_k: usize) -> SearchPlan {
    SearchPlan::build(
        &engine.db,
        &engine.config,
        &engine.ranking,
        &SearchRequest {
            query: query.into(),
            top_k,
            include_grep: false,
            ..Default::default()
        },
        None,
    )
    .unwrap()
}

fn context<'a>(engine: &'a SearchEngine, plan: &'a SearchPlan) -> LaneContext<'a> {
    LaneContext {
        plan,
        db: &engine.db,
        config: &engine.config,
        chunk_text_cache: &engine.chunk_text_cache,
        cache_epoch: 0,
    }
}

fn check(run: &LaneRun, count: usize, reason: Option<&str>) {
    assert_eq!(run.hits.len(), count);
    assert_eq!(run.truncation_reason.as_deref(), reason);
    assert_eq!(
        run.status,
        if reason.is_some() {
            LaneStatus::Partial
        } else {
            LaneStatus::Complete
        }
    );
}

#[test]
fn lexical_candidate_boundary_n_minus_one_n_n_plus_one() {
    for n in [2, 3, 4] {
        let (engine, _tmp) = scoped_test_engine();
        for i in 0..n {
            insert_chunk_file(
                &engine,
                &format!("src/d{i}.rs"),
                Language::Rust,
                "uniqueprobe",
            );
        }
        let plan = plan(&engine, "uniqueprobe", 3);
        let run = LexicalLane.run_detailed(&context(&engine, &plan)).unwrap();
        check(&run, n.min(3), (n > 3).then_some("candidate_limit"));
        // The cap+1 row is metadata/score lookahead, not a returned hit.
        assert_eq!(run.lexical_work.rows, n);
    }
}

#[test]
fn lexical_source_atom_boundary_preserves_empty_partial() {
    for n in [11, 12, 13] {
        let (engine, _tmp) = scoped_test_engine();
        let query = (0..n)
            .map(|i| format!("absent{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let plan = plan(&engine, &query, 3);
        check(
            &LexicalLane.run_detailed(&context(&engine, &plan)).unwrap(),
            0,
            (n > 12).then_some("query_expansion_atom_budget"),
        );
    }
    let (engine, _tmp) = scoped_test_engine();
    insert_chunk_file(&engine, "src/last.rs", Language::Rust, "absent12");
    let query = (0..13)
        .map(|i| format!("absent{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    let plan = plan(&engine, &query, 3);
    check(
        &LexicalLane.run_detailed(&context(&engine, &plan)).unwrap(),
        0,
        Some("query_expansion_atom_budget"),
    );
}

#[test]
fn lexical_optional_group_boundary_witnesses_missing_match() {
    // One literal + two component atoms, plus n plain terms: 11/12/13 total.
    for n in [8, 9, 10] {
        let (engine, _tmp) = scoped_test_engine();
        insert_chunk_file(
            &engine,
            "src/components.rs",
            Language::Rust,
            "unique component",
        );
        let query = format!(
            "uniqueComponent {}",
            (0..n)
                .map(|i| format!("absent{i}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let plan = plan(&engine, &query, 3);
        check(
            &LexicalLane.run_detailed(&context(&engine, &plan)).unwrap(),
            usize::from(n <= 9),
            (n > 9).then_some("query_expansion_atom_budget"),
        );
    }
}

#[test]
fn graph_token_boundary_preserves_empty_partial_and_omitted_seed() {
    for n in [4, 5, 6] {
        let (engine, _tmp) = scoped_test_engine();
        let query = (0..n)
            .map(|i| format!("absent{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let plan = plan(&engine, &query, 12);
        check(
            &GraphLane.run_detailed(&context(&engine, &plan)).unwrap(),
            0,
            (n > 5).then_some("graph_expansion_limit"),
        );
    }
    let (engine, _tmp) = scoped_test_engine();
    insert_graph_file(
        &engine,
        "src/last.rs",
        "fn absent5() {}",
        "absent5",
        "last",
        vec![],
    );
    let plan = plan(
        &engine,
        "absent0 absent1 absent2 absent3 absent4 absent5",
        12,
    );
    check(
        &GraphLane.run_detailed(&context(&engine, &plan)).unwrap(),
        0,
        Some("graph_expansion_limit"),
    );
}

#[test]
fn graph_per_token_seed_boundary() {
    for n in [9, 10, 11] {
        let (engine, _tmp) = scoped_test_engine();
        for i in 0..n {
            let name = format!("seedprobe{i}");
            insert_graph_file(
                &engine,
                &format!("src/s{i}.rs"),
                &format!("fn {name}() {{}}"),
                &name,
                &name,
                vec![],
            );
        }
        let plan = plan(&engine, "seedprobe", 30);
        check(
            &GraphLane.run_detailed(&context(&engine, &plan)).unwrap(),
            n.min(10),
            (n > 10).then_some("graph_expansion_limit"),
        );
    }
}

#[test]
fn graph_total_seed_boundary() {
    for n in [19, 20, 21] {
        let (engine, _tmp) = scoped_test_engine();
        for i in 0..n {
            let stem = ["firstprobe", "secondprobe", "thirdprobe"][i / 7];
            let name = format!("{stem}{i}");
            insert_graph_file(
                &engine,
                &format!("src/s{i}.rs"),
                &format!("fn {name}() {{}}"),
                &name,
                &name,
                vec![],
            );
        }
        let plan = plan(&engine, "firstprobe secondprobe thirdprobe", 30);
        check(
            &GraphLane.run_detailed(&context(&engine, &plan)).unwrap(),
            n.min(20),
            (n > 20).then_some("graph_expansion_limit"),
        );
    }
}

#[test]
fn graph_neighbor_boundary_both_directions() {
    for incoming in [false, true] {
        for n in [9, 10, 11] {
            let (engine, _tmp) = scoped_test_engine();
            let mut edges = vec![];
            for i in 0..n {
                let name = format!("neighbor{i}");
                insert_graph_file(
                    &engine,
                    &format!("src/n{i}.rs"),
                    &format!("fn {name}() {{}}"),
                    &name,
                    &name,
                    vec![],
                );
                edges.push(CallEdgeRecord {
                    edge_id: format!("edge{i}"),
                    file_path: "src/root.rs".into(),
                    line: i as u32 + 1,
                    caller_symbol_uid: Some(if incoming {
                        name.clone()
                    } else {
                        "rootprobe".into()
                    }),
                    callee_symbol_uid: Some(if incoming { "rootprobe".into() } else { name }),
                    ..Default::default()
                });
            }
            insert_graph_file(
                &engine,
                "src/root.rs",
                "fn rootprobe() {}",
                "rootprobe",
                "rootprobe",
                edges,
            );
            let plan = plan(&engine, "rootprobe", 30);
            check(
                &GraphLane.run_detailed(&context(&engine, &plan)).unwrap(),
                1 + n.min(10),
                (n > 10).then_some("graph_expansion_limit"),
            );
        }
    }
}

#[test]
fn graph_output_candidate_boundary() {
    for n in [2, 3, 4] {
        let (engine, _tmp) = scoped_test_engine();
        for i in 0..n {
            let name = format!("seedprobe{i}");
            insert_graph_file(
                &engine,
                &format!("src/s{i}.rs"),
                &format!("fn {name}() {{}}"),
                &name,
                &name,
                vec![],
            );
        }
        let plan = plan(&engine, "seedprobe", 3);
        check(
            &GraphLane::search_detailed(&engine.db, &plan, plan.query_tokens(), 3).unwrap(),
            n.min(3),
            (n > 3).then_some("candidate_limit"),
        );
    }
}

#[test]
fn disabled_graph_does_not_charge_omitted_tokens() {
    let (mut engine, _tmp) = scoped_test_engine();
    engine.config.graph_weight = 0.0;
    let plan = plan(
        &engine,
        "absent0 absent1 absent2 absent3 absent4 absent5",
        3,
    );
    let outcomes = run_lanes(&default_lanes(), &context(&engine, &plan)).unwrap();
    let graph = outcomes.iter().find(|r| r.lane_id == LANE_GRAPH).unwrap();
    assert_eq!(graph.status, LaneStatus::Disabled);
    assert!(graph.truncation_reason.is_none());
}

#[test]
fn cached_empty_partial_agrees_with_fresh_query_and_does_not_poison_short_query() {
    let (engine, _tmp) = scoped_test_engine();
    let request = SearchRequest {
        query: "absent0 absent1 absent2 absent3 absent4 absent5".into(),
        include_grep: false,
        ..Default::default()
    };
    let limits = cc_model::config::RepoSizeTier::Small.graph_enrich_limits();
    let first = engine
        .search_with_graph_context(&request, &limits, 4000)
        .unwrap();
    let cached = engine
        .search_with_graph_context(&request, &limits, 4000)
        .unwrap();
    assert!(Arc::ptr_eq(&first, &cached));
    assert!(first.0.is_empty());
    let fresh = engine.search_with_diagnostics(&request).unwrap();
    let originating_graph = first
        .1
        .lane_outcomes
        .iter()
        .find(|r| r.lane_id == LANE_GRAPH)
        .unwrap();
    let fresh_graph = fresh
        .lanes
        .iter()
        .find(|r| r.lane_id == LANE_GRAPH)
        .unwrap();
    assert_eq!(originating_graph.status, LaneStatus::Partial);
    assert_eq!(originating_graph.status, fresh_graph.status);
    assert_eq!(
        originating_graph.truncation_reason,
        fresh_graph.truncation_reason
    );
    let shorter = SearchRequest {
        query: "absent0 absent1 absent2 absent3 absent4".into(),
        ..request
    };
    let short = engine
        .search_with_graph_context(&shorter, &limits, 4000)
        .unwrap();
    assert!(!Arc::ptr_eq(&first, &short));
    assert_eq!(
        short
            .1
            .lane_outcomes
            .iter()
            .find(|r| r.lane_id == LANE_GRAPH)
            .unwrap()
            .status,
        LaneStatus::Complete
    );
}
