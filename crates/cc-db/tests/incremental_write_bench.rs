//! Focused micro-benchmark of the incremental batch-write hot path.
//!
//! `#[ignore]`d — run explicitly (release for meaningful numbers):
//!
//! ```sh
//! cargo test -p cc-db --release --test incremental_write_bench -- --ignored --nocapture
//! ```
//!
//! Populates a 10k-file index at synthetic-bench densities, then measures
//! `write_incremental_batch` on a 5% batch and prints a per-statement-class
//! breakdown (probe transaction mirroring the production SQL, rolled back).

use cc_db::index_db::{compress_chunk_text, FileWriteUnit, IndexDb, PrecompressedChunks};
use cc_model::edge::{CallEdgeRecord, ImportRecord, SemanticEdgeRecord, SemanticRelation};
use cc_model::{
    ChunkRecord, Language, LiteralRecord, ParseOutcome, ParserTier, SymbolKind, SymbolRecord,
    SymbolRefRecord,
};
use rusqlite::Connection;
use std::time::{Duration, Instant};

const FILE_COUNT: usize = 10_000;
const BATCH_STRIDE: usize = 20; // 5% batch, mirrors scale_bench

fn symbol(rel_path: &str, file_idx: usize, sym_idx: usize) -> SymbolRecord {
    let name = format!("fn_{:05}_{}", file_idx, sym_idx);
    SymbolRecord {
        symbol_id: format!("sym:{}:{}", rel_path, sym_idx),
        file_path: rel_path.to_string(),
        name: name.clone(),
        kind: SymbolKind::Function,
        container: None,
        start_line: (sym_idx * 10 + 1) as u32,
        end_line: (sym_idx * 10 + 8) as u32,
        start_col: 0,
        end_col: 1,
        signature: Some(format!("pub fn {}(value: i64) -> i64", name)),
        doc: None,
        parser_tier: ParserTier::TreeSitter,
        parser_confidence: 1.0,
        qname: Some(name.clone()),
        parent_symbol_id: None,
        scope_id: None,
        export_name: Some(name),
        is_default_export: false,
        symbol_uid: Some(format!("uid:{}:{}", rel_path, sym_idx)),
        framework_role: None,
        receiver_type: None,
        param_types: Some("i64".to_string()),
        return_type: Some("i64".to_string()),
        param_count: Some(1),
        base_types: None,
        implements: None,
    }
}

fn chunk(rel_path: &str, file_idx: usize, chunk_idx: u32) -> ChunkRecord {
    let body = format!(
        "pub fn fn_{:05}_{}(value: i64) -> i64 {{\n    let label = \"module marker {} {}\";\n    value.wrapping_mul(2654435761) % 4093 + label.len() as i64\n}}\n",
        file_idx, chunk_idx, file_idx, chunk_idx
    )
    .repeat(3);
    ChunkRecord {
        chunk_id: format!("chunk:{}:{}", rel_path, chunk_idx),
        file_path: rel_path.to_string(),
        language: Language::Rust,
        chunk_index: chunk_idx,
        start_line: chunk_idx * 20 + 1,
        end_line: chunk_idx * 20 + 18,
        breadcrumb: format!("module_{:05}", file_idx),
        text: body,
        symbol_name: Some(format!("fn_{:05}_{}", file_idx, chunk_idx)),
        symbol_kind: Some(SymbolKind::Function),
        token_estimate: 64,
        parser_tier: ParserTier::TreeSitter,
        parser_confidence: 1.0,
    }
}

fn make_unit(file_idx: usize) -> FileWriteUnit {
    let rel_path = format!("src/module_{:03}/file_{:05}.rs", file_idx % 200, file_idx);
    let mut outcome = ParseOutcome {
        summary: format!("synthetic module {} dispatch payload registry", file_idx),
        parser_tier: ParserTier::TreeSitter,
        parser_confidence: 1.0,
        ..ParseOutcome::default()
    };
    outcome.chunks = (0..2u32).map(|c| chunk(&rel_path, file_idx, c)).collect();
    outcome.symbols = (0..5).map(|s| symbol(&rel_path, file_idx, s)).collect();
    outcome.imports = (0..2)
        .map(|i| ImportRecord {
            file_path: rel_path.clone(),
            import_string: format!("crate::module_{:03}::dep_{}", (file_idx + i) % 200, i),
            resolved_path: Some(format!("src/module_{:03}/file_{:05}.rs", i, file_idx % 977)),
            imported_name: Some(format!("dep_{}", i)),
            alias: None,
            is_namespace: false,
            is_default: false,
            is_reexport: false,
        })
        .collect();
    outcome.symbol_refs = (0..6)
        .map(|r| SymbolRefRecord {
            ref_id: format!("ref:{}:{}", rel_path, r),
            file_path: rel_path.clone(),
            symbol_name: format!("fn_{:05}_{}", (file_idx + r) % FILE_COUNT, r % 5),
            container: None,
            ref_kind: "call".to_string(),
            line: (r * 7 + 2) as u32,
            column: 4,
            target_symbol_id: None,
            target_file_path: None,
            target_symbol_uid: Some(format!("uid:{}:{}", rel_path, r % 5)),
            ref_name: None,
            scope_id: None,
            resolution_kind: Default::default(),
            resolution_confidence: 0.9,
            resolution_strategy: "exact".to_string(),
            ref_end_line: None,
            ref_end_col: None,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 1.0,
        })
        .collect();
    outcome.call_edges = (0..5)
        .map(|e| CallEdgeRecord {
            edge_id: format!("ce:{}:{}", rel_path, e),
            file_path: rel_path.clone(),
            caller_symbol: Some(format!("fn_{:05}_{}", file_idx, e % 5)),
            callee_symbol: format!("fn_{:05}_{}", (file_idx + e + 1) % FILE_COUNT, e % 5),
            line: (e * 9 + 3) as u32,
            caller_symbol_uid: Some(format!("uid:{}:{}", rel_path, e % 5)),
            callee_symbol_uid: Some(format!("uid:x:{}", (file_idx + e + 1) % FILE_COUNT)),
            resolution_confidence: 0.9,
            resolution_strategy: "exact".to_string(),
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 1.0,
            ..CallEdgeRecord::default()
        })
        .collect();
    outcome.literal_index = vec![LiteralRecord {
        literal_id: format!("lit:{}", rel_path),
        file_path: rel_path.clone(),
        literal: format!("module marker {}", file_idx),
        literal_kind: "string".to_string(),
        line: 2,
        container: None,
        confidence: 0.8,
        enclosing_symbol_uid: Some(format!("uid:{}:0", rel_path)),
        key_path: None,
    }];
    outcome.semantic_edges = vec![SemanticEdgeRecord {
        edge_id: format!("se:{}", rel_path),
        file_path: rel_path.clone(),
        source_symbol: format!("fn_{:05}_0", file_idx),
        source_symbol_uid: Some(format!("uid:{}:0", rel_path)),
        target_symbol: format!("fn_{:05}_1", file_idx),
        target_symbol_uid: Some(format!("uid:{}:1", rel_path)),
        relation_kind: SemanticRelation::UsesType,
        line: 1,
        confidence: 0.9,
        parser_tier: ParserTier::TreeSitter,
    }];
    FileWriteUnit {
        rel_path,
        language: Language::Rust,
        content_hash: format!("hash-{}", file_idx),
        mtime: 1.0,
        size: 1024,
        outcome,
    }
}

struct Buckets(Vec<(&'static str, Duration)>);

impl Buckets {
    fn new() -> Self {
        Buckets(Vec::new())
    }
    fn add(&mut self, name: &'static str, d: Duration) {
        if let Some(entry) = self.0.iter_mut().find(|(n, _)| *n == name) {
            entry.1 += d;
        } else {
            self.0.push((name, d));
        }
    }
    fn print(&self, label: &str) {
        let total: Duration = self.0.iter().map(|(_, d)| *d).sum();
        eprintln!("── {} (probe total {:?}) ──", label, total);
        let mut sorted = self.0.clone();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        for (name, d) in sorted {
            eprintln!("  {:<28} {:>10.1?}", name, d);
        }
    }
}

impl Clone for Buckets {
    fn clone(&self) -> Self {
        Buckets(self.0.clone())
    }
}

impl std::ops::Deref for Buckets {
    type Target = Vec<(&'static str, Duration)>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Mirror of the batched FTS delete in `write_incremental_batch`
/// (rowid-aligned delete through the base table's file_path index, chunked
/// `IN (...)`), timed per table.
fn probe_fts_batch(conn: &Connection, batch: &[FileWriteUnit], buckets: &mut Buckets) {
    let paths: Vec<&str> = batch.iter().map(|u| u.rel_path.as_str()).collect();
    for chunk in paths.chunks(cc_db::sql_util::IN_BATCH_SIZE) {
        let placeholders = cc_db::sql_util::sql_in_placeholders(chunk.len());
        for (fts_table, base_table) in &[
            ("chunks_fts", "chunks"),
            ("files_fts", "files"),
            ("literal_fts", "literal_index"),
        ] {
            let t = Instant::now();
            conn.execute(
                &format!(
                    "DELETE FROM {} WHERE rowid IN \
                     (SELECT rowid FROM {} WHERE file_path IN ({}))",
                    fts_table, base_table, placeholders
                ),
                rusqlite::params_from_iter(chunk.iter()),
            )
            .unwrap();
            match *fts_table {
                "chunks_fts" => buckets.add("del chunks_fts (batched)", t.elapsed()),
                "files_fts" => buckets.add("del files_fts (batched)", t.elapsed()),
                _ => buckets.add("del literal_fts (batched)", t.elapsed()),
            }
        }
    }
}

/// Mirror of `delete_files_data_base_keep_test_edges_batch` (replace-in-place
/// keeps the path-derived test_edges; only removals cascade): one chunked
/// `IN (...)` DELETE per table for the whole batch, timed per table class.
/// Runs inside a probe transaction the caller rolls back, so it never
/// perturbs the database under measurement.
fn probe_delete_batch(conn: &Connection, batch: &[FileWriteUnit], buckets: &mut Buckets) {
    let paths: Vec<&str> = batch.iter().map(|u| u.rel_path.as_str()).collect();
    for chunk in paths.chunks(cc_db::sql_util::IN_BATCH_SIZE) {
        let placeholders = cc_db::sql_util::sql_in_placeholders(chunk.len());
        let t = Instant::now();
        conn.execute(
            &format!(
                "DELETE FROM frameworks WHERE scope='file' AND scope_id IN ({})",
                placeholders
            ),
            rusqlite::params_from_iter(chunk.iter()),
        )
        .unwrap();
        buckets.add("del frameworks (batched)", t.elapsed());
        let t = Instant::now();
        for table in &[
            "routes",
            "data_flow_edges",
            "http_call_edges",
            "semantic_edges",
            "dispatch_sites",
        ] {
            conn.execute(
                &format!("DELETE FROM {} WHERE file_path IN ({})", table, placeholders),
                rusqlite::params_from_iter(chunk.iter()),
            )
            .unwrap();
        }
        for column in &["file_a", "file_b"] {
            conn.execute(
                &format!(
                    "DELETE FROM co_change_edges WHERE {} IN ({})",
                    column, placeholders
                ),
                rusqlite::params_from_iter(chunk.iter()),
            )
            .unwrap();
        }
        buckets.add("del edge tables (batched)", t.elapsed());
        let t = Instant::now();
        conn.execute(
            &format!("DELETE FROM files WHERE file_path IN ({})", placeholders),
            rusqlite::params_from_iter(chunk.iter()),
        )
        .unwrap();
        buckets.add("del files (cascade+triggers)", t.elapsed());
    }
}

/// Mirror of the batch insert path: per-file base rows + chunks_fts via the
/// production deferred-FTS helper, then the batched files_fts / literal_fts
/// `INSERT .. SELECT` mirrors.
fn probe_insert_batch(conn: &Connection, batch: &[FileWriteUnit], buckets: &mut Buckets) {
    for unit in batch {
        let blobs: Vec<Option<Vec<u8>>> = unit
            .outcome
            .chunks
            .iter()
            .map(|c| compress_chunk_text(&c.text))
            .collect();
        let t = Instant::now();
        IndexDb::insert_file_data_deferred_fts(conn, unit, Some(&blobs)).unwrap();
        buckets.add("insert base rows (per file)", t.elapsed());
    }
    let paths: Vec<&str> = batch.iter().map(|u| u.rel_path.as_str()).collect();
    let t = Instant::now();
    IndexDb::insert_files_literal_fts_batch(conn, &paths).unwrap();
    buckets.add("insert files/literal fts (batched)", t.elapsed());
}

#[test]
#[ignore = "write-path micro-benchmark; run explicitly with --release"]
fn bench_incremental_batch_write_10k() {
    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("index.sqlite3");
    let db = IndexDb::open(&db_path).unwrap().0;

    let units: Vec<FileWriteUnit> = (0..FILE_COUNT).map(make_unit).collect();

    // Cold population through the full-rebuild path (no per-file deletes).
    let t = Instant::now();
    db.admin()
        .rebuild_with_temp_db(|conn| {
            for unit in &units {
                IndexDb::insert_file_data(conn, unit)?;
            }
            Ok(())
        })
        .unwrap();
    eprintln!("populate {} files: {:?}", FILE_COUNT, t.elapsed());

    let batch: Vec<FileWriteUnit> = units.iter().step_by(BATCH_STRIDE).cloned().collect();
    let precompressed: PrecompressedChunks = batch
        .iter()
        .map(|u| {
            (
                u.rel_path.clone(),
                u.outcome
                    .chunks
                    .iter()
                    .map(|c| compress_chunk_text(&c.text))
                    .collect(),
            )
        })
        .collect();

    // Real write path, twice (first run includes page-cache warmup).
    for round in 1..=2 {
        let t = Instant::now();
        db.writes()
            .write_incremental_batch(&[], &batch, &[], &[], &[], &precompressed)
            .unwrap();
        eprintln!(
            "write_incremental_batch round {} ({} files): {:?}",
            round,
            batch.len(),
            t.elapsed()
        );
    }

    // Single-file write (the latency-sensitive incremental path).
    let single = std::slice::from_ref(&batch[0]);
    for round in 1..=3 {
        let t = Instant::now();
        db.writes()
            .write_incremental_batch(&[], single, &[], &[], &[], &precompressed)
            .unwrap();
        eprintln!(
            "write_incremental_batch single-file round {}: {:?}",
            round,
            t.elapsed()
        );
    }

    // Per-statement-class breakdown in a rolled-back probe transaction.
    let probe = Connection::open(&db_path).unwrap();
    probe
        .execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;")
        .unwrap();
    probe.set_prepared_statement_cache_capacity(64);
    probe.execute_batch("BEGIN IMMEDIATE").unwrap();
    let mut buckets = Buckets::new();
    let t = Instant::now();
    probe_fts_batch(&probe, &batch, &mut buckets);
    probe_delete_batch(&probe, &batch, &mut buckets);
    probe_insert_batch(&probe, &batch, &mut buckets);
    let probe_elapsed = t.elapsed();
    probe.execute_batch("ROLLBACK").unwrap();
    buckets.print(&format!(
        "per-statement breakdown, {} files, wall {:?}",
        batch.len(),
        probe_elapsed
    ));
}
