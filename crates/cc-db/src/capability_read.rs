//! Typed capability observation on one short, read-only SQLite transaction.
//! First SELECT pins one deferred read transaction. All projected database
//! fields share that SQLite snapshot and its strict ReadGeneration. The server
//! reports this generation as an observation, never as response-time latest.
//! File identity is checked at the read boundaries. The separate typed identity
//! validation checks incarnation after this transaction and lease end.
use crate::{index_db::ReadOps, sql_util::db_err};
use cc_model::{freshness::ResolutionFreshness, generation::ReadGeneration, CcError, CcResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityReadSnapshot {
    pub generation: ReadGeneration,
    pub indexed_files: u64,
    pub indexed_symbols: u64,
    pub resolution_freshness: ResolutionFreshness,
    pub semantic: Option<CapabilitySemanticSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilitySemanticSnapshot {
    pub active_space: Option<String>,
    pub pending: u64,
    pub failed: u64,
    pub eligible: u64,
    pub published: u64,
}

impl ReadOps<'_> {
    /// All projected database values share one observed generation. Ordinary
    /// publications may commit before return without invalidating the observation.
    /// A detected main-file replacement refuses this lease; unsupported file
    /// identity validation fails closed. Call validate_capability_identity after
    /// releasing this transaction/lease and observing separate process state.
    /// No nested checkout; include_semantic=false avoids semantic counting.
    pub fn capability_snapshot(&self, include_semantic: bool) -> CcResult<CapabilityReadSnapshot> {
        let conn = self.0.read_conn()?;
        snapshot_on(&conn, include_semantic, || {})
    }

    /// Cheap identity control read, not an epoch fence or another counts snapshot.
    /// Call only after capability_snapshot has released its lease (pool size 1).
    /// False means a detectable replacement/incarnation change. Unverifiable
    /// VFS identity and SQL errors propagate; they are never assumed unmoved.
    pub fn validate_capability_identity(&self, expected_incarnation: [u8; 16]) -> CcResult<bool> {
        let conn = self.0.read_conn()?;
        if !file_identity_on(&conn)? {
            return Ok(false);
        }
        let observed = crate::read_generation::read_on(&conn)?;
        Ok(file_identity_on(&conn)? && observed.incarnation == expected_incarnation)
    }
}

// The observer is private and only used by deterministic snapshot tests.
fn snapshot_on(
    conn: &rusqlite::Connection,
    include_semantic: bool,
    observed_generation: impl FnOnce(),
) -> CcResult<CapabilityReadSnapshot> {
    if !file_identity_on(conn)? {
        return Err(CcError::RetrievalChanged { attempts: 1 });
    }
    let tx = conn.unchecked_transaction().map_err(db_err)?;
    let generation = crate::read_generation::read_on(&tx)?;
    observed_generation();
    let (indexed_files, indexed_symbols) = tx
        .query_row(
            "SELECT (SELECT count(*) FROM files), (SELECT count(*) FROM symbols)",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
        )
        .map_err(db_err)?;
    let indexed_files = u64::try_from(indexed_files).map_err(db_err)?;
    let indexed_symbols = u64::try_from(indexed_symbols).map_err(db_err)?;
    let resolution_freshness =
        crate::freshness_store::resolution_freshness_on(&tx, generation.index_epoch)?;
    let semantic = if include_semantic {
        let active_space = crate::semantic_outbox::active_space_on(&tx)?;
        let (pending, failed, eligible, published) = match active_space.as_deref() {
                None => (0, 0, 0, 0),
                Some(space) => tx.query_row(
                    "SELECT
                     (SELECT count(*) FROM semantic_outbox WHERE space_id=?1 AND state IN ('pending','claimed')),
                     (SELECT count(*) FROM semantic_outbox WHERE space_id=?1 AND state='failed'),
                     (SELECT count(*) FROM document_manifest WHERE encoding_key IS NOT NULL),
                     (SELECT count(*) FROM semantic_manifest WHERE space_id=?1)",
                    [space],
                    |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?,
                            r.get::<_, i64>(2)?, r.get::<_, i64>(3)?)),
                ).map_err(db_err)?,
            };
        Some(CapabilitySemanticSnapshot {
            active_space,
            pending: u64::try_from(pending).map_err(db_err)?,
            failed: u64::try_from(failed).map_err(db_err)?,
            eligible: u64::try_from(eligible).map_err(db_err)?,
            published: u64::try_from(published).map_err(db_err)?,
        })
    } else {
        None
    };
    tx.commit().map_err(db_err)?;
    if !file_identity_on(conn)? {
        return Err(CcError::RetrievalChanged { attempts: 1 });
    }
    Ok(CapabilityReadSnapshot {
        generation,
        indexed_files,
        indexed_symbols,
        resolution_freshness,
        semantic,
    })
}

/// Inspect the SQLite main file actually opened by this connection. Merely
/// statting the pathname would miss an old pooled lease after a rename.
fn file_identity_on(conn: &rusqlite::Connection) -> CcResult<bool> {
    let mut moved: std::ffi::c_int = -1;
    // SAFETY: the borrowed connection/lease keeps its sqlite3 handle alive and
    // exclusively used for this call. "main" is a static NUL-terminated name;
    // HAS_MOVED accepts a writable C int, which remains live for the whole call.
    // This file-control is read-only. No raw handle leaves cc-db.
    let code = unsafe {
        rusqlite::ffi::sqlite3_file_control(
            conn.handle(),
            c"main".as_ptr(),
            rusqlite::ffi::SQLITE_FCNTL_HAS_MOVED,
            (&mut moved as *mut std::ffi::c_int).cast(),
        )
    };
    file_identity_result(code, moved)
}

fn file_identity_result(code: std::ffi::c_int, moved: std::ffi::c_int) -> CcResult<bool> {
    match (code, moved) {
        (rusqlite::ffi::SQLITE_OK, 0) => Ok(true),
        (rusqlite::ffi::SQLITE_OK, 1) => Ok(false),
        _ => Err(CcError::Database(format!(
            "capability identity unverified: SQLite HAS_MOVED code={code}, result={moved}; diagnostic unavailable on this VFS"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index_db::IndexDb;

    #[test]
    fn all_fields_keep_the_pinned_snapshot_across_a_committed_publication() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("capability.db");
        let db = IndexDb::open_with_read_pool_size(&path, 1).unwrap().0;
        let writer = rusqlite::Connection::open(&path).unwrap();
        writer.execute_batch(
            "INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES('old','{}','active'),('new','{}','backfilling');
             INSERT INTO metadata(key,value) VALUES('semantic_epoch','1');"
        ).unwrap();
        let before = db.reads().capability_snapshot(true).unwrap();
        let lease = db.read_conn().unwrap();
        let crossed = snapshot_on(&lease, true, || {
            writer.execute_batch(
                "BEGIN;
                 INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) VALUES('a.rs','rust','hash',1,1,'now');
                 INSERT INTO symbols(symbol_id,file_path,name,kind,start_line,end_line) VALUES('symbol','a.rs','a','function',1,1);
                 INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text) VALUES('chunk','a.rs','rust',0,1,1,'body');
                 INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,encoding_key,reference_json,record_json) VALUES('doc','v1','a.rs','chunk','enc','{}','{}');
                 UPDATE semantic_spaces SET state='revoked' WHERE space_id='old';
                 UPDATE semantic_spaces SET state='active' WHERE space_id='new';
                 INSERT INTO semantic_manifest(doc_key,doc_version,file_path,encoding_key,space_id,input_digest,artifact_ref,published_at,published_incarnation) VALUES('doc','v1','a.rs','enc','new','input','artifact','now','incarnation');
                 INSERT INTO semantic_outbox(doc_key,doc_version,input_digest,space_id,op,state,available_at,created_at,updated_at) VALUES('doc','v1','input','new','embed','pending',0,0,0),('doc','v0','old','new','embed','failed',0,0,0);
                 INSERT INTO resolution_frontier(id,version,basis_epoch,reason,root_count,completed_files,payload,digest) VALUES(1,1,'0','disabled',1,0,'{}','unused');
                 UPDATE metadata SET value='2' WHERE key='semantic_epoch';
                 INSERT INTO metadata(key,value) VALUES('index_epoch','2'),('evidence_epoch','2') ON CONFLICT(key) DO UPDATE SET value=excluded.value;
                 COMMIT;"
            ).unwrap();
        }).unwrap();
        assert_eq!(
            crossed, before,
            "every value must come from the old transaction"
        );
        drop(lease);
        let after = db.reads().capability_snapshot(true).unwrap();
        assert_eq!(after.generation.semantic_epoch, Some(2));
        assert_eq!(after.indexed_files, 1);
        assert_eq!(after.indexed_symbols, 1);
        assert_eq!(after.semantic.as_ref().unwrap().pending, 1);
        assert_eq!(after.semantic.as_ref().unwrap().failed, 1);
        assert!(!after.resolution_freshness.complete);
        assert_eq!(
            after.semantic.as_ref().unwrap().active_space.as_deref(),
            Some("new")
        );
        assert_eq!(after.semantic.as_ref().unwrap().eligible, 1);
        assert_eq!(after.semantic.as_ref().unwrap().published, 1);
        assert_eq!(
            after.resolution_freshness.index_epoch,
            after.generation.index_epoch
        );
        assert_eq!(
            db.reads().read_generation().unwrap(),
            after.generation,
            "the one-connection lease is released before the fresh fence"
        );
    }

    #[test]
    fn unwired_read_skips_semantic_counts_and_does_not_write() {
        let dir = tempfile::tempdir().unwrap();
        let db = IndexDb::open_with_read_pool_size(&dir.path().join("capability.db"), 1)
            .unwrap()
            .0;
        let before = db.reads().read_generation().unwrap();
        let snapshot = db.reads().capability_snapshot(false).unwrap();
        assert_eq!(snapshot.generation, before);
        assert!(snapshot.semantic.is_none());
        assert_eq!(db.reads().read_generation().unwrap(), before);
    }

    #[test]
    fn unsupported_error_and_undefined_identity_results_are_never_unmoved() {
        for (code, moved) in [
            (rusqlite::ffi::SQLITE_NOTFOUND, -1),
            (rusqlite::ffi::SQLITE_IOERR, 0),
            (rusqlite::ffi::SQLITE_OK, -1),
            (rusqlite::ffi::SQLITE_OK, 2),
        ] {
            let error = file_identity_result(code, moved).unwrap_err();
            assert!(error.to_string().contains("identity unverified"));
        }
        assert!(file_identity_result(rusqlite::ffi::SQLITE_OK, 0).unwrap());
        assert!(!file_identity_result(rusqlite::ffi::SQLITE_OK, 1).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn normal_rename_checks_the_actual_open_file_without_rebuild_or_wal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("standalone-delete-journal.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("PRAGMA journal_mode=DELETE; CREATE TABLE sample(value TEXT); INSERT INTO sample VALUES('old');").unwrap();
        assert!(file_identity_on(&conn).unwrap());
        std::fs::rename(&path, dir.path().join("old-file.db")).unwrap();
        let replacement = rusqlite::Connection::open(&path).unwrap();
        replacement
            .execute_batch("CREATE TABLE sample(value TEXT); INSERT INTO sample VALUES('new');")
            .unwrap();
        assert!(
            !file_identity_on(&conn).unwrap(),
            "old lease must not impersonate the new pathname"
        );
        assert!(file_identity_on(&replacement).unwrap());
        assert_eq!(
            conn.query_row("SELECT value FROM sample", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "old"
        );
        assert!(!path.with_extension("db-wal").exists());
    }

    #[test]
    fn incarnation_validation_rejects_in_place_replacement_but_not_epoch_churn() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("incarnation.db");
        let db = IndexDb::open_with_read_pool_size(&path, 1).unwrap().0;
        let snapshot = db.reads().capability_snapshot(false).unwrap();
        let writer = rusqlite::Connection::open(&path).unwrap();
        writer.execute_batch("INSERT INTO metadata(key,value) VALUES('semantic_epoch','42') ON CONFLICT(key) DO UPDATE SET value=excluded.value;").unwrap();
        assert!(db
            .reads()
            .validate_capability_identity(snapshot.generation.incarnation)
            .unwrap());
        crate::read_generation::renew(&writer).unwrap();
        assert!(!db
            .reads()
            .validate_capability_identity(snapshot.generation.incarnation)
            .unwrap());
        let current = db.reads().capability_snapshot(false).unwrap();
        assert_ne!(
            snapshot.generation.incarnation,
            current.generation.incarnation
        );
        assert!(db
            .reads()
            .validate_capability_identity(current.generation.incarnation)
            .unwrap());
    }

    #[test]
    fn missing_count_table_errors_are_not_zero_or_a_partial_ready_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let conn = rusqlite::Connection::open(dir.path().join("minimal-schema.db")).unwrap();
        conn.execute_batch("CREATE TABLE metadata(key TEXT PRIMARY KEY,value TEXT);")
            .unwrap();
        crate::read_generation::ensure(&conn).unwrap();
        conn.execute_batch("PRAGMA query_only=ON;").unwrap();
        let changes = conn.total_changes();
        let error = snapshot_on(&conn, false, || {}).unwrap_err();
        assert!(error.to_string().contains("no such table: files"));
        assert!(
            conn.is_autocommit(),
            "failed read transaction must release its snapshot"
        );
        assert_eq!(
            conn.total_changes(),
            changes,
            "no read-side identity/schema repair"
        );
    }
}
