//! Typed multi-statement write transaction over the index database.
//!
//! [`UnitOfWork`] is the cc-db transaction seam for callers that must apply
//! several logically-coupled writes atomically (currently: applying a
//! dispatch-synthesis round's edge deltas). It holds the single write
//! connection for its whole lifetime and exposes only typed write methods —
//! never the raw `rusqlite::Connection` — so all SQL stays inside cc-db.
//!
//! Semantics:
//! - `begin` opens an `IMMEDIATE` transaction on the write connection.
//! - `query_json` reads through the transaction connection, so it observes
//!   the unit's own uncommitted writes.
//! - `commit` bumps `index_epoch` exactly once and commits; the per-method
//!   epoch bump done by `IndexDb` write methods is intentionally skipped
//!   inside a unit of work to avoid double-counting.
//! - Dropping an uncommitted unit rolls the transaction back.
//!
//! Failure and contention model:
//! - Units of work are expected to be short batch writes (synthesis compute
//!   happens *before* `begin`, against the read pool — see cc-index's
//!   `synthesis_pipeline`). The `IMMEDIATE` transaction holds SQLite's
//!   RESERVED lock only for that batch write, so writers in other processes
//!   wait well under the 5000 ms `busy_timeout`.
//! - A panic between `begin` and `commit` unwinds through `Drop` (rolling
//!   the transaction back) and poisons the write mutex: every later write
//!   fails with a lock error until the process restarts. This is an explicit
//!   fail-stop mode, not silent corruption — and it can only originate from
//!   the apply step itself, never from pass computation.
//!
//! The seam is designed to grow: future migrations (rebuild, evidence
//! ingest) can add their typed methods here without changing the contract.

use std::sync::MutexGuard;

use rusqlite::Connection;
use serde_json::Value;

use cc_model::{CcError, CcResult};

use crate::index_db::IndexDb;
use crate::sql_util::db_err;

/// A typed, atomic batch of index writes (plus transaction-local reads).
pub struct UnitOfWork<'db> {
    conn: MutexGuard<'db, Connection>,
    committed: bool,
}

impl<'db> UnitOfWork<'db> {
    /// Begin a unit of work, taking the write lock for its whole lifetime.
    ///
    /// While the unit is alive, calling any `IndexDb` write method from the
    /// same thread deadlocks (the write mutex is not reentrant) — all data
    /// access inside the unit must go through `UnitOfWork` methods.
    pub(crate) fn begin(db: &'db IndexDb) -> CcResult<Self> {
        let conn = db.write_conn.lock().map_err(db_err)?;
        conn.execute_batch("BEGIN IMMEDIATE;")
            .map_err(|e| CcError::Database(format!("begin unit of work: {}", e)))?;
        Ok(Self {
            conn,
            committed: false,
        })
    }

    /// Commit the unit of work, bumping `index_epoch` exactly once.
    pub fn commit(mut self) -> CcResult<()> {
        IndexDb::bump_index_epoch_on(&self.conn)?;
        self.conn
            .execute_batch("COMMIT;")
            .map_err(|e| CcError::Database(format!("commit unit of work: {}", e)))?;
        self.committed = true;
        Ok(())
    }

    // ── Writes ───────────────────────────────────────────────────

    /// Delete synthetic call edges produced by the given synthesis pass.
    pub fn delete_synthetic_call_edges(&self, synthesized_by: &str) -> CcResult<usize> {
        IndexDb::delete_synthetic_call_edges_on(&self.conn, synthesized_by)
    }

    /// Insert synthetic call edges.
    pub fn insert_synthetic_call_edges(
        &self,
        edges: &[cc_model::CallEdgeRecord],
    ) -> CcResult<usize> {
        IndexDb::insert_synthetic_call_edges_on(&self.conn, edges)
    }

    /// Delete synthetic semantic edges whose edge_id starts with the prefix.
    pub fn delete_synthetic_semantic_edges(&self, edge_id_prefix: &str) -> CcResult<usize> {
        IndexDb::delete_synthetic_semantic_edges_on(&self.conn, edge_id_prefix)
    }

    /// Insert (or replace) a batch of semantic edges (signature-aggregate
    /// maintained — see `signature_agg`).
    pub fn insert_semantic_edges_batch(
        &self,
        edges: &[cc_model::edge::SemanticEdgeRecord],
    ) -> CcResult<()> {
        IndexDb::insert_semantic_edges_batch_maintained_on(&self.conn, edges)
    }

    // ── Transaction-local reads ──────────────────────────────────

    /// Run a read-only query through the transaction connection (observes
    /// the unit's own uncommitted writes) and return rows as JSON objects.
    pub fn query_json(&self, sql: &str, params: &[String]) -> CcResult<Vec<Value>> {
        IndexDb::query_json_on(&self.conn, sql, params)
    }
}

impl Drop for UnitOfWork<'_> {
    fn drop(&mut self) {
        if !self.committed {
            if let Err(e) = self.conn.execute_batch("ROLLBACK;") {
                tracing::warn!(error = %e, "unit of work rollback failed");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::index_db::IndexDb;
    use tempfile::TempDir;

    fn setup() -> (TempDir, IndexDb) {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("uow.sqlite3")).unwrap().0;
        // Satisfy the call_edges → files foreign key for sample edges.
        // Seeded through the write connection: the pooled read connections
        // are query_only, and this fixture does not depend on epoch bumps.
        db.write_conn
            .lock()
            .unwrap()
            .execute(
                "INSERT OR IGNORE INTO files(file_path, language, content_hash, mtime, size, indexed_at) \
                 VALUES('src/a.ts', 'ts', 'abc', 0.0, 100, '2025-01-01')",
                [],
            )
            .unwrap();
        (tmp, db)
    }

    fn sample_edge(edge_id: &str) -> cc_model::CallEdgeRecord {
        cc_model::CallEdgeRecord {
            edge_id: edge_id.to_string(),
            file_path: "src/a.ts".to_string(),
            callee_symbol: "handler".to_string(),
            synthesized_by: Some("event_emitter".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn commit_applies_all_writes_and_bumps_epoch_once() {
        let (_tmp, db) = setup();
        let before = db.generation().unwrap();

        let uow = db.begin_unit_of_work().unwrap();
        uow.delete_synthetic_call_edges("event_emitter").unwrap();
        uow.insert_synthetic_call_edges(&[sample_edge("synth:ee:1"), sample_edge("synth:ee:2")])
            .unwrap();
        // The unit's own reads see its uncommitted writes.
        let in_tx = uow
            .query_json(
                "SELECT COUNT(*) AS cnt FROM call_edges WHERE synthesized_by = 'event_emitter'",
                &[],
            )
            .unwrap();
        assert_eq!(in_tx[0]["cnt"].as_i64(), Some(2));
        uow.commit().unwrap();

        let after = db.generation().unwrap();
        assert_eq!(
            after.index_epoch,
            before.index_epoch + 1,
            "a committed unit of work bumps index_epoch exactly once"
        );
        let rows = db
            .query_json(
                "SELECT COUNT(*) AS cnt FROM call_edges WHERE synthesized_by = 'event_emitter'",
                &[],
            )
            .unwrap();
        assert_eq!(rows[0]["cnt"].as_i64(), Some(2));
    }

    #[test]
    fn drop_without_commit_rolls_back_and_leaves_epoch_untouched() {
        let (_tmp, db) = setup();
        let before = db.generation().unwrap();

        {
            let uow = db.begin_unit_of_work().unwrap();
            uow.insert_synthetic_call_edges(&[sample_edge("synth:ee:rollback")])
                .unwrap();
            // Dropped without commit → rollback.
        }

        let after = db.generation().unwrap();
        assert_eq!(after.index_epoch, before.index_epoch);
        let rows = db
            .query_json(
                "SELECT COUNT(*) AS cnt FROM call_edges WHERE synthesized_by = 'event_emitter'",
                &[],
            )
            .unwrap();
        assert_eq!(rows[0]["cnt"].as_i64(), Some(0));
        // The write connection is usable again after the rollback.
        db.insert_synthetic_call_edges(&[sample_edge("synth:ee:after")])
            .unwrap();
    }
}
