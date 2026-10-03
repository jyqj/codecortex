//! Regression for the real 50k-file cold-index SQLite variable-limit failure.
use cc_db::index_db::IndexDb;

#[test]
fn large_exclusion_keeps_literal_order_and_escaped_paths_without_bind_overflow() {
    let root = tempfile::tempdir().unwrap();
    let (db, _) = IndexDb::open(&root.path().join("index.sqlite3")).unwrap();
    let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    for (id, path) in [
        ("a", "a-keep.rs"),
        ("b", "b-keep.rs"),
        ("q", "quote'keep.rs"),
        ("skip", "skip\",雪.rs"),
    ] {
        conn.execute("INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) VALUES(?1,'rust','hash',1,1,'2026-01-01')", [path]).unwrap();
        conn.execute("INSERT INTO symbols(symbol_id,file_path,name,kind,start_line,end_line,start_col,end_col,parser_tier,parser_confidence) VALUES(?1,?2,?1,'function',1,1,0,1,'tree_sitter',1.0)",rusqlite::params![id,path]).unwrap();
    }
    // No signature aggregate baseline: exercise the direct SQL fallback,
    // rather than the aggregate-keyed cache's existing in-memory exclusion.
    assert!(db.reads().seed_token().unwrap().is_none());
    let mut excluded: Vec<String> = (0..50_000).map(|i| format!("missing-{i}.rs")).collect();
    excluded.push("skip\",雪.rs".into());
    excluded.push("skip\",雪.rs".into()); // duplicate set member
    let rows = db
        .reads()
        .resolver_seed_symbols_excluding(&excluded)
        .unwrap();
    let actual: Vec<_> = rows
        .iter()
        .map(|r| (r.symbol_id.as_str(), r.file_path.as_str()))
        .collect();
    assert_eq!(
        actual,
        [
            ("a", "a-keep.rs"),
            ("b", "b-keep.rs"),
            ("q", "quote'keep.rs")
        ]
    );
    assert_eq!(
        db.reads()
            .resolver_seed_symbols_excluding(&[])
            .unwrap()
            .len(),
        4
    );
}
