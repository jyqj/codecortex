//! Public surfaces are part of the same file write transaction as symbols.
//! No independent write API: callers cannot publish a surface ahead of its file.
use crate::{
    index_db::{FileWriteUnit, ReadOps},
    sql_util::{db_err, sql_in_placeholders, IN_BATCH_SIZE},
};
use cc_model::{public_surface::PublicSurface, CcError, CcResult};
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

/// Existing cross-file `(symbol_id, symbol_uid)` addresses, grouped by target file.
pub type BoundSurfaceAddresses = HashMap<String, HashSet<(String, String)>>;

pub(crate) fn insert_on(conn: &Connection, file: &FileWriteUnit) -> CcResult<()> {
    let mut surface = file.outcome.public_surface.clone();
    surface.normalize();
    surface.validate()?;
    if !surface.module.is_empty() && surface.module != file.rel_path {
        return Err(CcError::InvalidParams(
            "surface/file identity mismatch".into(),
        ));
    }
    let payload = serde_json::to_string(&surface).map_err(db_err)?;
    let package =
        cc_model::package_surface::PackageKey::from_surface(&surface).map(|k| k.storage_key());
    conn.prepare_cached("INSERT INTO public_surfaces(file_path,format_version,payload,fingerprint,package_key) VALUES(?1,?2,?3,?4,?5)")
        .map_err(db_err)?.execute(rusqlite::params![file.rel_path, surface.format_version, payload, surface.fingerprint(),package]).map_err(db_err)?;
    crate::resolution_dependency_store::replace_on(conn, file)
}

impl ReadOps<'_> {
    /// Existing positive binding evidence supplements unresolved import routes.
    /// This is not the later name-bucket/missing-path dependency store: only
    /// callers already bound to a changed file are added to the same bounded
    /// dirty closure. No target or resolution strategy is synthesized here.
    pub fn surface_dependents(&self, paths: &[String]) -> CcResult<Vec<String>> {
        self.surface_dependents_bounded(paths, usize::MAX - 1, &[])
            .map(|r| r.0)
    }

    /// Bounded positive-binding expansion, retaining one overflow witness.
    /// Result memory is bounded independently of fanout. SQL may inspect more
    /// rows when excluding already-completed files; work is reported, not hidden.
    pub fn surface_dependents_bounded(
        &self,
        paths: &[String],
        limit: usize,
        excluded: &[String],
    ) -> CcResult<(Vec<String>, cc_model::retrieval_cost::SqlWork)> {
        let cap = limit.saturating_add(1);
        let probe = cap.saturating_add(excluded.len()).min(i64::MAX as usize) as i64;
        let excluded: HashSet<&str> = excluded.iter().map(String::as_str).collect();
        let mut result = std::collections::BTreeSet::new();
        let mut work = cc_model::retrieval_cost::SqlWork::default();
        if paths.is_empty() {
            return Ok((Vec::new(), work));
        }
        let conn = self.0.read_conn()?;
        for batch in paths.chunks(IN_BATCH_SIZE - 1) {
            let placeholders = sql_in_placeholders(batch.len());
            for (table, target) in [
                ("imports", "resolved_path"),
                ("symbol_refs", "target_file_path"),
                ("call_edges", "target_file_path"),
            ] {
                let nonlocal = if table == "imports" {
                    String::new()
                } else {
                    format!(" AND file_path != {target}")
                };
                let sql=format!("SELECT DISTINCT file_path FROM {table} WHERE {target} IN ({placeholders}){nonlocal} ORDER BY file_path LIMIT ?");
                let mut args: Vec<rusqlite::types::Value> =
                    batch.iter().cloned().map(Into::into).collect();
                args.push(probe.into());
                let mut stmt = conn.prepare_cached(&sql).map_err(db_err)?;
                crate::statement_work::reset(&stmt);
                let mut yielded = 0;
                {
                    let mut rows = stmt
                        .query(rusqlite::params_from_iter(args))
                        .map_err(db_err)?;
                    while let Some(row) = rows.next().map_err(db_err)? {
                        let path: String = row.get(0).map_err(db_err)?;
                        yielded += 1;
                        if !excluded.contains(path.as_str()) {
                            result.insert(path);
                            if result.len() > cap {
                                result.pop_last();
                            }
                        }
                    }
                }
                work.merge(crate::statement_work::finish(&stmt, yielded));
            }
        }
        Ok((result.into_iter().collect(), work))
    }

    /// Addresses already consumed by cross-file references. A semantic UID is
    /// resilient to source relocation, but the companion symbol_id is positional.
    /// Inspect only existing positive bindings; missing/negative lookups remain
    /// the later dependency planner's responsibility. One lease, bounded IN batches.
    pub fn surface_bound_addresses(&self, paths: &[String]) -> CcResult<BoundSurfaceAddresses> {
        let mut result = BoundSurfaceAddresses::new();
        if paths.is_empty() {
            return Ok(result);
        }
        let conn = self.0.read_conn()?;
        for batch in paths.chunks(IN_BATCH_SIZE) {
            let placeholders = (1..=batch.len())
                .map(|i| format!("?{i}"))
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "SELECT target_file_path, target_symbol_id, target_symbol_uid FROM symbol_refs \
                 WHERE target_file_path IN ({placeholders}) AND file_path != target_file_path \
                 AND target_symbol_id IS NOT NULL AND target_symbol_uid IS NOT NULL \
                 UNION SELECT target_file_path, target_symbol_id, callee_symbol_uid FROM call_edges \
                 WHERE target_file_path IN ({placeholders}) AND file_path != target_file_path \
                 AND target_symbol_id IS NOT NULL AND callee_symbol_uid IS NOT NULL"
            );
            let mut stmt = conn.prepare_cached(&sql).map_err(db_err)?;
            let rows = stmt
                .query_map(rusqlite::params_from_iter(batch.iter()), |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })
                .map_err(db_err)?;
            for row in rows {
                let (path, id, uid) = row.map_err(db_err)?;
                result.entry(path).or_default().insert((id, uid));
            }
        }
        Ok(result)
    }

    /// Missing records are absent; stored unknown records remain explicit.
    /// Recompute the canonical fingerprint to reject corrupt/drifted payloads.
    pub fn public_surfaces(&self, paths: &[String]) -> CcResult<HashMap<String, PublicSurface>> {
        let mut result = HashMap::new();
        if paths.is_empty() {
            return Ok(result);
        }
        let conn = self.0.read_conn()?;
        for batch in paths.chunks(IN_BATCH_SIZE) {
            let sql = format!("SELECT file_path,format_version,payload,fingerprint FROM public_surfaces WHERE file_path IN ({})", sql_in_placeholders(batch.len()));
            let mut stmt = conn.prepare_cached(&sql).map_err(db_err)?;
            let rows = stmt
                .query_map(rusqlite::params_from_iter(batch.iter()), |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, u32>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                })
                .map_err(db_err)?;
            for row in rows {
                let (path, version, payload, fingerprint) = row.map_err(db_err)?;
                let surface: PublicSurface = serde_json::from_str(&payload).map_err(db_err)?;
                surface.validate().map_err(db_err)?;
                if surface.format_version != version
                    || surface.fingerprint() != fingerprint
                    || (!surface.module.is_empty() && surface.module != path)
                {
                    return Err(CcError::Database("invalid public surface record".into()));
                }
                result.insert(path, surface);
            }
        }
        Ok(result)
    }
}
