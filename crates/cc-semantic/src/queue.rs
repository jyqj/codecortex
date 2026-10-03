//! Worker queue primitives (P6-013): the explicit, type-driven claim loop
//! over the semantic outbox — RAII lease, admission bounds, continuous-edit
//! merge semantics. This is the P6-007 deviation-5 hand-over (`queue.rs`
//! LeaseGuard) fused with the P6-013 worker resource contract.
//!
//! Boundary authority: `docs/adr/0003-semantic-persistence-single-db-boundary.md`
//! (P6-013 row "pending 合并 + 队列上限 + 公平批次 + 有界关闭；旧任务
//! supersede；任何时刻本地查询可用"; 明确不做: "不引入独立队列服务、常驻进程
//! 或网络端点") and 02-CONTRACTS C11 ("网络调用前释放 RwLock、build gate、读池
//! 连接和 SQL transaction；关闭时有界等待并留下可恢复 outbox").
//!
//! ## No implicit resident process (ADR red line)
//!
//! The serial API spawns no threads; the parallel API uses joined scoped workers.
//! Neither API starts a timer or daemon. [`drain_pending`]
//! is an EXPLICIT, caller-driven drain: it claims at most
//! [`WorkerLimits::max_batch`] tasks, processes each through the caller's
//! handler, and returns a [`BatchReport`] — whether to drain again, when, and
//! from where is entirely the composition root's decision. C11's bounded
//! shutdown falls out structurally: there is no loop to stop and no claimed
//! task to abandon at module level; a caller that stops draining leaves its
//! last claims to the lease-expiry + reclaim recovery path, which is the
//! designed "可恢复 outbox".
//!
//! ## Continuous-edit merge semantics (定案)
//!
//! 1. **DB layer (P6-005/006, already delivered — this module only relies on
//!    it)**: the partial unique index `semantic_outbox_live_per_doc` admits
//!    at most ONE live (`pending`/`claimed`) task per `(doc_key, space_id)`,
//!    and every source-transaction write supersedes the doc's live tasks
//!    before inserting the fresh `pending` task at the newest `doc_version`.
//!    Three rapid edits of one document therefore coalesce into a single
//!    live task before the worker ever looks at it — the worker does ONE
//!    unit of provider work for the final version, never one per edit.
//! 2. **Claimed-task supersede**: a write that lands while a task is
//!    `claimed` flips it to terminal `superseded`. The in-flight worker's
//!    publish is fenced out by the P6-011 CAS (the row is no longer
//!    `claimed` under the presented token → `LeaseLost`), and the fenced
//!    retry inside the CAS no-ops against a `superseded` row — the old
//!    attempt can never resurrect or ack. The waste bound is exactly one
//!    already-started embed; the re-enqueued task at the new version serves
//!    the next claim.
//! 3. **Renew-before-work is the liveness gate**: every drain renews the
//!    lease immediately before invoking the handler. `Ok(false)` means the
//!    task was reclaimed/superseded/finished under another identity — the
//!    attempt is skipped with no provider call and no retry write.
//! 4. **No time-window debouncing**: the write path is never blocked or
//!    delayed for merge purposes (TASK-BRIEFS P6-013: "上限由 worker 消费
//!    速率兜底 + `available_at` backoff"); queue depth stays bounded by
//!    consumption, and `max_batch` bounds each drain's claim.
//!
//! ## Resource bounds
//!
//! [`WorkerLimits`] carries the admission budget: per-drain claim bound
//! (`max_batch`, the bounded-executor capacity), lease length (`lease_secs`,
//! 对接 P6-007 lease 到期), retry backoff and attempt budget. The provider
//! call runs strictly BETWEEN database transactions — no DB lock or
//! connection is held across it (C11) — and every outcome routes through the
//! fenced primitives, so an overrunning worker whose lease expired simply
//! loses its voice at the next fenced write.
//!
//! Epoch discipline: the entire module is Auxiliary. Nothing here ever bumps
//! a clock; only the publish CAS may declare the `Semantic` effect (P6-011).

use std::time::{SystemTime, UNIX_EPOCH};

use cc_db::index_db::IndexDb;
use cc_db::semantic_outbox::{ClaimFairness, ClaimedTask, OutboxOp};
use cc_model::{CcError, CcResult};

use crate::ports::{DocumentInput, EmbeddingProvider, ProviderError};
use crate::publish::{PublishVerdict, Publisher};

/// Admission and resource bounds of one worker (P6-013). Validated by
/// [`WorkerLimits::validated`] before use.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkerLimits {
    /// Claim upper bound of ONE [`drain_pending`] call (the bounded
    /// executor's capacity). The queue itself is bounded by consumption —
    /// the write path only supersedes + inserts and never blocks.
    pub max_batch: usize,
    /// Lease length in seconds; the claim expires at claim-time + this
    /// (P6-007 lease expiry). An undisposed claim outlives its worker only
    /// up to here — expiry + reclaim is the recovery path.
    pub lease_secs: f64,
    /// Backoff applied when a failed attempt is handed back to the queue
    /// (the task becomes claimable again at now + this).
    pub backoff_secs: f64,
    /// Attempts before the fenced retry dead-letters a task to terminal
    /// `failed` (never claimable through the queue again).
    pub max_attempts: u32,
    /// Claim fairness policy of this worker's drains (P7-005, 接线轮待办 8):
    /// the candidate ORDER BY of every claim, as a closed enum. Defaults to
    /// arrival FIFO (the P6-007 semantics) in [`WorkerLimits::validated`];
    /// the composition root may opt into doc rotation
    /// ([`ClaimFairness::DocRoundRobin`]) so a continuously re-edited doc
    /// yields to docs whose work has been waiting longer. Cross-PROJECT
    /// fairness is the shared provider gate's job (`crate::admission`), not
    /// this per-project queue's.
    pub claim_order: ClaimFairness,
}

impl WorkerLimits {
    /// Validated constructor: every bound must be structurally meaningful.
    pub fn validated(
        max_batch: usize,
        lease_secs: f64,
        backoff_secs: f64,
        max_attempts: u32,
    ) -> CcResult<Self> {
        let limits = Self {
            max_batch,
            lease_secs,
            backoff_secs,
            max_attempts,
            claim_order: ClaimFairness::default(),
        };
        if limits.max_batch == 0 {
            return Err(CcError::InvalidParams(
                "worker max_batch must be at least 1".into(),
            ));
        }
        if limits.lease_secs <= 0.0 {
            return Err(CcError::InvalidParams(
                "worker lease_secs must be positive".into(),
            ));
        }
        if limits.backoff_secs < 0.0 {
            return Err(CcError::InvalidParams(
                "worker backoff_secs must not be negative".into(),
            ));
        }
        if limits.max_attempts == 0 {
            return Err(CcError::InvalidParams(
                "worker max_attempts must be at least 1".into(),
            ));
        }
        Ok(limits)
    }

    /// Heartbeat period the P6-007 brief prescribes (`lease_secs/3`): with
    /// one provider call per attempt the before-work renewal below is the
    /// effective heartbeat; this period is the budget for any future
    /// longer-than-lease work and for the P6-015 recovery scans.
    pub fn renew_period(&self) -> f64 {
        self.lease_secs / 3.0
    }

    /// Builder: opt this worker into a claim fairness policy (P7-005). The
    /// default stays arrival FIFO — every existing caller keeps its exact
    /// semantics.
    pub fn with_claim_order(mut self, claim_order: ClaimFairness) -> Self {
        self.claim_order = claim_order;
        self
    }
}

/// RAII handle over one claimed task's lease. Holding it does nothing to the
/// database — all advancement is explicit and fenced:
///
/// - **Drop policy (deliberate, documented — not an oversight)**: dropping an
///   undisposed guard performs NO ack and NO retry and no database I/O at
///   all. The task stays `claimed` until its lease expires and
///   `reclaim_expired_semantic` returns it to `pending` (attempt budget not
///   consumed). This is the recoverable-outbox contract (C11): a panicking
///   or cancelled worker leaves the task to the lease path instead of
///   burning attempts or masking the failure with a silent retry, and no
///   destructor can swallow a database error.
/// - [`LeaseGuard::renew`] is the heartbeat + liveness gate (the loop renews
///   before every handler invocation).
pub struct LeaseGuard<'a> {
    db: &'a IndexDb,
    task: ClaimedTask,
    lease_secs: f64,
}

impl<'a> LeaseGuard<'a> {
    /// Sanctioned constructor: claim one task and take RAII custody of its
    /// lease — the exact pairing [`drain_pending`] performs internally, so a
    /// caller (or test) can hold a single claim without reaching into the
    /// private fields.
    pub fn claim(db: &'a IndexDb, owner: &str, lease_secs: f64) -> CcResult<Option<Self>> {
        Ok(db.claim_semantic(owner, lease_secs)?.map(|task| Self {
            db,
            task,
            lease_secs,
        }))
    }

    pub fn task(&self) -> &ClaimedTask {
        &self.task
    }

    pub fn lease_secs(&self) -> f64 {
        self.lease_secs
    }

    /// Heartbeat: extend this claim's lease (token-fenced). `Ok(false)` is
    /// "lease lost" — the task left `claimed` under this token (expired +
    /// reclaimed, superseded by a write, or already done) and this worker
    /// must not touch it again.
    pub fn renew(&self) -> CcResult<bool> {
        self.db
            .renew_semantic_lease(self.task.task_id, &self.task.token, self.lease_secs)
    }
}

/// How one handler invocation ended for its claimed task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskExit {
    /// Stop this drain and return to pending without provider-failure backoff.
    Cancelled { started: bool },
    /// The handler fully disposed of the task: the publish CAS acked it, or
    /// the CAS rejection already wrote the fenced retry / left the row
    /// terminal (`superseded`). The loop must not touch the task again — a
    /// double retry would spend the attempt budget twice.
    Disposed,
    /// The task is still `claimed` under this token and failed BEFORE any
    /// queue-state write (provider error, unresolvable input, cache put
    /// failure). The loop hands it back through the fenced retry.
    NeedsRetry { reason: String },
}

/// What one [`drain_pending`] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BatchReport {
    pub claimed: usize,
    /// Tasks the handler disposed (published, or fenced out by the CAS).
    pub completed: usize,
    /// Tasks handed back to the queue through the fenced retry.
    pub retried: usize,
    /// Claims that turned out lost at the pre-work renewal (skip, no write).
    pub lease_lost: usize,
}

/// Hand one still-claimed task back through the fenced retry; returns
/// whether the hand-back really landed (`false` = lease lost, nothing to do).
fn hand_back(
    db: &IndexDb,
    guard: &LeaseGuard<'_>,
    reason: &str,
    limits: &WorkerLimits,
) -> CcResult<bool> {
    db.retry_semantic_task(
        guard.task().task_id,
        guard.task().token.as_str(),
        reason,
        limits.backoff_secs,
        limits.max_attempts,
    )
}

/// One explicit drain: claim up to `limits.max_batch` ready tasks of the
/// active space and process each through `handler`. Returns as soon as the
/// batch is exhausted or the queue reports nothing ready. Auxiliary end to
/// end: claim/renew/retry move no clock; only the handler's publish CAS can.
///
/// Per task: reclaim expired leases (so tasks stranded by a crashed worker
/// are not stuck behind their lease; P6-015 owns the periodic bounded scan)
/// → claim → renew (heartbeat + liveness gate; lost → skip) → `handler` →
/// `Disposed` (done) / `NeedsRetry` or `Err` (fenced retry with
/// `limits.backoff_secs`, dead-letter at `limits.max_attempts`). A retry on
/// a lost lease is a no-op by fencing — `retried` only counts real hand-backs.
pub fn drain_pending(
    db: &IndexDb,
    owner: &str,
    limits: &WorkerLimits,
    handler: &mut dyn FnMut(&LeaseGuard<'_>) -> CcResult<TaskExit>,
) -> CcResult<BatchReport> {
    drain_pending_with_lifecycle(db, owner, limits, None, handler)
}

pub fn drain_pending_with_lifecycle(
    db: &IndexDb,
    owner: &str,
    limits: &WorkerLimits,
    lifecycle: Option<&cc_db::semantic_publish::LifecycleFence>,
    handler: &mut dyn FnMut(&LeaseGuard<'_>) -> CcResult<TaskExit>,
) -> CcResult<BatchReport> {
    debug_assert!(
        limits.max_batch >= 1 && limits.lease_secs > 0.0,
        "unvalidated WorkerLimits"
    );
    let mut report = BatchReport::default();
    for _ in 0..limits.max_batch {
        if lifecycle.is_some_and(|fence| !fence.is_open()) {
            break;
        }
        db.reclaim_expired_semantic()?;
        let Some(task) = db.claim_semantic_with_lifecycle(
            owner,
            limits.lease_secs,
            limits.claim_order,
            lifecycle,
        )?
        else {
            break;
        };
        report.claimed += 1;
        let guard = LeaseGuard {
            db,
            task,
            lease_secs: limits.lease_secs,
        };

        if lifecycle.is_some_and(|fence| !fence.is_open()) {
            db.hand_back_cancelled_semantic_task(
                guard.task().task_id,
                guard.task().token.as_str(),
                false,
            )?;
            break;
        }

        // Liveness gate immediately before work (merge semantics point 3):
        // a task superseded between its claim statement and here is skipped
        // with zero provider cost and zero writes.
        if !guard.renew()? {
            report.lease_lost += 1;
            continue;
        }

        match handler(&guard) {
            Ok(TaskExit::Cancelled { started }) => {
                db.hand_back_cancelled_semantic_task(
                    guard.task().task_id,
                    guard.task().token.as_str(),
                    started,
                )?;
                break;
            }
            Ok(TaskExit::Disposed) => report.completed += 1,
            Ok(TaskExit::NeedsRetry { reason }) => {
                if lifecycle.is_some_and(|fence| !fence.is_open()) {
                    db.hand_back_cancelled_semantic_task(
                        guard.task().task_id,
                        guard.task().token.as_str(),
                        true,
                    )?;
                    break;
                }
                if hand_back(db, &guard, &reason, limits)? {
                    report.retried += 1;
                } else {
                    report.lease_lost += 1;
                }
            }
            Err(e) => {
                if lifecycle.is_some_and(|fence| !fence.is_open()) {
                    db.hand_back_cancelled_semantic_task(
                        guard.task().task_id,
                        guard.task().token.as_str(),
                        true,
                    )?;
                    break;
                }
                if hand_back(db, &guard, &format!("{e}"), limits)? {
                    report.retried += 1;
                } else {
                    report.lease_lost += 1;
                }
            }
        }
    }
    Ok(report)
}

/// Finite joined drain. Claims linearize under one admission mutex in the DB's
/// configured FIFO order; completion order is intentionally unspecified. Width
/// is two only for an explicit per-project bound >= 2 (zero stays serial).
/// No DB lock/connection survives claim, renewal, handler or join boundaries.
/// Old FnMut serial APIs retain their existing behavior. This API propagates
/// handler errors after fenced retry and physical joins, and stops on retry,
/// cancellation or an active-space change. Every claim has its own lease/token.
pub fn drain_pending_parallel_with_lifecycle(
    db: &IndexDb,
    owner: &str,
    limits: &WorkerLimits,
    lifecycle: Option<&cc_db::semantic_publish::LifecycleFence>,
    max_concurrent_per_project: usize,
    handler: &(dyn Fn(&LeaseGuard<'_>) -> CcResult<TaskExit> + Sync),
) -> CcResult<BatchReport> {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    };
    let width = if max_concurrent_per_project >= 2 {
        2
    } else {
        1
    };
    let initial_space = db.semantic_active_space()?;
    let admitted = Mutex::new(0usize);
    let stopped = AtomicBool::new(false);
    let cancelled = || stopped.load(Ordering::Acquire) || lifecycle.is_some_and(|f| !f.is_open());
    let work = || -> CcResult<BatchReport> {
        let mut report = BatchReport::default();
        let result = (|| -> CcResult<()> {
            loop {
                let task = {
                    let mut count = admitted.lock().map_err(|_| {
                        CcError::Database("parallel claim admission poisoned".into())
                    })?;
                    if cancelled() || db.semantic_active_space()? != initial_space {
                        stopped.store(true, Ordering::Release);
                        break;
                    }
                    if *count >= limits.max_batch {
                        break;
                    }
                    db.reclaim_expired_semantic()?;
                    let Some(task) = db.claim_semantic_with_lifecycle(
                        owner,
                        limits.lease_secs,
                        limits.claim_order,
                        lifecycle,
                    )?
                    else {
                        break;
                    };
                    *count += 1;
                    task
                }; // admission and every DB transaction are released before work.
                report.claimed += 1;
                let guard = LeaseGuard {
                    db,
                    task,
                    lease_secs: limits.lease_secs,
                };
                // The pre-work gate owns no DB connection after its result.
                // Even a pre-work DB error hands an unstarted claim back without
                // spending its attempt, then propagates after physical joins.
                let prework = (|| -> CcResult<bool> {
                    // ClaimedTask has no space field; read the immutable binding
                    // to fence a switch-away-and-back race as well.
                    let claimed_space: String = db
                        .read_conn()?
                        .query_row(
                            "SELECT space_id FROM semantic_outbox WHERE task_id=?1",
                            [guard.task().task_id],
                            |row| row.get(0),
                        )
                        .map_err(|error| CcError::Database(error.to_string()))?;
                    Ok(cancelled()
                        || initial_space.as_deref() != Some(claimed_space.as_str())
                        || db.semantic_active_space()? != initial_space)
                })();
                let prework = prework.and_then(|cancel| {
                    if cancel {
                        return Ok(None);
                    }
                    guard.renew().map(Some)
                });
                match prework {
                    Ok(Some(false)) => {
                        report.lease_lost += 1;
                        continue;
                    }
                    Ok(Some(true)) if !cancelled() => {}
                    Ok(_) => {
                        stopped.store(true, Ordering::Release);
                        db.hand_back_cancelled_semantic_task(
                            guard.task().task_id,
                            &guard.task().token,
                            false,
                        )?;
                        break;
                    }
                    Err(error) => {
                        stopped.store(true, Ordering::Release);
                        let handback = db.hand_back_cancelled_semantic_task(
                            guard.task().task_id,
                            &guard.task().token,
                            false,
                        );
                        return Err(match handback {
                            Ok(_) => error,
                            Err(other) => CcError::Database(format!(
                                "{error}; unstarted hand-back failed: {other}"
                            )),
                        });
                    }
                }
                let exit = handler(&guard);
                match exit {
                    Ok(TaskExit::Disposed) => report.completed += 1,
                    Ok(TaskExit::Cancelled { started }) => {
                        stopped.store(true, Ordering::Release);
                        db.hand_back_cancelled_semantic_task(
                            guard.task().task_id,
                            &guard.task().token,
                            started,
                        )?;
                        break;
                    }
                    Ok(TaskExit::NeedsRetry { reason }) => {
                        stopped.store(true, Ordering::Release);
                        if lifecycle.is_some_and(|f| !f.is_open()) {
                            db.hand_back_cancelled_semantic_task(
                                guard.task().task_id,
                                &guard.task().token,
                                true,
                            )?;
                        } else if hand_back(db, &guard, &reason, limits)? {
                            report.retried += 1;
                        } else {
                            report.lease_lost += 1;
                        }
                        break;
                    }
                    Err(error) => {
                        stopped.store(true, Ordering::Release);
                        if lifecycle.is_some_and(|f| !f.is_open()) {
                            db.hand_back_cancelled_semantic_task(
                                guard.task().task_id,
                                &guard.task().token,
                                true,
                            )?;
                        } else {
                            hand_back(db, &guard, &error.to_string(), limits)?;
                        }
                        return Err(error);
                    }
                }
            }
            Ok(())
        })();
        if result.is_err() {
            stopped.store(true, Ordering::Release);
        }
        result.map(|()| report)
    };
    std::thread::scope(|scope| {
        // Exactly width workers for the whole finite drain, never per-document spawn.
        let workers: Vec<_> = (0..width)
            .map(|_| {
                scope.spawn(|| {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work));
                    match result {
                        Ok(result) => result,
                        Err(_) => {
                            stopped.store(true, Ordering::Release);
                            Err(CcError::Database(
                                "parallel attempt panicked; lease recovery retained".into(),
                            ))
                        }
                    }
                })
            })
            .collect();
        let mut total = BatchReport::default();
        let mut errors = Vec::new();
        for worker in workers {
            match worker.join().expect("worker panic is captured") {
                Ok(report) => {
                    total.claimed += report.claimed;
                    total.completed += report.completed;
                    total.retried += report.retried;
                    total.lease_lost += report.lease_lost;
                }
                Err(error) => {
                    errors.push(error);
                }
            }
        }
        match errors.len() {
            0 => Ok(total),
            1 => Err(errors.pop().unwrap()),
            _ => Err(CcError::Database(format!(
                "parallel attempts failed: {}",
                errors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; ")
            ))),
        }
    })
}

/// Real-clock unix seconds for publish timestamps (the worker is the
/// runtime-side caller; deterministic tests inject their own clock through
/// the raw `*_on` primitives, as everywhere else in this crate).
fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Stable one-line reason for a provider failure (persisted into
/// `semantic_outbox.last_error` through the fenced retry).
fn provider_reason(err: &ProviderError) -> String {
    match err {
        ProviderError::RateLimited { retry_after } => {
            format!("provider rate limited, retry after {retry_after:?}")
        }
        ProviderError::ServerError => "provider server error".into(),
        ProviderError::AuthError => "provider auth error".into(),
        ProviderError::Timeout => "provider timeout".into(),
        ProviderError::Cancelled => "provider call cancelled".into(),
        ProviderError::InvalidInput(why) => format!("provider rejected the input: {why}"),
    }
}

/// The concrete embed-and-publish handler: the queue's reference callback.
/// Per claimed task, strictly in order:
///
/// 1. resolve the embeddable input through the injected resolver (the
///    composition root owns the record-schema read; this module stays
///    decoupled from `record_json`'s shape);
/// 2. call the provider for ONE input — no DB lock or connection is held
///    across the call (C11; the guard holds no lock);
/// 3. publish through the P6-011 orchestrator: artifact durable first,
///    verified read-back, then the five-fence CAS which acks the task on
///    success and writes the fenced retry itself on rejection.
///
/// Embed-only by design: `revoke` tasks (P6-017 space revocation) are not
/// consumable here and are reported as a retry reason rather than silently
/// acked.
pub struct EmbedHandler<'a> {
    publisher: Publisher<'a>,
    provider: &'a dyn EmbeddingProvider,
    resolve_input: &'a dyn Fn(&ClaimedTask) -> CcResult<Option<DocumentInput>>,
}

impl<'a> EmbedHandler<'a> {
    pub fn new(
        publisher: Publisher<'a>,
        provider: &'a dyn EmbeddingProvider,
        resolve_input: &'a dyn Fn(&ClaimedTask) -> CcResult<Option<DocumentInput>>,
    ) -> Self {
        Self {
            publisher,
            provider,
            resolve_input,
        }
    }

    /// Process one claimed task (see the struct docs for the order).
    pub fn handle(&self, guard: &LeaseGuard<'_>) -> CcResult<TaskExit> {
        if self.publisher.is_cancelled() {
            return Ok(TaskExit::Cancelled { started: false });
        }
        let task = guard.task();
        if task.op == OutboxOp::Revoke {
            return Ok(TaskExit::NeedsRetry {
                reason: "revoke tasks are not consumable by the embed handler (P6-017)".into(),
            });
        }
        let Some(input) = (self.resolve_input)(task)? else {
            if self.publisher.is_cancelled() {
                return Ok(TaskExit::Cancelled { started: false });
            }
            return Ok(TaskExit::NeedsRetry {
                reason: "embed input unavailable for task".into(),
            });
        };
        if let Err(e) = input.verify() {
            return Ok(TaskExit::NeedsRetry {
                reason: format!("embed input failed its digest binding: {e}"),
            });
        }

        // Single-input batch: the batch bound per attempt is one; `max_batch`
        // bounds claims per drain (admission), not provider inputs.
        let batch = [input];
        if self.publisher.is_cancelled() {
            return Ok(TaskExit::Cancelled { started: false });
        }
        let vectors = match self.provider.embed_documents(&batch) {
            Ok(vectors) => vectors,
            Err(ProviderError::Cancelled) => return Ok(TaskExit::Cancelled { started: true }),
            Err(e) => {
                return Ok(TaskExit::NeedsRetry {
                    reason: provider_reason(&e),
                });
            }
        };
        let Some(vector) = vectors.into_iter().next() else {
            return Ok(TaskExit::NeedsRetry {
                reason: "provider returned fewer vectors than inputs".into(),
            });
        };

        match self
            .publisher
            .publish_embedding(task, &vector, now_unix_secs())?
        {
            PublishVerdict::Cancelled => Ok(TaskExit::Cancelled { started: true }),
            // Acked (and the visible set bumped iff changed) inside the CAS.
            PublishVerdict::Published { .. } => Ok(TaskExit::Disposed),
            // The CAS already wrote the fenced retry (or no-oped against a
            // superseded row): the queue state is settled, do not retry again.
            PublishVerdict::Rejected(_) => Ok(TaskExit::Disposed),
            // Artifact never became verifiable: the DB was untouched and the
            // task is still claimed under our token — hand it back.
            PublishVerdict::ArtifactNotVerified { reason } => Ok(TaskExit::NeedsRetry { reason }),
        }
    }
}
