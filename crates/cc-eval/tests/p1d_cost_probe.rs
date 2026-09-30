//! Explicit fresh-query operation/cost experiment. A counterfactual snapshot may
//! restore the old implicit preselection filter; the test never changes product knobs.
use cc_model::{config::ProjectConfig, search::SearchRequest};
use cc_search::SearchEngine;
use cc_server::engine::CodeIndex;
use serde_json::json;
use std::time::Instant;

#[test]
#[ignore = "explicit 1k/5k cost probe; run release and retain all rows"]
fn p1d_cost_quality_and_sql_work_matrix() {
    let mut samples = Vec::new();
    let mut inventories = Vec::new();
    let mut resources = Vec::new();
    for count in [1000, 5000] {
        let dir = tempfile::tempdir().unwrap();
        let mut inventory = Vec::new();
        for n in 0..count {
            let value = if n == 0 {
                "needleMarker abcquartzdef"
            } else {
                "ordinarytext"
            };
            let text = format!(
                "def fun{n:04}():\n    return '{value} {}'\n",
                "padding ".repeat(128)
            );
            let path = format!("f{n:04}.py");
            std::fs::write(dir.path().join(&path), &text).unwrap();
            inventory.push((path, cc_eval::benchmark::manifest::digest(text.as_bytes())));
        }
        let start = Instant::now();
        let mut index = CodeIndex::new(Some(dir.path())).unwrap();
        index.build_index(true).unwrap();
        inventories.push(json!({"files":count,"source_digest":cc_eval::benchmark::manifest::digest(&serde_json::to_vec(&inventory).unwrap()),"index_us":start.elapsed().as_micros()}));
        resources.push(cc_eval::benchmark::sampler::sample(
            &format!("indexed-{count}"),
            None,
        ));
        for cap in [0, 1, 32, 256, 8192] {
            let mut config = ProjectConfig::default();
            config.search.grep_scan_cap = cap;
            config.search.graph_weight = 0.0;
            let engine = SearchEngine::new(index.index_db().unwrap().clone(), &config, None);
            // Rotate the case order, keep all observations, no best-of selection.
            for repetition in 0..30 {
                for offset in 0..3 {
                    let case = (repetition + offset) % 3;
                    let (query, hint) = match case {
                        0 => ("needleMarker", format!("f{:04}.py", count - 1)),
                        1 => ("quartz", "f0000.py".into()),
                        _ => ("absentTokenXYZ", format!("f{:04}.py", count - 1)),
                    };
                    let q = SearchRequest {
                        query: query.into(),
                        top_k: 10,
                        include_grep: true,
                        file_preselect_limit: Some(1),
                        pinned_file_paths: Some(vec![hint.clone()]),
                        recent_file_paths: Some(vec![hint.clone()]),
                        boost_file_paths: Some(vec![hint]),
                        ..Default::default()
                    };
                    // Isolate per-query execution work and avoid billing old cached receipts.
                    engine.invalidate_cache();
                    let start = Instant::now();
                    let result = engine.search_with_diagnostics(&q).unwrap();
                    let elapsed_us = start.elapsed().as_micros();
                    let g = result.grep.as_ref().unwrap();
                    assert!(g.scanned <= cap);
                    assert_eq!(
                        g.scanned,
                        g.stages
                            .iter()
                            .map(|s| s.work.text.storage_reads)
                            .sum::<usize>()
                    );
                    assert!(g
                        .stages
                        .iter()
                        .all(|s| s.work.sql.rows >= s.work.text.storage_reads));
                    samples.push(json!({"files":count,"case":case,"query":query,"cap":cap,"repetition":repetition,"elapsed_us":elapsed_us,"hit_count":result.hits.len(),"target_found":result.hits.iter().any(|h|h.file_path=="f0000.py"),"grep":g,"cost":result.cost}));
                }
            }
        }
        resources.push(cc_eval::benchmark::sampler::sample(
            &format!("queried-{count}"),
            None,
        ));
    }
    let report = json!({"profile":"fresh_query_operation_cost_not_release_certification","debug_assertions":cfg!(debug_assertions),"result_cache":"bypassed","text_cache":"cleared_before_each_query; same-query grep/hydration reuse retained","os_page_cache":"not_cleared","peak_rss":"not_continuously_measured","resources":resources,"inputs":inventories,"samples":samples});
    let out = std::env::var("CODECORTEX_BENCH_OBSERVATIONS")
        .expect("explicit evidence directory required");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(
        std::path::Path::new(&out).join("cost-probe.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!(
        "P1D_COST_PROBE rows={} files=1000,5000 debug={}",
        samples.len(),
        cfg!(debug_assertions)
    );
}
