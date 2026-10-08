//! Read-only GC mark support (P6-016).
//!
//! Boundary authority: `docs/adr/0003-semantic-persistence-single-db-boundary.md`
//! (row P6-016: "共享同步点；无『manifest 引用刚被 GC 删除产物』竞态；孤儿最终
//! 可回收") and `artifacts/checkpoints/p6-implementation-planning-20261002/
//! TASK-BRIEFS.md` P6-016 ("mark：活引用集合 = `semantic_manifest.artifact_ref`
//! （全 space，含非 active）∪ 进行中任务的 `input_digest` 派生目标（活跃 lease
//! 行）"). This module is the cc-db half of that mark set: three pure SELECT
//! predicates the cc-semantic sweep consumes per candidate.
//!
//! Scope discipline:
//!
//! - **Read-only, zero writes, zero epoch movement.** GC's sweep phase performs
//!   filesystem unlinks only ("全程无 DB 写……不刷 epoch", brief P6-016); nothing
//!   in this module can write. The caller opens the short read snapshot
//!   (synchronization point) around these checks — see
//!   `cc_semantic::gc` for the ordering contract.
//! - **All spaces count.** The manifest predicates have no `space_id` filter on
//!   the caller side beyond what the check itself needs: P6-017 rollback reuse
//!   depends on objects of *revoked* spaces staying resolvable, so a manifest
//!   row anywhere (active or not) protects its artifact.
//! - **Conservative pair predicate.** [`manifest_holds_input_on`] matches on
//!   `(space_id, input_digest)` without the checksum component of the
//!   `artifact_ref`. It exists for candidates whose checksum cannot be
//!   reconstructed (unparseable sidecar, half-written objects) — the red line
//!   is "判据保守：不确定即保留", so when the exact ref is unavailable the
//!   broader predicate decides.

use cc_model::CcResult;
use rusqlite::Connection;

/// Exact mark predicate: does any manifest row (any space, any state — the
/// table only holds the published visible set) reference this artifact?
pub fn manifest_references_artifact_on(conn: &Connection, artifact_ref: &str) -> CcResult<bool> {
    let hit: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM semantic_manifest WHERE artifact_ref = ?1 LIMIT 1",
            [artifact_ref],
            |row| row.get(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })
        .map_err(crate::sql_util::db_err)?;
    Ok(hit.is_some())
}

/// Conservative mark predicate: does any manifest row publish from this
/// `(space_id, input_digest)` pair, regardless of spec digest or payload
/// checksum? Used when the candidate's checksum is not reconstructable from
/// its sidecar (unparseable meta, half-written object).
pub fn manifest_holds_input_on(
    conn: &Connection,
    space_id: &str,
    input_digest: &str,
) -> CcResult<bool> {
    let hit: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM semantic_manifest \
             WHERE space_id = ?1 AND input_digest = ?2 LIMIT 1",
            [space_id, input_digest],
            |row| row.get(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })
        .map_err(crate::sql_util::db_err)?;
    Ok(hit.is_some())
}

/// Active-lease mark predicate: does any live outbox task (`pending` or
/// `claimed`) embed from this input digest? The brief's mark set names the
/// active lease rows; `pending` is added conservatively — a queued task may
/// still reuse the cached artifact, and deleting it would trade a paid vector
/// for a re-embed (红线：不确定即保留).
pub fn live_task_holds_input_on(conn: &Connection, input_digest: &str) -> CcResult<bool> {
    let hit: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM semantic_outbox \
             WHERE input_digest = ?1 AND state IN ('pending','claimed') LIMIT 1",
            [input_digest],
            |row| row.get(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })
        .map_err(crate::sql_util::db_err)?;
    Ok(hit.is_some())
}

/// One GC candidate probe (P6-016 sweep → mark). `artifact_ref` is the exact
/// content address when the sweep could reconstruct it from the object's
/// sidecar checksum; `None` routes the probe through the conservative
/// [`manifest_holds_input_on`] pair predicate instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GcMarkProbe {
    pub artifact_ref: Option<String>,
    pub space_id: String,
    pub input_digest: String,
}

/// Mark verdict for one probe, in probe order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GcMark {
    /// A manifest row references this exact artifact — never delete.
    ManifestRef,
    /// No exact ref was checkable but a manifest row publishes from this
    /// `(space_id, input_digest)` pair — never delete (conservative).
    ManifestPair,
    /// A live outbox task (`pending`/`claimed`) embeds from this input —
    /// never delete.
    LiveTask,
    /// No reference found under this snapshot — deletable.
    Unreferenced,
}

/// Mark a whole candidate batch under ONE short read snapshot: this is the
/// GC/publish synchronization point of P6-016 ("GC 候选收集与删除决定之间，
/// 取一次 DB 短事务快照"). The deferred transaction takes a consistent WAL
/// snapshot; every publish CAS that committed before the snapshot is visible
/// to the mark, so its artifact is returned as referenced and kept. A publish
/// committing after the snapshot is the freshness-grace's job (the publisher
/// re-put the object moments before its CAS — see `cc_semantic::gc`).
///
/// Read-only by construction (SELECTs only, read-pool connection); the
/// transaction is dropped, never written, and no epoch moves. The returned
/// verdicts run in probe order, one per probe.
pub fn mark_batch_on(conn: &Connection, probes: &[GcMarkProbe]) -> CcResult<Vec<GcMark>> {
    let mut marks = Vec::with_capacity(probes.len());
    for probe in probes {
        let mark = if let Some(artifact_ref) = &probe.artifact_ref {
            if manifest_references_artifact_on(conn, artifact_ref)? {
                GcMark::ManifestRef
            } else if live_task_holds_input_on(conn, &probe.input_digest)? {
                GcMark::LiveTask
            } else {
                GcMark::Unreferenced
            }
        } else if manifest_holds_input_on(conn, &probe.space_id, &probe.input_digest)? {
            GcMark::ManifestPair
        } else if live_task_holds_input_on(conn, &probe.input_digest)? {
            GcMark::LiveTask
        } else {
            GcMark::Unreferenced
        };
        marks.push(mark);
    }
    Ok(marks)
}

impl crate::index_db::IndexDb {
    /// Execute a filesystem sweep while this handle's rebuild/write mutex
    /// and a read-only SQLite snapshot remain held. The caller must acquire
    /// the cache namespace mutation lease FIRST and keep it until return.
    /// The callback performs bounded local file operations only: it must
    /// not re-enter this database, wait for another writer or call a provider.
    ///
    /// No SQL writes or epoch effects occur. Unlike `semantic_gc_mark`, this
    /// seam also excludes a same-handle rebuild between mark and unlink and
    /// refuses an already-detached database handle. Cross-process rebuild
    /// still follows the existing composition-root serialization contract.
    pub fn with_semantic_gc_marks<T>(
        &self,
        probes: &[GcMarkProbe],
        sweep: impl FnOnce(&[GcMark]) -> CcResult<T>,
    ) -> CcResult<T> {
        let conn = self.write_conn.lock().map_err(crate::sql_util::db_err)?;
        let snapshot = conn
            .unchecked_transaction()
            .map_err(crate::sql_util::db_err)?;
        let incarnation = crate::read_generation::read_on(&conn)?.incarnation;
        if let crate::semantic_rebuild::IncarnationFreshness::Stale { .. } =
            self.semantic_incarnation_freshness(incarnation)?
        {
            return Err(cc_model::CcError::RetrievalChanged { attempts: 1 });
        }
        let marks = mark_batch_on(&conn, probes)?;
        let result = sweep(&marks);
        drop(snapshot);
        result
    }

    /// Facade for the GC synchronization point: takes the read-pool
    /// connection, wraps [`mark_batch_on`] in one short deferred snapshot,
    /// and releases it. Same-order, same-length verdicts as the probes.
    pub fn semantic_gc_mark(&self, probes: &[GcMarkProbe]) -> CcResult<Vec<GcMark>> {
        let conn = self.read_conn()?;
        let snapshot = conn
            .unchecked_transaction()
            .map_err(crate::sql_util::db_err)?;
        let marks = mark_batch_on(&conn, probes)?;
        drop(snapshot);
        Ok(marks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v22_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        assert_eq!(
            crate::index_migrate::migrate_index_db(&conn).unwrap(),
            crate::index_migrate::SchemaStatus::Initialized
        );
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        conn
    }

    fn seed_published(conn: &Connection, space_id: &str, input_digest: &str, artifact_ref: &str) {
        conn.execute_batch(&format!(
            "INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) \
             VALUES('a.rs','rust','hash',1.0,1,'2026-01-01');
             INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text) \
             VALUES('c-d1','a.rs','rust',0,1,2,'body');
             INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,encoding_key,\
               reference_json,record_json) \
             VALUES('d1','v1','a.rs','c-d1',NULL,'{{}}','{{}}');
             INSERT INTO semantic_manifest(doc_key,doc_version,file_path,encoding_key,input_digest,\
               space_id,artifact_ref,published_at,published_incarnation) \
             VALUES('d1','v1','a.rs','enc','{input_digest}','{space_id}','{artifact_ref}',\
               '2026-01-01','inc');"
        ))
        .unwrap();
    }

    fn seed_live_task(conn: &Connection, input_digest: &str, state: &str) {
        conn.execute(
            "INSERT INTO semantic_outbox(doc_key,doc_version,input_digest,space_id,op,state,\
               available_at,created_at,updated_at) \
             VALUES('d2','v1',?1,'sp','embed',?2,0,'2026-01-01','2026-01-01')",
            rusqlite::params![input_digest, state],
        )
        .unwrap();
    }

    #[test]
    fn exact_ref_mark_is_reflexive_and_checksum_scoped() {
        let conn = v22_conn();
        seed_published(&conn, "sp", "in-1", "cas.v1:ns:sp:in-1:spec:aaa");
        assert!(manifest_references_artifact_on(&conn, "cas.v1:ns:sp:in-1:spec:aaa").unwrap());
        // A different checksum under the same pair is a different object.
        assert!(!manifest_references_artifact_on(&conn, "cas.v1:ns:sp:in-1:spec:bbb").unwrap());
        assert!(!manifest_references_artifact_on(&conn, "").unwrap());
    }

    #[test]
    fn pair_mark_holds_across_spec_and_checksum_and_spaces_are_separate() {
        let conn = v22_conn();
        seed_published(&conn, "sp", "in-1", "cas.v1:ns:sp:in-1:spec:aaa");
        assert!(manifest_holds_input_on(&conn, "sp", "in-1").unwrap());
        assert!(!manifest_holds_input_on(&conn, "other-space", "in-1").unwrap());
        assert!(!manifest_holds_input_on(&conn, "sp", "in-other").unwrap());
    }

    #[test]
    fn live_task_mark_covers_pending_and_claimed_but_not_terminal_states() {
        let conn = v22_conn();
        assert!(!live_task_holds_input_on(&conn, "in-1").unwrap());
        seed_live_task(&conn, "in-1", "pending");
        assert!(live_task_holds_input_on(&conn, "in-1").unwrap());
        conn.execute("UPDATE semantic_outbox SET state='claimed'", [])
            .unwrap();
        assert!(live_task_holds_input_on(&conn, "in-1").unwrap());
        for terminal in ["done", "failed", "superseded"] {
            conn.execute(
                "UPDATE semantic_outbox SET state=?1",
                rusqlite::params![terminal],
            )
            .unwrap();
            assert!(
                !live_task_holds_input_on(&conn, "in-1").unwrap(),
                "terminal state {terminal} must not hold the input"
            );
        }
    }
}
