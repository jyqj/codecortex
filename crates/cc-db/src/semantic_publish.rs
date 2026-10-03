//! Semantic publish CAS (P6-011): move one worker's finished embedding from
//! the artifact cache into the `semantic_manifest` visible set — atomically,
//! under five-way fencing, and only when the visible set actually changes.
//!
//! Boundary authority: `docs/adr/0003-semantic-persistence-single-db-boundary.md`
//! (row P6-011: "必须 artifact-before-manifest；五重 fencing 校验；慢旧结果不能
//! 挂到同路径新版本；发布幂等"; lines 60-62: no cross-store atomic commit —
//! artifact durable FIRST, manifest CAS second, crash replay is idempotent
//! recovery). The artifact-before-manifest order itself lives in the caller
//! (cc-semantic `publish` orchestration verifies the cache object before
//! invoking this module); inside the single authoritative library this module
//! is one `IMMEDIATE` short transaction with no half-publish window.
//!
//! CAS semantics (TASK-BRIEFS P6-011 step 3, verbatim contract): all five
//! fences are compared inside one transaction and the visible-set row is
//! written only on full agreement —
//!
//! ```sql
//! INSERT INTO semantic_manifest ... ON CONFLICT(doc_key) DO UPDATE ...
//! ```
//!
//! - **incarnation**: `read_generation().incarnation` must equal the worker's
//!   startup snapshot (a swapped/rebuilt database fences out results computed
//!   against the old one — ADR lines 120-124: strict `ReadGeneration` only,
//!   never the legacy dual-clock reader);
//! - **lease token**: the outbox row must still be `claimed` under the exact
//!   per-attempt token (P6-007 fenced domain; an expired/re-claimed worker
//!   cannot publish);
//! - **doc version**: `document_manifest.doc_version` must equal the task's —
//!   a slow old result can never attach to a newer version of the same path;
//! - **input digest**: the task's `input_digest` must equal the current
//!   manifest row's embedded-input hash (`record_json.input.input_hash`, the
//!   P6-003 input digest) — the vector being published is the vector the
//!   current document version was rendered from;
//! - **space**: the task's `space_id` must be the single `active` space.
//!
//! Q4 (P6-004 follow-up, fixed here): a repeat ack whose visible set did not
//! change — same `(doc, space)` already published with the identical
//! `artifact_ref`/`doc_version`/`input_digest` — acks the task but is NOT a
//! visible-set change: no manifest write, and the caller must not declare the
//! `Semantic` effect (the [`IndexDb::publish_semantic`] facade does exactly
//! that, so `semantic_epoch` stays put). Only an actual visible-set change
//! bumps.
//!
//! Rejections are CAS losses, never overwrites: nothing under the manifest is
//! written, the caller's transaction can commit safely, and the task is
//! handed back to the P6-007 machine by the same fenced [`retry_on`](crate::semantic_outbox::retry_on)
//! primitive (backoff → `pending`, attempt exhaustion → terminal `failed`)
//! inside the same transaction — a `LeaseLost` rejection no-ops there because
//! the token no longer matches anything. Epoch discipline: this module never
//! bumps any clock itself; the bump is the facade's `Semantic` effect,
//! declared exactly when [`PublishOutcome::visible_set_changed`] is true.

use std::fmt::Write as _;

use rusqlite::Connection;

use cc_model::{CcError, CcResult};

use crate::index_db::IndexDb;
use crate::semantic_outbox::{self, OutboxState};
use crate::sql_util::db_err;

/// Incarnation-local runtime fence. Take it only AFTER acquiring a SQLite
/// write transaction, and release it after commit. Closing never waits on
/// SQLite contention, and a close that completes precedes all later writes.
#[derive(Debug)]
pub struct LifecycleFence(std::sync::Mutex<bool>);
pub struct LifecyclePermit<'a> {
    _guard: std::sync::MutexGuard<'a, bool>,
}
impl Default for LifecycleFence {
    fn default() -> Self {
        Self(std::sync::Mutex::new(true))
    }
}
impl LifecycleFence {
    pub fn close(&self) {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = false;
    }
    pub fn is_open(&self) -> bool {
        self.0.lock().map(|open| *open).unwrap_or(false)
    }
    pub fn enter(&self) -> Option<LifecyclePermit<'_>> {
        let open = self.0.lock().ok()?;
        if *open {
            Some(LifecyclePermit { _guard: open })
        } else {
            None
        }
    }
}

/// Why the publish CAS refused to write (the five fences, one variant each,
/// plus the vanished-base-row case). A rejection is a clean loss: zero
/// manifest writes, zero epoch bumps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishRejection {
    /// The database under this transaction carries a different incarnation
    /// than the worker's startup snapshot — the index was rebuilt/swapped and
    /// results computed against the old one are fenced out (ADR-0003).
    IncarnationMismatch,
    /// The outbox row is no longer `claimed` under the presented per-attempt
    /// token (reclaimed, superseded, already done, or a forged token).
    LeaseLost,
    /// The current `document_manifest.doc_version` is newer than the task's —
    /// a slow old result cannot attach to the same path's new version.
    DocVersionStale,
    /// The task's input digest does not match the current manifest row's
    /// embedded-input hash (or the row lost its embeddable input).
    InputDigestMismatch,
    /// The task's space is not the single `active` space (semantic disabled,
    /// switched, or revoked).
    SpaceNotActive,
    /// The base document row is gone (deleted between claim and publish).
    DocumentMissing,
}

impl PublishRejection {
    /// Stable short code persisted into `semantic_outbox.last_error`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IncarnationMismatch => "publish-rejected:incarnation-mismatch",
            Self::LeaseLost => "publish-rejected:lease-lost",
            Self::DocVersionStale => "publish-rejected:doc-version-stale",
            Self::InputDigestMismatch => "publish-rejected:input-digest-mismatch",
            Self::SpaceNotActive => "publish-rejected:space-not-active",
            Self::DocumentMissing => "publish-rejected:document-missing",
        }
    }
}

/// One publish attempt's request: the claimed task's identity, the verified
/// cache artifact it produced, and the fencing expectations. `now_unix` is
/// the caller's clock (no statement-level time); `retry_backoff_secs` /
/// `max_attempts` parameterize the fenced retry applied on rejection.
#[derive(Debug, Clone, PartialEq)]
pub struct PublishRequest<'a> {
    pub task_id: i64,
    pub lease_token: &'a str,
    pub doc_key: &'a str,
    pub doc_version: &'a str,
    pub input_digest: &'a str,
    pub space_id: &'a str,
    /// The cache content address of the persisted, checksum-verified artifact
    /// (P6-008 `ArtifactCache::put` output). Presence/verification upstream is
    /// the artifact-before-manifest "先" side; this module trusts the ref.
    pub artifact_ref: &'a str,
    /// The worker's startup `ReadGeneration.incarnation` snapshot.
    pub expected_incarnation: [u8; 16],
    pub retry_backoff_secs: f64,
    pub max_attempts: u32,
    pub now_unix: f64,
}

/// What one publish attempt did. `published == rejection.is_none()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishOutcome {
    pub published: bool,
    /// Whether the visible set actually changed (fresh row or different
    /// content). `false` on a duplicate ack of identical content — the caller
    /// must NOT declare the `Semantic` effect (Q4: no epoch bump).
    pub visible_set_changed: bool,
    pub rejection: Option<PublishRejection>,
}

fn timestamp_text(now_unix: f64) -> CcResult<String> {
    let secs = now_unix.floor() as i64;
    let nanos = ((now_unix - secs as f64) * 1e9).round() as u32;
    chrono::DateTime::from_timestamp(secs, nanos)
        .map(|dt| dt.to_rfc3339())
        .ok_or_else(|| CcError::InvalidParams("semantic publish timestamp out of range".into()))
}

fn incarnation_hex(incarnation: [u8; 16]) -> String {
    let mut hex = String::with_capacity(32);
    for byte in incarnation {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Minimal read model of `document_manifest.record_json` for the input fence:
/// the embedded-input hash of the current record (P6-003 `InputDigest`).
/// Deliberately a field subset, not `DocumentRecord` — the fence reads one
/// digest and must not couple to record-schema evolution.
#[derive(serde::Deserialize)]
struct RecordInputPeek {
    #[serde(default)]
    input: Option<InputHashPeek>,
}

#[derive(serde::Deserialize)]
struct InputHashPeek {
    input_hash: String,
}

/// The publish CAS, executed on the caller's OPEN transaction connection
/// (`*_on` cc-db pattern). All five fences are compared first; the first
/// failing fence yields a rejection with zero writes plus the fenced retry
/// hand-back, and full agreement yields the upsert + fenced ack in the same
/// transaction. Never bumps any clock (the caller declares effects).
///
/// Rejection semantics (brief: "任一校验失败 → 事务回滚，任务走 retry_on 或按
/// 失败类别终态化"): the manifest side is untouched by construction, and the
/// task is returned through [`crate::semantic_outbox::retry_on`] — backoff to
/// `pending`, or terminal `failed` once `max_attempts` is exhausted. A
/// `LeaseLost` rejection no-ops the retry (the token matches nothing).
pub fn publish_and_ack_on(conn: &Connection, req: &PublishRequest<'_>) -> CcResult<PublishOutcome> {
    // Fence 1 — incarnation (strict ReadGeneration only; ADR line 120-124).
    let generation = crate::read_generation::read_on(conn)?;
    if generation.incarnation != req.expected_incarnation {
        return rejected(conn, req, PublishRejection::IncarnationMismatch);
    }

    // Fence 2 — lease token on a still-claimed row (P6-007 fenced domain).
    let lease: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT state, lease_token FROM semantic_outbox WHERE task_id=?1",
            [req.task_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })
        .map_err(db_err)?;
    let lease_lost = match lease {
        Some((state, token)) => {
            state != OutboxState::Claimed.as_str() || token.as_deref() != Some(req.lease_token)
        }
        None => true,
    };
    if lease_lost {
        return rejected(conn, req, PublishRejection::LeaseLost);
    }

    // Fence 3/4 — doc version and input digest against the current base row.
    let base: Option<(String, Option<String>, String, String)> = conn
        .query_row(
            "SELECT doc_version, encoding_key, file_path, record_json \
             FROM document_manifest WHERE doc_key=?1",
            [req.doc_key],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })
        .map_err(db_err)?;
    let Some((manifest_version, encoding_key, file_path, record_json)) = base else {
        return rejected(conn, req, PublishRejection::DocumentMissing);
    };
    if manifest_version != req.doc_version {
        return rejected(conn, req, PublishRejection::DocVersionStale);
    }
    let Some(encoding_key) = encoding_key else {
        // The current record has no embeddable input at all.
        return rejected(conn, req, PublishRejection::InputDigestMismatch);
    };
    let input_hash: Option<String> = serde_json::from_str::<RecordInputPeek>(&record_json)
        .map_err(|e| CcError::Database(format!("document record_json unreadable: {e}")))?
        .input
        .map(|i| i.input_hash);
    if input_hash.as_deref() != Some(req.input_digest) {
        return rejected(conn, req, PublishRejection::InputDigestMismatch);
    }

    // Fence 5 — the task's space must be the single active space.
    let Some(active_space) = semantic_outbox::active_space_on(conn)? else {
        return rejected(conn, req, PublishRejection::SpaceNotActive);
    };
    if active_space != req.space_id {
        return rejected(conn, req, PublishRejection::SpaceNotActive);
    }

    // All fences passed. Q4 visible-set diff: an existing row that is byte
    // -identical in every published field means nothing changed for readers.
    let existing: Option<(String, String, String, String)> = conn
        .query_row(
            "SELECT doc_version, input_digest, space_id, artifact_ref \
             FROM semantic_manifest WHERE doc_key=?1",
            [req.doc_key],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })
        .map_err(db_err)?;
    let visible_set_changed = !match existing {
        Some((version, input, space, artifact)) => {
            version == req.doc_version
                && input == req.input_digest
                && space == req.space_id
                && artifact == req.artifact_ref
        }
        None => false,
    };
    if visible_set_changed {
        // Brief step 3, verbatim CAS shape: upsert the visible-set row. The
        // doc_key PRIMARY KEY holds one current publication per document.
        let ts = timestamp_text(req.now_unix)?;
        conn.prepare_cached(
            "INSERT INTO semantic_manifest(doc_key,doc_version,file_path,encoding_key,\
             input_digest,space_id,artifact_ref,published_at,published_incarnation) \
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) \
             ON CONFLICT(doc_key) DO UPDATE SET \
               doc_version=excluded.doc_version, file_path=excluded.file_path, \
               encoding_key=excluded.encoding_key, input_digest=excluded.input_digest, \
               space_id=excluded.space_id, artifact_ref=excluded.artifact_ref, \
               published_at=excluded.published_at, \
               published_incarnation=excluded.published_incarnation",
        )
        .map_err(db_err)?
        .execute(rusqlite::params![
            req.doc_key,
            req.doc_version,
            file_path,
            encoding_key,
            req.input_digest,
            req.space_id,
            req.artifact_ref,
            ts,
            incarnation_hex(req.expected_incarnation),
        ])
        .map_err(db_err)?;
    }

    // Fenced ack inside the same transaction (P6-007 `claimed → done`). We
    // verified the lease above within this transaction, so this cannot lose;
    // a `false` here would mean a broken isolation assumption — fail loudly.
    let acked = semantic_outbox::ack_done_on(conn, req.task_id, req.lease_token, req.now_unix)?;
    debug_assert!(acked, "lease verified in-txn but ack lost");
    if !acked {
        return Err(CcError::Database(
            "publish ack lost after in-transaction lease verification".into(),
        ));
    }

    Ok(PublishOutcome {
        published: true,
        visible_set_changed,
        rejection: None,
    })
}

/// CAS loss path: write nothing under the manifest, hand the task back to the
/// P6-007 machine through the same fenced retry primitive (no-op when the
/// lease itself is the reason), and report the rejection.
fn rejected(
    conn: &Connection,
    req: &PublishRequest<'_>,
    rejection: PublishRejection,
) -> CcResult<PublishOutcome> {
    semantic_outbox::retry_on(
        conn,
        req.task_id,
        req.lease_token,
        rejection.as_str(),
        req.now_unix,
        req.retry_backoff_secs,
        req.max_attempts,
    )?;
    Ok(PublishOutcome {
        published: false,
        visible_set_changed: false,
        rejection: Some(rejection),
    })
}

impl IndexDb {
    /// Publish one finished embedding: one `IMMEDIATE` short transaction over
    /// the write connection running [`publish_and_ack_on`], committing the
    /// `Semantic` effect exactly when the visible set actually changed
    /// (Q4: a duplicate ack of identical content is Auxiliary — no bump).
    ///
    /// A rejection commits too: the fenced retry hand-back it wrote is real
    /// queue state, while the visible set is untouched by construction.
    /// `expected_incarnation` is the worker's startup snapshot; a mismatch
    /// fences the whole attempt out (ADR-0003 rebuild fencing).
    pub fn publish_semantic(&self, req: &PublishRequest<'_>) -> CcResult<PublishOutcome> {
        // Unscoped library callers retain their frozen behavior.
        self.publish_semantic_with_lifecycle(req, None)
            .map(|outcome| outcome.expect("unscoped publish"))
    }

    pub fn publish_semantic_with_lifecycle(
        &self,
        req: &PublishRequest<'_>,
        lifecycle: Option<&LifecycleFence>,
    ) -> CcResult<Option<PublishOutcome>> {
        let conn = self.write_conn.lock().map_err(db_err)?;
        conn.execute_batch("BEGIN IMMEDIATE;")
            .map_err(|e| CcError::Database(format!("begin publish: {e}")))?;
        let _lifecycle = match lifecycle {
            Some(fence) => match fence.enter() {
                Some(permit) => Some(permit),
                None => {
                    conn.execute_batch("ROLLBACK;").map_err(db_err)?;
                    return Ok(None);
                }
            },
            None => None,
        };
        let outcome = match publish_and_ack_on(&conn, req) {
            Ok(outcome) => outcome,
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK;");
                return Err(e);
            }
        };
        // The Semantic effect is declared exactly when the visible set actually
        // changed (Q4: a duplicate ack of identical content is Auxiliary).
        if outcome.published && outcome.visible_set_changed {
            if let Err(e) = IndexDb::bump_semantic_epoch_on(&conn) {
                let _ = conn.execute_batch("ROLLBACK;");
                return Err(e);
            }
        }
        match conn.execute_batch("COMMIT;") {
            Ok(()) => Ok(Some(outcome)),
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK;");
                Err(CcError::Database(format!("commit publish: {e}")))
            }
        }
    }
}
