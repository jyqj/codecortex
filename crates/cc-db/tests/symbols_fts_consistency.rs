//! Symbol replacement must preserve SQLite conflict semantics and FTS rowids.
use cc_db::index_db::{FileWriteUnit, IndexDb, PrecompressedChunks};
use cc_model::{parse::ParseOutcome, symbol::SymbolRecord, Language};
use rusqlite::{types::Value, Connection};
use tempfile::TempDir;

const SCHEMA: &str = include_str!("../src/sql/index_v1.sql");
const INSERT: &str = "INSERT OR REPLACE INTO symbols(symbol_id,file_path,name,kind,start_line,end_line,symbol_uid) VALUES(?1,'a.cpp',?2,'function',1,1,?3)";

fn connection() -> Connection {
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch(SCHEMA).unwrap();
    c.execute_batch("INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) VALUES('a.cpp','cpp','hash',0,0,'now'),('b.cpp','cpp','hash',0,0,'now')").unwrap();
    assert_eq!(
        c.pragma_query_value(None, "recursive_triggers", |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    c
}

fn rows(c: &Connection, sql: &str) -> Vec<Vec<Value>> {
    let mut q = c.prepare(sql).unwrap();
    let n = q.column_count();
    q.query_map([], |r| (0..n).map(|i| r.get(i)).collect())
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn aligned(c: &Connection) {
    assert_eq!(
        rows(
            c,
            "SELECT rowid,name,symbol_id,file_path FROM symbols ORDER BY rowid"
        ),
        rows(
            c,
            "SELECT rowid,name,symbol_id,file_path FROM symbols_fts ORDER BY rowid"
        )
    );
    // The same harness runs against accepted schema 27 for a genuine red
    // FTS regression; the new table's presence has its own assertion below.
    if c.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE name='symbols_fts_keys'",
        [],
        |r| r.get::<_, i64>(0),
    )
    .unwrap()
        == 1
    {
        assert_eq!(
            rows(c, "SELECT rowid,symbol_id,symbol_uid FROM symbols ORDER BY rowid"),
            rows(c, "SELECT symbol_rowid,symbol_id,symbol_uid FROM symbols_fts_keys ORDER BY symbol_rowid")
        );
    }
}

#[test]
fn mirror_keys_preserve_nullable_primary_key_and_uid_constraints() {
    let c = connection();
    c.execute_batch("INSERT INTO symbols(symbol_id,file_path,name,kind,start_line,end_line,symbol_uid) VALUES(NULL,'a.cpp','NullAlpha','function',1,1,NULL),(NULL,'a.cpp','NullBeta','function',1,1,NULL)").unwrap();
    assert_eq!(
        rows(
            &c,
            "SELECT rowid,symbol_id,symbol_uid FROM symbols ORDER BY rowid"
        ),
        rows(
            &c,
            "SELECT symbol_rowid,symbol_id,symbol_uid FROM symbols_fts_keys ORDER BY symbol_rowid"
        )
    );
    aligned(&c);
}

#[test]
fn replace_cleans_id_uid_and_two_distinct_victims() {
    let c = connection();
    for (id, name, uid) in [
        ("a", "FirstAlpha", Some("u1")),
        ("b", "SecondBeta", Some("u2")),
        ("a", "ThirdGamma", Some("u2")),
        ("null1", "NullFirst", None),
        ("null2", "NullSecond", None),
        ("a", "LastDelta", Some("u3")),
        ("c", "FinalEpsilon", Some("u3")),
    ] {
        c.execute(INSERT, rusqlite::params![id, name, uid]).unwrap();
        aligned(&c);
    }
    assert_eq!(
        rows(&c, "SELECT symbol_id FROM symbols ORDER BY symbol_id"),
        vec![
            vec![Value::Text("c".into())],
            vec![Value::Text("null1".into())],
            vec![Value::Text("null2".into())]
        ]
    );
    for term in ["FirstAlpha", "SecondBeta", "ThirdGamma", "LastDelta"] {
        assert!(rows(
            &c,
            &format!("SELECT rowid FROM symbols_fts WHERE symbols_fts MATCH '{term}'")
        )
        .is_empty());
    }
}

#[test]
fn insert_abort_ignore_fail_and_transaction_rollback_preserve_mirrors() {
    for (policy, count) in [("ABORT", 1), ("IGNORE", 2), ("FAIL", 2)] {
        let c = connection();
        c.execute(INSERT, rusqlite::params!["seed", "SeedName", "uid"])
            .unwrap();
        let result = c.execute_batch(&format!("INSERT OR {policy} INTO symbols(symbol_id,file_path,name,kind,start_line,end_line,symbol_uid) VALUES('good','a.cpp','GoodName','function',1,1,'other'),('collision','a.cpp','BadName','function',1,1,'uid')"));
        assert_eq!(result.is_ok(), policy == "IGNORE");
        assert_eq!(rows(&c, "SELECT symbol_id FROM symbols").len(), count);
        aligned(&c);
        let before = rows(&c, "SELECT rowid,* FROM symbols ORDER BY rowid");
        c.execute_batch("BEGIN; INSERT OR REPLACE INTO symbols(symbol_id,file_path,name,kind,start_line,end_line,symbol_uid) VALUES('replaced','a.cpp','ReplaceName','function',1,1,'uid'); ROLLBACK;").unwrap();
        assert_eq!(
            rows(&c, "SELECT rowid,* FROM symbols ORDER BY rowid"),
            before
        );
        aligned(&c);
    }
}

#[test]
fn updates_and_delete_keep_keys_and_all_mirrored_columns_aligned() {
    let c = connection();
    c.execute(INSERT, rusqlite::params!["a", "AlphaName", "u1"])
        .unwrap();
    c.execute(INSERT, rusqlite::params!["b", "BetaName", "u2"])
        .unwrap();
    c.execute(INSERT, rusqlite::params!["c", "GammaName", "u3"])
        .unwrap();
    c.execute_batch("UPDATE OR REPLACE symbols SET symbol_id='b',symbol_uid='u3',name='UpdatedName',file_path='b.cpp',oid=100 WHERE symbol_id='a'").unwrap();
    aligned(&c);
    assert_eq!(rows(&c, "SELECT symbol_id FROM symbols").len(), 1);
    c.execute_batch("UPDATE symbols SET _rowid_=200,symbol_uid=NULL")
        .unwrap();
    aligned(&c);
    c.execute_batch("DELETE FROM symbols").unwrap();
    aligned(&c);
}

fn symbol(id: &str, uid: Option<&str>, name: &str) -> SymbolRecord {
    serde_json::from_value(serde_json::json!({
        "symbol_id":id,"file_path":"a.cpp","name":name,"kind":"function",
        "container":null,"start_line":1,"end_line":2,"start_col":0,"end_col":3,
        "signature":format!("int {name}()"),"doc":format!("doc-{name}"),
        "parser_tier":"tree_sitter","parser_confidence":1.0,"qname":name,
        "parent_symbol_id":null,"scope_id":null,"export_name":null,
        "is_default_export":false,"symbol_uid":uid,"framework_role":null,
        "receiver_type":null,"param_types":null,"return_type":"int","param_count":0,
        "base_types":null,"implements":null
    }))
    .unwrap()
}

fn unit() -> FileWriteUnit {
    FileWriteUnit {
        rel_path: "a.cpp".into(),
        language: Language::Cpp,
        content_hash: "hash".into(),
        mtime: 0.,
        size: 1,
        outcome: ParseOutcome {
            symbols: vec![
                symbol("a", Some("u1"), "FirstName"),
                symbol("b", Some("u2"), "SecondName"),
                symbol("a", Some("u2"), "WinnerName"),
                symbol("n1", None, "NullFirst"),
                symbol("n2", None, "NullSecond"),
            ],
            ..Default::default()
        },
    }
}

#[test]
fn scalar_batch_and_both_full_writers_preserve_last_survivor_records() {
    let tmp = TempDir::new().unwrap();
    let file = unit();
    let mut reference = None;
    for mode in ["scalar", "batch", "temp", "direct"] {
        let path = tmp.path().join(format!("{mode}.sqlite3"));
        let db = IndexDb::open(&path).unwrap().0;
        match mode {
            "scalar" => {
                IndexDb::insert_file_data(&Connection::open(&path).unwrap(), &file).unwrap()
            }
            "batch" => db
                .writes()
                .replace_files_batch(std::slice::from_ref(&file))
                .unwrap(),
            "temp" => db
                .admin()
                .rebuild_with_temp_db(|c| IndexDb::insert_file_data(c, &file))
                .unwrap(),
            "direct" => db
                .admin()
                .rebuild_with_direct_writer(|c| IndexDb::insert_file_data(c, &file))
                .unwrap(),
            _ => unreachable!(),
        }
        let c = Connection::open(&path).unwrap();
        aligned(&c);
        let actual = rows(&c, "SELECT rowid,* FROM symbols ORDER BY rowid");
        assert_eq!(actual.len(), 3);
        if let Some(expected) = &reference {
            assert_eq!(&actual, expected, "{mode}");
        } else {
            reference = Some(actual);
        }
    }
}

#[test]
fn dirty_batch_after_anchor_delete_reuses_rowids_and_rolls_back_on_real_conflict() {
    let tmp = TempDir::new().unwrap();
    let db = IndexDb::open(&tmp.path().join("index.sqlite3")).unwrap().0;
    let mut full = unit();
    full.outcome.symbols.truncate(3);
    let mut anchor = full.clone();
    anchor.rel_path = "anchor.cpp".into();
    anchor.outcome.symbols = vec![symbol("anchor", Some("anchor_uid"), "AnchorName")];
    anchor.outcome.symbols[0].file_path = anchor.rel_path.clone();
    db.writes()
        .replace_files_batch(&[full.clone(), anchor])
        .unwrap();
    let mut dirty = full.clone();
    dirty.outcome.symbols = vec![full.outcome.symbols[2].clone()];
    db.writes()
        .write_incremental_batch(
            &["anchor.cpp".into()],
            &[],
            &[dirty.clone()],
            &[],
            &[],
            &PrecompressedChunks::new(),
        )
        .unwrap();
    let c = db.read_conn().unwrap();
    aligned(&c);
    assert_eq!(rows(&c, "SELECT symbol_id FROM symbols").len(), 1);
    let before = rows(&c, "SELECT rowid,* FROM symbols ORDER BY rowid");
    let generation = db.reads().read_generation().unwrap();
    drop(c);
    dirty
        .outcome
        .symbols
        .push(symbol("duplicate_uid", Some("u2"), "ConflictName"));
    assert!(db
        .writes()
        .write_incremental_batch(&[], &[], &[dirty], &[], &[], &PrecompressedChunks::new())
        .is_err());
    let c = db.read_conn().unwrap();
    aligned(&c);
    assert_eq!(
        rows(&c, "SELECT rowid,* FROM symbols ORDER BY rowid"),
        before
    );
    assert_eq!(db.reads().read_generation().unwrap(), generation);
}
