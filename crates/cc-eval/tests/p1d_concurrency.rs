//! Actual SQLite/WAL read/write contention. Bounds belong to this test watchdog,
//! not a product deadline or filesystem snapshot claim. P5-A may explicitly
//! reject sustained generation churn after three attempts. The test client
//! retries only that typed conflict, retaining all source/scope assertions.
use cc_db::index_db::{FileWriteUnit, IndexDb};
use cc_model::{
    config::ProjectConfig, search::SearchRequest, ChunkRecord, Language, ParseOutcome, ParserTier,
};
use cc_search::SearchEngine;
use serde_json::json;
use std::sync::{mpsc, Arc, Barrier};
use std::time::{Duration, Instant};
fn retry_generation_conflict<T>(
    mut query: impl FnMut() -> cc_model::CcResult<T>,
    conflicts: &std::sync::atomic::AtomicUsize,
) -> T {
    let started = Instant::now();
    loop {
        match query() {
            Ok(value) => return value,
            Err(cc_model::CcError::RetrievalChanged { attempts }) => {
                assert_eq!(attempts, 3);
                conflicts.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                assert!(
                    started.elapsed() < Duration::from_secs(10),
                    "query never reached a stable generation"
                );
                std::thread::yield_now();
            }
            Err(error) => panic!("unexpected retrieval failure: {error}"),
        }
    }
}

// Public MCP clients only retry the explicit generation-conflict contract,
// never arbitrary transport, source corruption, protocol or application errors.
fn mcp_generation_conflict(error: &rmcp::ServiceError) -> bool {
    matches!(error, rmcp::ServiceError::McpError(e)
        if e.code.0 == -32603
        && e.message.starts_with("index changed during retrieval after ")
        && e.data.as_ref().and_then(|d|d.get("retryable")).and_then(serde_json::Value::as_bool)==Some(true))
}

#[test]
fn mcp_retry_requires_explicit_generation_conflict_not_just_retryable() {
    let make = |message: &str, data: serde_json::Value| {
        rmcp::ServiceError::McpError(rmcp::ErrorData::internal_error(
            message.to_owned(),
            Some(data),
        ))
    };
    assert!(mcp_generation_conflict(&make(
        "index changed during retrieval after 1 attempts; retry the query",
        json!({"retryable":true})
    )));
    assert!(!mcp_generation_conflict(&make(
        "source proof mismatch",
        json!({"retryable":true})
    )));
    assert!(!mcp_generation_conflict(&make(
        "index changed during retrieval after 1 attempts; retry the query",
        json!({"retryable":false})
    )));
    assert!(!mcp_generation_conflict(&make(
        "index changed during retrieval after 1 attempts; retry the query",
        json!({})
    )));
}

fn unit(path: &str, version: usize) -> FileWriteUnit {
    let text = format!("fn needle() {{ marker{version}(); }}");
    let source = cc_model::source::SourceSnapshot::new(text.as_bytes());
    // Preserve the independent contention/scoring fixture, but publish valid
    // source and document identities through the same transaction as its text.
    let mut outcome = ParseOutcome {
        summary: text.clone(),
        chunks: vec![ChunkRecord {
            source: Some(cc_model::source::ChunkSource {
                source: source.identity().clone(),
                span: source.whole(),
                slice_digest: source.slice_digest(source.whole()).unwrap(),
                boundary: "authored_contention_fixture".into(),
                owner: None,
                signature: None,
            }),
            chunk_id: format!("chunk:{path}"),
            file_path: path.into(),
            language: Language::Rust,
            chunk_index: 0,
            start_line: 1,
            end_line: 1,
            breadcrumb: "needle".into(),
            text: text.clone(),
            symbol_name: Some("needle".into()),
            symbol_kind: None,
            token_estimate: 20,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 1.0,
        }],
        parser_tier: ParserTier::TreeSitter,
        parser_confidence: 1.0,
        chunk_policy: Some(cc_model::chunk_policy::ChunkPolicy::default().fingerprint()),
        document_spec: Some(cc_index::documents::delta::spec_fingerprint().into()),
        ..Default::default()
    };
    outcome.documents = Some(cc_index::documents::delta::prepare(&source, &outcome, &[]).unwrap());
    FileWriteUnit {
        rel_path: path.into(),
        language: Language::Rust,
        content_hash: source.identity().content_digest.clone(),
        mtime: version as f64,
        size: text.len() as u64,
        outcome,
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires explicit CODECORTEX_BENCH_BINARY; concurrent real stdio calls"]
async fn p1d_mcp_concurrent_search_and_incremental_calls_keep_scope() {
    use rmcp::{
        model::CallToolRequestParams,
        transport::{ConfigureCommandExt, TokioChildProcess},
        ServiceExt,
    };
    let d = Arc::new(tempfile::tempdir().unwrap());
    for n in 0..8 {
        let p = d.path().join(format!("scope{n}"));
        std::fs::create_dir(&p).unwrap();
        std::fs::write(p.join("a.py"), "def needle():\n    return 0\n").unwrap();
    }
    std::fs::write(
        d.path().join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1}}"#,
    )
    .unwrap();
    let binary = std::env::var("CODECORTEX_BENCH_BINARY").unwrap();
    let transport = TokioChildProcess::new(tokio::process::Command::new(binary).configure(|cmd| {
        cmd.args(["mcp", "--project-path"])
            .arg(d.path())
            .env("CODECORTEX_PPID_POLL_MS", "0")
            .env_remove("CODECORTEX_CACHE_DIR")
            .stderr(std::process::Stdio::null());
    }))
    .unwrap();
    let c = ().serve(transport).await.unwrap();
    let request = |name: &str, args: serde_json::Value| {
        CallToolRequestParams::new(name.to_owned())
            .with_arguments(args.as_object().unwrap().clone())
    };
    let r = c
        .call_tool(request("index", json!({"path":d.path(),"full":true})))
        .await
        .unwrap();
    assert_ne!(r.is_error, Some(true));
    // P4-C intentionally refuses current-source claims between a disk write
    // and the corresponding index commit. Verify that state deterministically
    // rather than relying on thread timing to exercise an empty/partial result.
    std::fs::write(
        d.path().join("scope0/a.py"),
        "def needle():\n    return 9\n",
    )
    .unwrap();
    let stale = c
        .call_tool(request(
            "search",
            json!({"query":"needle","path_prefix":"scope0","top_k":5}),
        ))
        .await
        .unwrap();
    assert_ne!(stale.is_error, Some(true));
    let stale_value = &stale.structured_content.as_ref().unwrap()["result"];
    assert!(stale_value["machine_pack"]["hits"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        stale_value["evidence_summary"]["source_freshness"]["partial"],
        true
    );
    assert_eq!(
        stale_value["evidence_summary"]["source_freshness"]["omitted_files"]["scope0/a.py"],
        "stale_with_disk_change"
    );
    std::fs::write(
        d.path().join("scope0/a.py"),
        "def needle():\n    return 0\n",
    )
    .unwrap();
    let mut set = tokio::task::JoinSet::new();
    for reader in 0..8 {
        let peer = c.peer().clone();
        set.spawn(async move {
            let mut latencies=Vec::new();
            let mut source_omissions=0;
            let mut generation_conflicts=0;
            for _ in 0..8 {
                let args=json!({"query":"needle","path_prefix":format!("scope{reader}"),"pinned_files":[format!("scope{}/a.py",(reader+1)%8)],"top_k":5});
                let start=Instant::now();
                let r=tokio::time::timeout(Duration::from_secs(10),async {
                    for _ in 0..64 {
                        match peer.call_tool(CallToolRequestParams::new("search").with_arguments(args.as_object().unwrap().clone())).await {
                            Ok(response)=>return response,
                            Err(error) if mcp_generation_conflict(&error)=> {
                                generation_conflicts+=1;
                                tokio::task::yield_now().await;
                            }
                            Err(error)=>panic!("unexpected MCP retrieval error: {error}"),
                        }
                    }
                    panic!("MCP query did not stabilize within 64 conflict retries")
                }).await.expect("MCP query generation retry watchdog expired");
                assert_ne!(r.is_error,Some(true));let value=&r.structured_content.as_ref().unwrap()["result"];
                let hits=value["machine_pack"]["hits"].as_array().unwrap();
                let path=format!("scope{reader}/a.py");
                assert!(hits.iter().all(|h|h["file_path"]==path));
                let freshness=&value["evidence_summary"]["source_freshness"];
                assert_eq!(freshness["budget_exhausted"],false);
                if hits.is_empty() {
                    assert_eq!(freshness["partial"],true,"empty success must carry source omission: {value}");
                    let reason=freshness["omitted_files"][&path].as_str().expect("explicit in-scope omission");
                    assert!(matches!(reason,"stale_with_disk_change"|"index_changed_retry"|"indexed_snapshot_mismatch"|"document_version_changed"),"unexpected omission: {reason}");
                    source_omissions+=1;
                } else {
                    assert!(hits.iter().all(|h|h["metadata"]["source_freshness"]["status"]=="current_verified" && h["metadata"]["document"]["doc_version"].is_string()));
                    for hit in hits {
                        let proof:cc_model::source::ChunkSource=serde_json::from_value(hit["metadata"]["source_evidence"].clone()).unwrap();
                        assert!(proof.validate(hit["text"].as_str().unwrap()));
                    }
                }
                latencies.push(start.elapsed().as_micros() as u64);
            }
            json!({"reader":reader,"latencies_us":latencies,"searches":8,"explicit_source_omissions":source_omissions,"generation_conflict_retries":generation_conflicts,"retry_contract":"MCP -32603 + generation-conflict message + retryable=true only; max64 within10s"})
        });
    }
    let peer = c.peer().clone();
    let dir = d.clone();
    set.spawn(async move {
        for n in 1..=8 {
            std::fs::write(
                dir.path().join("scope0/a.py"),
                format!("def needle():\n    return {n}\n"),
            )
            .unwrap();
            let args = json!({"path":dir.path(),"full":false,"changed_paths":["scope0/a.py"]});
            let r = peer
                .call_tool(
                    CallToolRequestParams::new("index")
                        .with_arguments(args.as_object().unwrap().clone()),
                )
                .await
                .unwrap();
            assert_ne!(r.is_error, Some(true));
        }
        json!({"incremental_calls":8})
    });
    let mut receipts = Vec::new();
    tokio::time::timeout(Duration::from_secs(25), async {
        while let Some(r) = set.join_next().await {
            receipts.push(r.unwrap());
        }
    })
    .await
    .expect("MCP concurrency watchdog expired");
    let r = c
        .call_tool(request(
            "search",
            json!({"query":"needle","path_prefix":"scope0"}),
        ))
        .await
        .unwrap();
    assert!(
        r.structured_content.as_ref().unwrap()["result"]["machine_pack"]["hits"][0]["text"]
            .as_str()
            .unwrap()
            .contains("return 8")
    );
    // Every scope must return current, nonempty source again after the writer
    // stops. This preserves the old availability assertion in the state where
    // its premise (disk and index agree) is actually true.
    for reader in 0..8 {
        let r = c
            .call_tool(request(
                "search",
                json!({"query":"needle","path_prefix":format!("scope{reader}"),"top_k":5}),
            ))
            .await
            .unwrap();
        assert_ne!(r.is_error, Some(true));
        let value = &r.structured_content.as_ref().unwrap()["result"];
        let hits = value["machine_pack"]["hits"].as_array().unwrap();
        assert!(!hits.is_empty());
        assert_eq!(
            value["evidence_summary"]["source_freshness"]["partial"],
            false
        );
        assert!(hits
            .iter()
            .all(|h| h["file_path"] == format!("scope{reader}/a.py")));
        assert!(hits[0]["text"].as_str().unwrap().contains(if reader == 0 {
            "return 8"
        } else {
            "return 0"
        }));
    }
    c.cancel().await.unwrap();
    if let Ok(out) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(std::path::Path::new(&out).join("mcp-concurrency.json"),serde_json::to_vec_pretty(&json!({"read_pool":1,"queries":64,"incremental_calls":8,"scope_errors":0,"deterministic_stale_disk_verified":true,"quiescent_scopes_verified":8,"post_write_content_verified":true,"watchdog_seconds":25,"receipts":receipts})).unwrap()).unwrap();
    }
}

#[test]
fn stats_metadata_does_not_reenter_single_connection_pool() {
    let d = Arc::new(tempfile::tempdir().unwrap());
    let db = Arc::new(
        IndexDb::open_with_read_pool_size(&d.path().join("index.sqlite3"), 1)
            .unwrap()
            .0,
    );
    db.writes()
        .replace_files_batch(&[unit("src/a.rs", 0)])
        .unwrap();
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result = db.reads().stats(d.path());
        let _ = tx.send(result);
    });
    let stats = rx
        .recv_timeout(Duration::from_secs(3))
        .expect("stats held the only read connection while requesting another")
        .unwrap();
    assert_eq!(stats.indexed_files, 1);
    assert_eq!(stats.indexed_chunks, 1);
    worker.join().unwrap();
}

#[test]
fn one_connection_wait_release_and_sql_error_do_not_leak_pool_slots() {
    let d = tempfile::tempdir().unwrap();
    let db = Arc::new(
        IndexDb::open_with_read_pool_size(&d.path().join("index.sqlite3"), 1)
            .unwrap()
            .0,
    );
    let held = db.read_conn().unwrap();
    let (start_tx, start_rx) = mpsc::channel();
    let (tx, rx) = mpsc::channel();
    let other = db.clone();
    let worker = std::thread::spawn(move || {
        start_tx.send(()).unwrap();
        let start = Instant::now();
        let r = other.read_conn().map(|_| start.elapsed().as_micros());
        tx.send(r).unwrap();
    });
    start_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(rx.recv_timeout(Duration::from_millis(40)).is_err());
    drop(held);
    let waited = rx
        .recv_timeout(Duration::from_secs(3))
        .expect("pool did not recover")
        .unwrap();
    worker.join().unwrap();
    assert!(db
        .retrieval()
        .fts_chunk_candidates("\"", &cc_db::ChunkScope::default(), 5)
        .is_err());
    assert!(db
        .retrieval()
        .fts_chunk_candidates("needle", &cc_db::ChunkScope::default(), 5)
        .is_ok());
    println!("P1D_POOL_RELEASE waited_us={waited} connection_slots=1 invalid_SQL_recovered=true");
}
#[test]
fn concurrent_queries_writes_single_reader_keep_scope_and_quiescent_freshness() {
    let mut results = Vec::new();
    for concurrency in [1, 4, 8, 16] {
        let d = Arc::new(tempfile::tempdir().unwrap());
        let db = Arc::new(
            IndexDb::open_with_read_pool_size(&d.path().join("index.sqlite3"), 1)
                .unwrap()
                .0,
        );
        let paths: Vec<_> = (0..4).map(|n| format!("scope{n}/a.rs")).collect();
        db.writes()
            .replace_files_batch(&paths.iter().map(|p| unit(p, 0)).collect::<Vec<_>>())
            .unwrap();
        let mut config = ProjectConfig::default();
        config.search.grep_scan_cap = 32;
        let engine = Arc::new(SearchEngine::new(db.clone(), &config, None));
        let conflicts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(concurrency + 2));
        let (tx, rx) = mpsc::channel();
        let mut handles = Vec::new();
        for worker in 0..concurrency {
            let (e, b, t, keep) = (engine.clone(), barrier.clone(), tx.clone(), d.clone());
            let conflicts = conflicts.clone();
            handles.push(std::thread::spawn(move || {
                let _keep = keep;
                b.wait();
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut latencies = Vec::new();
                    for n in 0..16 {
                        let scope = format!("scope{}", (worker + n) % 4);
                        let q = SearchRequest {
                            query: "needle".into(),
                            path_prefix: Some(scope.clone()),
                            include_grep: true,
                            top_k: 5,
                            pinned_file_paths: Some(vec![format!(
                                "scope{}/a.rs",
                                (worker + n + 1) % 4
                            )]),
                            ..Default::default()
                        };
                        let start = Instant::now();
                        let out =
                            retry_generation_conflict(|| e.search_with_diagnostics(&q), &conflicts);
                        for hit in &out.hits {
                            let proof: cc_model::source::ChunkSource =
                                serde_json::from_value(hit.metadata["source_evidence"].clone())
                                    .unwrap();
                            assert!(proof.validate(&hit.text));
                            for candidate in out
                                .lanes
                                .iter()
                                .flat_map(|lane| &lane.candidates)
                                .filter(|candidate| candidate.legacy_chunk_id == hit.chunk_id)
                            {
                                assert_eq!(
                                    serde_json::to_value(&candidate.document).unwrap(),
                                    hit.metadata["document"]
                                );
                            }
                        }
                        assert!(!out.hits.is_empty());
                        assert!(out
                            .hits
                            .iter()
                            .all(|h| h.file_path == format!("{scope}/a.rs")));
                        assert_eq!(out.cost.lexical_sql.rows, 1);
                        assert!(out.grep.as_ref().unwrap().scanned <= 1);
                        let cached = retry_generation_conflict(|| e.search(&q), &conflicts);
                        assert!(cached
                            .iter()
                            .all(|h| h.file_path == format!("{scope}/a.rs")));
                        latencies.push(start.elapsed().as_micros() as u64);
                    }
                    latencies
                }))
                .map_err(|_| "reader assertion failed");
                let _ = t.send((false, outcome));
            }));
        }
        let (wdb, b, t, keep) = (db.clone(), barrier.clone(), tx.clone(), d.clone());
        let write_paths = paths.clone();
        handles.push(std::thread::spawn(move || {
            let _keep = keep;
            b.wait();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                for n in 1..=32 {
                    wdb.writes()
                        .replace_files_batch(
                            &write_paths.iter().map(|p| unit(p, n)).collect::<Vec<_>>(),
                        )
                        .unwrap();
                    std::thread::yield_now();
                }
                Vec::new()
            }))
            .map_err(|_| "writer failed");
            let _ = t.send((true, result));
        }));
        drop(tx);
        let start = Instant::now();
        barrier.wait();
        let mut latencies = Vec::new();
        for _ in 0..=concurrency {
            let (_, r) = rx
                .recv_timeout(Duration::from_secs(20))
                .expect("bounded concurrency watchdog expired; possible nested checkout");
            latencies.extend(r.unwrap());
        }
        for h in handles {
            h.join().unwrap();
        }
        // Once writers and all old readers have stopped, no delayed text cache
        // entry is allowed to corrupt the new generation.
        for path in &paths {
            let out = engine
                .search_with_diagnostics(&SearchRequest {
                    query: "needle".into(),
                    file_paths: Some(vec![path.clone()]),
                    include_grep: false,
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(out.hits.len(), 1);
            assert!(out.hits[0].text.contains("marker32"));
        }
        results.push(json!({"readers":concurrency,"reader_connections":1,"paired_searches":concurrency*16,"retryable_generation_conflicts":conflicts.load(std::sync::atomic::Ordering::Relaxed),"writes":32,"elapsed_us":start.elapsed().as_micros(),"pair_latencies_us":latencies,"scope_errors":0,"quiescent_fresh":true,"watchdog_seconds":20}));
    }
    if let Ok(out) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(std::path::Path::new(&out).join("concurrency.json"),serde_json::to_vec_pretty(&json!({"profile":"bounded_contention_test_not_open_loop_throughput_or_atomic_snapshot_proof","samples":results})).unwrap()).unwrap();
    }
    println!(
        "P1D_CONCURRENCY {}",
        serde_json::to_string(&results).unwrap()
    );
}
