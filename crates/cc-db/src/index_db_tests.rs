//! Inline tests of the `index_db` core (open/schema, rebuild protocol,
//! batch write, metadata, stats). Child module of `index_db` via `#[path]`,
//! so `super::*` and crate-private items resolve exactly as before the
//! file split.

use super::*;

use crate::sql_util::IN_BATCH_SIZE;
use cc_model::parse::ParseOutcome;
use cc_model::Language;
use tempfile::TempDir;

#[test]
fn open_creates_schema() {
    let tmp = TempDir::new().unwrap();
    let db = IndexDb::open(&tmp.path().join("index.sqlite3")).unwrap().0;
    let conn = db.read_conn().unwrap();
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    assert!(tables.contains(&"files".to_string()));
    assert!(tables.contains(&"chunks".to_string()));
    assert!(tables.contains(&"symbols".to_string()));
    assert!(tables.contains(&"call_edges".to_string()));
}

#[test]
fn metadata_round_trip() {
    let tmp = TempDir::new().unwrap();
    let db = IndexDb::open(&tmp.path().join("test.db")).unwrap().0;
    db.set_metadata("version", "1.0").unwrap();
    assert_eq!(db.get_metadata("version").unwrap(), Some("1.0".to_string()));
    assert_eq!(db.get_metadata("nonexistent").unwrap(), None);
}

/// The read pool is `query_only`: writes through a pooled connection
/// must fail instead of silently bypassing the WriteOps epoch bump.
#[test]
fn read_pool_connections_reject_writes() {
    let tmp = TempDir::new().unwrap();
    let db = IndexDb::open(&tmp.path().join("ro.db")).unwrap().0;
    let conn = db.read_conn().unwrap();
    let err = conn
        .execute("INSERT INTO metadata(key, value) VALUES('rogue', '1')", [])
        .expect_err("INSERT through a read pool connection must fail");
    assert!(
        err.to_string().contains("readonly"),
        "expected SQLITE_READONLY, got: {err}"
    );
    // Reads keep working on the same connection.
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

fn file_unit(rel_path: &str) -> FileWriteUnit {
    FileWriteUnit {
        rel_path: rel_path.to_string(),
        language: Language::Rust,
        content_hash: format!("hash-{rel_path}"),
        mtime: 1.0,
        size: 1,
        outcome: ParseOutcome::default(),
    }
}

fn chunk(chunk_index: u32, text: &str) -> cc_model::ChunkRecord {
    cc_model::ChunkRecord {
        chunk_id: format!("chunk:{chunk_index}"),
        file_path: "src/c.rs".to_string(),
        language: Language::Rust,
        chunk_index,
        start_line: 1,
        end_line: 2,
        breadcrumb: "root".to_string(),
        text: text.to_string(),
        symbol_name: None,
        symbol_kind: None,
        token_estimate: 4,
        parser_tier: ParserTier::TreeSitter,
        parser_confidence: 1.0,
    }
}

/// prepare 阶段预压缩的 side-car 与事务内回退压缩必须产生逐字节一致的
/// chunks 行（text blob + text_encoding），且读路径还原出原始文本。
#[test]
fn precompressed_chunk_payloads_match_in_transaction_compression() {
    let tiny = "tiny";
    let big = "fn handler() { compute_all_the_things(); }\n".repeat(40);
    let mut unit = file_unit("src/c.rs");
    unit.outcome.chunks = vec![chunk(0, tiny), chunk(1, &big)];

    // Path A: no side-car — compression falls back inside the transaction.
    let tmp_a = TempDir::new().unwrap();
    let db_a = IndexDb::open(&tmp_a.path().join("a.db")).unwrap().0;
    db_a.replace_files_batch(std::slice::from_ref(&unit))
        .unwrap();

    // Path B: side-car pre-compressed with the shared policy helper.
    let tmp_b = TempDir::new().unwrap();
    let db_b = IndexDb::open(&tmp_b.path().join("b.db")).unwrap().0;
    let blobs: Vec<Option<Vec<u8>>> = unit
        .outcome
        .chunks
        .iter()
        .map(|c| compress_chunk_text(&c.text))
        .collect();
    let mut precompressed = PrecompressedChunks::new();
    precompressed.insert("src/c.rs".to_string(), blobs);
    db_b.write_incremental_batch(
        &[],
        std::slice::from_ref(&unit),
        &[],
        &[],
        &[],
        &precompressed,
    )
    .unwrap();

    let chunk_rows = |db: &IndexDb| -> Vec<(i64, rusqlite::types::Value, String)> {
        let conn = db.read_conn().unwrap();
        let mut stmt = conn
            .prepare("SELECT chunk_index, text, text_encoding FROM chunks ORDER BY chunk_index")
            .unwrap();
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, rusqlite::types::Value>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        rows
    };

    let rows_a = chunk_rows(&db_a);
    let rows_b = chunk_rows(&db_b);
    assert_eq!(rows_a, rows_b, "on-disk chunk rows must be byte-identical");
    assert_eq!(rows_a[0].2, "plain", "small chunk stays plain");
    assert_eq!(rows_a[1].2, "zstd", "large compressible chunk is zstd");

    // Read path decodes the precompressed payload back to the original.
    let conn = db_b.read_conn().unwrap();
    let restored: String = conn
        .query_row(
            "SELECT text, text_encoding FROM chunks WHERE chunk_index = 1",
            [],
            |row| read_chunk_text_with_encoding(row, 0, 1),
        )
        .unwrap();
    assert_eq!(restored, big, "decompressed chunk text round-trips");
}

/// 批量 FTS 删除（IN 分块）必须覆盖批内所有路径：替换后 chunks_fts /
/// files_fts / literal_fts 无陈旧行，移除后与基表同步清空。文件数刻意
/// 超过 IN_BATCH_SIZE，验证分块边界两侧都被删除。
#[test]
fn batched_fts_deletes_cover_all_paths_across_chunk_boundary() {
    let tmp = TempDir::new().unwrap();
    let db = IndexDb::open(&tmp.path().join("fts.db")).unwrap().0;

    let total = IN_BATCH_SIZE + 7;
    let units: Vec<FileWriteUnit> = (0..total)
        .map(|i| {
            let path = format!("src/f{i}.rs");
            let mut unit = file_unit(&path);
            let mut c = chunk(0, &format!("fn body_{i}() {{}}"));
            c.chunk_id = format!("chunk:{path}");
            c.file_path = path.clone();
            unit.outcome.chunks = vec![c];
            unit.outcome.literal_index = vec![cc_model::LiteralRecord {
                literal_id: format!("lit:{path}"),
                file_path: path,
                literal: format!("marker {i}"),
                literal_kind: "string".to_string(),
                line: 1,
                container: None,
                confidence: 0.8,
                enclosing_symbol_uid: None,
                key_path: None,
            }];
            unit
        })
        .collect();
    db.replace_files_batch(&units).unwrap();

    let count = |table: &str| -> i64 {
        db.read_conn()
            .unwrap()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    };
    assert_eq!(count("chunks_fts"), total as i64);

    // Re-replace every file through the incremental batch path: exactly
    // one FTS row per file must remain (no stale duplicates).
    db.write_incremental_batch(&[], &units, &[], &[], &[], &PrecompressedChunks::new())
        .unwrap();
    assert_eq!(count("chunks_fts"), total as i64);
    assert_eq!(count("files_fts"), total as i64);
    assert_eq!(count("literal_fts"), total as i64);

    // Remove every file: FTS mirrors empty out together with base tables.
    let paths: Vec<String> = units.iter().map(|u| u.rel_path.clone()).collect();
    db.remove_files_batch(&paths).unwrap();
    for table in ["chunks_fts", "files_fts", "literal_fts", "files"] {
        assert_eq!(count(table), 0, "{table} should be empty after removal");
    }
}

/// 原地替换（同路径 delete+insert）不得级联删除 test_edges：边是纯路径
/// 派生的，内容更新不会改变边集；批内 to_remove 与 remove_files_batch
/// 的级联删除必须保留。
#[test]
fn replace_in_place_keeps_test_edges_removal_cascades() {
    let tmp = TempDir::new().unwrap();
    let db = IndexDb::open(&tmp.path().join("te.db")).unwrap().0;

    let code = file_unit("src/foo.rs");
    let mut test = file_unit("tests/foo_test.rs");
    test.outcome.is_test_file = true;
    let units = vec![code, test];
    db.write_incremental_batch(&[], &units, &[], &[], &[], &PrecompressedChunks::new())
        .unwrap();
    db.rebuild_test_edges_for_files(&["src/foo.rs".to_string(), "tests/foo_test.rs".to_string()])
        .unwrap();

    let edge_count = |db: &IndexDb| -> i64 {
        db.read_conn()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM test_edges", [], |r| r.get(0))
            .unwrap()
    };
    assert_eq!(edge_count(&db), 1, "fixture must produce one test edge");

    // Content-only update through both replace paths: the edge survives.
    db.write_incremental_batch(&[], &units, &[], &[], &[], &PrecompressedChunks::new())
        .unwrap();
    assert_eq!(
        edge_count(&db),
        1,
        "in-place incremental replace must keep test_edges"
    );
    db.replace_files_batch(&units).unwrap();
    assert_eq!(
        edge_count(&db),
        1,
        "replace_files_batch must keep test_edges for unchanged paths"
    );

    // Removal still cascades.
    db.write_incremental_batch(
        &["tests/foo_test.rs".to_string()],
        &[],
        &[],
        &[],
        &[],
        &PrecompressedChunks::new(),
    )
    .unwrap();
    assert_eq!(edge_count(&db), 0, "removing a path must drop its edges");
}

/// schema v5 不变量：app-maintained FTS 表（chunks_fts / files_fts /
/// literal_fts）的 rowid 必须与基表行 rowid 对齐且内容一致——索引化
/// 删除（rowid IN 子查询）依赖该对齐。覆盖首次插入与增量替换（基表
/// rowid 变化后重新对齐），以及重复 literal_id 不产生 FTS 孤儿行。
#[test]
fn app_maintained_fts_rowids_align_with_base_tables() {
    let tmp = TempDir::new().unwrap();
    let db = IndexDb::open(&tmp.path().join("align.db")).unwrap().0;

    let literal = |path: &str, suffix: &str| cc_model::LiteralRecord {
        literal_id: format!("lit:{path}:{suffix}"),
        file_path: path.to_string(),
        literal: format!("marker {suffix}"),
        literal_kind: "string".to_string(),
        line: 1,
        container: None,
        confidence: 0.8,
        enclosing_symbol_uid: None,
        key_path: None,
    };
    let units: Vec<FileWriteUnit> = (0..3)
        .map(|i| {
            let path = format!("src/a{i}.rs");
            let mut unit = file_unit(&path);
            unit.outcome.chunks = (0..2)
                .map(|ci| {
                    let mut c = chunk(ci, &format!("fn body_{i}_{ci}() {{}}"));
                    c.chunk_id = format!("chunk:{path}:{ci}");
                    c.file_path = path.clone();
                    c
                })
                .collect();
            // 同一 literal_id 刻意重复：OR IGNORE 必须跳过重复项的
            // FTS 写入，而不是留下指向已死 rowid 的孤儿行。
            unit.outcome.literal_index = vec![literal(&path, "x"), literal(&path, "x")];
            unit
        })
        .collect();
    db.replace_files_batch(&units).unwrap();
    // 增量替换：基表行获得新 rowid，FTS 必须随之重新对齐。
    db.write_incremental_batch(&[], &units, &[], &[], &[], &PrecompressedChunks::new())
        .unwrap();

    let conn = db.read_conn().unwrap();
    let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
    for (fts, base, id_col) in [
        ("chunks_fts", "chunks", "chunk_id"),
        ("files_fts", "files", "file_path"),
        ("literal_fts", "literal_index", "literal_id"),
    ] {
        assert_eq!(
            count(&format!("SELECT COUNT(*) FROM {fts}")),
            count(&format!("SELECT COUNT(*) FROM {base}")),
            "{fts} must mirror {base} 1:1 (no stale/orphan rows)"
        );
        assert_eq!(
            count(&format!(
                "SELECT COUNT(*) FROM {fts} f JOIN {base} b ON b.rowid = f.rowid \
                     WHERE b.{id_col} = f.{id_col}"
            )),
            count(&format!("SELECT COUNT(*) FROM {base}")),
            "every {fts} rowid must point at its own {base} row"
        );
    }
    // 对齐删除端到端：按 file_path 删除后 FTS 不留任何行。
    db.remove_files_batch(&["src/a0.rs".to_string(), "src/a1.rs".to_string()])
        .unwrap();
    assert_eq!(count("SELECT COUNT(*) FROM chunks_fts"), 2);
    assert_eq!(count("SELECT COUNT(*) FROM files_fts"), 1);
    assert_eq!(count("SELECT COUNT(*) FROM literal_fts"), 1);
}

#[test]
fn route_edge_resolution_provenance_round_trips() {
    let tmp = TempDir::new().unwrap();
    let db = IndexDb::open(&tmp.path().join("routes.db")).unwrap().0;

    let mut unit = file_unit("src/routes.ts");
    unit.outcome
        .route_edges
        .push(cc_model::edge::RouteEdgeRecord {
            edge_id: "route:1".to_string(),
            file_path: "src/routes.ts".to_string(),
            route_path: "/users".to_string(),
            handler_name: Some("getUsers".to_string()),
            method: Some("GET".to_string()),
            line: 5,
            start_col: 0,
            end_line: None,
            end_col: 0,
            handler_symbol_id: Some("id:getUsers".to_string()),
            handler_symbol_uid: Some("uid:getUsers".to_string()),
            handler_expr: None,
            router_symbol_uid: None,
            framework: Some("express".to_string()),
            route_kind: None,
            confidence: 0.8,
            parser_tier: cc_model::ParserTier::TreeSitter,
            resolution_strategy: Some("route_ladder:global_unique".to_string()),
            resolution_confidence: Some(0.75),
        });
    db.replace_files_batch(&[unit]).unwrap();

    let edges = db
        .reads()
        .load_file_edges_for_reresolve("src/routes.ts")
        .unwrap();
    assert_eq!(edges.route_edges.len(), 1);
    let route = &edges.route_edges[0];
    assert_eq!(
        route.resolution_strategy.as_deref(),
        Some("route_ladder:global_unique")
    );
    assert_eq!(route.resolution_confidence, Some(0.75));
}

#[test]
fn generation_starts_at_zero_and_index_writes_bump_index_epoch() {
    let tmp = TempDir::new().unwrap();
    let db = IndexDb::open(&tmp.path().join("gen.db")).unwrap().0;

    // Old databases (or fresh ones) without the epoch keys read as 0/0.
    assert_eq!(db.generation().unwrap(), IndexGeneration::default());

    db.replace_files_batch(&[file_unit("src/a.rs")]).unwrap();
    let after_write = db.generation().unwrap();
    assert_eq!(after_write.index_epoch, 1);
    assert_eq!(after_write.evidence_epoch, 0);

    db.write_incremental_batch(
        &["src/a.rs".to_string()],
        &[],
        &[],
        &[],
        &[],
        &PrecompressedChunks::new(),
    )
    .unwrap();
    let after_batch = db.generation().unwrap();
    assert!(after_batch.index_epoch > after_write.index_epoch);
    assert_eq!(after_batch.evidence_epoch, 0);

    // An empty incremental batch writes nothing and must not bump.
    db.write_incremental_batch(&[], &[], &[], &[], &[], &PrecompressedChunks::new())
        .unwrap();
    assert_eq!(db.generation().unwrap(), after_batch);
}

#[test]
fn full_rebuild_advances_both_epochs_past_previous_values() {
    let tmp = TempDir::new().unwrap();
    // Production naming: rebuild_with_temp_db derives WAL/tmp paths from
    // the `.sqlite3` extension, so the test must use it too.
    let db = IndexDb::open(&tmp.path().join("index.sqlite3")).unwrap().0;

    db.replace_files_batch(&[file_unit("src/a.rs")]).unwrap();
    db.upsert_runtime_evidence(
        "ev1",
        "svc",
        Some("GET"),
        "/x",
        None,
        "2024-01-01T00:00:00Z",
    )
    .unwrap();
    let before = db.generation().unwrap();
    assert!(before.index_epoch >= 1 && before.evidence_epoch >= 1);

    db.rebuild_with_temp_db(|_conn| Ok(())).unwrap();
    let after = db.generation().unwrap();
    assert!(after.index_epoch > before.index_epoch);
    assert!(after.evidence_epoch > before.evidence_epoch);
}

#[test]
fn rebuild_generation_exceeds_writes_committed_during_rebuild() {
    use std::sync::mpsc;
    use std::sync::Arc;

    let tmp = TempDir::new().unwrap();
    let db = Arc::new(IndexDb::open(&tmp.path().join("index.sqlite3")).unwrap().0);
    db.replace_files_batch(&[file_unit("src/a.rs")]).unwrap();

    let (started_tx, started_rx) = mpsc::channel::<()>();
    let (go_tx, go_rx) = mpsc::channel::<()>();
    let rebuild_db = Arc::clone(&db);
    let rebuild = std::thread::spawn(move || {
        rebuild_db.rebuild_with_temp_db(move |_conn| {
            started_tx.send(()).unwrap();
            // Park mid-rebuild so the main thread can commit writes that
            // advance the live epochs past the floor snapshot.
            go_rx.recv().unwrap();
            Ok(())
        })
    });

    started_rx.recv().unwrap();
    db.replace_files_batch(&[file_unit("src/b.rs")]).unwrap();
    db.upsert_runtime_evidence(
        "ev-mid",
        "svc",
        Some("GET"),
        "/y",
        None,
        "2024-01-01T00:00:00Z",
    )
    .unwrap();
    let live_during_rebuild = db.generation().unwrap();
    go_tx.send(()).unwrap();
    rebuild.join().unwrap().unwrap();

    // The swapped-in database must exceed the epochs observed for the
    // concurrent writes, not just the floor taken when the rebuild began.
    let after = db.generation().unwrap();
    assert!(after.index_epoch > live_during_rebuild.index_epoch);
    assert!(after.evidence_epoch > live_during_rebuild.evidence_epoch);
}

#[test]
fn schema_mismatch_rebuild_advances_generation_past_old_values() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("index.sqlite3");
    let old_generation;
    {
        let db = IndexDb::open(&path).unwrap().0;
        db.replace_files_batch(&[file_unit("src/a.rs")]).unwrap();
        db.upsert_runtime_evidence(
            "ev1",
            "svc",
            Some("GET"),
            "/x",
            None,
            "2024-01-01T00:00:00Z",
        )
        .unwrap();
        old_generation = db.generation().unwrap();
        assert!(old_generation.index_epoch >= 1 && old_generation.evidence_epoch >= 1);
        // Force a schema mismatch for the next open.
        let conn = db.write_conn.lock().unwrap();
        conn.pragma_update(None, "user_version", 1u32).unwrap();
    }

    let (db, status) = IndexDb::open(&path).unwrap();
    assert!(matches!(status, SchemaStatus::Initialized));
    let rebuilt = db.generation().unwrap();
    assert!(
        rebuilt.index_epoch > old_generation.index_epoch,
        "mismatch rebuild must not roll index_epoch back ({} <= {})",
        rebuilt.index_epoch,
        old_generation.index_epoch
    );
    assert!(rebuilt.evidence_epoch > old_generation.evidence_epoch);
}

// Stress repro for the pool-drop vs file-delete race fixed by in-place
// schema reset; run with --ignored when touching the mismatch-rebuild path.
#[test]
#[ignore]
fn mismatch_rebuild_stress_loop() {
    for iteration in 0..300 {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("index.sqlite3");
        {
            let db = IndexDb::open(&path).unwrap().0;
            db.replace_files_batch(&[file_unit("src/a.rs")]).unwrap();
            let _ = db.generation().unwrap();
            let conn = db.write_conn.lock().unwrap();
            conn.pragma_update(None, "user_version", 1u32).unwrap();
        }
        let (db, _status) =
            IndexDb::open(&path).unwrap_or_else(|e| panic!("iter {iteration}: {e:?}"));
        let _ = db.generation().unwrap();
    }
}

#[test]
fn empty_file_state() {
    let tmp = TempDir::new().unwrap();
    let db = IndexDb::open(&tmp.path().join("test.db")).unwrap().0;
    assert!(db.get_file_state().unwrap().is_empty());
}

#[test]
fn chunk_text_encoding_reads_plain_and_zstd() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
            "CREATE TABLE chunks (chunk_id TEXT PRIMARY KEY, text TEXT NOT NULL, text_encoding TEXT NOT NULL);",
        )
        .unwrap();
    conn.execute(
        "INSERT INTO chunks(chunk_id, text, text_encoding) VALUES('plain', ?1, 'plain')",
        ["hello plain"],
    )
    .unwrap();
    let compressed = zstd::encode_all(std::io::Cursor::new("hello compressed"), 3).unwrap();
    conn.execute(
        "INSERT INTO chunks(chunk_id, text, text_encoding) VALUES('zstd', ?1, 'zstd')",
        rusqlite::params![compressed],
    )
    .unwrap();

    let plain = conn
        .query_row(
            "SELECT text, text_encoding FROM chunks WHERE chunk_id='plain'",
            [],
            |row| read_chunk_text_with_encoding(row, 0, 1),
        )
        .unwrap();
    let zstd = conn
        .query_row(
            "SELECT text, text_encoding FROM chunks WHERE chunk_id='zstd'",
            [],
            |row| read_chunk_text_with_encoding(row, 0, 1),
        )
        .unwrap();

    assert_eq!(plain, "hello plain");
    assert_eq!(zstd, "hello compressed");
}

#[test]
fn resolver_seed_symbols_excludes_requested_files() {
    let tmp = TempDir::new().unwrap();
    let db = IndexDb::open(&tmp.path().join("resolver_seed.db"))
        .unwrap()
        .0;

    {
        let mut conn = db.write_conn.lock().unwrap();
        let tx = conn.transaction().unwrap();
        tx.execute(
                "INSERT INTO files(file_path,language,content_hash,mtime,size,summary,content_excerpt,parser_tier,parser_confidence,is_test_file,indexed_at)
                 VALUES('src/lib.rs','Rust','h1',1.0,1,'','','tree_sitter',1.0,0,'2024-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
        tx.execute(
                "INSERT INTO files(file_path,language,content_hash,mtime,size,summary,content_excerpt,parser_tier,parser_confidence,is_test_file,indexed_at)
                 VALUES('src/main.rs','Rust','h2',1.0,1,'','','tree_sitter',1.0,0,'2024-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
        tx.execute(
                "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,parent_symbol_id,export_name,is_default_export,symbol_uid,framework_role,receiver_type,param_types,return_type,param_count,base_types,implements)
                 VALUES(?1,?2,?3,?4,NULL,1,5,0,0,NULL,NULL,'tree_sitter',1.0,?5,NULL,?6,0,?7,NULL,NULL,NULL,NULL,NULL,NULL,NULL)",
                rusqlite::params!["sym_keep", "src/lib.rs", "helper", "function", "crate.lib.helper", "helper", "uid_keep"],
            ).unwrap();
        tx.execute(
                "INSERT INTO symbols(symbol_id,file_path,name,kind,container,start_line,end_line,start_col,end_col,signature,doc,parser_tier,parser_confidence,qname,parent_symbol_id,export_name,is_default_export,symbol_uid,framework_role,receiver_type,param_types,return_type,param_count,base_types,implements)
                 VALUES(?1,?2,?3,?4,NULL,1,5,0,0,NULL,NULL,'tree_sitter',1.0,?5,NULL,?6,0,?7,NULL,NULL,NULL,NULL,NULL,NULL,NULL)",
                rusqlite::params!["sym_skip", "src/main.rs", "main", "function", "crate.main.main", "main", "uid_skip"],
            ).unwrap();
        tx.commit().unwrap();
    }

    let rows = db
        .resolver_seed_symbols_excluding(&["src/main.rs".to_string()])
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].file_path, "src/lib.rs");
    assert_eq!(rows[0].symbol_uid.as_deref(), Some("uid_keep"));
}

/// End-to-end test: HTTP call edges → route nodes cross-service chain.
///
/// Verifies that:
/// 1. http_calls_by_caller_uid returns the correct outbound call
/// 2. route_nodes_by_normalized_path_and_method resolves the handler
/// 3. http_callers_by_normalized_path_and_method returns the reverse lookup
#[test]
fn http_call_cross_service_chain() {
    let tmp = TempDir::new().unwrap();
    let db = IndexDb::open(&tmp.path().join("http_chain.db")).unwrap().0;

    // Insert test data via raw SQL
    {
        let mut conn = db.write_conn.lock().unwrap();
        let tx = conn.transaction().unwrap();

        // Insert prerequisite files rows for FK constraints
        tx.execute(
                "INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at) VALUES('src/client.ts', 'typescript', 'hash1', 1000, 100, '2024-01-01T00:00:00Z')",
                [],
            ).unwrap();
        tx.execute(
                "INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at) VALUES('src/server/users.ts', 'typescript', 'hash2', 1000, 200, '2024-01-01T00:00:00Z')",
                [],
            ).unwrap();

        // Insert an outbound HTTP call edge: caller_fn_uid calls GET /api/users
        tx.execute(
                "INSERT INTO http_call_edges(edge_id, file_path, caller_symbol_uid, url_or_path, normalized_path, method, call_kind, line, confidence, parser_tier)
                 VALUES('hce_1', 'src/client.ts', 'caller_fn_uid', '/api/users', '/api/users', 'GET', 'http', 42, 0.9, 'tree_sitter')",
                [],
            ).unwrap();

        // Insert a route node: GET /api/users → handler_fn_uid
        tx.execute(
                "INSERT INTO routes(edge_id, file_path, route_path, method, handler_symbol_uid, handler_name, framework, line, end_line, normalized_path, confidence, parser_tier, route_id)
                 VALUES('rn_1', 'src/server/users.ts', '/api/users', 'GET', 'handler_fn_uid', 'getUsers', 'express', 10, 25, '/api/users', 0.85, 'tree_sitter', 'rn_1')",
                [],
            ).unwrap();

        tx.commit().unwrap();
    }

    // 1. Forward: caller_fn_uid → outbound HTTP calls
    let calls = db
        .frontier()
        .http_calls_by_caller_uid("caller_fn_uid", 10)
        .unwrap();
    assert_eq!(calls.len(), 1, "should find 1 outbound HTTP call");
    assert_eq!(calls[0].normalized_path.as_deref(), Some("/api/users"));
    assert_eq!(calls[0].method.as_deref(), Some("GET"));
    assert_eq!(calls[0].caller_symbol_uid.as_deref(), Some("caller_fn_uid"));

    // 2. Resolve: normalized path → route handler
    let routes = db
        .frontier()
        .route_nodes_by_normalized_path_and_method("/api/users", Some("GET"), 10)
        .unwrap();
    assert_eq!(routes.len(), 1, "should find 1 matching route node");
    assert_eq!(
        routes[0].handler_symbol_uid.as_deref(),
        Some("handler_fn_uid")
    );
    assert_eq!(routes[0].handler_name.as_deref(), Some("getUsers"));
    assert_eq!(routes[0].file_path, "src/server/users.ts");

    // 3. Reverse: who calls /api/users via HTTP?
    let callers = db
        .frontier()
        .http_callers_by_normalized_path_and_method("/api/users", Some("GET"), 10)
        .unwrap();
    assert_eq!(callers.len(), 1, "should find 1 HTTP caller");
    assert_eq!(
        callers[0].caller_symbol_uid.as_deref(),
        Some("caller_fn_uid")
    );
    assert_eq!(callers[0].file_path, "src/client.ts");

    // 4. Negative case: non-existent path returns empty
    let empty = db
        .frontier()
        .http_calls_by_caller_uid("nonexistent_uid", 10)
        .unwrap();
    assert!(empty.is_empty(), "non-existent caller should return empty");

    let empty_routes = db
        .frontier()
        .route_nodes_by_normalized_path_and_method("/api/nonexistent", Some("GET"), 10)
        .unwrap();
    assert!(
        empty_routes.is_empty(),
        "non-existent route should return empty"
    );
}

// --- Security: SQL injection regression tests ---

#[test]
fn test_find_symbol_safe_with_injection_string() {
    let tmp = TempDir::new().unwrap();
    let db = IndexDb::open(&tmp.path().join("injection_test.db"))
        .unwrap()
        .0;
    // Query with a classic SQL injection payload — should return empty, not panic or corrupt.
    let result = db
        .query()
        .find_symbol("'; DROP TABLE symbols; --", true, 10);
    assert!(result.is_ok(), "injection string should not cause error");
    assert!(result.unwrap().is_empty());
}

#[test]
fn test_find_symbol_safe_with_union_injection() {
    let tmp = TempDir::new().unwrap();
    let db = IndexDb::open(&tmp.path().join("injection_union.db"))
        .unwrap()
        .0;
    let result = db
        .query()
        .find_symbol("' UNION SELECT * FROM sqlite_master --", false, 10);
    assert!(result.is_ok(), "UNION injection should not cause error");
    assert!(result.unwrap().is_empty());
}

#[test]
fn test_find_symbol_safe_with_null_byte() {
    let tmp = TempDir::new().unwrap();
    let db = IndexDb::open(&tmp.path().join("injection_null.db"))
        .unwrap()
        .0;
    let result = db.query().find_symbol("test\0evil", true, 10);
    assert!(result.is_ok(), "null byte should not cause error");
    assert!(result.unwrap().is_empty());
}

#[test]
fn test_find_symbol_safe_with_unicode_injection() {
    let tmp = TempDir::new().unwrap();
    let db = IndexDb::open(&tmp.path().join("injection_unicode.db"))
        .unwrap()
        .0;
    let result = db
        .query()
        .find_symbol("name\u{200B}; DROP TABLE symbols", true, 10);
    assert!(result.is_ok(), "unicode injection should not cause error");
    assert!(result.unwrap().is_empty());
}

#[test]
fn test_metadata_safe_with_injection_key() {
    let tmp = TempDir::new().unwrap();
    let db = IndexDb::open(&tmp.path().join("injection_meta.db"))
        .unwrap()
        .0;
    // Setting metadata with injection key — should work safely via parameterized queries.
    let set_result = db.set_metadata("'; DROP TABLE metadata; --", "value");
    assert!(
        set_result.is_ok(),
        "injection in metadata key should be safe"
    );
    let get_result = db.get_metadata("'; DROP TABLE metadata; --");
    assert!(get_result.is_ok());
    assert_eq!(get_result.unwrap(), Some("value".to_string()));
    // Confirm metadata table still exists by doing another operation.
    let get_normal = db.get_metadata("normal_key");
    assert!(get_normal.is_ok(), "metadata table should still exist");
}
