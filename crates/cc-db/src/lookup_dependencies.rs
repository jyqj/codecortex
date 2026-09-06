//! Decision dependencies are distinct from successfully bound code edges.
//! Negative/global-name lookups must be revisited when a candidate bucket changes.
use crate::index_db::{FileWriteUnit, IndexDb, ReadOps};
use crate::sql_util::{db_err, IN_BATCH_SIZE};
use cc_model::CcResult;
use rusqlite::Connection;
use std::collections::BTreeSet;

pub const RESOLUTION_FRESHNESS_KEY: &str = "resolution_freshness_v1";

/// Match the resolver's Unicode case-folded dotted leaf buckets. Also accept
/// Rust-qualified names, which otherwise have no dotted segment.
pub fn lookup_name_key(name: &str) -> String {
    name.rsplit(['.', ':'])
        .next()
        .unwrap_or(name)
        .to_lowercase()
}

impl IndexDb {
    pub(crate) fn refresh_lookup_dependencies_on(
        conn: &Connection,
        unit: &FileWriteUnit,
    ) -> CcResult<()> {
        let mut keys = BTreeSet::new();
        let mut name = |text: &str, strategy: &str| {
            if strategy != "parser_exact" && !cc_model::edge::is_terminal_syntax_miss(strategy) {
                let key = lookup_name_key(text);
                if !key.is_empty() {
                    keys.insert(("name", key));
                }
            }
        };
        for r in &unit.outcome.symbol_refs {
            name(&r.symbol_name, &r.resolution_strategy);
        }
        for e in &unit.outcome.call_edges {
            name(&e.callee_symbol, &e.resolution_strategy);
        }
        for import in &unit.outcome.imports {
            keys.insert(("module", import.import_string.clone()));
        }
        conn.execute(
            "DELETE FROM lookup_dependencies WHERE file_path=?1",
            [&unit.rel_path],
        )
        .map_err(db_err)?;
        let mut stmt = conn
            .prepare_cached(
                "INSERT INTO lookup_dependencies(file_path,kind,lookup_key) VALUES(?1,?2,?3)",
            )
            .map_err(db_err)?;
        for (kind, key) in keys {
            stmt.execute(rusqlite::params![unit.rel_path, kind, key])
                .map_err(db_err)?;
        }
        Ok(())
    }
}
impl ReadOps<'_> {
    pub fn reexport_imports(&self) -> CcResult<Vec<cc_model::edge::ImportRecord>> {
        let conn = self.0.read_conn()?;
        let mut stmt=conn.prepare_cached("SELECT file_path,import_string,resolved_path,imported_name,alias,is_namespace,is_default,is_reexport FROM imports WHERE is_reexport=1 ORDER BY file_path,import_string,imported_name,alias").map_err(db_err)?;
        let result = stmt
            .query_map([], |r| {
                Ok(cc_model::edge::ImportRecord {
                    file_path: r.get(0)?,
                    import_string: r.get(1)?,
                    resolved_path: r.get(2)?,
                    imported_name: r.get(3)?,
                    alias: r.get(4)?,
                    is_namespace: r.get(5)?,
                    is_default: r.get(6)?,
                    is_reexport: r.get(7)?,
                })
            })
            .map_err(db_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err);
        result
    }
    /// Indexed reverse lookup, including previously unresolved references. File
    /// topology changes conservatively revisit every module lookup: a newly
    /// added higher-priority module can replace a previously successful target.
    pub fn find_lookup_dependents(
        &self,
        names: &[String],
        topology_changed: bool,
    ) -> CcResult<Vec<String>> {
        let conn = self.0.read_conn()?;
        let mut files = BTreeSet::new();
        for batch in names.chunks(IN_BATCH_SIZE) {
            let sql=format!("SELECT DISTINCT file_path FROM lookup_dependencies WHERE kind='name' AND lookup_key IN ({}) ORDER BY file_path", vec!["?";batch.len()].join(","));
            let mut stmt = conn.prepare(&sql).map_err(db_err)?;
            for row in stmt
                .query_map(rusqlite::params_from_iter(batch), |r| r.get::<_, String>(0))
                .map_err(db_err)?
            {
                files.insert(row.map_err(db_err)?);
            }
        }
        if topology_changed {
            let mut stmt=conn.prepare_cached("SELECT DISTINCT file_path FROM lookup_dependencies WHERE kind='module' ORDER BY file_path").map_err(db_err)?;
            for row in stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(db_err)?
            {
                files.insert(row.map_err(db_err)?);
            }
        }
        Ok(files.into_iter().collect())
    }
}
