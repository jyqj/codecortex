//! Typed capability observation on one short, read-only SQLite transaction.
//! First SELECT pins one deferred read transaction. All projected database
//! fields share that SQLite snapshot and its strict ReadGeneration. The server
//! retains its latest-generation fence after this transaction and lease end.
//! This read alone does not certify live-path identity during database swap.
use crate::{index_db::ReadOps, sql_util::db_err};
use cc_model::{freshness::ResolutionFreshness, generation::ReadGeneration, CcResult};

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
    /// Only an observation of this leased DB incarnation. Ordinary publications
    /// may commit before return. Caller must retain the existing latest fence
    /// and independently verify live database identity before projecting ready.
    /// No nested checkout; include_semantic=false avoids semantic counting.
    pub fn capability_snapshot(&self, include_semantic: bool) -> CcResult<CapabilityReadSnapshot> {
        let conn = self.0.read_conn()?;
        snapshot_on(&conn, include_semantic, || {})
    }
}

// The observer is private and only used by deterministic snapshot tests.
fn snapshot_on(
    conn: &rusqlite::Connection,
    include_semantic: bool,
    observed_generation: impl FnOnce(),
) -> CcResult<CapabilityReadSnapshot> {
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
    Ok(CapabilityReadSnapshot {
        generation,
        indexed_files,
        indexed_symbols,
        resolution_freshness,
        semantic,
    })
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
}
