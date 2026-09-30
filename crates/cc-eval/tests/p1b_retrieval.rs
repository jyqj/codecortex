//! P1-B boundary and bounded-scan contracts; public APIs, no answer-aware production code.
use cc_model::{config::ProjectConfig, search::SearchRequest};
use cc_server::engine::CodeIndex;
use serde_json::{json, Value};
use std::{collections::BTreeSet, path::Path, time::Duration};
fn put(root: &Path, p: &str, text: &str) {
    let p = root.join(p);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}
fn indexed(files: &[(&str, &str)], config: Value) -> (tempfile::TempDir, CodeIndex) {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), ".codecortex.json", &config.to_string());
    for (p, t) in files {
        put(d.path(), p, t);
    }
    let mut i = CodeIndex::new(Some(d.path())).unwrap();
    i.build_index(true).unwrap();
    (d, i)
}
fn local() -> Value {
    json!({"auto_index":{"enabled":false}})
}
fn paths(i: &CodeIndex, q: &str, r: SearchRequest) -> BTreeSet<String> {
    i.search()
        .search_in_context_with(q, 40, None, r)
        .unwrap()
        .machine_pack["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["file_path"].as_str().unwrap().to_string())
        .collect()
}
#[test]
fn p1b_component_scope_and_separator_normalization() {
    let (_d, i) = indexed(
        &[
            ("src/api/a.py", "def needle():\n    return 1\n"),
            ("src/apix/b.py", "def needle():\n    return 2\n"),
        ],
        local(),
    );
    for prefix in ["src/api", "src/api/", "./src//api", "src\\api"] {
        assert_eq!(
            paths(
                &i,
                "needle",
                SearchRequest {
                    path_prefix: Some(prefix.into()),
                    ..Default::default()
                }
            ),
            BTreeSet::from(["src/api/a.py".into()]),
            "prefix={prefix}"
        );
    }
}
#[test]
fn p1b_invalid_hard_paths_are_not_silently_empty() {
    let (_d, i) = indexed(&[("a.py", "def needle():\n    return 1\n")], local());
    for p in [
        "../secret",
        "src/../a.py",
        "/etc",
        "C:\\secret",
        "C:secret",
        "\\\\server\\share",
        "a\0b",
    ] {
        assert!(
            i.search()
                .search_in_context_with(
                    "needle",
                    10,
                    None,
                    SearchRequest {
                        path_prefix: Some(p.into()),
                        ..Default::default()
                    }
                )
                .is_err(),
            "accepted {p:?}"
        );
    }
}
#[test]
fn p1b_lexical_sql_filters_before_limit_and_preserves_case() {
    let (_d, i) = indexed(
        &[("src/api/a.py", "def needle():\n    return 1\n")],
        local(),
    );
    let db = i.index_db().unwrap();
    let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    conn.execute("INSERT INTO files(file_path,language,content_hash,mtime,size,summary,indexed_at) VALUES('SRC/API/decoy.py','python','fake',0,1,'needle','fixture')",[]).unwrap();
    conn.execute("INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,breadcrumb,text,token_estimate,parser_tier,parser_confidence) SELECT 'decoy','SRC/API/decoy.py',language,0,1,2,'needle needle needle','needle needle needle',1,parser_tier,parser_confidence FROM chunks LIMIT 1",[]).unwrap();
    conn.execute("INSERT INTO chunks_fts(chunk_id,file_path,breadcrumb,symbol_name,text) VALUES('decoy','SRC/API/decoy.py','needle needle needle','needle','needle needle needle')",[]).unwrap();
    let rows = db
        .retrieval()
        .fts_chunk_candidates(
            "needle",
            &cc_db::ChunkScope {
                path_prefix: Some("src/api".into()),
                ..Default::default()
            },
            1,
        )
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].1, "src/api/a.py",
        "scope must precede candidate cap"
    );
}
#[test]
fn p1b_soft_priority_recalls_old_midtoken_match_under_budget() {
    let (_d, i) = indexed(
        &[
            ("a.py", "def old():\n    return 'abcNeedleTail'\n"),
            ("z.py", "def fresh():\n    return 0\n"),
        ],
        local(),
    );
    let db = i.index_db().unwrap();
    let mut c = ProjectConfig::default();
    c.search.grep_scan_cap = 1;
    c.search.grep_weight = 1.0;
    c.search.lexical_weight = 0.0;
    c.search.graph_weight = 0.0;
    let engine = cc_search::SearchEngine::new(db.clone(), &c, None);
    let hits = engine
        .search(&SearchRequest {
            query: "Needle".into(),
            top_k: 5,
            include_grep: true,
            pinned_file_paths: Some(vec!["a.py".into()]),
            file_preselect_limit: Some(1),
            ..Default::default()
        })
        .unwrap();
    assert!(
        hits.iter().any(|h| h.file_path == "a.py"),
        "soft priority was ignored"
    );
}
#[test]
fn p1b_empty_partial_query_retains_scan_diagnostic() {
    let (_d, i) = indexed(
        &[
            ("a.py", "def aa():\n    return 0\n"),
            ("z.py", "def zz():\n    return 0\n"),
        ],
        json!({"auto_index":{"enabled":false},"search":{"grep_scan_cap":1}}),
    );
    let e = i
        .search()
        .search_in_context("absentXXsubstring", 10, None)
        .unwrap();
    assert!(e.machine_pack["hits"].as_array().unwrap().is_empty());
    let g = &e.evidence_summary["retrieval"]["grep"];
    assert_eq!(
        g["status"], "partial",
        "missing empty-result diagnostic: {}",
        e.evidence_summary
    );
    assert_eq!(g["scanned"], 1);
    assert_eq!(g["scan_cap"], 1);
    assert!(e.summary.contains("incomplete"));
}
#[cfg(unix)]
#[test]
fn p1b_scanner_and_source_guard_reject_external_symlinks() {
    use std::os::unix::fs::symlink;
    let outer = tempfile::tempdir().unwrap();
    put(
        outer.path(),
        "secret.py",
        "def leakedMarker():\n    return 1\n",
    );
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "local.py", "def local():\n    return 0\n");
    symlink(outer.path().join("secret.py"), d.path().join("alias.py")).unwrap();
    let mut i = CodeIndex::new(Some(d.path())).unwrap();
    i.build_index(true).unwrap();
    assert!(
        paths(&i, "leakedMarker", SearchRequest::default()).is_empty(),
        "external symlink was indexed"
    );
    let scanner = cc_index::scanner::Scanner::new(d.path(), &ProjectConfig::default().indexing);
    assert!(scanner.scan_paths(&["alias.py".into()]).is_empty());
}
#[test]
fn p1b_exact_identifier_and_path_keep_top1_with_distractors() {
    let (_d, i) = indexed(
        &[
            ("src/codec.py", "def decodeFrame():\n    return 17\n"),
            (
                "src/noise.py",
                "def other():\n    return 'frame decoder semantics'\n",
            ),
        ],
        local(),
    );
    for q in ["decodeFrame", "src/codec.py"] {
        let e = i
            .search()
            .search_in_context_with(
                q,
                1,
                None,
                SearchRequest {
                    pinned_file_paths: Some(vec!["src/noise.py".into()]),
                    recent_file_paths: Some(vec!["src/noise.py".into()]),
                    file_preselect_limit: Some(1),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(
            e.machine_pack["hits"][0]["file_path"], "src/codec.py",
            "query={q}"
        );
    }
}
#[test]
fn p1b_decode_cap_is_checked_before_the_next_blob() {
    let (_d, i) = indexed(
        &[
            ("a.py", "def aa():\n    return 0\n"),
            ("z.py", "def zz():\n    return 0\n"),
        ],
        local(),
    );
    let db = i.index_db().unwrap();
    let c = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    c.execute("UPDATE chunks SET text=X'0001',text_encoding='zstd' WHERE rowid=(SELECT min(rowid) FROM chunks)",[]).unwrap();
    let mut visited = 0;
    let report = db
        .retrieval()
        .scan_grep_stage(
            &cc_db::ChunkScope::default(),
            None,
            &Default::default(),
            1,
            |_| {
                visited += 1;
                true
            },
        )
        .unwrap();
    assert_eq!(visited, 1);
    assert_eq!(report.decoded, 1);
    assert!(!report.exhausted);
    assert!(
        db.retrieval()
            .scan_grep_stage(
                &cc_db::ChunkScope::default(),
                None,
                &Default::default(),
                2,
                |_| true
            )
            .is_err(),
        "second malformed blob must prove it was skipped, not decoded silently"
    );
}
#[test]
fn p1b_shared_scan_budget_deduplicates_all_stages_and_detects_exact_exhaustion() {
    let (_d, i) = indexed(
        &[
            ("a.py", "def aa():\n    return 'abcNeedleTail'\n"),
            ("z.py", "def zz():\n    return 'needle'\n"),
        ],
        local(),
    );
    let db = i.index_db().unwrap();
    let mut cfg = ProjectConfig::default();
    cfg.search.grep_scan_cap = 2;
    let engine = cc_search::SearchEngine::new(db.clone(), &cfg, None);
    let r = engine
        .search_with_diagnostics(&SearchRequest {
            query: "Needle".into(),
            top_k: 10,
            include_grep: true,
            pinned_file_paths: Some(vec!["a.py".into()]),
            file_preselect_limit: Some(1),
            ..Default::default()
        })
        .unwrap();
    let g = r.grep.unwrap();
    assert_eq!(g.scanned, 2);
    assert_eq!(g.soft_scanned + g.prefilter_scanned + g.fallback_scanned, 2);
    assert_eq!(g.status, "complete");
    assert!(r.hits.iter().any(|h| h.file_path == "a.py"));
    assert!(r.hits.iter().any(|h| h.file_path == "z.py"));
    let r = engine
        .search_with_diagnostics(&SearchRequest {
            query: "absent".into(),
            top_k: 10,
            include_grep: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(r.grep.unwrap().status, "complete");
}
#[test]
fn p1b_zero_scan_budget_is_explicit_and_cannot_decode() {
    let (_d, i) = indexed(&[("a.py", "def aa():\n    return 0\n")], local());
    let mut cfg = ProjectConfig::default();
    cfg.search.grep_scan_cap = 0;
    let e = cc_search::SearchEngine::new(i.index_db().unwrap().clone(), &cfg, None);
    let r = e
        .search_with_diagnostics(&SearchRequest {
            query: "absent".into(),
            include_grep: true,
            ..Default::default()
        })
        .unwrap();
    let g = r.grep.unwrap();
    assert_eq!(g.scanned, 0);
    assert_eq!(g.status, "partial");
}
#[test]
fn p1b_graph_scopes_seeds_and_neighbors_before_their_caps() {
    let (d, mut i) = indexed(
        &[
            ("pkg/seed.py", "def focus():\n    return 0\n"),
            ("pkg/neighbor.py", "def destination():\n    return 0\n"),
        ],
        local(),
    );
    for k in 0..25 {
        put(
            d.path(),
            &format!("outside/{k:02}.py"),
            "def focus():\n    return 0\n",
        );
    }
    i.build_index(true).unwrap();
    let db = i.index_db().unwrap();
    let c = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    let uid = |p: &str| {
        c.query_row(
            "SELECT symbol_uid FROM symbols WHERE file_path=?1 LIMIT 1",
            [p],
            |r| r.get::<_, String>(0),
        )
        .unwrap()
    };
    let seed = uid("pkg/seed.py");
    let target = uid("pkg/neighbor.py");
    // This test owns the adjacency fixture; parser-produced edges are a separate
    // correctness surface (including the recorded self-call baseline).
    c.execute("DELETE FROM call_edges", []).unwrap();
    for k in 0..25 {
        c.execute("INSERT INTO call_edges(edge_id,file_path,caller_symbol,callee_symbol,line,caller_symbol_uid,callee_symbol_uid) VALUES(?1,'pkg/seed.py','focus','focus',?2,?3,?4)",rusqlite::params![format!("d{k}"),k+1,seed,uid(&format!("outside/{k:02}.py"))]).unwrap();
    }
    c.execute("INSERT INTO call_edges(edge_id,file_path,caller_symbol,callee_symbol,line,caller_symbol_uid,callee_symbol_uid) VALUES('allowed','pkg/seed.py','focus','destination',100,?1,?2)",rusqlite::params![seed,target]).unwrap();
    let mut cfg = ProjectConfig::default();
    cfg.search.lexical_weight = 0.0;
    cfg.search.grep_weight = 0.0;
    cfg.search.graph_weight = 1.0;
    cfg.search.graph_top_k = 10;
    let engine = cc_search::SearchEngine::new(db.clone(), &cfg, None);
    let req = SearchRequest {
        query: "focus".into(),
        top_k: 10,
        path_prefix: Some("pkg".into()),
        include_grep: false,
        pinned_file_paths: Some(vec!["pkg/seed.py".into()]),
        file_preselect_limit: Some(1),
        ..Default::default()
    };
    let hits = engine.search(&req).unwrap();
    assert!(
        hits.iter().any(|h| h.file_path == "pkg/neighbor.py"),
        "valid neighbor crowded out"
    );
    assert!(hits.iter().all(|h| h.file_path.starts_with("pkg/")));
    let scope = cc_db::ChunkScope {
        path_prefix: Some("pkg".into()),
        ..Default::default()
    };
    let seeds = db
        .retrieval()
        .symbol_seed_hits_scoped("focus", &scope, 1)
        .unwrap();
    assert_eq!(seeds[0].0, seed);
    let witnesses = db
        .retrieval()
        .scoped_call_rows(&[&seed], &scope, 1, false)
        .unwrap();
    assert_eq!(witnesses[&seed].len(), 1);
    assert_eq!(
        witnesses[&seed][0].callee_symbol_uid.as_deref(),
        Some(target.as_str())
    );
    let limits = cc_model::config::RepoSizeTier::Small.graph_enrich_limits();
    let graph_result = engine
        .search_with_graph_context(&req, &limits, 4000)
        .unwrap();
    let context = &graph_result.1;
    assert!(
        context
            .nodes
            .iter()
            .any(|n| n.title.contains("destination")),
        "legal late witness starved"
    );
    assert!(
        context
            .nodes
            .iter()
            .all(|n| !n.node_id.contains(&uid("outside/00.py"))),
        "outside target leaked in in-scope witness"
    );
}

#[tokio::test]
#[ignore = "requires CODECORTEX_BENCH_BINARY; explicit old/new subprocess"]
async fn p1b_stdio_symbol_mode_honors_path_scope() {
    use cc_eval::benchmark::adapters::{mcp_stdio::McpStdio, Backend};
    let bin = std::env::var("CODECORTEX_BENCH_BINARY").unwrap();
    let (d, _) = indexed(
        &[
            ("a_outside.py", "def needle():\n    return 1\n"),
            ("pkg/target.py", "def needle():\n    return 2\n"),
        ],
        local(),
    );
    let mut c = McpStdio::spawn(Path::new(&bin), d.path(), Duration::from_secs(30))
        .await
        .unwrap();
    let v = c
        .call(
            "search",
            json!({"query":"needle","mode":"symbol","exact":true,"path_prefix":"pkg","top_k":1}),
        )
        .await
        .unwrap();
    assert_eq!(v[0]["file_path"], "pkg/target.py");
    assert!(c
        .call(
            "search",
            json!({"query":"needle","mode":"symbol","path_prefix":"../outside"})
        )
        .await
        .is_err());
    c.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires CODECORTEX_BENCH_BINARY; explicit old/new subprocess"]
async fn p1b_stdio_boundaries_and_empty_partial() {
    use cc_eval::benchmark::adapters::{mcp_stdio::McpStdio, Backend};
    let bin = std::env::var("CODECORTEX_BENCH_BINARY").unwrap();
    let (d, _i) = indexed(
        &[
            ("src/api/a.py", "def needle():\n    return 1\n"),
            ("src/apix/z.py", "def needle():\n    return 2\n"),
        ],
        json!({"auto_index":{"enabled":false},"search":{"grep_scan_cap":1}}),
    );
    let mut c = McpStdio::spawn(Path::new(&bin), d.path(), Duration::from_secs(30))
        .await
        .unwrap();
    let v = c
        .call(
            "search",
            json!({"query":"needle","path_prefix":"src/api","top_k":10}),
        )
        .await
        .unwrap();
    assert!(v["machine_pack"]["hits"]
        .as_array()
        .unwrap()
        .iter()
        .all(|h| h["file_path"] == "src/api/a.py"));
    let v = c
        .call("search", json!({"query":"absentXXsubstring","top_k":10}))
        .await
        .unwrap();
    assert_eq!(
        v["evidence_summary"]["retrieval"]["grep"]["status"],
        "partial"
    );
    c.close().await.unwrap();
}
