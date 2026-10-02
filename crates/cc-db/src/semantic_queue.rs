//! Semantic queue write facade (P6-013): the `IndexDb`-side short
//! transactions the worker drives claim/renew/retry/reclaim through.
//!
//! Hand-over of P6-007 deviation 8 ("写门面随 P6-013 worker 消费方（首个生产
//! 调用方）一起落"): the fenced free-function primitives in
//! [`crate::semantic_outbox`] stay the frozen mechanism layer (P6-007
//! deliverable, untouched); this file only composes each primitive with the
//! write connection and the caller-side clock into one `IMMEDIATE` short
//! transaction on the [`IndexDb`] facade — the same "归类细节收进 cc-db 门面"
//! pattern as `semantic_publish`.
//!
//! Every operation here is Auxiliary: no epoch ever moves (P6-004 effect
//! taxonomy; the lease lifecycle tests in `semantic_lease.rs` keep holding).
//! There is deliberately NO plain-ack facade method: ack belongs to the
//! publish CAS transaction (P6-011, fence 2 + fenced ack are one atomic
//! unit); a standalone ack would let a worker advance a task whose artifact
//! was never verified. The revoke-task consumer (P6-017) adds its own ack
//! path when it becomes the second production consumer.

use rusqlite::Connection;

use cc_model::{CcError, CcResult};

use crate::index_db::IndexDb;
use crate::semantic_outbox::{self, ClaimFairness, ClaimedTask};
use crate::sql_util::db_err;

/// Run `body` as one `IMMEDIATE` short transaction on the write connection
/// (the claim/renew/retry/reclaim pattern of the brief: "每个都是独立
/// IMMEDIATE 短事务"). Commit on success, roll back on error — the caller
/// sees either the fenced primitive's full effect or nothing.
fn queue_txn<T>(
    conn: &Connection,
    what: &str,
    body: impl FnOnce(&Connection) -> CcResult<T>,
) -> CcResult<T> {
    conn.execute_batch("BEGIN IMMEDIATE;")
        .map_err(|e| CcError::Database(format!("begin {what}: {e}")))?;
    match body(conn) {
        Ok(value) => match conn.execute_batch("COMMIT;") {
            Ok(()) => Ok(value),
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK;");
                Err(CcError::Database(format!("commit {what}: {e}")))
            }
        },
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK;");
            Err(e)
        }
    }
}

impl IndexDb {
    /// Atomically claim the oldest ready `pending` task of the single active
    /// semantic space for `owner` (diagnostics only — fencing judges by the
    /// returned per-attempt token). `Ok(None)` means "nothing to do": either
    /// semantic is not configured (no `active` space — full no-op, the default
    /// build path pays nothing) or the queue is empty / all tasks are in
    /// backoff. The lease expires at (caller clock) + `lease_secs`; renew it
    /// with [`Self::renew_semantic_lease`].
    pub fn claim_semantic(&self, owner: &str, lease_secs: f64) -> CcResult<Option<ClaimedTask>> {
        self.claim_semantic_ordered(owner, lease_secs, ClaimFairness::default())
    }

    /// [`Self::claim_semantic`] with the injectable claim fairness policy
    /// (P7-005, 接线轮待办 8): the worker picks the candidate order —
    /// arrival FIFO (default, unchanged semantics) or doc rotation across a
    /// continuously re-edited hot doc. Same IMMEDIATE short transaction, same
    /// fencing.
    pub fn claim_semantic_ordered(
        &self,
        owner: &str,
        lease_secs: f64,
        fairness: ClaimFairness,
    ) -> CcResult<Option<ClaimedTask>> {
        let conn = self.write_conn.lock().map_err(db_err)?;
        queue_txn(&conn, "claim", |conn| {
            let Some(space_id) = semantic_outbox::active_space_on(conn)? else {
                return Ok(None);
            };
            semantic_outbox::claim_next_fair_on(
                conn,
                &space_id,
                owner,
                semantic_outbox::now_unix(),
                lease_secs,
                fairness,
            )
        })
    }

    /// Heartbeat: extend the caller's lease to (caller clock) + `lease_secs`.
    /// Only the current token holder succeeds; `Ok(false)` is "lease lost"
    /// (reclaimed + re-claimed by someone else, superseded, or already done)
    /// and writes nothing. Auxiliary: no epoch moves.
    pub fn renew_semantic_lease(
        &self,
        task_id: i64,
        token: &str,
        lease_secs: f64,
    ) -> CcResult<bool> {
        let conn = self.write_conn.lock().map_err(db_err)?;
        queue_txn(&conn, "renew", |conn| {
            semantic_outbox::renew_lease_on(
                conn,
                task_id,
                token,
                semantic_outbox::now_unix(),
                lease_secs,
            )
        })
    }

    /// Hand a failed attempt back through the fenced retry machine: backoff
    /// to `pending` while attempts remain, terminal `failed` once the budget
    /// is exhausted. Either way the lease is cleared (the caller loses all
    /// credentials) and `err` persists into `last_error` for audit.
    /// `Ok(false)` is "lease lost" — a stale token wins nothing.
    pub fn retry_semantic_task(
        &self,
        task_id: i64,
        token: &str,
        err: &str,
        backoff_secs: f64,
        max_attempts: u32,
    ) -> CcResult<bool> {
        let conn = self.write_conn.lock().map_err(db_err)?;
        queue_txn(&conn, "retry", |conn| {
            semantic_outbox::retry_on(
                conn,
                task_id,
                token,
                err,
                semantic_outbox::now_unix(),
                backoff_secs,
                max_attempts,
            )
        })
    }

    /// Third-party reclaim of every expired lease back to `pending`
    /// (immediately claimable, attempt budget not consumed). Returns the
    /// number of reclaimed tasks. The periodic bounded scan orchestration
    /// itself stays P6-015; the worker calls this opportunistically before a
    /// drain so tasks stranded by a crashed worker are not waited on.
    pub fn reclaim_expired_semantic(&self) -> CcResult<usize> {
        let conn = self.write_conn.lock().map_err(db_err)?;
        queue_txn(&conn, "reclaim", |conn| {
            semantic_outbox::reclaim_expired_on(conn, semantic_outbox::now_unix())
        })
    }
}
