//! Crash-recovery scan (P6-015): one BOUNDED, deterministic pass over the
//! persistent crash residue of the semantic pipeline — caller-driven, no
//! resident process.
//!
//! Boundary authority: `docs/adr/0003-semantic-persistence-single-db-boundary.md`
//! (row P6-015: "对每个持久化边界真实 kill/restart 验证；恢复有界且可复算；已存
//! artifact 优先复用"; lines 60-62/104-107: "artifact durable 在先、manifest
//! CAS 在后；两存储间不存在原子提交，任一 crash 点由幂等 recovery 重放，重复
//! ack 被 fencing 吸收") and the TASK-BRIEFS P6-015 crash-point table. This
//! module is the recovery-side orchestration; the table's crash points map
//! onto it as follows:
//!
//! | crash 点 | 残态 | 本模块的恢复动作 |
//! |---|---|---|
//! | outbox 提交前 | 无痕 | 无需动作（事务回滚） |
//! | artifact put 中 | cache 半文件/tmp | 读时 checksum 失败 → `Corrupt` → 同 Miss 交还 worker；物理清扫归 P6-016 GC |
//! | put 后、CAS 前 | cache 有、manifest 无、任务 claimed | lease 过期 → 有界 reclaim → 重放循环 `cache.get` 命中即**已存 artifact 优先复用**，走完整 P6-011 五重 fence CAS 发布，0 provider 调用 |
//! | CAS 后、ack 前 | — | **不可达**（P6-011：manifest 写与 ack 同事务原子）；其"幂等重放吸收"由本模块恢复侧验证：对等值已发布内容的重放产生 `visible_set_changed=false`（Q4 零 bump），计数进 [`RecoveryReport::replay_absorbed`] |
//! | lease 持有中 crash | 任务 claimed 将过期 | 过期归还 `pending`（有界 [`cc_db::IndexDb::reclaim_expired_semantic_bounded`]，不耗 attempt） |
//! | 换库 rename 中 | 旧库/新库并存 | fence 先行（P6-014 [`IncarnationFreshness`]）→ [`RecoveryVerdict::Fenced`] 零写入 |
//!
//! 口径（恢复侧甄别规则，全部可复算）：
//!
//! - **孤儿任务**：`claimed` 且 `lease_expires_at` 已过期。进程死亡在本设计
//!   中唯一可观测信号就是 lease 过期（无 liveness registry、无进程表——
//!   ADR 红线"不引入隐式常驻服务"），因此"过期回收"即"孤儿回收"。未过期
//!   lease 的持有者视为存活，P6-007 结构上禁止回收（reclaim 只匹配过期窗口）。
//! - **failed 死信**：终态。恢复扫描只清点（[`RecoveryReport::dead_letters`]）
//!   **绝不复活**——attempt 预算是有意耗尽的，静默重开会绕过费用策略
//!   （P6-018）；重置/补偿归费用策略与显式任务侧动作，不归恢复路径。
//! - **cache Miss/Corrupt**：不是恢复可吸收的残态——worker 是唯一付费方，
//!   交还 `pending`（fenced retry）后归 worker 消费。
//!
//! Boundedness contract (task 红线"单次调用有界返回，调用方循环驱动"): one
//! [`recover_scan`] call = 1 page-capped reclaim (≤ `scan_batch`) + 1
//! page-capped replay pass (≤ `scan_batch` claims) + 1 dead-letter census.
//! [`RecoveryReport::converged`] tells the caller whether to drive another
//! round; the residue therefore converges over caller-driven rounds no
//! matter how large the stranded backlog. Nothing here spawns a thread,
//! timer, or daemon — every epoch effect is still owned by the publish CAS
//! (P6-011); the scan itself is Auxiliary end to end.
//!
//! Composition with P6-014: an interrupted `reconcile_after_rebuild` pass
//! leaves exactly the residue this module absorbs (re-enqueued desired
//! tasks, possibly one claimed mid-reuse-loop). Recovery re-runs the same
//! reuse-or-requeue logic under the same incarnation fence, so a half-done
//! reconcile converges without a second provider cost. The P6-014 `seen`
//! guard convention is preserved: a task revisited within one pass means
//! nothing new is claimable and the pass ends (bounded).

use std::collections::HashSet;

use cc_db::index_db::IndexDb;
use cc_db::semantic_outbox::OutboxOp;
use cc_db::semantic_publish::PublishRequest;
use cc_db::semantic_rebuild::IncarnationFreshness;
use cc_model::CcResult;

use crate::cache::{ArtifactCache, CacheRead};
use crate::spec::VectorSpace;
use crate::types::{ArtifactRef, DocSpecDigest, InputDigest};

/// Tunables of one recovery scan. Defaults suit a runtime pass; tests pass
/// tighter bounds (small `scan_batch` to exercise multi-round convergence,
/// zero backoff to stress the revisit guard).
#[derive(Debug, Clone, PartialEq)]
pub struct RecoveryOptions {
    /// Per-call page cap shared by the reclaim scan and the replay pass:
    /// at most this many leases reclaimed and this many tasks claimed per
    /// call. Must be >= 1.
    pub scan_batch: usize,
    /// Lease length for claims taken during the replay pass.
    pub lease_secs: f64,
    /// Backoff applied when a task is handed back to the worker (cache
    /// miss / corrupt / revoke / CAS refusal already handled inside the CAS).
    pub retry_backoff_secs: f64,
    /// Attempt budget forwarded to the fenced retry on hand-back (the
    /// counters live on the outbox row, shared with the worker's budget).
    /// The cache-miss/corrupt hand-back uses the no-attempt direct write
    /// instead and never consumes this budget; only the revisit-guard and
    /// revoke-op hand-backs (and the replay CAS's fenced retry) do.
    pub max_attempts: u32,
}

impl Default for RecoveryOptions {
    fn default() -> Self {
        Self {
            scan_batch: 64,
            lease_secs: 30.0,
            retry_backoff_secs: 1.0,
            max_attempts: 3,
        }
    }
}

/// What one recovery scan did. Every counter is bounded by [`RecoveryOptions::scan_batch`]
/// except `dead_letters` (a read-only census, no mutation).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    /// Expired leases returned to `pending` this call (the orphan-triage
    /// action: claimed + expired = the only observable form of a dead
    /// worker's task; attempts not consumed).
    pub reclaimed: usize,
    /// `true` when the reclaim class is drained — no further expired lease
    /// exists (decided inside the reclaim transaction).
    pub reclaim_exhausted: bool,
    /// Interrupted publishes replayed from an already-durable artifact
    /// through the full P6-011 five-fence CAS — zero provider cost ("已存
    /// artifact 优先复用").
    pub replayed: usize,
    /// Of the replays, how many were duplicate acks of identical content
    /// (`visible_set_changed == false`, Q4: no epoch bump) — the recovery
    /// -side proof that a replayed publish is absorbed idempotently.
    pub replay_absorbed: usize,
    /// Tasks handed back to the worker: cache miss / corrupt object / revoke
    /// op. A CAS refusal does not count here (it wrote its own fenced retry
    /// inside the CAS transaction and left the row settled).
    pub requeued: usize,
    /// Terminal `failed` rows counted for the active space. Dead letters are
    /// NEVER resurrected by recovery (口径见模块文档).
    pub dead_letters: u64,
    /// `true` when this call leaves no recoverable residue unexamined and
    /// made no recovery progress — the caller may stop driving rounds. A
    /// `false` value means: call again (the batch caps were hit, or reclaim
    /// reported more expired leases).
    pub converged: bool,
}

/// Outcome of one recovery scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryVerdict {
    /// The scan ran; see [`RecoveryReport`].
    Applied(RecoveryReport),
    /// The authoritative path carries a different incarnation than the
    /// caller's snapshot: this process is a ghost of the pre-swap database
    /// and was fenced before touching anything (P6-014).
    Fenced {
        snapshot: [u8; 16],
        path_incarnation: [u8; 16],
    },
}

fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Hand one still-claimed task back through the fenced retry (the recovery
/// twin of the worker loop's / reconcile's hand-back).
fn hand_back(
    db: &IndexDb,
    task: &cc_db::semantic_outbox::ClaimedTask,
    reason: &str,
    options: &RecoveryOptions,
) -> CcResult<()> {
    db.retry_semantic_task(
        task.task_id,
        &task.token,
        reason,
        options.retry_backoff_secs,
        options.max_attempts,
    )?;
    Ok(())
}

/// Replay one interrupted publish through the full P6-011 CAS. The
/// artifact-before-manifest order holds by construction: the `cache.get`
/// that produced `artifact_ref` verified the addressing triple, the blake3
/// payload checksum and the frozen space — the artifact is ALREADY durable
/// (it survived the crash that interrupted the original publisher), so the
/// replay does no `put` and no provider call. Mirrors
/// `reconcile::publish_cached` (kept local: the P6-014 file is a frozen
/// deliverable).
fn publish_replay(
    db: &IndexDb,
    task: &cc_db::semantic_outbox::ClaimedTask,
    artifact_ref: &ArtifactRef,
    space_id: &str,
    expected_incarnation: [u8; 16],
    options: &RecoveryOptions,
) -> CcResult<cc_db::semantic_publish::PublishOutcome> {
    let request = PublishRequest {
        task_id: task.task_id,
        lease_token: &task.token,
        doc_key: &task.doc_key,
        doc_version: &task.doc_version,
        input_digest: &task.input_digest,
        space_id,
        artifact_ref: artifact_ref.as_str(),
        expected_incarnation,
        retry_backoff_secs: options.retry_backoff_secs,
        max_attempts: options.max_attempts,
        now_unix: now_unix_secs() as f64,
    };
    db.publish_semantic(&request)
}

/// One bounded recovery scan (see the module docs for the crash-point table
/// and the triage 口径).
///
/// `expected_incarnation` is the caller's startup snapshot of
/// `ReadGeneration.incarnation`; `cache`/`space`/`doc_spec` must be the
/// composition root's frozen semantic configuration — the SAME cache
/// namespace the original publisher used (Q5: the namespace never contained
/// the incarnation), or every replay lookup misses and the residue falls
/// back to the worker (correct, but re-pays).
pub fn recover_scan(
    db: &IndexDb,
    cache: &ArtifactCache,
    space: &VectorSpace,
    doc_spec: &DocSpecDigest,
    expected_incarnation: [u8; 16],
    options: &RecoveryOptions,
) -> CcResult<RecoveryVerdict> {
    if options.scan_batch == 0 {
        return Err(cc_model::CcError::InvalidParams(
            "recovery scan_batch must be at least 1".into(),
        ));
    }
    // Fence FIRST (crash-point row "换库 rename 中"): a ghost process must
    // not reclaim, claim, or publish anything — every write it could attempt
    // would land in the detached old inode (P6-014).
    if let IncarnationFreshness::Stale {
        snapshot,
        path_incarnation,
    } = db.semantic_incarnation_freshness(expected_incarnation)?
    {
        return Ok(RecoveryVerdict::Fenced {
            snapshot,
            path_incarnation,
        });
    }
    space.validate()?;
    let space_digest = space.digest()?;

    // The replay pass publishes under fence 5 (space active); validating the
    // frozen space against the single active row up front says a
    // configuration error out loud instead of N per-task rejections (same
    // convention as reconcile).
    let Some(active) = db.semantic_active_space()? else {
        // Semantic not configured: nothing was ever claimed, nothing to
        // recover, nothing is paid.
        return Ok(RecoveryVerdict::Applied(RecoveryReport::default()));
    };
    if active != space_digest.as_str() {
        return Err(cc_model::CcError::InvalidParams(format!(
            "recovery space {} is not the active space {active}",
            space_digest.as_str()
        )));
    }

    // 1. Orphan triage, bounded: claimed + expired = a dead worker's task.
    //    One page per call; `exhausted` drives the caller's loop.
    let bounded = db.reclaim_expired_semantic_bounded(options.scan_batch)?;
    let mut report = RecoveryReport {
        reclaimed: bounded.reclaimed,
        reclaim_exhausted: bounded.exhausted,
        ..RecoveryReport::default()
    };

    // 2. Replay pass, bounded: absorb interrupted publishes whose artifact
    //    is already durable. The P6-014 `seen` guard convention bounds the
    //    pass against zero-backoff re-claims; hitting the batch cap or the
    //    revisit guard leaves `converged = false` so the caller drives
    //    another round.
    let mut seen: HashSet<i64> = HashSet::new();
    let mut queue_exhausted = false;
    for _ in 0..options.scan_batch {
        let Some(task) = db.claim_semantic("semantic-recovery", options.lease_secs)? else {
            queue_exhausted = true;
            break;
        };
        if !seen.insert(task.task_id) {
            hand_back(
                db,
                &task,
                "recovery: revisit guard, leaving the task to the worker",
                options,
            )?;
            break;
        }
        if task.op != OutboxOp::Embed {
            hand_back(
                db,
                &task,
                "recovery: revoke tasks are not consumable here (P6-017)",
                options,
            )?;
            report.requeued += 1;
            continue;
        }
        let input = InputDigest::new(task.input_digest.clone());
        // A replay can publish an aged object without a re-put. Protect the
        // verified read through CAS, not merely a freshness timestamp.
        let _mutation = cache.lock_mutation()?;
        match cache.get(space, &input, doc_spec)? {
            CacheRead::Hit(hit) => {
                let outcome = publish_replay(
                    db,
                    &task,
                    &hit.artifact_ref,
                    space_digest.as_str(),
                    expected_incarnation,
                    options,
                )?;
                if outcome.published {
                    report.replayed += 1;
                    report.replay_absorbed += usize::from(!outcome.visible_set_changed);
                } else {
                    // CAS refusal: the rejection already wrote its own fenced
                    // retry inside the CAS transaction — the row is settled.
                    report.requeued += 1;
                }
            }
            CacheRead::Miss | CacheRead::Corrupt(_) => {
                // Review action ② (batch-3 closure): a cache miss is not the
                // task's fault, so the hand-back must not spend the worker's
                // attempt budget. The fenced no-attempt direct write returns
                // the row to `pending` immediately claimable — no dead-letter
                // decision, no backoff, attempts untouched (the fenced retry
                // below remains only for the revisit-guard and revoke-op
                // hand-backs, whose attempt accounting is deliberate).
                db.hand_back_semantic_task(
                    task.task_id,
                    &task.token,
                    "recovery: paid artifact not reusable from cache, requeued for the worker",
                )?;
                report.requeued += 1;
            }
        }
    }

    // 3. Dead-letter census (read-only, never a resurrection).
    report.dead_letters = db.semantic_dead_letter_count()?;

    // Convergence: the reclaim class drained AND this pass either drained
    // the instantly-claimable queue or did nothing recoverable — a no-op
    // round is the steady-state signal (normal worker backlog is not crash
    // residue and must not keep the loop spinning).
    report.converged = report.reclaim_exhausted
        && (queue_exhausted || (report.reclaimed == 0 && report.replayed == 0));
    Ok(RecoveryVerdict::Applied(report))
}
