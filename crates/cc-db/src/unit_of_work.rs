//! Typed multi-statement write transaction over the index database.
//!
//! [`UnitOfWork`] is the cc-db transaction seam for callers that must apply
//! several logically-coupled writes atomically (currently: the dispatch
//! synthesis passes). It holds the single write connection for its whole
//! lifetime and exposes only typed read/write methods — never the raw
//! `rusqlite::Connection` — so all SQL stays inside cc-db.
//!
//! Semantics:
//! - `begin` opens an `IMMEDIATE` transaction on the write connection.
//! - All reads go through the transaction connection, so they observe the
//!   unit's own uncommitted writes (required by passes that consume edges
//!   produced by earlier passes in the same unit).
//! - `commit` bumps `index_epoch` exactly once and commits; the per-method
//!   epoch bump done by `IndexDb` write methods is intentionally skipped
//!   inside a unit of work to avoid double-counting.
//! - Dropping an uncommitted unit rolls the transaction back.
//!
//! Failure and contention model:
//! - The write mutex is held for the unit's whole lifetime. A panic inside
//!   a pass unwinds through `Drop` (rolling the transaction back) but then
//!   poisons the write mutex: every later write fails with a lock error
//!   until the process restarts. This is an explicit fail-stop mode, not
//!   silent corruption.
//! - The `IMMEDIATE` transaction holds SQLite's RESERVED lock for the whole
//!   synthesis phase, so writers in *other processes* block for at most
//!   `busy_timeout` (5000 ms) before erroring. If a unit of work ever runs
//!   longer than ~5 s, re-evaluate this trade-off (chunked units or a larger
//!   busy timeout).
//!
//! The seam is designed to grow: future migrations (rebuild, evidence
//! ingest) can add their typed methods here without changing the contract.

use std::sync::MutexGuard;

use rusqlite::Connection;
use serde_json::Value;

use cc_model::{CcError, CcResult};

use crate::index_db::{IndexDb, SymbolRow};
use crate::index_db_graph::MethodsByContainer;

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
        let conn = db
            .write_conn
            .lock()
            .map_err(|e| CcError::Database(e.to_string()))?;
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

    /// Insert (or replace) a batch of semantic edges.
    pub fn insert_semantic_edges_batch(
        &self,
        edges: &[cc_model::edge::SemanticEdgeRecord],
    ) -> CcResult<()> {
        IndexDb::insert_semantic_edges_batch_on(&self.conn, edges)
    }

    // ── Transaction-local reads ──────────────────────────────────

    /// Load every dispatch site.
    pub fn load_all_dispatch_sites(&self) -> CcResult<Vec<cc_model::DispatchSiteRecord>> {
        IndexDb::load_all_dispatch_sites_on(&self.conn)
    }

    /// Load dispatch sites of a single kind.
    pub fn load_dispatch_sites_by_kind(
        &self,
        kind: &str,
    ) -> CcResult<Vec<cc_model::DispatchSiteRecord>> {
        IndexDb::load_dispatch_sites_by_kind_on(&self.conn, kind)
    }

    /// Find symbols by exact name restricted to the given kinds.
    pub fn find_symbols_by_name_and_kinds(
        &self,
        name: &str,
        kinds: &[&str],
    ) -> CcResult<Vec<SymbolRow>> {
        IndexDb::find_symbols_by_name_and_kinds_on(&self.conn, name, kinds)
    }

    /// All symbols of a file, ordered by start line.
    pub fn file_symbols(&self, file_path: &str) -> CcResult<Vec<SymbolRow>> {
        IndexDb::file_symbols_on(&self.conn, file_path)
    }

    /// Find a method by name in the same class as the given member symbol.
    pub fn find_method_in_same_class(
        &self,
        member_symbol_uid: &str,
        method_name: &str,
    ) -> CcResult<Option<String>> {
        IndexDb::find_method_in_same_class_on(&self.conn, member_symbol_uid, method_name)
    }

    /// Methods of many containers in one query, grouped by container name.
    pub fn find_methods_by_containers(&self, containers: &[&str]) -> CcResult<MethodsByContainer> {
        IndexDb::find_methods_by_containers_on(&self.conn, containers)
    }

    /// Classes that have methods matching any of the given names.
    pub fn find_classes_with_method_names(
        &self,
        method_names: &[&str],
    ) -> CcResult<Vec<(String, String)>> {
        IndexDb::find_classes_with_method_names_on(&self.conn, method_names)
    }

    /// Run a read-only query and return rows as JSON objects.
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
        db.read_conn()
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
