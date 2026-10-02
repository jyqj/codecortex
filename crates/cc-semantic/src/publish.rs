//! Publish orchestration (P6-011): artifact → manifest, durability-first.
//!
//! Boundary authority: `docs/adr/0003-semantic-persistence-single-db-boundary.md`
//! (row P6-011; lines 60-62/104-107: "artifact 持久化成功在先，manifest CAS 在后；
//! 两存储间不存在原子提交"). The two stores are the discardable artifact cache
//! (this crate, [`crate::cache`]) and the authoritative `semantic_manifest`
//! (cc-db). This module owns the ORDER, not a cross-store transaction: there
//! is none, every crash point replays idempotently (P6-015), and no
//! exactly-once claim is made.
//!
//! Flow of [`Publisher::publish_embedding`] (worker has finished embed):
//!
//! 1. **artifact first** — `cache.put` persists the vector (atomic rename +
//!    fsync, P6-008). Failure here touches nothing else.
//! 2. **re-verify presence** — `cache.get` must yield `Hit` under the very
//!    ref `put` returned (checksum + addressing chain). A miss (evicted,
//!    deleted, unreadable) refuses the publish before the database is touched
//!    — the artifact-before-manifest "先验在场且校验通过" gate. `put` failure
//!    (bad dimension, NaN/Inf, I/O) is refused the same way.
//! 3. **manifest CAS** — one cc-db `IMMEDIATE` transaction
//!    ([`cc_db::semantic_publish`]) under five-way fencing (incarnation /
//!    lease token / doc version / input digest / space). The Q4 visible-set
//!    diff inside that CAS decides the `Semantic` effect: a duplicate ack of
//!    identical content acks the task without bumping `semantic_epoch`.
//! 4. **on rejection** — the CAS itself handed the task back through the
//!    fenced retry machine (backoff → `pending`, exhaustion → `failed`);
//!    the artifact stays in the cache, harmless until GC (P6-016).
//!
//! Fencing snapshot: `expected_incarnation` is the worker's startup
//! `ReadGeneration.incarnation`; the CAS compares it against the live
//! generation inside the transaction, so a rebuilt/swapped database fences
//! out results computed against the old one (strict read only, never the
//! legacy dual-clock path — ADR lines 120-124).

use cc_db::index_db::IndexDb;
use cc_db::semantic_outbox::ClaimedTask;
use cc_db::semantic_publish::{PublishRejection, PublishRequest};
use cc_model::CcResult;

use crate::cache::{ArtifactCache, CacheRead};
use crate::spec::VectorSpace;
use crate::types::{ArtifactRef, DocSpecDigest, InputDigest};

/// Why a publish attempt ended without touching the manifest.
#[derive(Debug, Clone, PartialEq)]
pub enum PublishVerdict {
    /// The manifest CAS accepted the attempt. `visible_set_changed == false`
    /// is the duplicate-ack case: the task is done and `semantic_epoch`
    /// deliberately did not move (Q4).
    Published { visible_set_changed: bool },
    /// The artifact never became verifiable in the cache (`put` failed, or
    /// the re-verify read did not return the expected ref). The database was
    /// NOT touched — artifact-before-manifest, enforced.
    ArtifactNotVerified { reason: String },
    /// The manifest CAS refused (one fence broken). The task has already been
    /// handed back through the fenced retry machine inside the CAS
    /// transaction; the artifact remains in the cache for the retry/GC.
    Rejected(PublishRejection),
}

/// Worker-side publish coordinator bound to one frozen space and doc spec.
pub struct Publisher<'a> {
    db: &'a IndexDb,
    cache: &'a ArtifactCache,
    space: &'a VectorSpace,
    space_digest: crate::types::SpaceDigest,
    doc_spec: &'a DocSpecDigest,
    expected_incarnation: [u8; 16],
}

impl<'a> Publisher<'a> {
    /// Bind a publisher to its stores. `expected_incarnation` is the worker's
    /// startup `ReadGeneration.incarnation` snapshot (fence 1 input).
    pub fn new(
        db: &'a IndexDb,
        cache: &'a ArtifactCache,
        space: &'a VectorSpace,
        doc_spec: &'a DocSpecDigest,
        expected_incarnation: [u8; 16],
    ) -> CcResult<Self> {
        space.validate()?;
        let space_digest = space.digest()?;
        Ok(Self {
            db,
            cache,
            space,
            space_digest,
            doc_spec,
            expected_incarnation,
        })
    }

    /// Publish one finished embedding for one claimed task (see module docs
    /// for the order). `now_unix` is the caller's clock (deterministic tests).
    ///
    /// The task's `input_digest` addresses the cache; `doc_key`/`doc_version`
    /// are fenced inside the CAS against the live document manifest.
    pub fn publish_embedding(
        &self,
        task: &ClaimedTask,
        vector: &[f32],
        now_unix: i64,
    ) -> CcResult<PublishVerdict> {
        let input = InputDigest::new(task.input_digest.clone());

        // 1. artifact durable FIRST (idempotent: same tuple converges to the
        //    same ref, so a crash-replay re-put is a no-op overwrite). A put
        //    failure (dimension mismatch, NaN/Inf payload, I/O) ends the
        //    attempt before the database is touched.
        let artifact_ref: ArtifactRef =
            match self
                .cache
                .put(self.space, &input, self.doc_spec, vector, now_unix)
            {
                Ok(reference) => reference,
                Err(e) => {
                    return Ok(PublishVerdict::ArtifactNotVerified {
                        reason: format!("cache put failed: {e}"),
                    });
                }
            };

        // 2. presence + verification gate: the object must read back as Hit
        //    under the exact ref we are about to publish. A miss or corrupt
        //    object means the manifest must not gain a row referencing it.
        match self.cache.get(self.space, &input, self.doc_spec)? {
            CacheRead::Hit(verified) if verified.artifact_ref == artifact_ref => {}
            CacheRead::Hit(_) => {
                return Ok(PublishVerdict::ArtifactNotVerified {
                    reason: "cache read-back resolved a different artifact_ref".into(),
                });
            }
            CacheRead::Miss => {
                return Ok(PublishVerdict::ArtifactNotVerified {
                    reason: "cache read-back missed the just-put artifact".into(),
                });
            }
            CacheRead::Corrupt(report) => {
                return Ok(PublishVerdict::ArtifactNotVerified {
                    reason: format!(
                        "cache read-back found the artifact corrupt: {}",
                        report.reason
                    ),
                });
            }
        }

        // 3. manifest CAS (five-way fencing + Q4 visible-set diff + fenced
        //    ack, one transaction inside cc-db).
        let request = PublishRequest {
            task_id: task.task_id,
            lease_token: task.token.as_str(),
            doc_key: task.doc_key.as_str(),
            doc_version: task.doc_version.as_str(),
            input_digest: task.input_digest.as_str(),
            space_id: self.space_digest.as_str(),
            artifact_ref: artifact_ref.as_str(),
            expected_incarnation: self.expected_incarnation,
            retry_backoff_secs: RETRY_BACKOFF_SECS,
            max_attempts: PUBLISH_MAX_ATTEMPTS,
            now_unix: now_unix as f64,
        };
        let outcome = self.db.publish_semantic(&request)?;
        if outcome.published {
            Ok(PublishVerdict::Published {
                visible_set_changed: outcome.visible_set_changed,
            })
        } else {
            debug_assert!(outcome.rejection.is_some());
            Ok(PublishVerdict::Rejected(
                outcome.rejection.unwrap_or(PublishRejection::LeaseLost),
            ))
        }
    }
}

/// Backoff for the fenced retry handed back on a CAS rejection (one retry
/// cycle per publish attempt; the worker loop's own retry budget is P6-013).
const RETRY_BACKOFF_SECS: f64 = 1.0;

/// Attempts before the fenced retry dead-letters the task to terminal
/// `failed`. Conservative default; admission/cost policy refines it in
/// P6-013/P6-018 without changing this module's contract.
const PUBLISH_MAX_ATTEMPTS: u32 = 3;
