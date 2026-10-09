//! Resolution debt commits with indexed facts. No independent setter exists.
use crate::{
    index_db::{IndexDb, ReadOps},
    sql_util::db_err,
};
use cc_model::{freshness::*, CcError, CcResult};
use rusqlite::{Connection, OptionalExtension};

/// The payload is validated before indexed facts change. Private fields bind
/// its bytes to the exact immutable state used by the later SQL publication.
pub(crate) struct PreparedUpdate<'a> {
    next: Option<(&'a ReconcileState, String)>,
}

pub(crate) fn prepare_update(update: &ReconcileUpdate) -> CcResult<PreparedUpdate<'_>> {
    let next = update
        .next
        .as_ref()
        .map(|state| state.payload().map(|payload| (state, payload)))
        .transpose()?;
    Ok(PreparedUpdate { next })
}

pub(crate) fn replace_on(conn: &Connection, prepared: PreparedUpdate<'_>) -> CcResult<()> {
    match prepared.next {
        None => {
            conn.execute("DELETE FROM resolution_frontier WHERE id=1", [])
                .map_err(db_err)?;
        }
        Some((state, payload)) => {
            let digest = blake3::hash(payload.as_bytes()).to_hex().to_string();
            conn.execute("INSERT OR REPLACE INTO resolution_frontier(id,version,basis_epoch,reason,root_count,completed_files,payload,digest) VALUES(1,?1,?2,?3,?4,?5,?6,?7)",
                rusqlite::params![state.version,state.basis_epoch.to_string(),state.stop.as_str(),u32::try_from(state.roots.len()).map_err(db_err)?,u32::try_from(state.completed.len()).map_err(db_err)?,payload,digest]).map_err(db_err)?;
        }
    }
    Ok(())
}

impl ReadOps<'_> {
    pub fn resolution_frontier(&self) -> CcResult<Option<ReconcileState>> {
        let conn = self.0.read_conn()?;
        let row: Option<(u32, String, String)> = conn
            .query_row(
                "SELECT version,payload,digest FROM resolution_frontier WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(db_err)?;
        row.map(|(version, payload, digest)| {
            if version != RECONCILE_VERSION
                || payload.len() > MAX_FRONTIER_BYTES
                || blake3::hash(payload.as_bytes()).to_hex().as_str() != digest
            {
                return Err(CcError::Database(
                    "invalid resolution frontier version/digest; full rebuild required".into(),
                ));
            }
            let state: ReconcileState = serde_json::from_str(&payload).map_err(db_err)?;
            state.validate().map_err(db_err)?;
            Ok(state)
        })
        .transpose()
    }

    /// Constant-size summary, read with the epoch in the same SQLite snapshot.
    /// No nested read-pool checkout and no deserialization of a large frontier
    /// on the search hot path. A corrupt version fails closed, never ready.
    pub fn resolution_freshness(&self) -> CcResult<ResolutionFreshness> {
        let conn = self.0.read_conn()?;
        let tx = conn.unchecked_transaction().map_err(db_err)?;
        let epoch = IndexDb::read_generation_on(&tx)?.index_epoch;
        let result = resolution_freshness_on(&tx, epoch)?;
        tx.commit().map_err(db_err)?;
        Ok(result)
    }
}

/// Same-connection summary; caller owns the read transaction and epoch.
pub(crate) fn resolution_freshness_on(
    conn: &Connection,
    epoch: u64,
) -> CcResult<ResolutionFreshness> {
    let mut result = ResolutionFreshness::ready(epoch);
    let row: Option<(u32,String,String,u32,u32)> = conn.query_row(
        "SELECT version,basis_epoch,reason,root_count,completed_files FROM resolution_frontier WHERE id=1", [],
        |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(db_err)?;
    if let Some((version, basis, reason, roots, completed)) = row {
        if version != RECONCILE_VERSION {
            return Err(CcError::Database(
                "unsupported resolution frontier version".into(),
            ));
        }
        result.status = "incomplete".into();
        result.complete = false;
        result.basis_epoch = Some(basis.parse().map_err(db_err)?);
        result.root_count = roots as usize;
        result.completed_files = completed as usize;
        result.reason = Some(reason.clone());
        result.retry = Some(
            if reason == "disabled" {
                "enable dirty propagation and run incremental index, or request full index"
            } else {
                "run incremental index to resume bounded work, or request full index"
            }
            .into(),
        );
    }
    Ok(result)
}
