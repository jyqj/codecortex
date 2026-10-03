//! Run only in an owned checkout at the pinned v24 base.
#[test]
fn independent_generate_v24_fixture() {
    let root = std::path::PathBuf::from(std::env::var("QNAME_DB_V24_ROOT").unwrap());
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join(".codecortex.json"),
        r#"{"auto_index":{"enabled":false}}"#,
    )
    .unwrap();
    std::fs::write(root.join("review.py"),"# fresh independent fixture 雪\nclass Harbor:\n    def beacon(self):\n        return '灯'\n\nclass Ridge:\n    def beacon(self):\n        return '山'\n").unwrap();
    let mut index = cc_server::engine::CodeIndex::new(Some(&root)).unwrap();
    assert_eq!(index.build_index(true).unwrap().files_parsed, 1);
    let db = index.index_db().unwrap();
    let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        24
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='chunk_symbol_identity'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    let output = std::env::var("QNAME_DB_V24_OUT").unwrap();
    // SQLite backup by VACUUM INTO; never copy a live WAL-dependent main file.
    conn.execute("VACUUM INTO ?1", [output]).unwrap();
    println!("base v24 independent cache generated from real build");
}
