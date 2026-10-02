//! Crash-recovery primitives (P6-015): the BOUNDED scans the recovery
//! orchestration drives — a page-capped reclaim of expired leases, the
//! dead-letter census, and the keyset-paginated desired-set projection that
//! the P6-014 unbounded projection deferred to this task.
//!
//! Boundary authority: `docs/adr/0003-semantic-persistence-single-db-boundary.md`
//! (row P6-015: "对每个持久化边界真实 kill/restart 验证；恢复有界且可复算；已存
//! artifact 优先复用") and the batch hand-overs: P6-007 delivery note
//! ("`reclaim_expired_on` 无界扫描有界化归 P6-015") and P6-014 deviation 7
//! ("`semantic_rebuild_desired_set` 全表投影无行数上限……P6-015 owns the
//! bounded-recovery budgets").
//!
//! Red-line discipline: the frozen P6-006/007/013 primitives
//! ([`crate::semantic_outbox::reclaim_expired_on`],
//! [`crate::semantic_queue`]) are NOT touched. This module adds parallel
//! bounded variants with IDENTICAL row transformations — same closed
//! `claimed → pending` transition, same lease clearing, same attempt-count
//! preservation, Auxiliary end to end (no epoch ever moves). The unbounded
//! originals keep their semantics for existing callers.
//!
//! Boundedness contract: every call touches at most `limit` rows and returns
//! a bounded answer (`exhausted` tells the caller whether the class is
//! drained); the caller — the cc-semantic recovery orchestration — loops
//! until `exhausted`. No resident process, no timer: recovery is an explicit
//! caller-driven pass.

use cc_model::{CcError, CcResult};

use crate::index_db::IndexDb;
use crate::semantic_outbox::{self, OutboxState, OutboxUpsert};
use crate::sql_util::db_err;

/// One bounded [`IndexDb::reclaim_expired_semantic_bounded`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundedReclaim {
    /// Expired leases returned to `pending` by THIS call (at most `limit`).
    pub reclaimed: usize,
    /// `true` when no further expired lease remains — the caller may stop
    /// driving the reclaim class. `false` means more expired rows exist and
    /// the next call will reclaim them (the batch cap was hit).
    pub exhausted: bool,
}

/// RFC3339 text for `updated_at` (the same derivation as every other
/// semantic module — caller-supplied clock, no statement-level `now()`).
fn timestamp_text(now_unix: f64) -> CcResult<String> {
    let secs = now_unix.floor() as i64;
    let nanos = ((now_unix - secs as f64) * 1e9).round() as u32;
    chrono::DateTime::from_timestamp(secs, nanos)
        .map(|dt| dt.to_rfc3339())
        .ok_or_else(|| CcError::InvalidParams("semantic recovery timestamp out of range".into()))
}

/// Minimal read model of `document_manifest.record_json` for the bounded
/// desired-set projection — the same one-digest field subset as the P6-011
/// input fence and the P6-014 projection (never coupled to record-schema
/// evolution).
#[derive(serde::Deserialize)]
struct RecordInputPeek {
    #[serde(default)]
    input: Option<InputHashPeek>,
}

#[derive(serde::Deserialize)]
struct InputHashPeek {
    input_hash: String,
}

impl IndexDb {
    /// Bounded variant of [`crate::semantic_queue`]'s
    /// `reclaim_expired_semantic`: at most `limit` expired leases return to
    /// `pending` in one `IMMEDIATE` short transaction. Identical row
    /// transformation to the frozen [`semantic_outbox::reclaim_expired_on`]
    /// (state, `available_at = now`, lease columns cleared, attempts NOT
    /// consumed), page-capped by `task_id` order so a large stranded backlog
    /// converges over caller-driven rounds instead of one unbounded UPDATE.
    /// `exhausted` is decided inside the same transaction, so the pair
    /// `(reclaimed, exhausted)` is one consistent observation.
    ///
    /// Auxiliary: no epoch ever moves (P6-004 taxonomy).
    pub fn reclaim_expired_semantic_bounded(&self, limit: usize) -> CcResult<BoundedReclaim> {
        if limit == 0 {
            return Err(CcError::InvalidParams(
                "semantic bounded reclaim requires limit >= 1".into(),
            ));
        }
        debug_assert!(OutboxState::Claimed.can_transition_to(OutboxState::Pending));
        let conn = self.write_conn.lock().map_err(db_err)?;
        conn.execute_batch("BEGIN IMMEDIATE;")
            .map_err(|e| CcError::Database(format!("begin bounded reclaim: {e}")))?;
        let outcome = (|| -> CcResult<BoundedReclaim> {
            let now = semantic_outbox::now_unix();
            let ts = timestamp_text(now)?;
            // The page subselect makes the mutation self-advancing: reclaimed
            // rows leave the `claimed` predicate, so the next call pages past
            // them without a cursor.
            let reclaimed = conn
                .prepare_cached(
                    "UPDATE semantic_outbox \
                     SET state='pending', available_at=?1, lease_token=NULL, \
                         lease_expires_at=NULL, claim_owner=NULL, updated_at=?2 \
                     WHERE task_id IN (SELECT task_id FROM semantic_outbox \
                                       WHERE state='claimed' AND lease_expires_at<?1 \
                                       ORDER BY task_id LIMIT ?3)",
                )
                .map_err(db_err)?
                .execute(rusqlite::params![now, ts, limit as i64])
                .map_err(db_err)?;
            let more: i64 = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM semantic_outbox \
                     WHERE state='claimed' AND lease_expires_at<?1)",
                    rusqlite::params![now],
                    |r| r.get(0),
                )
                .map_err(db_err)?;
            Ok(BoundedReclaim {
                reclaimed,
                exhausted: more == 0,
            })
        })();
        match (outcome, conn.execute_batch("COMMIT;")) {
            (Ok(value), Ok(())) => Ok(value),
            (Ok(_), Err(e)) => {
                let _ = conn.execute_batch("ROLLBACK;");
                Err(CcError::Database(format!("commit bounded reclaim: {e}")))
            }
            (Err(e), _) => {
                let _ = conn.execute_batch("ROLLBACK;");
                Err(e)
            }
        }
    }

    /// Fenced hand-back WITHOUT consuming the attempt budget (P6 batch-3
    /// closure, review action ②): the claimed→pending direct write the
    /// recovery cache-miss path needs. One CAS statement —
    /// `WHERE task_id=? AND lease_token=? AND state='claimed'` — so a stale
    /// token wins nothing: `Ok(false)` is "lease lost" with rowcount=0, i.e.
    /// a ZERO-WRITE no-op. The row returns to `pending` immediately
    /// claimable (`available_at = now`, lease cleared), the hand-back reason
    /// persists into `last_error` for audit, and — unlike the frozen fenced
    /// retry ([`IndexDb::retry_semantic_task`]) — there is deliberately NO
    /// dead-letter decision and NO attempt consumption: a cache miss is not
    /// the task's fault, so the attempt budget stays fully with the worker
    /// (the next claim still increments the counter — that is the worker's
    /// own attempt, not this primitive's).
    ///
    /// Auxiliary: no epoch ever moves (P6-004 taxonomy).
    pub fn hand_back_semantic_task(
        &self,
        task_id: i64,
        token: &str,
        reason: &str,
    ) -> CcResult<bool> {
        debug_assert!(OutboxState::Claimed.can_transition_to(OutboxState::Pending));
        let conn = self.write_conn.lock().map_err(db_err)?;
        let now = semantic_outbox::now_unix();
        let ts = timestamp_text(now)?;
        let updated = conn
            .prepare_cached(
                "UPDATE semantic_outbox \
                 SET state='pending', available_at=?1, lease_token=NULL, \
                     lease_expires_at=NULL, claim_owner=NULL, last_error=?2, updated_at=?3 \
                 WHERE task_id=?4 AND lease_token=?5 AND state='claimed'",
            )
            .map_err(db_err)?
            .execute(rusqlite::params![now, reason, ts, task_id, token])
            .map_err(db_err)?;
        Ok(updated == 1)
    }

    /// Dead-letter census for the single active space (read-only).
    ///
    /// 口径 (P6-015 recovery): terminal `failed` rows are NEVER resurrected
    /// by recovery — the attempt budget was exhausted deliberately and a
    /// silent reopen would bypass the cost policy (P6-018). Recovery only
    /// makes the backlog observable; counters surface through the P6-012
    /// coverage reads and this census. No active space → 0 (nothing is
    /// trackable for a space that does not exist).
    pub fn semantic_dead_letter_count(&self) -> CcResult<u64> {
        let conn = self.read_conn()?;
        let Some(space_id) = semantic_outbox::active_space_on(&conn)? else {
            return Ok(0);
        };
        let count: i64 = conn
            .prepare_cached(
                "SELECT count(*) FROM semantic_outbox WHERE space_id=?1 AND state='failed'",
            )
            .map_err(db_err)?
            .query_row(rusqlite::params![space_id], |r| r.get(0))
            .map_err(db_err)?;
        Ok(count.max(0) as u64)
    }

    /// Bounded, keyset-paginated variant of
    /// [`IndexDb::semantic_rebuild_desired_set`] (P6-014 deviation-7
    /// hand-over): the same embeddable-row projection (`encoding_key IS NOT
    /// NULL`, render-failed rows skipped — the P6-006 planner convention),
    /// one deterministic `doc_key`-ordered page of at most `limit` rows
    /// strictly after `after_doc_key` (empty string = first page). The
    /// caller loops until an empty page — keyset pagination, no OFFSET
    /// re-scan (same discipline as `semantic_manifest_reads` /
    /// `semantic_coverage::uncovered_on`). Read-only; deterministic;
    /// `limit == 0` is a caller bug and is rejected.
    ///
    /// 不变式前提（批次 3 收口注明，观察项 2）：`encoding_key` 非空的行必然
    /// 在 `record_json.input.input_hash` 内嵌输入摘要——这是 P6-006 写路径
    /// 口径（可嵌入 ⇒ 渲染成功 ⇒ 摘要存在），本投影不校验它。违反后果：
    /// 缺摘要的行与渲染失败行同样被静默跳过，该 doc_key 被排除在期望集之外
    /// ——重建 reconcile 不会为其重入队任务，形成 desired 集与 coverage
    /// 分母之间的不可见覆盖缺口，只能由下一轮全量 reconcile 或显式修复
    /// 动作闭合。写入方（P6-006）是本前提的唯一保证者。
    pub fn semantic_rebuild_desired_set_bounded(
        &self,
        after_doc_key: &str,
        limit: usize,
    ) -> CcResult<Vec<OutboxUpsert>> {
        if limit == 0 {
            return Err(CcError::InvalidParams(
                "semantic desired-set scan requires limit >= 1".into(),
            ));
        }
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT doc_key, doc_version, record_json FROM document_manifest \
                 WHERE encoding_key IS NOT NULL AND doc_key > ?1 \
                 ORDER BY doc_key LIMIT ?2",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(rusqlite::params![after_doc_key, limit as i64], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(db_err)?;
        let mut desired = Vec::new();
        for row in rows {
            let (doc_key, doc_version, record_json) = row.map_err(db_err)?;
            let input_hash = serde_json::from_str::<RecordInputPeek>(&record_json)
                .map_err(|e| CcError::Database(format!("document record_json unreadable: {e}")))?
                .input
                .map(|i| i.input_hash);
            if let Some(input_digest) = input_hash {
                desired.push(OutboxUpsert {
                    doc_key,
                    doc_version,
                    input_digest,
                });
            }
        }
        Ok(desired)
    }
}
