//! Explicit operation-count experiment, not a release latency/peak-RSS certification.
use cc_model::{config::ProjectConfig, search::SearchRequest};
use cc_server::engine::CodeIndex;
use serde_json::json;
#[test]
#[ignore = "explicit 1000-file scan accounting experiment"]
fn p1b_scan_accounting_1000_files() {
    let dir = tempfile::tempdir().unwrap();
    for n in 0..1000 {
        let value = if n == 0 {
            "abcneedleQdef"
        } else {
            "ordinarydata"
        };
        std::fs::write(
            dir.path().join(format!("f{n:04}.py")),
            format!("def fun{n:04}():\n    return '{value}'\n"),
        )
        .unwrap();
    }
    let mut index = CodeIndex::new(Some(dir.path())).unwrap();
    index.build_index(true).unwrap();
    let mut samples = Vec::new();
    for cap in [0, 1, 8, 32, 256, 2048] {
        let mut config = ProjectConfig::default();
        config.search.grep_scan_cap = cap;
        config.search.graph_weight = 0.0;
        let engine = cc_search::SearchEngine::new(index.index_db().unwrap().clone(), &config, None);
        for repetition in 0..5 {
            for (case, query, pinned) in [
                ("no-match", "notPresentToken", None),
                ("midtoken-global", "needleQ", None),
                ("midtoken-hint", "needleQ", Some(vec!["f0000.py".into()])),
            ] {
                let request = SearchRequest {
                    query: query.into(),
                    include_grep: true,
                    top_k: 10,
                    file_preselect_limit: Some(0),
                    pinned_file_paths: pinned,
                    ..Default::default()
                };
                let start = std::time::Instant::now();
                let result = engine.search_with_diagnostics(&request).unwrap();
                let elapsed = start.elapsed().as_micros();
                let g = result.grep.unwrap();
                assert!(g.scanned <= cap);
                assert_eq!(
                    g.scanned,
                    g.soft_scanned + g.prefilter_scanned + g.fallback_scanned
                );
                if case == "midtoken-hint" && cap > 0 {
                    assert!(result.hits.iter().any(|h| h.file_path == "f0000.py"));
                }
                samples.push(json!({"case":case,"cap":cap,"repetition":repetition,"elapsed_us":elapsed,"hit_count":result.hits.len(),"grep":g}));
            }
        }
    }
    let report = json!({"profile":"debug_operation_counts_not_release_performance","files":1000,"samples":samples,"peak_rss":"not_measured","result_cache":"bypassed_by_detailed_api","os_page_cache":"not_cleared"});
    if let Ok(out) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(
            std::path::Path::new(&out).join("scan-accounting.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
    }
    println!(
        "P1B_SCAN_ACCOUNTING {}",
        serde_json::to_string(&report).unwrap()
    );
}
