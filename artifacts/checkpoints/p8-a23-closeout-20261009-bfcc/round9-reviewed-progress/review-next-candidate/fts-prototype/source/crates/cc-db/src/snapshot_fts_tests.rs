//! Differential controls for the snapshot-only FTS window; no scale-study credit.
use super::{can_defer_snapshot_fts, has_rowid_capacity, SnapshotFtsWindow};
use crate::index_db::{FileWriteUnit, IndexDb, PrecompressedChunks};
use crate::SnapshotWriteTxn;
use cc_model::{Language, LiteralRecord, ParseOutcome, ParserTier};
use rusqlite::{types::Value, Connection, StatementStatus};

const TABLES: &[&str] = &[
    "document_manifest",
    "resolution_frontier",
    "semantic_edges",
    "dispatch_sites",
    "resolution_manifests",
    "resolution_dependencies",
    "public_surfaces",
    "files",
    "symbols",
    "imports",
    "symbol_refs",
    "call_edges",
    "chunks",
    "test_edges",
    "routes",
    "chunk_symbol_identity",
    "literal_index",
    "files_fts",
    "chunks_fts",
    "literal_fts",
    "symbols_fts",
    "file_paths_fts",
];

fn connection() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON").unwrap();
    conn.execute_batch(crate::index_migrate::FULL_SCHEMA_SQL)
        .unwrap();
    // Match the actual staging adapter's removal of explicit secondary indexes.
    conn.execute_batch(&crate::direct_writer::drop_index_statements(
        crate::index_migrate::FULL_SCHEMA_SQL,
    ))
    .unwrap();
    conn.set_prepared_statement_cache_capacity(64);
    conn
}

fn literal(id: &str, path: &str, text: &str) -> LiteralRecord {
    LiteralRecord {
        literal_id: id.into(),
        file_path: path.into(),
        literal: text.into(),
        literal_kind: "string".into(),
        line: 1,
        container: None,
        confidence: 0.8,
        enclosing_symbol_uid: None,
        key_path: None,
    }
}

fn units(count: usize) -> Vec<FileWriteUnit> {
    (0..count)
        .map(|n| {
            let path = format!("src/f{n:04}.rs");
            let mut outcome = ParseOutcome {
                summary: format!("summarymarker {n}"),
                ..Default::default()
            };
            outcome.chunks.push(cc_model::ChunkRecord {
                source: None,
                chunk_id: format!("chunk:{n}"),
                file_path: path.clone(),
                language: Language::Rust,
                chunk_index: 0,
                start_line: 1,
                end_line: 2,
                breadcrumb: "root".into(),
                text: format!("chunkmarker {n}"),
                symbol_name: None,
                symbol_kind: None,
                token_estimate: 4,
                parser_tier: ParserTier::Generic,
                parser_confidence: 0.8,
            });
            outcome
                .literal_index
                .push(literal(&format!("literal:{n}"), &path, "literalmarker"));
            FileWriteUnit {
                rel_path: path,
                language: Language::Rust,
                content_hash: format!("hash{n}"),
                mtime: 1.0,
                size: 1,
                outcome,
            }
        })
        .collect()
}

fn original(conn: &Connection, files: &[FileWriteUnit]) -> cc_model::CcResult<()> {
    // The retained original snapshot entry includes the same leaf batching.
    for file in files {
        IndexDb::insert_snapshot_file_data_precompressed(conn, file, None)?;
    }
    Ok(())
}

fn candidate(conn: &Connection, files: &[FileWriteUnit]) -> cc_model::CcResult<()> {
    SnapshotWriteTxn::new(conn).write_file_data(files, &PrecompressedChunks::new())
}

fn rows(conn: &Connection, table: &str) -> Vec<Vec<Value>> {
    let mut statement = conn
        .prepare(&format!("SELECT rowid,* FROM {table} ORDER BY rowid"))
        .unwrap();
    // indexed_at is produced by the unchanged wall clock independently on each
    // writer call. No other physical column or rowid is omitted.
    let columns: Vec<_> = statement
        .column_names()
        .iter()
        .enumerate()
        .filter_map(|(n, name)| (!(table == "files" && *name == "indexed_at")).then_some(n))
        .collect();
    statement
        .query_map([], |row| columns.iter().map(|&n| row.get(n)).collect())
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn dump(conn: &Connection) -> Vec<(String, Vec<Vec<Value>>)> {
    // pragma_table_list distinguishes logical virtual tables from their
    // physical shadow tables without maintaining an incomplete table allowlist.
    let names: Vec<String> = conn
        .prepare("SELECT name FROM pragma_table_list WHERE schema='main' AND type IN ('table','virtual') AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap().query_map([], |row| row.get(0)).unwrap()
        .collect::<rusqlite::Result<_>>().unwrap();
    names
        .into_iter()
        .map(|table| {
            let values = rows(conn, &table);
            (table, values)
        })
        .collect()
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
        row.get(0)
    })
    .unwrap()
}

fn hits(conn: &Connection, table: &str, needle: &str) -> Vec<i64> {
    conn.prepare(&format!(
        "SELECT rowid FROM {table} WHERE {table} MATCH ?1 ORDER BY rowid"
    ))
    .unwrap()
    .query_map([needle], |row| row.get(0))
    .unwrap()
    .collect::<rusqlite::Result<_>>()
    .unwrap()
}

fn assert_mirrors(conn: &Connection) {
    for (base, fts, columns) in [
        (
            "files",
            "files_fts",
            "rowid,file_path,summary,content_excerpt",
        ),
        (
            "literal_index",
            "literal_fts",
            "rowid,literal_id,file_path,literal,literal_kind",
        ),
    ] {
        let query = |table| {
            let mut s = conn
                .prepare(&format!("SELECT {columns} FROM {table} ORDER BY rowid"))
                .unwrap();
            let n = s.column_count();
            s.query_map([], |r| {
                (0..n)
                    .map(|c| r.get::<_, Value>(c))
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
        };
        assert_eq!(query(base), query(fts));
    }
    assert_eq!(count(conn, "files"), count(conn, "file_paths_fts"));
    assert_eq!(count(conn, "chunks"), count(conn, "chunks_fts"));
    assert!(conn
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .query([])
        .unwrap()
        .next()
        .unwrap()
        .is_none());
}

#[test]
fn windows_preserve_all_rows_fts_search_and_cross_path_survivors() {
    for n in [0, 1, 199, 200, 255, 256, 257, 513] {
        let old = connection();
        let new = connection();
        let mut files = units(n);
        if n > 0 {
            // Within-outcome duplicate: first original base row must win.
            let path = files[0].rel_path.clone();
            files[0]
                .outcome
                .literal_index
                .push(literal("literal:0", &path, "shouldnotwin"));
        }
        if n > 256 {
            let path = files[0].rel_path.clone();
            // A later window can insert a new literal belonging to an old file.
            files[256]
                .outcome
                .literal_index
                .push(literal("cross-window", &path, "crossmarker"));
            files[256]
                .outcome
                .literal_index
                .push(literal("literal:0", &path, "shouldnotwin"));
        }
        let a = old.unchecked_transaction().unwrap();
        let b = new.unchecked_transaction().unwrap();
        assert!(can_defer_snapshot_fts(&b).unwrap());
        original(&a, &files).unwrap();
        candidate(&b, &files).unwrap();
        assert_eq!(dump(&a), dump(&b), "file count {n}");
        assert_mirrors(&b);
        for (table, needle) in [
            ("files_fts", "summarymarker"),
            ("chunks_fts", "chunkmarker"),
            ("literal_fts", "literalmarker"),
            ("literal_fts", "crossmarker"),
            ("literal_fts", "shouldnotwin"),
            ("file_paths_fts", "src"),
        ] {
            assert_eq!(
                hits(&a, table, needle),
                hits(&b, table, needle),
                "{table} count={n}"
            );
        }
        assert!(hits(&b, "literal_fts", "shouldnotwin").is_empty());
        a.commit().unwrap();
        b.commit().unwrap();
        assert_eq!(dump(&old), dump(&new));
    }
}

#[test]
fn checked_capacity_and_near_maximum_rowids_use_original_fallback() {
    assert!(has_rowid_capacity(0, 256));
    assert!(!has_rowid_capacity(i64::MAX - 1, 2));
    assert!(!has_rowid_capacity(i64::MAX - 1, 1));
    assert!(!has_rowid_capacity(i64::MAX - 1, usize::MAX));
    assert!(!has_rowid_capacity(i64::MAX, 0));
    for base in ["files", "literal_index"] {
        let conn = connection();
        let tx = conn.unchecked_transaction().unwrap();
        original(&tx, &units(1)).unwrap();
        if base == "files" {
            for table in ["files", "files_fts", "file_paths_fts"] {
                tx.execute(
                    &format!("UPDATE {table} SET rowid=?1 WHERE rowid=1"),
                    [i64::MAX - 1],
                )
                .unwrap();
            }
        } else {
            for table in ["literal_index", "literal_fts"] {
                tx.execute(
                    &format!("UPDATE {table} SET rowid=?1 WHERE rowid=1"),
                    [i64::MAX - 1],
                )
                .unwrap();
            }
        }
        let files = units(3).into_iter().skip(1).collect::<Vec<_>>();
        assert!(SnapshotFtsWindow::begin(&tx, &files).unwrap().is_none());
        candidate(&tx, &files).unwrap(); // includes SQLite's normal random-rowid path
        assert_mirrors(&tx);
        assert_eq!(count(&tx, "files"), 3);
        assert_eq!(count(&tx, "literal_index"), 3);
        tx.commit().unwrap();
    }
}

#[test]
fn autocommit_keeps_original_partial_failure_and_immediate_mirrors() {
    let old = connection();
    let new = connection();
    let mut files = units(1);
    files[0].outcome.literal_index[0].file_path = "missing.rs".into();
    assert!(!can_defer_snapshot_fts(&new).unwrap());
    assert!(original(&old, &files).is_err());
    assert!(candidate(&new, &files).is_err());
    assert_eq!(dump(&old), dump(&new));
    assert_eq!(count(&new, "files"), 1);
    assert_eq!(count(&new, "files_fts"), 1);
}

#[test]
fn noncanonical_or_temp_schema_falls_back_without_losing_mirrors() {
    for mutation in [
        "CREATE TRIGGER replace_prior AFTER INSERT ON files WHEN new.file_path='src/f0001.rs' BEGIN DELETE FROM literal_index WHERE file_path='src/f0000.rs'; DELETE FROM literal_fts WHERE file_path='src/f0000.rs'; END;",
        "CREATE TEMP TRIGGER observe_insert AFTER INSERT ON main.files BEGIN SELECT 1; END;",
        "CREATE TABLE extra_fk(file_path TEXT REFERENCES files(file_path) ON DELETE CASCADE);",
        "CREATE TEMP TABLE files(rowid INTEGER);",
    ] {
        let old=connection(); let new=connection();
        old.execute_batch(mutation).unwrap(); new.execute_batch(mutation).unwrap();
        let a=old.unchecked_transaction().unwrap(); let b=new.unchecked_transaction().unwrap();
        assert!(!can_defer_snapshot_fts(&b).unwrap(),"{mutation}");
        let x=original(&a,&units(2)); let y=candidate(&b,&units(2));
        assert_eq!(x.is_ok(),y.is_ok());
        // TEMP shadowing intentionally makes both original calls fail; compare
        // error class without querying its deliberately incompatible columns.
        if !mutation.contains("TEMP TABLE") { assert_eq!(dump(&a),dump(&b)); }
        else { assert!(x.is_err()); }
    }
}

#[test]
fn late_failure_rolls_back_flushed_windows_and_preserves_live_generation() {
    let files = units(257);
    for duplicate in [false, true] {
        let conn = connection();
        let mut bad = files.clone();
        if duplicate {
            bad[256].rel_path = bad[0].rel_path.clone();
        } else {
            bad[256].outcome.literal_index[0].file_path = "missing.rs".into();
        }
        {
            let tx = conn.unchecked_transaction().unwrap();
            assert!(candidate(&tx, &bad).is_err());
            // The first complete window really was flushed before the error.
            assert!(count(&tx, "files_fts") >= 256);
        }
        for table in TABLES {
            assert_eq!(count(&conn, table), 0, "{table}");
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.db");
    let db = IndexDb::open(&path).unwrap().0;
    db.replace_files_batch(&units(1)).unwrap();
    let generation = db.generation().unwrap();
    let before = dump(&db.read_conn().unwrap());
    let mut bad = files;
    bad[256].outcome.literal_index[0].file_path = "missing.rs".into();
    let result = db.rebuild_with_temp_db(|conn| candidate(conn, &bad));
    assert!(result.is_err());
    assert_eq!(db.generation().unwrap(), generation);
    assert_eq!(dump(&db.read_conn().unwrap()), before);
    drop(db);
    let reopened = IndexDb::open(&path).unwrap().0;
    assert_eq!(dump(&reopened.read_conn().unwrap()), before);
}

#[test]
fn small_write_stage_ab_ba_diagnostic_keeps_complete_outputs() {
    let files = units(257);
    let mut samples = Vec::new();
    let mut expected = None;
    for pair in 0..32 {
        let order = if pair % 2 == 0 {
            ["original", "candidate"]
        } else {
            ["candidate", "original"]
        };
        for method in order {
            let conn = connection();
            let start = std::time::Instant::now();
            let tx = conn.unchecked_transaction().unwrap();
            if method == "original" {
                original(&tx, &files).unwrap();
            } else {
                candidate(&tx, &files).unwrap();
            }
            tx.commit().unwrap();
            let elapsed_ns = start.elapsed().as_nanos();
            // Include every logical table (including FTS rowids/content), but
            // not physical FTS shadow pages or the independent indexed_at clock.
            let output = dump(&conn);
            if let Some(reference) = &expected {
                assert_eq!(&output, reference);
            } else {
                expected = Some(output.clone());
            }
            assert_mirrors(&conn);
            let digest = blake3::hash(format!("{output:?}").as_bytes())
                .to_hex()
                .to_string();
            samples.push(serde_json::json!({
                "pair":pair,"phase":if pair < 2 {"warmup"} else {"measured"},
                "order":if pair%2==0 {"AB"} else {"BA"},"method":method,
                "elapsed_ns":elapsed_ns,"complete_logical_output_digest":digest
            }));
        }
    }
    eprintln!(
        "P8_FTS_ROWID_AB_BA={}",
        serde_json::json!({
            "kind":"small_257_file_write_plus_commit_diagnostic",
            "not_a_formal_scale_sample":true,
            "schema_and_fixture_setup_outside_timing":true,
            "schema_guard_inside_candidate_timing":true,
            "indexed_at_only_clock_column_excluded_from_parity":true,
            "all_samples_retained":samples
        })
    );
}

#[test]
fn rowid_range_uses_primary_key_and_reduces_mirror_statement_runs() {
    let old = connection();
    let new = connection();
    let files = units(257);
    let a = old.unchecked_transaction().unwrap();
    let b = new.unchecked_transaction().unwrap();
    original(&a, &files).unwrap();
    candidate(&b, &files).unwrap();
    let details:Vec<String>=b.prepare("EXPLAIN QUERY PLAN SELECT rowid,literal_id,file_path,literal,literal_kind FROM literal_index WHERE rowid > ?1 AND rowid <= ?2 ORDER BY rowid")
        .unwrap().query_map([0,1000],|r|r.get(3)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
    assert!(details.iter().any(|s| s.contains("INTEGER PRIMARY KEY")));
    assert!(!details.iter().any(|s| s.contains("SCAN literal_index")));
    let old_files =
        "INSERT INTO files_fts(rowid,file_path,summary,content_excerpt) VALUES(?1,?2,?3,?4)";
    let old_literals="INSERT INTO literal_fts(rowid,literal_id,file_path,literal,literal_kind) VALUES(?1,?2,?3,?4,?5)";
    let new_files="INSERT INTO files_fts(rowid,file_path,summary,content_excerpt) SELECT rowid,file_path,summary,content_excerpt FROM files WHERE rowid > ?1 AND rowid <= ?2 ORDER BY rowid";
    let new_literals="INSERT INTO literal_fts(rowid,literal_id,file_path,literal,literal_kind) SELECT rowid,literal_id,file_path,literal,literal_kind FROM literal_index WHERE rowid > ?1 AND rowid <= ?2 ORDER BY rowid";
    let runs = |c: &Connection, s: &str| {
        c.prepare_cached(s)
            .unwrap()
            .get_status(StatementStatus::Run)
    };
    assert_eq!(runs(&a, old_files), 257);
    assert_eq!(runs(&a, old_literals), 257);
    assert_eq!(runs(&b, new_files), 2);
    assert_eq!(runs(&b, new_literals), 2);
    assert_eq!(dump(&a), dump(&b));
    eprintln!(
        "snapshot FTS diagnostic: original mirror runs=514 candidate=4; query_plan={details:?}"
    );
}
