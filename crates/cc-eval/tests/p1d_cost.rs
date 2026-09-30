//! Work accounting tests exercise actual storage/FTS; no synthetic cost formula.
use cc_model::{config::ProjectConfig, search::SearchRequest};
use cc_server::engine::CodeIndex;
use std::{collections::HashMap, sync::Arc, time::Duration};

fn fixture() -> (tempfile::TempDir, CodeIndex) {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join("src")).unwrap();
    let text = format!("def needle():\n    return '{}'\n", "payload ".repeat(256));
    std::fs::write(d.path().join("src/a.py"), text).unwrap();
    std::fs::write(
        d.path().join("src/b.py"),
        "def other():\n    return 'unrelated'\n",
    )
    .unwrap();
    let mut index = CodeIndex::new(Some(d.path())).unwrap();
    index.build_index(true).unwrap();
    (d, index)
}
#[test]
fn measured_hydration_reuses_text_and_resets_cached_statement_counters() {
    let (_d, index) = fixture();
    let db = index.index_db().unwrap();
    let conn = db.read_conn().unwrap();
    let ids = conn
        .prepare("SELECT chunk_id FROM chunks ORDER BY chunk_id")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    drop(conn);
    let refs: Vec<_> = ids.iter().map(String::as_str).collect();
    let cold = db
        .retrieval()
        .chunk_rows_by_ids_with_work(&refs, &HashMap::new())
        .unwrap();
    assert_eq!(cold.work.text.storage_reads, cold.rows.len());
    assert!(cold.work.text.zstd_decodes > 0);
    assert_eq!(cold.work.sql.statements, 1);
    assert!(cold.work.sql.vm_steps.unwrap() > 0);
    let cache: HashMap<_, _> = cold
        .rows
        .iter()
        .map(|r| (r.chunk_id.clone(), Arc::from(r.text.as_str())))
        .collect();
    let warm = db
        .retrieval()
        .chunk_rows_by_ids_with_work(&refs, &cache)
        .unwrap();
    assert_eq!(warm.work.text.storage_reads, 0);
    assert_eq!(warm.work.text.zstd_decodes, 0);
    assert_eq!(warm.work.text.cache_hits, refs.len());
    assert_eq!(cold.work.sql, warm.work.sql);
    assert_eq!(cold.work.text.utf8_bytes, warm.work.text.utf8_bytes);
    let empty = db
        .retrieval()
        .chunk_rows_by_ids_with_work(&[], &cache)
        .unwrap();
    assert_eq!(empty.work.sql.statements, 0);
}
#[test]
fn cached_text_is_a_hint_not_authority_for_versioned_rows() {
    let (_d, index) = fixture();
    let db = index.index_db().unwrap();
    let ids = {
        let conn = db.read_conn().unwrap();
        let mut stmt = conn
            .prepare("SELECT chunk_id FROM chunks ORDER BY chunk_id")
            .unwrap();
        let values = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        values
    };
    let refs: Vec<_> = ids.iter().map(String::as_str).collect();
    let cold = db
        .retrieval()
        .chunk_rows_by_ids_with_work(&refs, &HashMap::new())
        .unwrap();
    let stale = ids
        .iter()
        .map(|id| (id.clone(), Arc::<str>::from("stale cached bytes")))
        .collect();
    let recovered = db
        .retrieval()
        .chunk_rows_by_ids_with_work(&refs, &stale)
        .unwrap();
    assert_eq!(recovered.work.text.cache_hits, 0);
    assert_eq!(recovered.work.text.storage_reads, recovered.rows.len());
    assert_eq!(
        cold.rows
            .iter()
            .map(|row| (&row.chunk_id, &row.text))
            .collect::<HashMap<_, _>>(),
        recovered
            .rows
            .iter()
            .map(|row| (&row.chunk_id, &row.text))
            .collect::<HashMap<_, _>>()
    );
    // A rejected hint may fall back to the current blob; genuinely corrupt
    // source evidence still fails after that fallback, rather than being ignored.
    let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    conn.execute(
        "UPDATE chunks SET source_json=json_set(source_json,'$.slice_digest','corrupt')",
        [],
    )
    .unwrap();
    assert!(db
        .retrieval()
        .chunk_rows_by_ids_with_work(&refs, &stale)
        .is_err());
}

#[test]
fn query_local_costs_cover_empty_cap_probe_and_all_text_reads() {
    let (_d, index) = fixture();
    let db = index.index_db().unwrap();
    for cap in [0, 1, 8] {
        let mut config = ProjectConfig::default();
        config.search.grep_scan_cap = cap;
        let engine = cc_search::SearchEngine::new(db.clone(), &config, None);
        let request = SearchRequest {
            query: "absentStringToken".into(),
            include_grep: true,
            file_preselect_limit: Some(0),
            ..Default::default()
        };
        let result = engine.search_with_diagnostics(&request).unwrap();
        let g = result.grep.unwrap();
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
        assert!(g.stages.iter().all(|s| s.work.sql.vm_steps.is_some()));
        if cap == 0 {
            assert!(g.stages.iter().any(|s| s.work.sql.rows > 0));
            assert_eq!(g.status, "partial");
        }
        let empty = engine
            .search_with_diagnostics(&SearchRequest {
                file_paths: Some(vec![]),
                ..request
            })
            .unwrap();
        assert_eq!(empty.cost.lexical_sql.statements, 0);
        assert!(empty.hits.is_empty());
    }
}
#[test]
fn old_or_unknown_work_receipts_are_unavailable_not_zero() {
    use cc_eval::benchmark::sampler::retrieval_work;
    assert!(retrieval_work(&serde_json::json!({"nodes":[]})).is_none());
    assert!(retrieval_work(
        &serde_json::json!({"evidence_summary":{"retrieval":{"cost":{"schema_version":99}}}})
    )
    .is_none());
}
#[tokio::test]
#[ignore = "requires explicit CODECORTEX_BENCH_BINARY; actual MCP cost receipts"]
async fn p1d_mcp_cost_receipts_are_preserved_on_cache_hits() {
    use cc_eval::benchmark::{
        adapters::{mcp_stdio::McpStdio, Backend},
        schema::SearchInput,
    };
    let (d, _index) = fixture();
    let binary = std::env::var("CODECORTEX_BENCH_BINARY").unwrap();
    let mut client = McpStdio::spawn(
        std::path::Path::new(&binary),
        d.path(),
        Duration::from_secs(20),
    )
    .await
    .unwrap();
    client.prepare(&[]).await.unwrap();
    let input = SearchInput {
        query: "needle".into(),
        top_k: 5,
        path_prefix: Some("src".into()),
    };
    let a = client.search(&input).await.unwrap();
    let b = client.search(&input).await.unwrap();
    let work = cc_eval::benchmark::sampler::retrieval_work(&a).expect("missing cost receipts");
    assert_eq!(Some(work), cc_eval::benchmark::sampler::retrieval_work(&b));
    client.close().await.unwrap();
}
