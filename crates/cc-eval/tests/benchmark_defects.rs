//! Recorded P0 observations promoted to fixed-behavior regressions in P1-A.
//! Raw diagnostics remain available; a reproduced B01/B02 defect now fails.
use cc_eval::benchmark::{manifest, oracle, report};
use cc_model::{config::RankingConfig, search::SearchRequest};
use serde_json::json;
use std::path::Path;
fn save(name: &str, v: &serde_json::Value) {
    println!("BASELINE_OBSERVATION {name}: {v}");
    if let Ok(out) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&out).unwrap();
        report::json(&Path::new(&out).join(format!("{name}.json")), v).unwrap();
    }
}
#[test]
fn observe_bm25_mapping_against_actual_preselect() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("index.sqlite3");
    let (db, _) = cc_db::index_db::IndexDb::open(&path).unwrap();
    let c = rusqlite::Connection::open(&path).unwrap();
    for i in 0..12 {
        let p = format!("src/file{i}.rs");
        let summary = if i == 0 {
            "alpha alpha alpha alpha"
        } else if i == 1 {
            "alpha filler filler filler"
        } else {
            "unrelated tokens"
        };
        c.execute("INSERT INTO files(file_path,language,content_hash,mtime,size,summary,indexed_at) VALUES (?1,'rust','fixture',0,1,?2,'fixture')",rusqlite::params![p,summary]).unwrap();
        c.execute(
            "INSERT INTO files_fts(file_path,summary) VALUES (?1,?2)",
            rusqlite::params![p, summary],
        )
        .unwrap();
    }
    let raw = db
        .retrieval()
        .fts_file_summaries("alpha", None, 10)
        .unwrap();
    assert_eq!(raw.len(), 2);
    assert!(raw[0].1 < raw[1].1);
    let ranked = cc_search::preselect::preselect(
        &db,
        &cc_search::preselect::PreselectRequest {
            query: "alpha",
            path_prefix: None,
            boost_paths: None,
            recent_paths: None,
            pinned_paths: None,
            overlay_paths: None,
            explicit_file_paths: None,
            limit: 10,
            ranking: &RankingConfig::default(),
        },
    )
    .unwrap();
    let inverted = ranked.scores[&raw[0].0] < ranked.scores[&raw[1].0];
    save(
        "B01-bm25",
        &json!({"defect_reproduced":inverted,"raw":raw,"preselect_scores":ranked.scores,"repair_task":"P1-002","layer":"actual preselect and SQLite, not formula-only"}),
    );
    assert!(
        !inverted,
        "B01: stronger BM25 match received a lower preselect contribution"
    );
    assert_eq!(ranked.files.first(), Some(&raw[0].0));
}
#[test]
fn observe_soft_hints_excluding_actual_symbol() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("target.py"),
        "def rareSymbol():\n    return 17\n",
    )
    .unwrap();
    std::fs::write(d.path().join("noise.py"), "def other():\n    return 0\n").unwrap();
    let mut index = cc_server::engine::CodeIndex::new(Some(d.path())).unwrap();
    index.build_index(true).unwrap();
    let plain = index
        .search()
        .search_in_context("rareSymbol", 10, None)
        .unwrap();
    let biased = index
        .search()
        .search_in_context_with(
            "rareSymbol",
            10,
            None,
            SearchRequest {
                boost_file_paths: Some(vec!["noise.py".into()]),
                pinned_file_paths: Some(vec!["noise.py".into()]),
                recent_file_paths: Some(vec!["noise.py".into()]),
                file_preselect_limit: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
    let contains = |e: &cc_model::ContextEnvelope| {
        e.nodes
            .iter()
            .any(|n| n.file_path.as_deref() == Some("target.py"))
    };
    assert!(
        contains(&plain),
        "fixture must first demonstrate the exact target is indexed"
    );
    save(
        "B02-soft-scope",
        &json!({"defect_reproduced":!contains(&biased),"unbiased":plain,"biased":biased,"repair_task":"P1-004","layer":"public in-process search API"}),
    );
    assert!(
        contains(&biased),
        "B02: soft hints excluded a valid exact/lexical target"
    );
}
#[test]
fn canonical_oracle_preserves_target_identity_and_strategy() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("a.py"),
        "def leaf():\n    return 1\n\ndef caller():\n    return leaf()\n",
    )
    .unwrap();
    let mut index = cc_server::engine::CodeIndex::new(Some(d.path())).unwrap();
    index.build_index(true).unwrap();
    let before = oracle::canonical(d.path()).unwrap();
    assert!(!before["call_edges"].is_empty());
    let c = rusqlite::Connection::open(d.path().join(".codecortex/index.sqlite3")).unwrap();
    c.execute("UPDATE call_edges SET callee_symbol_uid='different-target', resolution_strategy='fixture-different'",[]).unwrap();
    let after = oracle::canonical(d.path()).unwrap();
    assert_ne!(before["call_edges"], after["call_edges"]);
}
#[test]
fn source_fixtures_are_real_utf8_not_placeholder_references() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/index-v2");
    for path in [
        "python-api/provider.py",
        "python-api/consumer.py",
        "rust-api/lib.rs",
        "rust-api/provider.rs",
    ] {
        assert!(!manifest::source_bytes(&root, path).unwrap().is_empty());
    }
}
