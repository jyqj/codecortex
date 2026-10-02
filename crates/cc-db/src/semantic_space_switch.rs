//! Model space switching (P6-017): the `semantic_spaces` three-state machine,
//! the single-transaction active-space switch, and the `op='revoke'` producer
//! + consumer it owns.
//!
//! Boundary authority: `docs/adr/0003-semantic-persistence-single-db-boundary.md`
//! (row P6-017: "新空间回填/切 active/撤销三段；不同空间分数永不混排；旧 cache
//! 经校验可回滚复用") and `artifacts/checkpoints/p6-implementation-planning-
//! 20261002/TASK-BRIEFS.md` P6-017 (steps 1-3 + "记录用户 revision 与未 pin
//! 限制"). Module placement deviation (documented per the batch red line, same
//! reasoning as P6-016's sweep placement): the brief assigns the orchestration
//! to `cc-semantic/spec.rs + reconcile.rs`, but `reconcile.rs` is a sealed
//! P6-014 deliverable and the red line says "不改既有交付物（只调用/组合）" —
//! the state machine and its SQL therefore live in this NEW cc-db module, and
//! the cc-semantic side (`cc_semantic::space_switch`) only composes these
//! facades with the frozen spec types.
//!
//! ## The three-state machine (closed transition table)
//!
//! ```text
//! backfilling ──activate──▶ active ──switch away──▶ revoked
//!                              ▲                      │
//!                              └──── rollback ────────┘
//! ```
//!
//! Every edge is guarded by [`SpaceState::can_transition_to`]; an illegal edge
//! is a caller bug and fails loudly (same discipline as the outbox
//! `OutboxState` machine). `register_space_on` inserts `backfilling` rows and
//! refuses an existing row in ANY state — re-activating a revoked space goes
//! through the `revoked → active` edge, never a re-registration.
//!
//! ## The switch is ONE transaction (brief step 2)
//!
//! [`switch_active_space_on`] runs on the caller's open transaction and does,
//! atomically: old active → `revoked`, supersede EVERY live task of the old
//! space (pending embeds of a revoked space would be paid for and then fenced
//! out by the publish CAS anyway — superseding here saves exactly that waste),
//! new space → `active` (+`activated_at`), and the revoke producer: ONE
//! `op='revoke'` task per old-space `semantic_manifest` row via a set-based
//! `INSERT .. SELECT` (brief P6-006 风险注: "禁止逐行"). The facade
//! ([`IndexDb::switch_semantic_active_space`]) declares the `Semantic` effect
//! exactly when the retrieval-visible set switched (`old active` existed and
//! differed from the target — the dense lane reads the active space only, so
//! the flip changes what queries can see; a FIRST activation switches nothing
//! because the visible set was empty before and after, and the epoch key stays
//! absent = "not ready", P6-004).
//!
//! ## Revoke consumption (the second production consumer)
//!
//! `semantic_queue.rs` reserved the ack path for exactly this consumer
//! (P6-006 left `op='revoke'` with no producer, P6-013's embed handler refuses
//! the op). [`IndexDb::consume_semantic_revoke`] is one `IMMEDIATE`
//! transaction: delete the task's OWN-space manifest row (the `AND space_id`
//! guard can never touch a row republished under the new space), then the
//! fenced `claimed → done` ack; a lost lease rolls the delete back with it.
//! The `Semantic` effect is declared exactly when a row was actually removed
//! (Q4: bump iff the visible set changed).
//!
//! Rollback reuse contract (brief step 3): a revoked space's cache artifacts
//! are never deleted by this module; while the old manifest rows exist they
//! are GC-protected (P6-016 marks ALL spaces), and after the revoke drain
//! consumes them the artifacts survive on the cache side until an aged GC
//! pass reclaims them as orphans — the rollback window is therefore "switch
//! back before the drain converges, or re-embed whatever an aged GC already
//! reclaimed" (`cache.get` re-verifies every reused object; the rollback
//! orchestration + zero-provider-call proof lives in the cc-semantic tests).
//!
//! ## Switch audit + the 未 pin declaration
//!
//! Every effective switch appends one event to the `semantic_space_switch_log`
//! metadata key (brief: "记录用户 revision 与未 pin 限制"; the schema red line
//! forbids new columns, so the metadata key is the sanctioned carrier). Each
//! event records `from`/`to`/caller `revision` and `pinned: false` — the
//! machine-readable statement that an unpinned configuration does not
//! guarantee cross-version behavior. Metadata events are rare human-driven
//! writes; no retention policy is imposed at this layer.

use rusqlite::Connection;
use serde::Serialize;

use cc_model::{CcError, CcResult};

use crate::index_db::IndexDb;
use crate::semantic_outbox::{
    self, ClaimedTask, OutboxUpsert, OutboxWriteStats, OutboxState,
};
use crate::sql_util::{db_err, sql_in_placeholders, IN_BATCH_SIZE};

/// Metadata key carrying the append-only switch event log.
pub const SWITCH_LOG_KEY: &str = "semantic_space_switch_log";

/// The typed state of a `semantic_spaces` row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceState {
    Backfilling,
    Active,
    Revoked,
}

impl SpaceState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Backfilling => "backfilling",
            Self::Active => "active",
            Self::Revoked => "revoked",
        }
    }

    pub fn from_db(value: &str) -> CcResult<Self> {
        match value {
            "backfilling" => Ok(Self::Backfilling),
            "active" => Ok(Self::Active),
            "revoked" => Ok(Self::Revoked),
            other => Err(CcError::InvalidParams(format!(
                "unknown semantic space state '{other}'"
            ))),
        }
    }

    /// The closed legal-transition table: `backfilling → active` (activation,
    /// first or after rollback), `active → revoked` (switch away), and
    /// `revoked → active` (rollback switch-back, brief step 3). A space never
    /// returns to `backfilling` — a revoked space's unfinished backfill
    /// resumes under `active`, because activation is what makes its queue
    /// claimable.
    pub const fn can_transition_to(self, to: Self) -> bool {
        use SpaceState::*;
        matches!(
            (self, to),
            (Backfilling, Active) | (Active, Revoked) | (Revoked, Active)
        )
    }
}

/// One event of the append-only switch audit log (`semantic_space_switch_log`
/// metadata key). `pinned: false` is the brief's "未 pin = 不保证跨版本行为"
/// declaration, recorded with every event.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SpaceSwitchEvent<'a> {
    pub at: &'a str,
    pub from: Option<&'a str>,
    pub to: &'a str,
    pub revision: &'a str,
    pub pinned: bool,
}

/// What one switch actually did (`previous_active` is the pre-switch pointer).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpaceSwitchStats {
    pub previous_active: Option<String>,
    /// The target row actually flipped into `active` (a no-op re-switch of
    /// the already-active space leaves this `false`).
    pub activated: bool,
    /// The previous active space actually flipped into `revoked`.
    pub old_revoked: bool,
    /// Live (`pending`/`claimed`) tasks of the old space superseded.
    pub superseded_live_tasks: usize,
    /// `op='revoke'` tasks enqueued for the old space's manifest rows.
    pub revoke_tasks_enqueued: usize,
    /// Whether retrieval-visible content switched: exactly when a previous
    /// active space existed and differed from the target (the facade declares
    /// the `Semantic` effect on this flag, Q4 口径).
    pub visible_set_switched: bool,
}

impl SpaceSwitchStats {
    /// Whether this switch changed semantic state at all (effect declaration
    /// input; a pure no-op switch moves nothing).
    pub fn changed_semantic_state(&self) -> bool {
        self.activated || self.old_revoked
    }
}

/// Outcome of one fenced revoke consumption.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevokeOutcome {
    /// The fenced ack landed (`claimed → done` under the presented token).
    /// `false` = lease lost: nothing was written, the manifest row (if it
    /// existed) survived.
    pub acked: bool,
    /// A manifest row of THIS space was actually removed — the caller declares
    /// the `Semantic` effect exactly on this flag (Q4).
    pub visible_set_changed: bool,
}

fn timestamp_text(now_unix: f64) -> CcResult<String> {
    let secs = now_unix.floor() as i64;
    let nanos = ((now_unix - secs as f64) * 1e9).round() as u32;
    chrono::DateTime::from_timestamp(secs, nanos)
        .map(|dt| dt.to_rfc3339())
        .ok_or_else(|| CcError::InvalidParams("semantic space switch timestamp out of range".into()))
}

/// Read one space's current state (+`activated_at`), or `None` when unknown.
pub fn space_state_on(conn: &Connection, space_id: &str) -> CcResult<Option<(SpaceState, Option<String>)>> {
    let raw = conn
        .query_row(
            "SELECT state, activated_at FROM semantic_spaces WHERE space_id=?1",
            [space_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(db_err(other)),
        })?;
    match raw {
        Some((state, activated_at)) => Ok(Some((SpaceState::from_db(&state)?, activated_at))),
        None => Ok(None),
    }
}

/// Register a new space row in `backfilling` (brief step 1, the state carrier
/// half). Refuses an existing row in ANY state — a revoked space is
/// re-activated through its `revoked → active` edge, never re-registered.
/// Auxiliary: a `backfilling` row is invisible to every reader (the dense
/// lane reads the active space only), so no epoch moves.
pub fn register_space_on(
    conn: &Connection,
    space_id: &str,
    spec_json: &str,
    now_unix: f64,
) -> CcResult<()> {
    if let Some((state, _)) = space_state_on(conn, space_id)? {
        return Err(CcError::InvalidParams(format!(
            "semantic space {space_id} already registered in state '{}'",
            state.as_str()
        )));
    }
    let _ = now_unix; // kept for call-shape symmetry; spaces carry activated_at only
    conn.prepare_cached(
        "INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES(?1,?2,'backfilling')",
    )
    .map_err(db_err)?
    .execute(rusqlite::params![space_id, spec_json])
    .map_err(db_err)?;
    Ok(())
}

/// Append one switch event to the audit log (read-modify-write of the single
/// metadata key, inside the caller's transaction).
fn append_switch_log_on(conn: &Connection, event: &SpaceSwitchEvent<'_>) -> CcResult<()> {
    let mut log: Vec<serde_json::Value> = match conn.query_row(
        "SELECT value FROM metadata WHERE key=?1",
        [SWITCH_LOG_KEY],
        |row| row.get::<_, String>(0),
    ) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|e| CcError::Database(format!("switch log unreadable: {e}")))?,
        Err(rusqlite::Error::QueryReturnedNoRows) => Vec::new(),
        Err(e) => return Err(db_err(e)),
    };
    log.push(
        serde_json::to_value(event)
            .map_err(|e| CcError::Database(format!("switch event not serializable: {e}")))?,
    );
    let text = serde_json::to_string(&log)
        .map_err(|e| CcError::Database(format!("switch log not serializable: {e}")))?;
    conn.execute(
        "INSERT INTO metadata(key,value) VALUES(?1,?2) \
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        rusqlite::params![SWITCH_LOG_KEY, text],
    )
    .map_err(db_err)?;
    Ok(())
}

/// Supersede every live (`pending`/`claimed`) task of one space — embeds of a
/// space being revoked, and stale live revoke tasks from a previous
/// revocation of the same space (the `semantic_outbox_live_per_doc` unique
/// index admits only one live task per `(doc_key, space)`, so the fresh
/// INSERT..SELECT below must find no live rows of the old space).
fn supersede_space_live_tasks_on(
    conn: &Connection,
    space_id: &str,
    now_unix: f64,
) -> CcResult<usize> {
    let ts = timestamp_text(now_unix)?;
    conn.prepare_cached(
        "UPDATE semantic_outbox SET state='superseded', updated_at=?2 \
         WHERE space_id=?1 AND state IN ('pending','claimed')",
    )
    .map_err(db_err)?
    .execute(rusqlite::params![space_id, ts])
    .map_err(db_err)
}

/// Execute the active-space switch on the caller's open transaction (brief
/// step 2 + the revoke producer of step 3).
///
/// Guards (the three-state machine): the target row must exist and sit in
/// `backfilling` or `revoked`; switching to the already-`active` space is a
/// no-op (`activated=false`, nothing written); an illegal target state fails
/// loudly. Ordering inside the transaction:
///
/// 1. old active → `revoked` (guarded `Active → Revoked`);
/// 2. supersede ALL live tasks of the old space (pending embeds of a revoked
///    space are fenced out at publish anyway; stale live revoke tasks would
///    collide with the fresh ones on the unique index);
/// 3. target → `active` + `activated_at` (guarded `backfilling|revoked →
///    active`; a rowcount of zero here means a concurrent writer moved the
///    row — fail the transaction rather than guess);
/// 4. revoke producer: `INSERT .. SELECT` one `op='revoke'` pending task per
///    old-space manifest row (set-based, never per-row);
/// 5. append the audit event (`from`/`to`/`revision`/`pinned: false`).
///
/// Never bumps any clock itself — the facade declares the `Semantic` effect
/// exactly when [`SpaceSwitchStats::visible_set_switched`] is true.
pub fn switch_active_space_on(
    conn: &Connection,
    new_space_id: &str,
    trigger_revision: &str,
    now_unix: f64,
) -> CcResult<SpaceSwitchStats> {
    let ts = timestamp_text(now_unix)?;
    let previous_active = semantic_outbox::active_space_on(conn)?;
    let mut stats = SpaceSwitchStats {
        previous_active: previous_active.clone(),
        ..SpaceSwitchStats::default()
    };
    if previous_active.as_deref() == Some(new_space_id) {
        // Idempotent no-op: the target already carries the pointer.
        return Ok(stats);
    }
    let Some((target_state, _)) = space_state_on(conn, new_space_id)? else {
        return Err(CcError::InvalidParams(format!(
            "unknown semantic space '{new_space_id}'"
        )));
    };
    if !target_state.can_transition_to(SpaceState::Active) {
        return Err(CcError::InvalidParams(format!(
            "illegal semantic space transition {} -> active for '{new_space_id}'",
            target_state.as_str()
        )));
    }

    if let Some(old_space_id) = &previous_active {
        let revoked = conn
            .prepare_cached("UPDATE semantic_spaces SET state='revoked' WHERE space_id=?1 AND state='active'")
            .map_err(db_err)?
            .execute([old_space_id])
            .map_err(db_err)?;
        if revoked != 1 {
            return Err(CcError::Database(format!(
                "active space '{old_space_id}' moved concurrently during switch"
            )));
        }
        stats.old_revoked = true;
        stats.superseded_live_tasks = supersede_space_live_tasks_on(conn, old_space_id, now_unix)?;
        stats.revoke_tasks_enqueued = conn
            .prepare_cached(
                "INSERT INTO semantic_outbox(doc_key,doc_version,input_digest,space_id,op,state,\
                 attempt_count,available_at,created_at,updated_at) \
                 SELECT doc_key,doc_version,input_digest,space_id,'revoke','pending',0,?1,?2,?2 \
                 FROM semantic_manifest WHERE space_id=?3",
            )
            .map_err(db_err)?
            .execute(rusqlite::params![now_unix, ts, old_space_id])
            .map_err(db_err)?;
    }

    let activated = conn
        .prepare_cached(
            "UPDATE semantic_spaces SET state='active', activated_at=?2 \
             WHERE space_id=?1 AND state IN ('backfilling','revoked')",
        )
        .map_err(db_err)?
        .execute(rusqlite::params![new_space_id, ts])
        .map_err(db_err)?;
    if activated != 1 {
        return Err(CcError::Database(format!(
            "target space '{new_space_id}' moved concurrently during switch"
        )));
    }
    stats.activated = true;
    stats.visible_set_switched = stats.old_revoked;

    append_switch_log_on(
        conn,
        &SpaceSwitchEvent {
            at: &ts,
            from: previous_active.as_deref(),
            to: new_space_id,
            revision: trigger_revision,
            pinned: false,
        },
    )?;
    Ok(stats)
}

/// Enqueue the backfill desired set under one `backfilling` space (brief
/// step 1's queue half). The P6-006 write path can never produce tasks for a
/// non-active space (`supersede_and_enqueue_on` pins the active space), so
/// the backfill plan has its own guarded writer: the space row must exist in
/// exactly `backfilling` state, then per batch: supersede this space's live
/// tasks for the doc, plain-INSERT the fresh `pending` embed (the
/// `semantic_outbox_live_per_doc` unique index must NEVER be swallowed — a
/// conflict after an explicit supersede is a bug and fails the transaction,
/// P6-006 convention).
///
/// Epoch discipline follows the delivered `enqueue_semantic_rebuild_plan`
/// convention (P6-006 Q4 口径 "any nonzero stat is a semantic change"): the
/// facade bumps `semantic_epoch` exactly when the plan changed queue state.
pub fn enqueue_backfill_on(
    conn: &Connection,
    space_id: &str,
    upserts: &[OutboxUpsert],
    now_unix: f64,
) -> CcResult<OutboxWriteStats> {
    let mut stats = OutboxWriteStats::default();
    match space_state_on(conn, space_id)? {
        Some((SpaceState::Backfilling, _)) => {}
        Some((state, _)) => {
            return Err(CcError::InvalidParams(format!(
                "semantic space '{space_id}' is '{}' — backfill enqueue requires 'backfilling'",
                state.as_str()
            )));
        }
        None => {
            return Err(CcError::InvalidParams(format!(
                "unknown semantic space '{space_id}'"
            )));
        }
    }
    let ts = timestamp_text(now_unix)?;
    for batch in upserts.chunks(IN_BATCH_SIZE) {
        if batch.is_empty() {
            continue;
        }
        let doc_keys: Vec<&str> = batch.iter().map(|u| u.doc_key.as_str()).collect();
        let placeholders = sql_in_placeholders(doc_keys.len());
        let space_slot = doc_keys.len() + 1;
        let ts_slot = doc_keys.len() + 2;
        let sql = format!(
            "UPDATE semantic_outbox SET state='superseded', updated_at=?{ts_slot} \
             WHERE space_id=?{space_slot} AND doc_key IN ({placeholders}) \
               AND state IN ('pending','claimed')"
        );
        let mut params: Vec<&dyn rusqlite::ToSql> =
            doc_keys.iter().map(|k| k as &dyn rusqlite::ToSql).collect();
        params.push(&space_id);
        params.push(&ts);
        stats.superseded_tasks += conn
            .prepare_cached(&sql)
            .map_err(db_err)?
            .execute(params.as_slice())
            .map_err(db_err)?;
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
                now_unix,
                ts
            ])
            .map_err(db_err)?;
            stats.enqueued += 1;
        }
    }
    Ok(stats)
}

/// Count one space's live (`pending`/`claimed`) revoke tasks — the drain
/// driver's convergence hint.
pub fn live_revoke_count_on(conn: &Connection, space_id: &str) -> CcResult<u64> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM semantic_outbox \
             WHERE space_id=?1 AND op='revoke' AND state IN ('pending','claimed')",
            [space_id],
            |row| row.get(0),
        )
        .map_err(db_err)?;
    Ok(count.max(0) as u64)
}

/// Consume one claimed revoke task on the caller's open transaction: delete
/// the task's OWN-space manifest row, then the fenced `claimed → done` ack —
/// one atomic unit. The `AND space_id` guard means a document republished
/// under the new space before its revoke task ran is NOT deleted (the task
/// still acks: the old space's row is gone either way, by supersede or by
/// never existing under the old space). A lost lease (`acked=false`) leaves
/// the transaction with nothing to commit — the facade rolls back, so the
/// delete can never survive a lost voice.
pub fn consume_revoke_on(
    conn: &Connection,
    task_id: i64,
    lease_token: &str,
    doc_key: &str,
    space_id: &str,
    now_unix: f64,
) -> CcResult<RevokeOutcome> {
    debug_assert!(OutboxState::Claimed.can_transition_to(OutboxState::Done));
    let deleted = conn
        .prepare_cached("DELETE FROM semantic_manifest WHERE doc_key=?1 AND space_id=?2")
        .map_err(db_err)?
        .execute(rusqlite::params![doc_key, space_id])
        .map_err(db_err)?;
    let acked = semantic_outbox::ack_done_on(conn, task_id, lease_token, now_unix)?;
    Ok(RevokeOutcome {
        acked,
        visible_set_changed: acked && deleted > 0,
    })
}

impl IndexDb {
    /// Register a new `backfilling` space (brief step 1, state carrier).
    /// Auxiliary: a `backfilling` row is invisible to every reader, no epoch
    /// moves. Refuses an existing row in any state.
    pub fn register_semantic_space(&self, space_id: &str, spec_json: &str) -> CcResult<()> {
        let conn = self.write_conn.lock().map_err(db_err)?;
        conn.execute_batch("BEGIN IMMEDIATE;")
            .map_err(|e| CcError::Database(format!("begin space registration: {e}")))?;
        let outcome = register_space_on(&conn, space_id, spec_json, semantic_outbox::now_unix());
        match (outcome, conn.execute_batch("COMMIT;")) {
            (Ok(()), Ok(())) => Ok(()),
            (Ok(()), Err(e)) => {
                let _ = conn.execute_batch("ROLLBACK;");
                Err(CcError::Database(format!("commit space registration: {e}")))
            }
            (Err(e), _) => {
                let _ = conn.execute_batch("ROLLBACK;");
                Err(e)
            }
        }
    }

    /// Enqueue the backfill desired set under one `backfilling` space. One
    /// `IMMEDIATE` short transaction over [`enqueue_backfill_on`], declaring
    /// the `Semantic` effect exactly when the plan changed queue state (the
    /// delivered rebuild-re-enqueue convention).
    pub fn enqueue_semantic_backfill_plan(
        &self,
        space_id: &str,
        upserts: &[OutboxUpsert],
    ) -> CcResult<OutboxWriteStats> {
        let conn = self.write_conn.lock().map_err(db_err)?;
        conn.execute_batch("BEGIN IMMEDIATE;")
            .map_err(|e| CcError::Database(format!("begin backfill enqueue: {e}")))?;
        let outcome =
            enqueue_backfill_on(&conn, space_id, upserts, semantic_outbox::now_unix());
        let outcome = match outcome {
            Ok(stats) => {
                if stats.changed_semantic_state() {
                    match IndexDb::bump_semantic_epoch_on(&conn) {
                        Ok(()) => Ok(stats),
                        Err(e) => Err(e),
                    }
                } else {
                    Ok(stats)
                }
            }
            Err(e) => Err(e),
        };
        match (outcome, conn.execute_batch("COMMIT;")) {
            (Ok(stats), Ok(())) => Ok(stats),
            (Ok(_), Err(e)) => {
                let _ = conn.execute_batch("ROLLBACK;");
                Err(CcError::Database(format!("commit backfill enqueue: {e}")))
            }
            (Err(e), _) => {
                let _ = conn.execute_batch("ROLLBACK;");
                Err(e)
            }
        }
    }

    /// The three-stage switch's stage 2 (brief step 2): one `IMMEDIATE` short
    /// transaction over [`switch_active_space_on`], declaring the `Semantic`
    /// effect (bump `semantic_epoch` — 可见集合切换) exactly when the retrieval
    /// visible set switched. A no-op re-switch of the already-active space is
    /// Auxiliary and still appends nothing.
    pub fn switch_semantic_active_space(
        &self,
        new_space_id: &str,
        trigger_revision: &str,
    ) -> CcResult<SpaceSwitchStats> {
        let conn = self.write_conn.lock().map_err(db_err)?;
        conn.execute_batch("BEGIN IMMEDIATE;")
            .map_err(|e| CcError::Database(format!("begin space switch: {e}")))?;
        let outcome =
            switch_active_space_on(&conn, new_space_id, trigger_revision, semantic_outbox::now_unix());
        let outcome = match outcome {
            Ok(stats) => {
                if stats.visible_set_switched {
                    match IndexDb::bump_semantic_epoch_on(&conn) {
                        Ok(()) => Ok(stats),
                        Err(e) => Err(e),
                    }
                } else {
                    Ok(stats)
                }
            }
            Err(e) => Err(e),
        };
        match (outcome, conn.execute_batch("COMMIT;")) {
            (Ok(stats), Ok(())) => Ok(stats),
            (Ok(_), Err(e)) => {
                let _ = conn.execute_batch("ROLLBACK;");
                Err(CcError::Database(format!("commit space switch: {e}")))
            }
            (Err(e), _) => {
                let _ = conn.execute_batch("ROLLBACK;");
                Err(e)
            }
        }
    }

    /// Claim the oldest ready task of an EXPLICIT space (the revoked-space
    /// drain needs this: [`Self::claim_semantic`] pins the active space, and a
    /// revoked space is by definition not active). Auxiliary; identical lease
    /// semantics.
    pub fn claim_semantic_space(
        &self,
        space_id: &str,
        owner: &str,
        lease_secs: f64,
    ) -> CcResult<Option<ClaimedTask>> {
        let conn = self.write_conn.lock().map_err(db_err)?;
        conn.execute_batch("BEGIN IMMEDIATE;")
            .map_err(|e| CcError::Database(format!("begin space claim: {e}")))?;
        let outcome = semantic_outbox::claim_next_on(
            &conn,
            space_id,
            owner,
            semantic_outbox::now_unix(),
            lease_secs,
        );
        match (outcome, conn.execute_batch("COMMIT;")) {
            (Ok(claimed), Ok(())) => Ok(claimed),
            (Ok(_), Err(e)) => {
                let _ = conn.execute_batch("ROLLBACK;");
                Err(CcError::Database(format!("commit space claim: {e}")))
            }
            (Err(e), _) => {
                let _ = conn.execute_batch("ROLLBACK;");
                Err(e)
            }
        }
    }

    /// Consume one claimed revoke task (the second production consumer; see
    /// [`consume_revoke_on`]). One `IMMEDIATE` short transaction: the
    /// own-space manifest delete + the fenced ack land together or not at
    /// all, and the `Semantic` effect is declared exactly when a row was
    /// actually removed (Q4: bump iff the visible set changed).
    pub fn consume_semantic_revoke(
        &self,
        task_id: i64,
        lease_token: &str,
        doc_key: &str,
        space_id: &str,
    ) -> CcResult<RevokeOutcome> {
        let conn = self.write_conn.lock().map_err(db_err)?;
        conn.execute_batch("BEGIN IMMEDIATE;")
            .map_err(|e| CcError::Database(format!("begin revoke consume: {e}")))?;
        let outcome = consume_revoke_on(
            &conn,
            task_id,
            lease_token,
            doc_key,
            space_id,
            semantic_outbox::now_unix(),
        );
        let outcome = match outcome {
            Ok(o) if !o.acked => {
                // Lease lost: the delete must not survive a lost voice.
                let _ = conn.execute_batch("ROLLBACK;");
                return Ok(o);
            }
            Ok(o) => {
                if o.visible_set_changed {
                    match IndexDb::bump_semantic_epoch_on(&conn) {
                        Ok(()) => Ok(o),
                        Err(e) => Err(e),
                    }
                } else {
                    Ok(o)
                }
            }
            Err(e) => Err(e),
        };
        match (outcome, conn.execute_batch("COMMIT;")) {
            (Ok(o), Ok(())) => Ok(o),
            (Ok(_), Err(e)) => {
                let _ = conn.execute_batch("ROLLBACK;");
                Err(CcError::Database(format!("commit revoke consume: {e}")))
            }
            (Err(e), _) => {
                let _ = conn.execute_batch("ROLLBACK;");
                Err(e)
            }
        }
    }

    /// Live (`pending`/`claimed`) revoke-task count of one space.
    pub fn live_semantic_revokes(&self, space_id: &str) -> CcResult<u64> {
        let conn = self.read_conn()?;
        live_revoke_count_on(&conn, space_id)
    }

    /// One space's current state (+`activated_at`), `None` when unknown.
    pub fn semantic_space_state(
        &self,
        space_id: &str,
    ) -> CcResult<Option<(SpaceState, Option<String>)>> {
        let conn = self.read_conn()?;
        space_state_on(&conn, space_id)
    }
}
