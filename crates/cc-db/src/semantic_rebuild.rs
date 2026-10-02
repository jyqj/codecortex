//! Semantic rebuild protocol (P6-014): the authoritative-path incarnation
//! fence and the post-rebuild desired-set re-enqueue facade.
//!
//! Boundary authority: `docs/adr/0003-semantic-persistence-single-db-boundary.md`
//! (row P6-014: "重建换 incarnation 后从 artifact cache 补 manifest（付费产物
//! 保留），旧 DB 时代回包被 fencing 拒绝"; lines 120-124: "换库（staging 重建后
//! rename）改变 incarnation，必须围栏住仍持有旧连接的另一进程……权威判定只认
//! strict `ReadGeneration`").
//!
//! ## The cross-process gap this module closes
//!
//! The P6-011 publish CAS fence 1 compares the incarnation read *on the
//! transaction's own connection* against the worker's startup snapshot. That
//! covers every same-file swap (the swap reopens the connections of the
//! rebuilding instance). It cannot cover one scenario, recorded verbatim in
//! the P6-011 delivery notes (§8): **another process** that opened the
//! database before the swap keeps file descriptors to the OLD inode after the
//! rename. Its own connections keep reporting the old incarnation, so a
//! fence that only reads "the incarnation on this connection" passes — and
//! the publish would silently land in a detached ghost file while the
//! authoritative database stays untouched.
//!
//! The authoritative judgment therefore reads the incarnation from a FRESH
//! read-only connection opened at [`IndexDb`]'s canonical path
//! ([`IndexDb::semantic_incarnation_freshness`]): the path is the authority,
//! a fresh open cannot be a ghost. This is strict `ReadGeneration` only —
//! never the legacy dual-clock reader (ADR lines 120-124).
//!
//! Facades:
//!
//! - [`IndexDb::publish_semantic_fenced`] — refuses with
//!   [`PublishRejection::IncarnationMismatch`] BEFORE any transaction when
//!   the authoritative path carries a different incarnation than the
//!   caller's snapshot. Deviation from the P6-011 rejection semantics,
//!   deliberate: no fenced retry is written anywhere, because the only file
//!   this process could write is the ghost — writing queue state there would
//!   be worse than losing it.
//! - [`IndexDb::claim_semantic_fenced`] — `Ok(None)` for a ghost process
//!   ("this process must not do work"), zero writes; `Ok(None)` from an
//!   empty queue is indistinguishable by design (a worker that suspects a
//!   swap asks the freshness check directly).
//! - [`IndexDb::enqueue_semantic_rebuild_plan`] — the P6-006 deviation-6
//!   hand-over: the full-rebuild paths do not hook the outbox (the staging
//!   database has an empty `semantic_spaces`, so the hook is a structural
//!   no-op there), and the post-swap desired set is re-enqueued HERE instead:
//!   one `IMMEDIATE` short transaction running the frozen
//!   [`supersede_and_enqueue_on`](crate::semantic_outbox::supersede_and_enqueue_on),
//!   declaring the `Semantic` effect exactly when the plan changed semantic
//!   state (P6-006 convention: any nonzero stat is a semantic change). The
//!   re-derivation of WHAT to enqueue (`document_manifest × active space`)
//!   belongs to the cc-semantic reconcile side; this facade only applies a
//!   plan under the write lock.

use cc_model::generation::ReadGeneration;
use cc_model::{CcError, CcResult};

use crate::index_db::IndexDb;
use crate::read_generation;
use crate::semantic_outbox::{self, ClaimedTask, OutboxPlan, OutboxUpsert, OutboxWriteStats};
use crate::semantic_publish::{PublishOutcome, PublishRejection, PublishRequest};
use crate::sql_util::db_err;

/// Minimal read model of `document_manifest.record_json` for the desired-set
/// projection: the embedded-input hash (P6-003 `InputDigest`) — the same
/// one-digest field subset discipline as the P6-011 input fence, never
/// coupled to record-schema evolution.
#[derive(serde::Deserialize)]
struct RecordInputPeek {
    #[serde(default)]
    input: Option<InputHashPeek>,
}

#[derive(serde::Deserialize)]
struct InputHashPeek {
    input_hash: String,
}

/// Whether this process still matches the database at the authoritative
/// path. `Stale` is the P6-014 ghost verdict: the caller's snapshot belongs
/// to a database generation that is no longer the one at the path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncarnationFreshness {
    /// The authoritative path carries the caller's snapshot incarnation.
    Current,
    /// The authoritative path was swapped/rebuilt under this process: the
    /// snapshot is from the old generation. Every semantic write this
    /// process could attempt would land in the detached old inode.
    Stale {
        snapshot: [u8; 16],
        path_incarnation: [u8; 16],
    },
}

impl IndexDb {
    /// Strict `ReadGeneration` read from a FRESH read-only connection at the
    /// authoritative database path. Unlike the pooled read path this can
    /// never be a ghost: the path is the authority, so a swap/rebuild that
    /// renamed a new file into place is visible here even for a process
    /// whose pooled connections still reference the old inode.
    ///
    /// busy_timeout fail-stop 方向行为（批次 3 收口注明）：该连接未配置
    /// busy_timeout（SQLite 默认零等待），权威路径上的并发写锁（如 rebuild
    /// staging rename 的提交窗口）会使本次读以 `SQLITE_BUSY` 类错误即时失败
    /// 上抛——fence 宁可 fail-stop（拒绝本次 freshness 判定/发布，错误按可
    /// 重试处理），也绝不退化为无权威读的放行；这不是数据不一致信号。
    fn generation_at_path(&self) -> CcResult<ReadGeneration> {
        let conn = rusqlite::Connection::open_with_flags(
            &self.db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|e| CcError::Database(format!("open authoritative db path: {e}")))?;
        read_generation::read_on(&conn)
    }

    /// Compare the caller's startup incarnation snapshot against the
    /// authoritative path (P6-014 fence input). See the module docs for why
    /// the pooled read generation is not sufficient.
    pub fn semantic_incarnation_freshness(
        &self,
        expected: [u8; 16],
    ) -> CcResult<IncarnationFreshness> {
        let path_incarnation = self.generation_at_path()?.incarnation;
        Ok(if path_incarnation == expected {
            IncarnationFreshness::Current
        } else {
            IncarnationFreshness::Stale {
                snapshot: expected,
                path_incarnation,
            }
        })
    }

    /// [`IndexDb::publish_semantic`] behind the authoritative-path fence:
    /// a process whose snapshot incarnation no longer matches the database
    /// at the path is refused with [`PublishRejection::IncarnationMismatch`]
    /// BEFORE any transaction begins — zero writes anywhere (a ghost's only
    /// writable file is the detached old inode, and queue state written
    /// there is lost; see module docs).
    ///
    /// TOCTOU 边界声明（批次 3 收口注明）：fence 读（权威路径 incarnation）
    /// 与发布事务之间不是原子的——两时刻之间发生的换库 rename 本调用不可见，
    /// 该窗口内的发布仍可能落入旧时代判定。在本仓库的 owner 串行集成模型下
    /// （rebuild 与 publish 由同一 owner 编排、串行推进）该窗口可接受；脱离
    /// 该模型的调用方须自行在外层串行化 rebuild 与 publish，不得依赖本
    /// fence 提供跨进程互斥。
    pub fn publish_semantic_fenced(&self, req: &PublishRequest<'_>) -> CcResult<PublishOutcome> {
        if let IncarnationFreshness::Stale { .. } =
            self.semantic_incarnation_freshness(req.expected_incarnation)?
        {
            return Ok(PublishOutcome {
                published: false,
                visible_set_changed: false,
                rejection: Some(PublishRejection::IncarnationMismatch),
            });
        }
        self.publish_semantic(req)
    }

    /// [`IndexDb::claim_semantic`] behind the authoritative-path fence: a
    /// ghost process gets `Ok(None)` — it must not claim work — with zero
    /// writes. `Ok(None)` therefore means "nothing to do OR fenced"; a
    /// caller that must distinguish the two asks
    /// [`IndexDb::semantic_incarnation_freshness`] directly.
    pub fn claim_semantic_fenced(
        &self,
        owner: &str,
        lease_secs: f64,
        expected: [u8; 16],
    ) -> CcResult<Option<ClaimedTask>> {
        if let IncarnationFreshness::Stale { .. } = self.semantic_incarnation_freshness(expected)? {
            return Ok(None);
        }
        self.claim_semantic(owner, lease_secs)
    }

    /// The single active semantic space (read-only resolve; `None` = semantic
    /// not configured). The reconcile side reads this to validate its frozen
    /// space before paying for any desired-set projection.
    pub fn semantic_active_space(&self) -> CcResult<Option<String>> {
        let conn = self.read_conn()?;
        semantic_outbox::active_space_on(&conn)
    }

    /// Project the post-rebuild desired set from the rebuilt document
    /// manifest: every embeddable row (`encoding_key IS NOT NULL`) with its
    /// current `doc_version` and embedded-input digest, newest projection
    /// only (one row per `doc_key` by primary key), in stable `doc_key`
    /// order. Render-failed rows (no embeddable input in `record_json`) are
    /// skipped — the P6-006 planner convention: a task without an input
    /// digest would be unactionable. Read-only; deterministic; unbounded by
    /// design at this layer (the caller bounds the pass, P6-015 owns the
    /// bounded-recovery budgets).
    pub fn semantic_rebuild_desired_set(&self) -> CcResult<Vec<OutboxUpsert>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare_cached(
                "SELECT doc_key, doc_version, record_json FROM document_manifest \
                 WHERE encoding_key IS NOT NULL ORDER BY doc_key",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| {
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

    /// Re-enqueue the post-rebuild desired set (P6-006 deviation-6
    /// hand-over): one `IMMEDIATE` short transaction running the frozen
    /// [`supersede_and_enqueue_on`](crate::semantic_outbox::supersede_and_enqueue_on)
    /// over caller-derived upserts, bumping `semantic_epoch` exactly when
    /// the plan changed semantic state (P6-006 effect convention; a pure
    /// re-run that enqueues nothing — e.g. no active space — is Auxiliary:
    /// no clock moves, the key is never created).
    ///
    /// This is the ONLY sanctioned outbox write on the rebuild path: the
    /// rebuild itself hooks nothing (empty `semantic_spaces` in the staging
    /// database makes the P6-006 hook a structural no-op there), and the
    /// swap drops the old queue with the old file — the desired set is
    /// re-derived from the rebuilt `document_manifest` by the cc-semantic
    /// reconcile side and applied through this facade.
    pub fn enqueue_semantic_rebuild_plan(
        &self,
        upserts: &[OutboxUpsert],
    ) -> CcResult<OutboxWriteStats> {
        let conn = self.write_conn.lock().map_err(db_err)?;
        conn.execute_batch("BEGIN IMMEDIATE;")
            .map_err(|e| CcError::Database(format!("begin rebuild re-enqueue: {e}")))?;
        let outcome = (|| -> CcResult<OutboxWriteStats> {
            let removals: Vec<String> = Vec::new();
            let stats = semantic_outbox::supersede_and_enqueue_on(
                &conn,
                &OutboxPlan {
                    upserts,
                    removals: &removals,
                    now_unix: semantic_outbox::now_unix(),
                },
            )?;
            if stats.changed_semantic_state() {
                IndexDb::bump_semantic_epoch_on(&conn)?;
            }
            Ok(stats)
        })();
        match (outcome, conn.execute_batch("COMMIT;")) {
            (Ok(stats), Ok(())) => Ok(stats),
            (Ok(_), Err(e)) => {
                let _ = conn.execute_batch("ROLLBACK;");
                Err(CcError::Database(format!("commit rebuild re-enqueue: {e}")))
            }
            (Err(e), _) => {
                let _ = conn.execute_batch("ROLLBACK;");
                Err(e)
            }
        }
    }
}
