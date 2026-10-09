//! Independent old-row/full-snapshot comparison using real parser/source evidence.
use cc_db::{
    direct_writer::DirectWriter,
    index_db::{FileWriteUnit, IndexDb, PrecompressedChunks},
    index_migrate::migrate_index_db,
    SnapshotWriteTxn,
};
use cc_model::{source::SourceSnapshot, CcResult, Language};
use cc_parsers::ParserRegistry;
use rusqlite::{types::Value, Connection};
use serde_json::json;

const SCHEMA: &str = include_str!("../../cc-db/src/sql/index_v1.sql");
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

fn unit(path: &str, n: usize) -> FileWriteUnit {
    let text = (0..n)
        .map(|i| format!("def f{i:03}():\n    return target()\n"))
        .collect::<String>();
    let snapshot = SourceSnapshot::new(text.as_bytes());
    let mut outcome = ParserRegistry::new()
        .parse(path, &text, Language::Python)
        .unwrap();
    outcome.document_spec = Some(cc_index::documents::delta::spec_fingerprint().into());
    outcome.documents =
        Some(cc_index::documents::delta::prepare(&snapshot, &outcome, &[]).unwrap());
    outcome.symbol_identities =
        cc_index::documents::symbol_identity::prepare(&snapshot, &outcome).unwrap();
    assert_eq!(outcome.symbols.len(), n);
    assert_eq!(outcome.symbol_identities.len(), n);
    outcome.symbol_refs = (0..n).map(|i| serde_json::from_value(json!({
        "ref_id":format!("{path}:ref:{i}"),"file_path":path,"symbol_name":"target","ref_kind":"call",
        "line":i+1,"column":2,"resolution_kind":"unresolved","resolution_confidence":0.0,
        "resolution_strategy":"unresolved","parser_tier":"tree_sitter","parser_confidence":1.0
    })).unwrap()).collect();
    outcome.call_edges = (0..n)
        .map(|i| cc_model::edge::CallEdgeRecord {
            edge_id: format!("{path}:call:{i}"),
            file_path: path.into(),
            callee_symbol: "target".into(),
            line: i as u32 + 1,
            ..Default::default()
        })
        .collect();
    outcome.route_edges = (0..n)
        .map(|i| {
            serde_json::from_value(json!({
        "edge_id":format!("{path}:route:{i}"),"file_path":path,"route_path":format!("/route/{i}"),
        "line":i+1,"start_col":0,"end_col":2,"confidence":0.5,"parser_tier":"tree_sitter"
    })).unwrap()
        })
        .collect();
    outcome.semantic_edges = (0..n).map(|i| serde_json::from_value(json!({
        "edge_id":format!("{path}:semantic:{i}"),"file_path":path,"source_symbol":format!("f{i}"),
        "target_symbol":"base","relation_kind":"inherits","line":i+1,"confidence":0.5,"parser_tier":"tree_sitter"
    })).unwrap()).collect();
    outcome.dispatch_sites = (0..n)
        .map(|i| {
            serde_json::from_value(json!({
                "site_id":format!("{path}:site:{i}"),"file_path":path,"line":i+1,"col":0,
                "site_kind":"event_on","key":"event","confidence":0.5
            }))
            .unwrap()
        })
        .collect();
    FileWriteUnit {
        rel_path: path.into(),
        language: Language::Python,
        content_hash: snapshot.identity().content_digest.clone(),
        mtime: 1.0,
        size: text.len() as u64,
        outcome,
    }
}

fn drop_indexes(conn: &Connection) {
    let names: Vec<String> = conn
        .prepare(
            "SELECT name FROM sqlite_schema WHERE type='index' AND sql IS NOT NULL ORDER BY name",
        )
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    for name in names {
        conn.execute_batch(&format!("DROP INDEX \"{}\"", name.replace('"', "\"\"")))
            .unwrap();
    }
}

fn wal(path: &std::path::Path) -> Connection {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA synchronous=OFF;")
        .unwrap();
    conn.set_prepared_statement_cache_capacity(64);
    migrate_index_db(&conn).unwrap();
    drop_indexes(&conn);
    conn
}

fn contents(conn: &Connection, units: &[FileWriteUnit], batched: bool) -> CcResult<()> {
    if batched {
        SnapshotWriteTxn::new(conn).write_file_data(units, &PrecompressedChunks::new())
    } else {
        for u in units {
            IndexDb::insert_file_data_precompressed(conn, u, None)?;
        }
        Ok(())
    }
}

fn write(conn: &Connection, units: &[FileWriteUnit], batched: bool) -> CcResult<()> {
    let tx = conn.unchecked_transaction().unwrap();
    contents(&tx, units, batched)?;
    tx.commit().unwrap();
    Ok(())
}

fn rows(conn: &Connection, table: &str) -> Vec<Vec<Value>> {
    let cols: Vec<String> = conn
        .prepare(&format!("PRAGMA table_info('{table}')"))
        .unwrap()
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
        .into_iter()
        .filter(|c| !(table == "files" && c == "indexed_at"))
        .collect();
    // Explicit rowid also verifies the application-maintained FTS alignment.
    let prefix = if table == "symbols" || table.ends_with("_fts") {
        "rowid,"
    } else {
        ""
    };
    let sql = format!(
        "SELECT {prefix}{} FROM {table} ORDER BY {}",
        cols.join(","),
        (1..=cols.len() + usize::from(!prefix.is_empty()))
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );
    let mut stmt = conn.prepare(&sql).unwrap();
    let width = stmt.column_count();
    let result = stmt
        .query_map([], |r| {
            (0..width)
                .map(|c| r.get(c))
                .collect::<rusqlite::Result<Vec<Value>>>()
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    result
}

fn equal(old: &Connection, new: &Connection) {
    for table in TABLES {
        assert_eq!(rows(old, table), rows(new, table), "table {table}");
    }
    for conn in [old, new] {
        assert_eq!(
            conn.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
        assert!(conn
            .prepare("PRAGMA foreign_key_check")
            .unwrap()
            .query([])
            .unwrap()
            .next()
            .unwrap()
            .is_none());
    }
}

fn dual_wal(units: &[FileWriteUnit]) {
    let dir = tempfile::tempdir().unwrap();
    let old = wal(&dir.path().join("old.db"));
    let new = wal(&dir.path().join("new.db"));
    write(&old, units, false).unwrap();
    write(&new, units, true).unwrap();
    equal(&old, &new);
}

#[test]
fn snapshot_leaf_tier_boundaries_preserve_all_fifteen_tables_and_fts() {
    for n in [0, 1, 7, 8, 9, 63, 64, 65, 73] {
        dual_wal(&[unit("a.py", n)]);
    }
}

fn symbol_fts_matches(conn: &Connection, token: &str) -> Vec<(i64, String, String, String)> {
    let mut stmt = conn.prepare("SELECT rowid,name,symbol_id,file_path FROM symbols_fts WHERE symbols_fts MATCH ?1 ORDER BY rowid").unwrap();
    let result = stmt
        .query_map([token], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    result
}

fn symbol_fts_base_rowids(conn: &Connection) -> Vec<(i64, String, String)> {
    let mut stmt = conn.prepare("SELECT s.rowid,s.symbol_id,s.file_path,f.rowid,f.symbol_id,f.file_path FROM symbols s LEFT JOIN symbols_fts f ON f.rowid=s.rowid ORDER BY s.rowid").unwrap();
    let result = stmt
        .query_map([], |row| {
            let base = (
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            );
            assert_eq!(row.get::<_, Option<i64>>(3)?, Some(base.0));
            assert_eq!(row.get::<_, Option<String>>(4)?.as_ref(), Some(&base.1));
            assert_eq!(row.get::<_, Option<String>>(5)?.as_ref(), Some(&base.2));
            Ok(base)
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    result
}

#[test]
fn snapshot_leaf_symbol_id_and_uid_conflicts_keep_sql_survivors_and_fts() {
    let mut u = unit("a.py", 65);
    for (target, source, uid) in [
        (7, 0, false),
        (8, 1, true),
        (63, 2, false),
        (63, 3, true),
        (64, 4, true),
    ] {
        if uid {
            u.outcome.symbols[target].symbol_uid = u.outcome.symbols[source].symbol_uid.clone();
        } else {
            u.outcome.symbols[target].symbol_id = u.outcome.symbols[source].symbol_id.clone();
        }
    }
    for id in &mut u.outcome.symbol_identities {
        let symbol = u
            .outcome
            .symbols
            .iter()
            .find(|s| s.name == id.name)
            .unwrap();
        id.symbol_id = symbol.symbol_id.clone();
        id.symbol_uid = symbol.symbol_uid.clone().unwrap();
    }
    let dir = tempfile::tempdir().unwrap();
    let old = wal(&dir.path().join("old.db"));
    let new = wal(&dir.path().join("new.db"));
    write(&old, std::slice::from_ref(&u), false).unwrap();
    write(&new, &[u], true).unwrap();
    equal(&old, &new);
    assert_eq!(symbol_fts_base_rowids(&old), symbol_fts_base_rowids(&new));
    // Trigram MATCH exercises actual postings after both UNIQUE conflicts,
    // not only the FTS content table. Every named replacement must be visible.
    for token in ["f007", "f008", "f063", "f064"] {
        let before = symbol_fts_matches(&old, token);
        let after = symbol_fts_matches(&new, token);
        assert!(
            before.iter().any(|row| row.1 == token),
            "missing original posting for {token}"
        );
        assert_eq!(before, after, "FTS MATCH {token}");
    }
}

#[test]
fn snapshot_leaf_never_delays_identity_checks_past_the_next_file() {
    let a = unit("a.py", 8);
    let mut b = unit("b.py", 8);
    b.outcome.symbols[0].symbol_id = a.outcome.symbols[0].symbol_id.clone();
    b.outcome.symbol_identities[0].symbol_id = b.outcome.symbols[0].symbol_id.clone();
    dual_wal(&[a, b]);
}

#[test]
fn snapshot_leaf_byte_windows_and_oversize_single_rows_preserve_values() {
    let mut u = unit("a.py", 73);
    for (i, s) in u.outcome.symbols.iter_mut().enumerate() {
        s.doc = Some("中文".repeat(if i == 8 { 12000 } else { 1500 }));
    }
    for (i, r) in u.outcome.symbol_refs.iter_mut().enumerate() {
        r.symbol_name = "r".repeat(if i == 64 { 70000 } else { 9000 });
    }
    for r in &mut u.outcome.route_edges {
        r.route_path = "/r".repeat(5000);
    }
    for e in &mut u.outcome.call_edges {
        e.callee_symbol = "c".repeat(9000);
    }
    for e in &mut u.outcome.semantic_edges {
        e.target_symbol = "s".repeat(9000);
    }
    for s in &mut u.outcome.dispatch_sites {
        s.key = "d".repeat(9000);
    }
    dual_wal(&[u]);
}

#[test]
fn snapshot_leaf_duplicate_paths_future_fk_and_late_identity_error_roll_back_wal() {
    let good = unit("a.py", 65);
    let mut future = unit("first.py", 65);
    future.outcome.symbols[7].file_path = "later.py".into();
    let mut bad = unit("bad.py", 65);
    bad.outcome.symbol_identities[64].qname = "wrong.owner".into();
    for units in [
        vec![good.clone(), good.clone()],
        vec![future, unit("later.py", 1)],
        vec![good, bad],
    ] {
        let dir = tempfile::tempdir().unwrap();
        let old = wal(&dir.path().join("old.db"));
        let new = wal(&dir.path().join("new.db"));
        assert!(write(&old, &units, false).is_err());
        assert!(write(&new, &units, true).is_err());
        equal(&old, &new);
        assert!(rows(&new, "files").is_empty());
        // A failed cached batch cannot contaminate the next transaction.
        let retry = unit("retry.py", 9);
        write(&old, std::slice::from_ref(&retry), false).unwrap();
        write(&new, &[retry], true).unwrap();
        equal(&old, &new);
    }
}

#[test]
fn snapshot_leaf_direct_writer_preserves_original_adapter_pragmas_and_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let units = [unit("a.py", 65), unit("b.py", 9)];
    for (name, batch) in [("old.db", false), ("new.db", true)] {
        DirectWriter::write_db(&dir.path().join(name), SCHEMA, |tx| {
            assert_eq!(
                tx.query_row("PRAGMA journal_mode", [], |r| r.get::<_, String>(0))
                    .unwrap(),
                "off"
            );
            // No new FK policy is installed by SnapshotWriteTxn.
            let foreign_keys = tx
                .query_row("PRAGMA foreign_keys", [], |r| r.get::<_, i64>(0))
                .unwrap();
            contents(tx, &units, batch)?;
            assert_eq!(
                tx.query_row("PRAGMA foreign_keys", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                foreign_keys
            );
            Ok(())
        })
        .unwrap();
    }
    let old = Connection::open(dir.path().join("old.db")).unwrap();
    let new = Connection::open(dir.path().join("new.db")).unwrap();
    equal(&old, &new);
    // This success comparison does not claim journal-OFF physical rollback.
}
