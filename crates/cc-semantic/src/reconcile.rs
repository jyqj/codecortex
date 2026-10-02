//! Post-rebuild reconcile (P6-014): re-derive the desired set from the
//! rebuilt `document_manifest × active space`, re-enqueue it, and repopulate
//! the visible set from the artifact cache — paid vectors are reused, the
//! provider is only ever called by the WORKER for cache misses.
//!
//! Boundary authority: `docs/adr/0003-semantic-persistence-single-db-boundary.md`
//! (row P6-014: "重建换 incarnation 后从 artifact cache 补 manifest（付费产物
//! 保留），旧 DB 时代回包被 fencing 拒绝") and the Q5 user decision
//! (OPEN-QUESTIONS 2026-10-02): the cache **namespace does not include the
//! incarnation** — a rebuilt index resolves the SAME namespace, so this
//! module's `cache.get` hits on every input that was ever embedded under the
//! same `(space, input digest, spec)`. That decision is the reason a rebuild
//! can repopulate the manifest at zero provider cost; this module is its
//! fulfillment point.
//!
//! Protocol ([`reconcile_after_rebuild`], TASK-BRIEFS P6-014 补齐侧 steps 1-3):
//!
//! 1. **fence first** — the caller's incarnation snapshot is checked against
//!    the authoritative database path
//!    ([`cc_db::semantic_rebuild::IncarnationFreshness`]). A process holding
//!    pre-swap connections is a ghost: `ReconcileVerdict::Fenced`, ZERO
//!    writes anywhere (its only writable file is the detached old inode).
//! 2. **re-derive + re-enqueue** — the desired set is re-projected inside
//!    cc-db ([`IndexDb::semantic_rebuild_desired_set`]: embeddable rows,
//!    `record_json` input digest, render-failed rows skipped — the P6-006
//!    planner convention) and applied through the cc-db rebuild facade
//!    ([`IndexDb::enqueue_semantic_rebuild_plan`]), the P6-006 deviation-6
//!    hand-over (the rebuild itself hooks no outbox: the staging database's
//!    `semantic_spaces` is empty by protocol; manifest/outbox/spaces all
//!    start from zero after the swap and the composition root re-registers
//!    the space).
//! 3. **reuse or requeue** — each desired task is claimed and its input
//!    digest looked up in the cache: a verified `Hit` is published straight
//!    through the full P6-011 five-fence CAS (artifact-before-manifest holds
//!    because the checksum/addressing-verified `get` precedes the CAS — the
//!    artifact is ALREADY durable, no `put`, no provider); a miss (or
//!    corrupt object) is handed back `pending` for the worker, which is the
//!    only component that ever pays.
//!
//! Epoch discipline: re-enqueue bumps `semantic_epoch` exactly when the
//! desired set actually changed (P6-006 convention, inside the facade); the
//! first visible-set publication moves the freshly-absent `semantic_epoch`
//! key from `None` to 1 (ADR: absent = not ready, never folded to 0). Claim /
//! hand-back are Auxiliary. Every reuse publish runs the same five fences —
//! it is not a second-class write path.
//!
//! No resident process: like [`crate::queue`], reconcile is an explicit
//! caller-driven pass (composition root invokes it after a rebuild swap).

use std::collections::HashSet;

use cc_db::index_db::IndexDb;
use cc_db::semantic_outbox::OutboxOp;
use cc_db::semantic_publish::PublishRequest;
use cc_db::semantic_rebuild::IncarnationFreshness;
use cc_model::CcResult;

use crate::cache::{ArtifactCache, CacheRead};
use crate::spec::VectorSpace;
use crate::types::{ArtifactRef, DocSpecDigest, InputDigest};

/// Tunables of one reconcile pass. Defaults suit a runtime pass; tests pass
/// tighter bounds (e.g. zero backoff so the worker can re-claim immediately).
#[derive(Debug, Clone, PartialEq)]
pub struct ReconcileOptions {
    /// Lease length for claims taken during the pass.
    pub lease_secs: f64,
    /// Backoff applied when a task is handed back (cache miss / revoke op).
    pub retry_backoff_secs: f64,
    /// Attempt budget forwarded to the fenced retry on hand-back (shared
    /// with the worker's budget — the counters live on the outbox row).
    pub max_attempts: u32,
}

impl Default for ReconcileOptions {
    fn default() -> Self {
        Self {
            lease_secs: 30.0,
            retry_backoff_secs: 1.0,
            max_attempts: 3,
        }
    }
}

/// What one applied reconcile pass did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReconcileReport {
    /// Embeddable documents found in the rebuilt `document_manifest`.
    pub desired: usize,
    /// Desired tasks (re-)enqueued into the outbox by the rebuild facade.
    pub enqueued: usize,
    /// Tasks whose paid artifact was cache-verified and published through
    /// the full five-fence CAS — zero provider cost.
    pub reused: usize,
    /// Tasks handed back to the queue for the worker (cache miss, corrupt
    /// object, revoke op, or a CAS refusal — which already wrote its own
    /// fenced retry inside the CAS transaction).
    pub requeued: usize,
    /// Of the reused publishes, how many actually changed the visible set
    /// (Q4: a byte-identical republication acks without bumping).
    pub visible_changes: usize,
}

/// Outcome of one reconcile attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileVerdict {
    /// The pass ran; see [`ReconcileReport`].
    Applied(ReconcileReport),
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

/// Hand one still-claimed task back through the fenced retry (the reconcile
/// twin of the worker loop's `hand_back`).
fn hand_back(
    db: &IndexDb,
    task: &cc_db::semantic_outbox::ClaimedTask,
    reason: &str,
    options: &ReconcileOptions,
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

/// One post-rebuild reconcile pass (see the module docs for the protocol).
///
/// `expected_incarnation` is the caller's snapshot taken from the rebuilt
/// database (e.g. `db.reads().read_generation().incarnation` right after the
/// swap); `cache`/`space`/`doc_spec` must be the composition root's frozen
/// semantic configuration — the SAME cache namespace as before the rebuild
/// (Q5: the namespace never contained the incarnation) and the SAME frozen
/// space, or every reuse lookup misses and the worker pays for a full
/// re-embed.
pub fn reconcile_after_rebuild(
    db: &IndexDb,
    cache: &ArtifactCache,
    space: &VectorSpace,
    doc_spec: &DocSpecDigest,
    expected_incarnation: [u8; 16],
    options: &ReconcileOptions,
) -> CcResult<ReconcileVerdict> {
    // 0. The fence is absolute: a ghost process must not re-enqueue, claim,
    //    or publish anything — every write it could attempt would land in
    //    the detached old inode, invisible to the authoritative database.
    if let IncarnationFreshness::Stale {
        snapshot,
        path_incarnation,
    } = db.semantic_incarnation_freshness(expected_incarnation)?
    {
        return Ok(ReconcileVerdict::Fenced {
            snapshot,
            path_incarnation,
        });
    }
    space.validate()?;
    let space_digest = space.digest()?;

    // 1. desired set = document_manifest × active space (strictly: the
    //    single active space; anything else is a caller configuration error
    //    — publishing under a different space would fail CAS fence 5 anyway,
    //    and saying so here beats N per-task rejections).
    let Some(active) = db.semantic_active_space()? else {
        // Semantic not configured: nothing is desired, nothing is paid.
        return Ok(ReconcileVerdict::Applied(ReconcileReport::default()));
    };
    if active != space_digest.as_str() {
        return Err(cc_model::CcError::InvalidParams(format!(
            "reconcile space {} is not the active space {active}",
            space_digest.as_str()
        )));
    }
    let desired = db.semantic_rebuild_desired_set()?;

    // 2. re-enqueue (P6-006 deviation-6 hand-over; one IMMEDIATE short
    //    transaction, Semantic effect declared inside the facade).
    let stats = db.enqueue_semantic_rebuild_plan(&desired)?;
    let mut report = ReconcileReport {
        desired: desired.len(),
        enqueued: stats.enqueued,
        ..ReconcileReport::default()
    };

    // 3. reuse loop: claim → verified cache hit → full five-fence CAS;
    //    anything else goes back to the worker. The `seen` guard bounds the
    //    pass: a task handed back at zero backoff could be re-claimed by
    //    this same loop — revisiting one means nothing NEW is claimable.
    let mut seen: HashSet<i64> = HashSet::new();
    while let Some(task) = db.claim_semantic("semantic-reconcile", options.lease_secs)? {
        if !seen.insert(task.task_id) {
            hand_back(
                db,
                &task,
                "reconcile: revisit guard, leaving the task to the worker",
                options,
            )?;
            break;
        }
        if task.op != OutboxOp::Embed {
            hand_back(
                db,
                &task,
                "reconcile: revoke tasks are not consumable here (P6-017)",
                options,
            )?;
            report.requeued += 1;
            continue;
        }
        let input = InputDigest::new(task.input_digest.clone());
        match cache.get(space, &input, doc_spec)? {
            CacheRead::Hit(hit) => {
                let outcome = publish_cached(
                    db,
                    &task,
                    &hit.artifact_ref,
                    space_digest.as_str(),
                    expected_incarnation,
                    options,
                )?;
                if outcome.published {
                    report.reused += 1;
                    report.visible_changes += usize::from(outcome.visible_set_changed);
                } else {
                    report.requeued += 1;
                }
            }
            CacheRead::Miss | CacheRead::Corrupt(_) => {
                hand_back(
                    db,
                    &task,
                    "reconcile: paid artifact not reusable from cache, requeued for the worker",
                    options,
                )?;
                report.requeued += 1;
            }
        }
    }
    Ok(ReconcileVerdict::Applied(report))
}

/// Publish one cache-verified artifact through the full P6-011 CAS. The
/// artifact-before-manifest order holds by construction: the `get` that
/// produced `artifact_ref` verified the addressing triple, the blake3
/// payload checksum and the frozen space — the artifact is already durable,
/// so there is no `put` and no provider call. Returns the CAS outcome:
/// `published == false` is a CAS refusal, which already wrote its own fenced
/// retry inside the CAS transaction.
fn publish_cached(
    db: &IndexDb,
    task: &cc_db::semantic_outbox::ClaimedTask,
    artifact_ref: &ArtifactRef,
    space_id: &str,
    expected_incarnation: [u8; 16],
    options: &ReconcileOptions,
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
