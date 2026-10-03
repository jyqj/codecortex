//! Execute real resolver reads against independent literal-set oracles.
//! Same file can run on fixed parent source to reproduce the bind-limit error.
use cc_db::index_db::IndexDb;
use std::{collections::HashSet, time::Instant};

fn fixture(paths: &[String]) -> (tempfile::TempDir, IndexDb, rusqlite::Connection) {
    let root = tempfile::tempdir().unwrap();
    let (db, _) = IndexDb::open(&root.path().join("index.sqlite3")).unwrap();
    let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON; BEGIN;")
        .unwrap();
    for (i, p) in paths.iter().enumerate().rev() {
        conn.execute("INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) VALUES(?1,'rust','synthetic',1,1,'2026-10-03')", [p]).unwrap();
        for line in [9, 2] {
            conn.execute("INSERT INTO symbols(symbol_id,file_path,name,kind,start_line,end_line,container,qname,param_count,base_types,implements) VALUES(?1,?2,'synthetic','function',?3,?3,NULL,NULL,NULL,NULL,NULL)", rusqlite::params![format!("s-{i}-{line}"), p, line]).unwrap();
        }
    }
    conn.execute_batch("COMMIT;").unwrap();
    assert!(db.reads().seed_token().unwrap().is_none());
    (root, db, conn)
}
fn paths() -> Vec<String> {
    [
        "",
        "a.rs",
        "A.rs",
        "a.rs/child",
        "aXrs",
        "a%rs",
        "a_rs",
        "./a.rs",
        "x/../a.rs",
        "x/a.rs",
        "x\\a.rs",
        "quote'\".rs",
        "'); DROP TABLE symbols;--",
        "[null,1,true]",
        "null",
        "雪/😀.rs",
        "é.rs",
        "e\u{301}.rs",
        "tab\tline\n.rs",
        "trailing .rs ",
        "back\\slash.rs",
        "NULL",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}
fn assert_oracle(db: &IndexDb, paths: &[String], excluded: &[String]) -> usize {
    assert!(db.reads().seed_token().unwrap().is_none());
    let excluded_set: HashSet<&str> = excluded.iter().map(String::as_str).collect();
    let mut expected: Vec<_> = paths
        .iter()
        .filter(|p| !excluded_set.contains(p.as_str()))
        .flat_map(|p| [2, 9].into_iter().map(move |line| (p.clone(), line)))
        .collect();
    expected.sort();
    let actual = db
        .reads()
        .resolver_seed_symbols_excluding(excluded)
        .unwrap();
    let observed: Vec<_> = actual
        .iter()
        .map(|r| (r.file_path.clone(), r.start_line))
        .collect();
    assert_eq!(observed, expected);
    for r in &actual {
        assert!(r.container.is_none() && r.qname.is_none() && r.param_count.is_none());
        assert!(r.base_types.is_none() && r.implements.is_none());
    }
    actual.len()
}
#[test]
fn literal_set_semantics_null_safety_and_order() {
    let paths = paths();
    let (_root, db, conn) = fixture(&paths);
    let mut cases = vec![
        vec![],
        paths.clone(),
        vec!["not-present.rs".into()],
        vec!["NULL".into(), "null".into()],
        vec!["a".into(), "x/".into(), "*.rs".into(), "a%".into()],
    ];
    for p in &paths {
        cases.push(vec![p.clone(), p.clone()]);
    }
    for case in &cases {
        assert_oracle(&db, &paths, case);
    }
    // API accepts Vec<String>, not nullable JSON values. Schema disallows NULL paths.
    let err = conn.execute("INSERT INTO symbols(symbol_id,file_path,name,kind,start_line,end_line) VALUES('null-path',NULL,'n','function',1,1)", []).unwrap_err();
    assert_eq!(
        err.sqlite_error_code(),
        Some(rusqlite::ErrorCode::ConstraintViolation)
    );
    assert_eq!(assert_oracle(&db, &paths, &[]), paths.len() * 2);
    println!(
        "REVIEW {}",
        serde_json::json!({"case":"literal-matrix","oracle_cases":cases.len(),"rows":paths.len()*2,"NULL_path_rejected":true,"sql_injection_no_mutation":true})
    );
}
#[test]
fn actual_legacy_overflow_or_fixed_large_inputs() {
    let paths = paths();
    let (_root, db, conn) = fixture(&paths);
    let mut excluded: Vec<_> = (0..50_000).map(|i| format!("absent-{i:06}.rs")).collect();
    excluded.push("quote'\".rs".into());
    excluded.push("雪/😀.rs".into());
    let before = Instant::now();
    if std::env::var_os("RESOLVER_REVIEW_LEGACY").is_some() {
        let err = db
            .reads()
            .resolver_seed_symbols_excluding(&excluded)
            .unwrap_err();
        assert!(err.to_string().contains("too many SQL variables"), "{err}");
        println!(
            "REVIEW {}",
            serde_json::json!({"case":"legacy-actual-API","excluded_count":excluded.len(),"error":"too many SQL variables","elapsed_ms":before.elapsed().as_millis()})
        );
    } else {
        for n in [50_000, 200_000] {
            let mut exclusion: Vec<_> = (0..n).map(|i| format!("absent-{i:06}.rs")).collect();
            exclusion.extend(["quote'\".rs".into(), "雪/😀.rs".into(), "雪/😀.rs".into()]);
            let bytes = serde_json::to_vec(&exclusion).unwrap().len();
            let start = Instant::now();
            let returned = assert_oracle(&db, &paths, &exclusion);
            println!(
                "REVIEW {}",
                serde_json::json!({"case":"fixed-large-API","excluded_count":exclusion.len(),"json_bytes":bytes,"returned_rows":returned,"elapsed_ms":start.elapsed().as_millis()})
            );
        }
        // Input byte size is also a resource dimension, independently of element count.
        let long = "x".repeat(4 * 1024 * 1024);
        let start = Instant::now();
        assert_oracle(&db, &paths, &[long]);
        println!(
            "REVIEW {}",
            serde_json::json!({"case":"single-4MiB-string","elapsed_ms":start.elapsed().as_millis()})
        );
    }
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        (paths.len() * 2) as i64
    );
    assert_eq!(
        conn.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
}
#[test]
fn embedded_nul_api_observation() {
    // NUL is not an OS pathname, but is representable by the current Rust/DB APIs.
    let paths = vec!["prefix".into(), "prefix\0suffix".into()];
    let (_root, db, _conn) = fixture(&paths);
    let got = db
        .reads()
        .resolver_seed_symbols_excluding(&["prefix\0suffix".into()])
        .unwrap();
    let observed: Vec<_> = got.iter().map(|r| r.file_path.clone()).collect();
    let exact = observed == vec!["prefix".to_owned(), "prefix".to_owned()];
    println!(
        "REVIEW {}",
        serde_json::json!({"case":"embedded-NUL","exact_set_semantics":exact,"returned_paths":observed,"OS_path_supported":false})
    );
    // Observation intentionally reports rather than widening a real-filesystem contract.
}

#[test]
fn fifty_thousand_file_database_real_queries() {
    if std::env::var_os("RESOLVER_REVIEW_LEGACY").is_some() {
        return; // legacy reproduction is the separate overflow API test
    }
    let paths: Vec<_> = (0..50_000)
        .map(|i| format!("synthetic/{i:06}.rs"))
        .collect();
    let setup = Instant::now();
    let (_root, db, conn) = fixture(&paths);
    let setup_ms = setup.elapsed().as_millis();
    let start = Instant::now();
    assert_eq!(assert_oracle(&db, &paths, &paths), 0);
    let all_ms = start.elapsed().as_millis();
    let half: Vec<_> = paths.iter().step_by(2).cloned().collect();
    let start = Instant::now();
    assert_eq!(assert_oracle(&db, &paths, &half), 50_000);
    let half_ms = start.elapsed().as_millis();
    let start = Instant::now();
    assert_eq!(assert_oracle(&db, &paths, &[]), 100_000);
    println!(
        "REVIEW {}",
        serde_json::json!({"case":"50k-file-database","files":50_000,"symbols":100_000,"baseline_absent":true,"setup_ms":setup_ms,"all_excluded_ms":all_ms,"half_excluded_ms":half_ms,"empty_exclusion_ms":start.elapsed().as_millis()})
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
