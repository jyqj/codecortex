//! Semantic outbox (ADR-0003): desired embed/revoke tasks written ATOMICALLY
//! inside the source-code transaction that mutates `document_manifest`.
//!
//! Reliability contract (P6-006): every outbox row is written by
//! [`supersede_and_enqueue_on`] on the caller's already-open transaction
//! connection — the same transaction that replaces/removes the files and
//! their manifests. A rollback therefore cannot leak "half a task", and a
//! deletion never enqueues an embed task. When no semantic space is active
//! (`semantic_spaces` empty of `active` rows) the whole module is a no-op, so
//! the default build path pays nothing and changes nothing (ADR Decision
//! Drivers: zero default behavior change).
//!
//! Lease fencing (P6-007): [`claim_next_on`] is one CAS `UPDATE ... WHERE
//! state='pending' ... RETURNING` — the winner is decided inside a single
//! statement, so two processes can never hold the same lease. Every attempt
//! carries a fresh token (`lower(hex(randomblob(16)))`); [`renew_lease_on`],
//! [`ack_done_on`] and [`retry_on`] all judge by `task_id + lease_token +
//! state='claimed'` only (`claim_owner` is diagnostics, never a credential),
//! and a rowcount of zero means "lease lost" — an expired worker whose task
//! was reclaimed and re-claimed cannot advance the new lease. Expired leases
//! are returned to `pending` by [`reclaim_expired_on`]; claim only ever eats
//! `pending` rows. All of these are Auxiliary short transactions: no epoch
//! ever moves. [`transition_state_on`] itself still never touches the lease
//! columns.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;

use cc_model::{CcError, CcResult};

use crate::index_db_types::FileWriteUnit;
use crate::sql_util::{db_err, sql_in_placeholders, IN_BATCH_SIZE};

/// Wall-clock unix seconds for `available_at` filters and batch timestamps.
/// Callers pass it INTO the SQL (never `now()` inside a statement) so tests
/// stay deterministic.
pub(crate) fn now_unix() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

/// RFC3339 text for the `created_at`/`updated_at` columns, derived from the
/// caller-supplied unix time (deterministic, no statement-level clock).
fn timestamp_text(now_unix: f64) -> CcResult<String> {
    let secs = now_unix.floor() as i64;
    let nanos = ((now_unix - secs as f64) * 1e9).round() as u32;
    chrono::DateTime::from_timestamp(secs, nanos)
        .map(|dt| dt.to_rfc3339())
        .ok_or_else(|| CcError::InvalidParams("semantic outbox timestamp out of range".into()))
}

/// The typed state of a desired outbox task (`semantic_outbox.state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboxState {
    Pending,
    Claimed,
    Done,
    Failed,
    Superseded,
}

impl OutboxState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Claimed => "claimed",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Superseded => "superseded",
        }
    }

    pub fn from_db(value: &str) -> CcResult<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "claimed" => Ok(Self::Claimed),
            "done" => Ok(Self::Done),
            "failed" => Ok(Self::Failed),
            "superseded" => Ok(Self::Superseded),
            other => Err(CcError::InvalidParams(format!(
                "unknown semantic outbox state '{other}'"
            ))),
        }
    }

    /// The closed legal-transition table of the desired-task machine:
    /// `pending → claimed` (claim), `*live → superseded` (write-path merge),
    /// `claimed → done|failed` (worker ack/failure), `claimed → pending`
    /// (lease reclaim / atomic retry collapse, P6-007), `failed → pending`
    /// (retry with backoff). `done` and `superseded` are terminal; `pending`
    /// can never reach `done` without a claim.
    pub const fn can_transition_to(self, to: Self) -> bool {
        use OutboxState::*;
        matches!(
            (self, to),
            (Pending, Claimed)
                | (Pending, Superseded)
                | (Claimed, Done)
                | (Claimed, Failed)
                | (Claimed, Superseded)
                | (Claimed, Pending) // lease reclaim / atomic retry (P6-007)
                | (Failed, Pending)
                | (Failed, Superseded)
        )
    }
}

/// The typed operation of an outbox task (`semantic_outbox.op`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboxOp {
    Embed,
    Revoke,
}

impl OutboxOp {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Embed => "embed",
            Self::Revoke => "revoke",
        }
    }

    pub fn from_db(value: &str) -> CcResult<Self> {
        match value {
            "embed" => Ok(Self::Embed),
            "revoke" => Ok(Self::Revoke),
            other => Err(CcError::InvalidParams(format!(
                "unknown semantic outbox op '{other}'"
            ))),
        }
    }
}

/// One projected document whose embed task must be (re-)enqueued.
/// `input_digest` is the actual embedding-input digest (P6-003 `InputDigest`
/// = blake3 of the input bytes) — a task without it would be unactionable, so
/// the batch planner skips render-failed documents instead of enqueueing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxUpsert {
    pub doc_key: String,
    pub doc_version: String,
    pub input_digest: String,
}

/// The write-path plan for one source-code transaction. Executed by
/// [`supersede_and_enqueue_on`] on the caller's transaction connection.
pub struct OutboxPlan<'a> {
    pub upserts: &'a [OutboxUpsert],
    /// Revoked doc_keys (file removals and replaced-away projections).
    pub removals: &'a [String],
    pub now_unix: f64,
}

/// What one outbox plan actually wrote (also the `{Index, Semantic}`
/// declaration input: any nonzero stat means the semantic state changed).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OutboxWriteStats {
    pub superseded_tasks: usize,
    pub enqueued: usize,
    pub manifest_revoked: usize,
}

impl OutboxWriteStats {
    /// Whether this plan changed semantic state at all — the caller declares
    /// the `Semantic` write effect exactly when this is true (the visible-set
    /// diff is the write itself; P6-004 Q4).
    pub fn changed_semantic_state(&self) -> bool {
        self.superseded_tasks != 0 || self.enqueued != 0 || self.manifest_revoked != 0
    }
}

/// One ready task as served by [`ready_tasks_on`] (claiming is P6-007).
#[derive(Debug, Clone, PartialEq)]
pub struct OutboxTask {
    pub task_id: i64,
    pub doc_key: String,
    pub doc_version: String,
    pub input_digest: String,
    pub space_id: String,
    pub op: OutboxOp,
    pub attempt_count: i64,
    pub available_at: f64,
}

/// The single active space pointer, or `None` when semantic is not
/// configured. More than one `active` row violates the P6-017 single-space
/// invariant — fail-stop instead of silently picking one.
pub fn active_space_on(conn: &Connection) -> CcResult<Option<String>> {
    let mut stmt = conn
        .prepare_cached("SELECT space_id FROM semantic_spaces WHERE state='active'")
        .map_err(db_err)?;
    let mut rows = stmt.query([]).map_err(db_err)?;
    let mut found: Option<String> = None;
    while let Some(row) = rows.next().map_err(db_err)? {
        let space_id: String = row.get(0).map_err(db_err)?;
        if found.replace(space_id).is_some() {
            return Err(CcError::Database("multiple active semantic spaces".into()));
        }
    }
    Ok(found)
}

fn supersede_live_on(conn: &Connection, doc_keys: &[&str], now_unix: f64) -> CcResult<usize> {
    let placeholders = sql_in_placeholders(doc_keys.len());
    let last = doc_keys.len() + 1;
    let sql = format!(
        "UPDATE semantic_outbox SET state='superseded', updated_at=?{last} \
         WHERE doc_key IN ({placeholders}) AND state IN ('pending','claimed')"
    );
    let mut params: Vec<&dyn rusqlite::ToSql> =
        doc_keys.iter().map(|k| k as &dyn rusqlite::ToSql).collect();
    params.push(&now_unix);
    conn.prepare_cached(&sql)
        .map_err(db_err)?
        .execute(params.as_slice())
        .map_err(db_err)
}

/// Execute one outbox plan on the caller's open transaction connection.
///
/// Order inside the transaction (single transaction = atomic with the file
/// batch, ADR-0003):
/// 1. removals → `DELETE FROM semantic_manifest` (visible-set revoke; the
///    FK cascade into the base manifest is only a backstop — CASCADE never
///    fires application logic) + supersede the doc's live tasks
///    (`pending`/`claimed`; a superseded formerly-claimed task is dropped by
///    the P6-007 worker's fencing on ack).
/// 2. upserts → supersede live tasks first, then INSERT one `pending` embed
///    task. The insert is a plain INSERT: the partial unique index
///    `semantic_outbox_live_per_doc` (one live task per doc+space, the
///    P6-013 merge invariant) must NEVER be swallowed — a conflict after an
///    explicit supersede is a bug and fails the whole transaction.
///
/// `revoke` tasks are only produced by explicit space revocation (P6-017),
/// never by this function.
pub fn supersede_and_enqueue_on(
    conn: &Connection,
    plan: &OutboxPlan<'_>,
) -> CcResult<OutboxWriteStats> {
    let mut stats = OutboxWriteStats::default();
    // No active space = semantic not configured: full no-op (the caller's
    // transaction is untouched, so the default build path is zero-change).
    let Some(space_id) = active_space_on(conn)? else {
        return Ok(stats);
    };
    let ts = timestamp_text(plan.now_unix)?;
    for batch in plan.removals.chunks(IN_BATCH_SIZE) {
        if batch.is_empty() {
            continue;
        }
        let placeholders = sql_in_placeholders(batch.len());
        let sql = format!("DELETE FROM semantic_manifest WHERE doc_key IN ({placeholders})");
        stats.manifest_revoked += conn
            .prepare_cached(&sql)
            .map_err(db_err)?
            .execute(rusqlite::params_from_iter(batch.iter()))
            .map_err(db_err)?;
        stats.superseded_tasks += supersede_live_on(
            conn,
            &batch.iter().map(String::as_str).collect::<Vec<_>>(),
            plan.now_unix,
        )?;
    }
    for batch in plan.upserts.chunks(IN_BATCH_SIZE) {
        if batch.is_empty() {
            continue;
        }
        let doc_keys: Vec<&str> = batch.iter().map(|u| u.doc_key.as_str()).collect();
        stats.superseded_tasks += supersede_live_on(conn, &doc_keys, plan.now_unix)?;
        for task in batch {
            conn.prepare_cached(
                "INSERT INTO semantic_outbox(doc_key,doc_version,input_digest,space_id,op,state,\
                 attempt_count,available_at,created_at,updated_at) \
                 VALUES(?1,?2,?3,?4,'embed','pending',0,?5,?6,?6)",
            )
            .map_err(db_err)?
            .execute(rusqlite::params![
                task.doc_key,
                task.doc_version,
                task.input_digest,
                space_id,
                plan.now_unix,
                ts
            ])
            .map_err(db_err)?;
            stats.enqueued += 1;
        }
    }
    Ok(stats)
}

/// Plan and apply the outbox half of one file batch (the cc-db hook used by
/// the file batch writers). MUST run inside the batch transaction BEFORE its
/// base deletes: removal doc_keys are resolved through
/// `semantic_manifest.file_path` while the rows still exist (the batch's
/// chunk deletes cascade the base manifests away).
///
/// No-op unless a semantic space is active — callers need no feature branch.
pub(crate) fn apply_file_batch_on(
    conn: &Connection,
    units: &[FileWriteUnit],
    to_remove: &[String],
) -> CcResult<OutboxWriteStats> {
    if active_space_on(conn)?.is_none() {
        return Ok(OutboxWriteStats::default());
    }
    let now = now_unix();
    let mut removals: BTreeSet<String> = BTreeSet::new();
    for batch in to_remove.chunks(IN_BATCH_SIZE) {
        let placeholders = sql_in_placeholders(batch.len());
        // Removal doc_keys come from BOTH layers while the rows still exist:
        // the published visible set (`semantic_manifest`) and the current
        // projection (`document_manifest`, whose outbox tasks may still be
        // pending even though nothing was published yet). Both are indexed
        // by file_path.
        for table in ["semantic_manifest", "document_manifest"] {
            let sql = format!("SELECT doc_key FROM {table} WHERE file_path IN ({placeholders})");
            let mut stmt = conn.prepare_cached(&sql).map_err(db_err)?;
            let rows = stmt
                .query_map(rusqlite::params_from_iter(batch.iter()), |r| {
                    r.get::<_, String>(0)
                })
                .map_err(db_err)?;
            for key in rows {
                removals.insert(key.map_err(db_err)?);
            }
        }
    }
    let mut upserts: BTreeMap<String, OutboxUpsert> = BTreeMap::new();
    for unit in units {
        let Some(batch) = &unit.outcome.documents else {
            continue;
        };
        // doc_key → persisted input digest. Render-failed records carry no
        // input (nothing to embed) and enqueue nothing; the next successful
        // render changes doc_version and enqueues then.
        let inputs: BTreeMap<&str, &str> = batch
            .records
            .iter()
            .filter_map(|r| {
                r.input
                    .as_ref()
                    .map(|i| (r.reference.doc_key.as_str(), i.input_hash.as_str()))
            })
            .collect();
        for reference in &batch.delta.upsert {
            if let Some(digest) = inputs.get(reference.doc_key.as_str()) {
                upserts.insert(
                    reference.doc_key.clone(),
                    OutboxUpsert {
                        doc_key: reference.doc_key.clone(),
                        doc_version: reference.doc_version.clone(),
                        input_digest: (*digest).to_string(),
                    },
                );
            }
        }
        for reference in &batch.delta.removed {
            removals.insert(reference.doc_key.clone());
        }
    }
    let upsert_list: Vec<OutboxUpsert> = upserts.into_values().collect();
    let removal_list: Vec<String> = removals.into_iter().collect();
    supersede_and_enqueue_on(
        conn,
        &OutboxPlan {
            upserts: &upsert_list,
            removals: &removal_list,
            now_unix: now,
        },
    )
}

/// Ready tasks: `pending` rows at or before the caller's clock, oldest first
/// (`available_at`, then `task_id`). Served through the `semantic_outbox_ready`
/// index; claiming/leasing stays P6-007's fenced domain.
pub fn ready_tasks_on(conn: &Connection, limit: usize, now_unix: f64) -> CcResult<Vec<OutboxTask>> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT task_id,doc_key,doc_version,input_digest,space_id,op,attempt_count,\
             available_at FROM semantic_outbox \
             WHERE state='pending' AND available_at<=?1 ORDER BY available_at,task_id LIMIT ?2",
        )
        .map_err(db_err)?;
    let rows = stmt
        .query_map(rusqlite::params![now_unix, limit as i64], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, f64>(7)?,
            ))
        })
        .map_err(db_err)?;
    let mut tasks = Vec::new();
    for row in rows {
        let (
            task_id,
            doc_key,
            doc_version,
            input_digest,
            space_id,
            op,
            attempt_count,
            available_at,
        ) = row.map_err(db_err)?;
        tasks.push(OutboxTask {
            task_id,
            doc_key,
            doc_version,
            input_digest,
            space_id,
            op: OutboxOp::from_db(&op)?,
            attempt_count,
            available_at,
        });
    }
    Ok(tasks)
}

/// Advance one task's state under the typed transition guard. Writes exactly
/// `state`/`updated_at`/`last_error` — never the lease columns (P6-007). A
/// legal transition whose row is not in `expected_from` (e.g. a concurrent
/// writer won) returns `Ok(false)` and changes nothing; an illegal
/// transition is a caller bug and fails loudly.
pub fn transition_state_on(
    conn: &Connection,
    task_id: i64,
    expected_from: OutboxState,
    to: OutboxState,
    now_unix: f64,
    last_error: Option<&str>,
) -> CcResult<bool> {
    if !expected_from.can_transition_to(to) {
        return Err(CcError::InvalidParams(format!(
            "illegal semantic outbox transition {} -> {}",
            expected_from.as_str(),
            to.as_str()
        )));
    }
    let ts = timestamp_text(now_unix)?;
    let updated = conn
        .prepare_cached(
            "UPDATE semantic_outbox SET state=?1, updated_at=?2, last_error=?3 \
             WHERE task_id=?4 AND state=?5",
        )
        .map_err(db_err)?
        .execute(rusqlite::params![
            to.as_str(),
            ts,
            last_error,
            task_id,
            expected_from.as_str()
        ])
        .map_err(db_err)?;
    Ok(updated == 1)
}

// ── lease fencing (P6-007): claim / renew / ack / retry / reclaim ───────

/// One task atomically claimed by [`claim_next_on`], carrying everything the
/// worker needs to do the work and to keep its lease: the fresh per-attempt
/// `token`, the task identity, and the current `lease_expires_at` deadline.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaimedTask {
    pub task_id: i64,
    pub token: String,
    pub doc_key: String,
    pub doc_version: String,
    pub input_digest: String,
    pub op: OutboxOp,
    pub lease_expires_at: f64,
}

/// Claim fairness policy of the semantic worker (P7-005; 接线轮待办 8, the
/// P6-013 deviation-3 hand-over "公平化作为 ORDER BY 可注入参数"). The claim's
/// candidate `ORDER BY` is this CLOSED enum — never an arbitrary SQL
/// fragment, so the injection cannot smuggle SQL in.
///
/// Scope note: this is fairness WITHIN one project's outbox (the active
/// space of one `IndexDb`). Fairness ACROSS projects — several projects
/// sharing one process-wide provider — is the shared provider gate's job
/// (`cc_semantic::admission::ProviderGate`, P7-005), not the outbox's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClaimFairness {
    /// Arrival order (`ORDER BY task_id`): the P6-007 delivery and the
    /// default. Strict FIFO across docs; every existing caller keeps it.
    #[default]
    Fifo,
    /// Doc rotation (`ORDER BY` the doc's least-recent `updated_at` first,
    /// ties by `task_id`): serve the doc whose rows were touched LEAST
    /// recently. A doc under continuous re-editing (or repeated retrying)
    /// keeps its rows' `updated_at` fresh and yields to docs whose work has
    /// been waiting longer — fair rotation across docs instead of pure
    /// arrival order.
    DocRoundRobin,
}

impl ClaimFairness {
    /// The candidate-selection `ORDER BY` body. Closed enum ⇒ the only two
    /// fragments that can ever reach the claim statement.
    fn order_clause(self) -> &'static str {
        match self {
            ClaimFairness::Fifo => "task_id ASC",
            // A live pending row's own updated_at is always in the MAX, so
            // the expression is never NULL here.
            ClaimFairness::DocRoundRobin => {
                "(SELECT MAX(p.updated_at) FROM semantic_outbox p \
                  WHERE p.doc_key = semantic_outbox.doc_key \
                    AND p.space_id = semantic_outbox.space_id) ASC, task_id ASC"
            }
        }
    }
}

/// Atomically claim the oldest ready `pending` task of `space_id` for
/// `owner` (diagnostics only — fencing judges by token, never by owner).
///
/// The claim is ONE CAS statement (`UPDATE ... WHERE task_id = (SELECT ...
/// state='pending' ...) RETURNING`): there is no SELECT-then-UPDATE window,
/// so of any number of racing claimers exactly one wins the row and every
/// other caller observes `None`. The winner gets a fresh per-attempt token,
/// `attempt_count` is incremented, and the lease columns
/// (`lease_token`/`lease_expires_at`/`claim_owner`) are written atomically
/// with the state flip. `now_unix` is the caller's clock; the lease expires
/// at `now_unix + lease_secs`. Future tasks (`available_at > now_unix`) and
/// rows of other spaces are never claimed.
///
/// Auxiliary effect: no epoch ever moves.
pub fn claim_next_on(
    conn: &Connection,
    space_id: &str,
    owner: &str,
    now_unix: f64,
    lease_secs: f64,
) -> CcResult<Option<ClaimedTask>> {
    claim_next_fair_on(
        conn,
        space_id,
        owner,
        now_unix,
        lease_secs,
        ClaimFairness::default(),
    )
}

/// [`claim_next_on`] with the injectable claim fairness policy (P7-005,
/// 接线轮待办 8). Identical CAS/fencing semantics; only the candidate
/// selection order differs ([`ClaimFairness`]).
pub fn claim_next_fair_on(
    conn: &Connection,
    space_id: &str,
    owner: &str,
    now_unix: f64,
    lease_secs: f64,
    fairness: ClaimFairness,
) -> CcResult<Option<ClaimedTask>> {
    if lease_secs <= 0.0 {
        return Err(CcError::InvalidParams(
            "semantic outbox lease_secs must be positive".into(),
        ));
    }
    // The state flip (pending → claimed) is a closed-table edge; assert the
    // linkage instead of silently drifting away from the typed machine.
    debug_assert!(OutboxState::Pending.can_transition_to(OutboxState::Claimed));
    let ts = timestamp_text(now_unix)?;
    let sql = format!(
        "UPDATE semantic_outbox \
         SET state='claimed', lease_token=lower(hex(randomblob(16))), \
             lease_expires_at=?1+?2, claim_owner=?3, attempt_count=attempt_count+1, \
             updated_at=?4 \
         WHERE task_id = (SELECT task_id FROM semantic_outbox \
                          WHERE space_id=?5 AND state='pending' AND available_at<=?1 \
                          ORDER BY {} LIMIT 1) \
         RETURNING task_id, lease_token, doc_key, doc_version, input_digest, op, \
                   lease_expires_at",
        fairness.order_clause()
    );
    let mut stmt = conn.prepare_cached(&sql).map_err(db_err)?;
    let mut rows = stmt
        .query(rusqlite::params![now_unix, lease_secs, owner, ts, space_id])
        .map_err(db_err)?;
    match rows.next().map_err(db_err)? {
        None => Ok(None),
        Some(row) => {
            let op: String = row.get(5).map_err(db_err)?;
            Ok(Some(ClaimedTask {
                task_id: row.get(0).map_err(db_err)?,
                token: row.get(1).map_err(db_err)?,
                doc_key: row.get(2).map_err(db_err)?,
                doc_version: row.get(3).map_err(db_err)?,
                input_digest: row.get(4).map_err(db_err)?,
                op: OutboxOp::from_db(&op)?,
                lease_expires_at: row.get(6).map_err(db_err)?,
            }))
        }
    }
}

/// Fenced guard shared by renew/ack/retry: the caller must present the exact
/// per-attempt token on a row that is still `claimed`. `claim_owner` is never
/// consulted (multiple processes may share an owner string; the token is the
/// only credential). Returns `Ok(false)` — lease lost, task superseded or
/// already finished — and writes nothing.
fn fenced_lease_update(
    conn: &Connection,
    task_id: i64,
    token: &str,
    set_sql: &str,
    set_params: &[&dyn rusqlite::ToSql],
) -> CcResult<bool> {
    let sql = format!(
        "UPDATE semantic_outbox SET {set_sql} \
         WHERE task_id=? AND lease_token=? AND state='claimed'"
    );
    let mut params: Vec<&dyn rusqlite::ToSql> = set_params.to_vec();
    params.push(&task_id);
    params.push(&token);
    let updated = conn
        .prepare_cached(&sql)
        .map_err(db_err)?
        .execute(params.as_slice())
        .map_err(db_err)?;
    Ok(updated == 1)
}

/// Heartbeat: extend the caller's lease to `now_unix + lease_secs`. Only the
/// current token holder succeeds; a token whose lease was reclaimed and
/// re-claimed, or whose task left `claimed`, is rejected with `Ok(false)` and
/// nothing is written. Renewal does not consume an attempt.
pub fn renew_lease_on(
    conn: &Connection,
    task_id: i64,
    token: &str,
    now_unix: f64,
    lease_secs: f64,
) -> CcResult<bool> {
    if lease_secs <= 0.0 {
        return Err(CcError::InvalidParams(
            "semantic outbox lease_secs must be positive".into(),
        ));
    }
    let ts = timestamp_text(now_unix)?;
    let expires_at = now_unix + lease_secs;
    fenced_lease_update(
        conn,
        task_id,
        token,
        "lease_expires_at=?1, updated_at=?2",
        &[&expires_at, &ts],
    )
}

/// Ack success: `claimed → done` for the token holder only (closed-table
/// edge). Terminal — a repeat ack or any later stale-token write finds no
/// claimed row and returns `Ok(false)`.
pub fn ack_done_on(conn: &Connection, task_id: i64, token: &str, now_unix: f64) -> CcResult<bool> {
    debug_assert!(OutboxState::Claimed.can_transition_to(OutboxState::Done));
    let ts = timestamp_text(now_unix)?;
    fenced_lease_update(
        conn,
        task_id,
        token,
        "state='done', updated_at=?1, last_error=NULL",
        &[&ts],
    )
}

/// Ack failure with retry bookkeeping: back to `pending` for backoff
/// (`available_at = now_unix + backoff_secs`) while attempts remain, or
/// dead-letter to terminal `failed` once `attempt_count >= max_attempts`.
/// Either way the lease is cleared — the caller loses all credentials — and
/// `last_error` persists the reason for audit. One atomic statement (the
/// closed table admits `claimed → pending` and `claimed → failed`); a stale
/// token wins nothing.
pub fn retry_on(
    conn: &Connection,
    task_id: i64,
    token: &str,
    err: &str,
    now_unix: f64,
    backoff_secs: f64,
    max_attempts: u32,
) -> CcResult<bool> {
    debug_assert!(OutboxState::Claimed.can_transition_to(OutboxState::Pending));
    debug_assert!(OutboxState::Claimed.can_transition_to(OutboxState::Failed));
    let ts = timestamp_text(now_unix)?;
    fenced_lease_update(
        conn,
        task_id,
        token,
        "state = CASE WHEN attempt_count >= ?1 THEN 'failed' ELSE 'pending' END, \
         available_at = CASE WHEN attempt_count >= ?1 THEN available_at \
                             ELSE ?2 + ?3 END, \
         lease_token=NULL, lease_expires_at=NULL, claim_owner=NULL, \
         last_error=?4, updated_at=?5",
        &[&(max_attempts as i64), &now_unix, &backoff_secs, &err, &ts],
    )
}

/// Third-party reclaim of expired leases: every `claimed` row whose
/// `lease_expires_at` is in the past returns to `pending` with its lease
/// cleared and `available_at = now_unix` (immediately claimable again).
/// Attempts are not consumed — the claim that follows increments the counter.
/// Reclaiming an unexpired lease is impossible by construction; only rows in
/// the past window match. Returns the number of reclaimed tasks. Auxiliary
/// effect; the periodic bounded scan orchestration is P6-015.
pub fn reclaim_expired_on(conn: &Connection, now_unix: f64) -> CcResult<usize> {
    debug_assert!(OutboxState::Claimed.can_transition_to(OutboxState::Pending));
    let ts = timestamp_text(now_unix)?;
    conn.prepare_cached(
        "UPDATE semantic_outbox \
         SET state='pending', available_at=?1, lease_token=NULL, lease_expires_at=NULL, \
             claim_owner=NULL, updated_at=?2 \
         WHERE state='claimed' AND lease_expires_at<?1",
    )
    .map_err(db_err)?
    .execute(rusqlite::params![now_unix, ts])
    .map_err(db_err)
}
