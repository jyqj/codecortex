//! Resolver evidence shares the file transaction; reverse lookup uses indexed
//! kind/key pairs and a bounded unique file frontier, never independent writes.
use crate::{
    index_db::{FileWriteUnit, IndexDb, ReadOps},
    sql_util::{db_err, sql_in_placeholders, IN_BATCH_SIZE},
};
use cc_model::{
    package_surface::{PackageKey, PackageSurface},
    public_surface::PublicSurface,
    resolution::*,
    CcError, CcResult,
};
use rusqlite::Connection;
use std::collections::{BTreeMap, BTreeSet, HashMap};
pub(crate) fn replace_on(conn: &Connection, file: &FileWriteUnit) -> CcResult<()> {
    let mut m = file.outcome.resolution.clone();
    m.normalize();
    m.validate()
        .map_err(|e| CcError::InvalidParams(format!("{}: {e}", file.rel_path)))?;
    let payload = serde_json::to_string(&m).map_err(db_err)?;
    if payload.len() > MAX_RESOLUTION_BYTES + 2 * 1024 * 1024 {
        return Err(CcError::InvalidParams(
            "resolution payload budget exceeded".into(),
        ));
    }
    IndexDb::execute_cached(
        conn,
        "DELETE FROM resolution_dependencies WHERE file_path=?1",
        [&file.rel_path],
    )?;
    IndexDb::execute_cached(conn,"INSERT OR REPLACE INTO resolution_manifests(file_path,version,payload,digest) VALUES(?1,?2,?3,?4)",rusqlite::params![file.rel_path,m.version,payload,blake3::hash(payload.as_bytes()).to_hex().to_string()])?;
    let mut stmt = conn
        .prepare_cached("INSERT INTO resolution_dependencies(file_path,kind,key) VALUES(?1,?2,?3)")
        .map_err(db_err)?;
    for d in &m.dependencies {
        stmt.execute(rusqlite::params![file.rel_path, d.kind.as_str(), d.key])
            .map_err(db_err)?;
    }
    Ok(())
}
impl ReadOps<'_> {
    pub fn resolution_manifests(
        &self,
        paths: &[String],
    ) -> CcResult<HashMap<String, ResolutionManifest>> {
        let mut result = HashMap::new();
        if paths.is_empty() {
            return Ok(result);
        }
        let conn = self.0.read_conn()?;
        for batch in paths.chunks(IN_BATCH_SIZE) {
            let sql=format!("SELECT file_path,version,payload,digest FROM resolution_manifests WHERE file_path IN ({})",sql_in_placeholders(batch.len()));
            let mut stmt = conn.prepare_cached(&sql).map_err(db_err)?;
            let rows = stmt
                .query_map(rusqlite::params_from_iter(batch.iter()), |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, u32>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                })
                .map_err(db_err)?;
            for row in rows {
                let (path, version, payload, digest) = row.map_err(db_err)?;
                if payload.len() > MAX_RESOLUTION_BYTES + 2 * 1024 * 1024
                    || blake3::hash(payload.as_bytes()).to_hex().as_str() != digest
                {
                    return Err(CcError::Database(
                        "resolution payload digest mismatch".into(),
                    ));
                }
                let m: ResolutionManifest = serde_json::from_str(&payload).map_err(db_err)?;
                m.validate().map_err(db_err)?;
                if m.version != version {
                    return Err(CcError::Database("resolution version mismatch".into()));
                }
                result.insert(path, m);
            }
        }
        Ok(result)
    }
    /// Return at most limit+1 paths. The extra path lets the existing dirty
    /// closure report BudgetExceeded rather than silently certifying a prefix.
    pub fn resolution_dependents(
        &self,
        events: &BTreeSet<ResolutionDependency>,
        limit: usize,
        excluded: &[String],
    ) -> CcResult<Vec<String>> {
        self.resolution_dependents_with_work(events, limit, excluded)
            .map(|r| r.0)
    }
    /// Measured counterpart, scoped to these reverse-index statements only.
    pub fn resolution_dependents_with_work(
        &self,
        events: &BTreeSet<ResolutionDependency>,
        limit: usize,
        excluded: &[String],
    ) -> CcResult<(Vec<String>, cc_model::retrieval_cost::SqlWork)> {
        let mut work = cc_model::retrieval_cost::SqlWork::default();
        let cap = limit.saturating_add(1);
        let mut result = BTreeSet::new();
        let excluded: BTreeSet<&str> = excluded.iter().map(String::as_str).collect();
        let probe_cap = cap.saturating_add(excluded.len());
        if events.is_empty() {
            return Ok((Vec::new(), work));
        }
        let mut grouped: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for d in events {
            grouped.entry(d.kind.as_str()).or_default().push(&d.key);
        }
        let conn = self.0.read_conn()?;
        for (kind, keys) in grouped {
            for batch in keys.chunks(IN_BATCH_SIZE - 2) {
                let placeholders = (2..batch.len() + 2)
                    .map(|i| format!("?{i}"))
                    .collect::<Vec<_>>()
                    .join(",");
                let sql=format!("SELECT DISTINCT file_path FROM resolution_dependencies WHERE kind=?1 AND key IN ({placeholders}) ORDER BY file_path LIMIT ?{}",batch.len()+2);
                let mut args: Vec<rusqlite::types::Value> =
                    vec![rusqlite::types::Value::Text(kind.into())];
                args.extend(
                    batch
                        .iter()
                        .map(|k| rusqlite::types::Value::Text((*k).into())),
                );
                args.push((probe_cap.min(i64::MAX as usize) as i64).into());
                let mut stmt = conn.prepare_cached(&sql).map_err(db_err)?;
                crate::statement_work::reset(&stmt);
                let mut yielded = 0;
                {
                    let rows = stmt
                        .query_map(rusqlite::params_from_iter(args), |r| r.get::<_, String>(0))
                        .map_err(db_err)?;
                    for row in rows {
                        let path = row.map_err(db_err)?;
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
    /// OLD candidate evidence for changed files only, batched in one read lease.
    pub fn resolution_symbols_in_files(&self, paths: &[String]) -> CcResult<Vec<ResolutionSymbol>> {
        if paths.is_empty() {
            return Ok(vec![]);
        }
        let conn = self.0.read_conn()?;
        let mut result = Vec::new();
        for batch in paths.chunks(IN_BATCH_SIZE) {
            let sql=format!("SELECT name,qname,symbol_id,symbol_uid,kind,signature,receiver_type,param_types,return_type,param_count,base_types,implements FROM symbols WHERE file_path IN ({})",sql_in_placeholders(batch.len()));
            let mut stmt = conn.prepare_cached(&sql).map_err(db_err)?;
            let rows = stmt
                .query_map(rusqlite::params_from_iter(batch.iter()), |r| {
                    Ok(ResolutionSymbol {
                        name: r.get(0)?,
                        qname: r.get(1)?,
                        symbol_id: r.get(2)?,
                        symbol_uid: r.get(3)?,
                        kind: r.get(4)?,
                        signature: r.get(5)?,
                        receiver_type: r.get(6)?,
                        param_types: r.get(7)?,
                        return_type: r.get(8)?,
                        param_count: r.get(9)?,
                        base_types: r.get(10)?,
                        implements: r.get(11)?,
                    })
                })
                .map_err(db_err)?;
            for row in rows {
                result.push(row.map_err(db_err)?);
            }
        }
        Ok(result)
    }
    /// Only requested package groups are read. Their contribution records remain
    /// authoritative public_surfaces rows, not a second independently mutable index.
    pub fn package_contributions(&self, keys: &[PackageKey]) -> CcResult<Vec<PublicSurface>> {
        if keys.is_empty() {
            return Ok(vec![]);
        }
        let encoded: BTreeSet<_> = keys.iter().map(PackageKey::storage_key).collect();
        let keys: Vec<_> = encoded.into_iter().collect();
        let mut paths = BTreeSet::new();
        {
            let conn = self.0.read_conn()?;
            for batch in keys.chunks(IN_BATCH_SIZE) {
                let sql = format!(
                    "SELECT file_path FROM public_surfaces WHERE package_key IN ({})",
                    sql_in_placeholders(batch.len())
                );
                let mut stmt = conn.prepare_cached(&sql).map_err(db_err)?;
                let rows = stmt
                    .query_map(rusqlite::params_from_iter(batch.iter()), |r| {
                        r.get::<_, String>(0)
                    })
                    .map_err(db_err)?;
                for row in rows {
                    paths.insert(row.map_err(db_err)?);
                }
            }
        }
        let surfaces = self.public_surfaces(&paths.into_iter().collect::<Vec<_>>())?;
        let expected: BTreeSet<_> = keys.into_iter().collect();
        let mut out: Vec<_> = surfaces.into_values().collect();
        out.sort_by(|a, b| a.module.cmp(&b.module));
        if out.iter().any(|s| {
            PackageKey::from_surface(s).is_none_or(|k| !expected.contains(&k.storage_key()))
        }) {
            return Err(CcError::Database("package index mismatch".into()));
        }
        Ok(out)
    }
    pub fn package_surface(&self, key: &PackageKey) -> CcResult<PackageSurface> {
        PackageSurface::combine(
            key.clone(),
            &self.package_contributions(&key.contribution_keys())?,
        )
    }
}
