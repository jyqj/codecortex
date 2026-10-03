use super::*;
use crate::index_db::IndexDb;

fn fixture() -> (tempfile::TempDir, IndexDb, rusqlite::Connection) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("independent.sqlite");
    let db = IndexDb::open_with_read_pool_size(&path, 1).unwrap().0;
    let w = rusqlite::Connection::open(path).unwrap();
    w.execute_batch("INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES('review-a','{}','active'),('review-b','{}','backfilling'); INSERT INTO metadata(key,value) VALUES('semantic_epoch','17');").unwrap();
    (dir, db, w)
}

#[test]
fn independent_transaction_all_fields_and_actual_split_mutant() {
    let (_dir, db, w) = fixture();
    let old = db.reads().capability_snapshot(true).unwrap();
    let lease = db.read_conn().unwrap();
    assert!(lease.is_autocommit());
    let pinned = snapshot_on(&lease,true,||{
        // Different thread/connection performs ordinary commit while reader tx lives.
        let writer = std::thread::spawn(move || {
            w.execute_batch("BEGIN;
              INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) VALUES('x.rs','rust','x',4,1,'now'),('y.rs','rust','y',5,1,'now');
              INSERT INTO symbols(symbol_id,file_path,name,kind,start_line,end_line) VALUES('sx','x.rs','x','function',1,1),('sy','y.rs','y','function',1,1);
              INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text) VALUES('cx','x.rs','rust',0,1,1,'x');
              INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,encoding_key,reference_json,record_json) VALUES('dx','v','x.rs','cx','e','{}','{}');
              UPDATE semantic_spaces SET state='revoked' WHERE space_id='review-a';
              UPDATE semantic_spaces SET state='active' WHERE space_id='review-b';
              INSERT INTO semantic_manifest(doc_key,doc_version,file_path,encoding_key,space_id,input_digest,artifact_ref,published_at,published_incarnation) VALUES('dx','v','x.rs','e','review-b','i','a','now','inc');
              INSERT INTO semantic_outbox(doc_key,doc_version,input_digest,space_id,op,state,available_at,created_at,updated_at) VALUES('dx','v','i','review-b','embed','pending',0,0,0),('dx-claimed','v2','j','review-b','embed','claimed',0,0,0),('dx-failed','v3','k','review-b','embed','failed',0,0,0);
              INSERT INTO resolution_frontier(id,version,basis_epoch,reason,root_count,completed_files,payload,digest) VALUES(1,1,'8','disabled',2,1,'{}','unused');
              INSERT INTO metadata(key,value) VALUES('index_epoch','23'),('evidence_epoch','29') ON CONFLICT(key) DO UPDATE SET value=excluded.value;
              UPDATE metadata SET value='31' WHERE key='semantic_epoch'; COMMIT;").unwrap();
        });
        writer.join().unwrap();
    }).unwrap();
    assert!(lease.is_autocommit());
    assert_eq!(
        pinned, old,
        "all fields, including generation/freshness/active/pending/failed must be the old tuple"
    );
    drop(lease);
    let new = db.reads().capability_snapshot(true).unwrap();
    assert_ne!(new, old);
    assert_eq!(new.generation.incarnation, old.generation.incarnation);
    assert_eq!(
        (
            new.generation.index_epoch,
            new.generation.evidence_epoch,
            new.generation.semantic_epoch
        ),
        (23, 29, Some(31))
    );
    assert_eq!((new.indexed_files, new.indexed_symbols), (2, 2));
    assert_eq!(new.resolution_freshness.index_epoch, 23);
    assert!(!new.resolution_freshness.complete);
    assert_eq!(new.resolution_freshness.basis_epoch, Some(8));
    let s = new.semantic.as_ref().unwrap();
    assert_eq!(s.active_space.as_deref(), Some("review-b"));
    assert_eq!((s.pending, s.failed, s.eligible, s.published), (2, 1, 1, 1));
    assert!(db
        .reads()
        .validate_capability_identity(old.generation.incarnation)
        .unwrap());
    // Executed bad algorithm: keep old root, perform coverage on a fresh transaction.
    let mut split = old.clone();
    split.semantic = db.reads().capability_snapshot(true).unwrap().semantic;
    assert!(
        split != old && split != new,
        "independent complete-tuple oracle kills old-root/new-count mutant"
    );
    eprintln!("DB_TUPLES old={old:?} pinned={pinned:?} new={new:?} actual_split_mutant={split:?}");
}

#[test]
fn independent_has_moved_ffi_lifetime_and_unsupported_vfs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("delete.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(
        "PRAGMA journal_mode=DELETE; CREATE TABLE proof(x); INSERT INTO proof VALUES(73);",
    )
    .unwrap();
    assert!(file_identity_on(&conn).unwrap());
    std::fs::rename(&path, dir.path().join("old.db")).unwrap();
    let new = rusqlite::Connection::open(&path).unwrap();
    new.execute_batch("CREATE TABLE proof(x); INSERT INTO proof VALUES(97);")
        .unwrap();
    assert!(!file_identity_on(&conn).unwrap());
    assert!(file_identity_on(&new).unwrap());
    assert_eq!(
        conn.query_row("SELECT x FROM proof", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        73
    );
    assert_eq!(
        new.query_row("SELECT x FROM proof", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        97
    );
    assert!(!path.with_extension("db-wal").exists());
    let memory = rusqlite::Connection::open_in_memory().unwrap();
    assert!(
        file_identity_on(&memory).is_err(),
        "real unsupported SQLite memory VFS must fail closed"
    );
    for (code, moved) in [
        (rusqlite::ffi::SQLITE_NOTFOUND, 0),
        (rusqlite::ffi::SQLITE_IOERR, 0),
        (rusqlite::ffi::SQLITE_OK, -1),
        (rusqlite::ffi::SQLITE_OK, 2),
    ] {
        assert!(file_identity_result(code, moved).is_err());
    }
    eprintln!("FFI Linux Unix VFS normal rename rejected; memory unsupported rejected");
}

#[test]
fn independent_pool_one_incarnation_and_read_only() {
    let (_dir, db, w) = fixture();
    let before = db.reads().capability_snapshot(false).unwrap();
    assert!(before.semantic.is_none());
    w.execute_batch("INSERT INTO metadata(key,value) VALUES('index_epoch','81') ON CONFLICT(key) DO UPDATE SET value=excluded.value; UPDATE metadata SET value='99' WHERE key='semantic_epoch';").unwrap();
    assert!(db
        .reads()
        .validate_capability_identity(before.generation.incarnation)
        .unwrap());
    w.execute(
        "UPDATE metadata SET value=?1 WHERE key='index_incarnation'",
        ["77777777777777777777777777777777"],
    )
    .unwrap();
    assert!(!db
        .reads()
        .validate_capability_identity(before.generation.incarnation)
        .unwrap());
    let current = db.reads().capability_snapshot(false).unwrap();
    assert_eq!(current.generation.incarnation, [0x77; 16]);
    assert!(db
        .reads()
        .validate_capability_identity(current.generation.incarnation)
        .unwrap());
    let lease = db.read_conn().unwrap();
    assert_eq!(
        lease
            .query_row("PRAGMA query_only", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    let changes = lease.total_changes();
    let got = snapshot_on(&lease, false, || {}).unwrap();
    assert_eq!(got, current);
    assert_eq!(lease.total_changes(), changes);
    assert!(lease.is_autocommit());
}
