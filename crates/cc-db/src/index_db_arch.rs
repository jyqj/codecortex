//! IndexDb methods: architecture analysis, ADR (Architecture Decision Records).
//!
//! 架构分析读查询（languages/packages/entry_points/routes/hotspots/boundaries/
//! communities/get_architecture_info/infra_nodes/adr_list/adr_get）已从 `impl IndexDb`
//! 下沉到独立真模块 [`ArchReads`]，形态对齐 RetrievalReadModel/FrontierReads/
//! QueryReads：零成本借用 `&IndexDb`，SQL 自持，经 [`IndexDb::arch`] 工厂暴露。
//! [`ReadOps`](crate::index_db::ReadOps) facet 仍作 capability boundary，转发委托到
//! 本真模块（调用方 `db.reads().x()` 不变）。
//!
//! `adr_upsert` / `adr_delete`（写）紧耦合 write_conn + `bump_index_epoch_on` 写路径
//! 机器，且无既有 Writes 真模块模式，留 `impl IndexDb`（[`WriteOps`](crate::index_db::WriteOps)
//! 转发不变）。get_architecture_info 内调的 architecture_* 兄弟方法随之一并迁入 ArchReads
//! （同模块互调，self. 不变）；language_distribution/query_json/list_communities/
//! get_metadata 仍回调 IndexDb（self.db.）。

use std::collections::HashMap;

use cc_model::CcResult;

use crate::index_db::{IndexDb, ReadOps, WriteOps};
use crate::sql_util::db_err;

impl IndexDb {
    /// 架构分析读模型：languages/packages/entry_points/routes/hotspots/boundaries/
    /// communities/infra_nodes/adr_list/adr_get。零成本借用，经此工厂暴露。
    pub fn arch(&self) -> ArchReads<'_> {
        ArchReads::new(self)
    }
}

/// Deep 架构分析读模型 over [`IndexDb`]：语言/包/入口/路由/热点/边界/社区/基础设施/
/// ADR 列表与读取。零成本借用，经 [`IndexDb::arch`] 获取，与 catch-all
/// [`ReadOps`](crate::index_db::ReadOps) 分立，架构分析 SQL 有单一归属。
pub struct ArchReads<'a> {
    db: &'a IndexDb,
}

impl<'a> ArchReads<'a> {
    /// Borrow `db` for architecture analysis queries（mirror `RetrievalReadModel::new`）.
    pub fn new(db: &'a IndexDb) -> Self {
        Self { db }
    }

    pub(crate) fn architecture_languages(
        &self,
    ) -> CcResult<Vec<cc_model::architecture::LanguageStat>> {
        let dist = self.db.language_distribution()?;
        let total: usize = dist.iter().map(|(_, c)| c).sum();
        Ok(dist
            .into_iter()
            .take(15)
            .map(
                |(language, file_count)| cc_model::architecture::LanguageStat {
                    percentage: if total > 0 {
                        file_count as f64 / total as f64 * 100.0
                    } else {
                        0.0
                    },
                    language,
                    file_count,
                },
            )
            .collect())
    }

    pub(crate) fn architecture_packages(
        &self,
        limit: usize,
    ) -> CcResult<Vec<cc_model::architecture::PackageInfo>> {
        let conn = self.db.read_conn()?;

        let mut file_stmt = conn
            .prepare("SELECT file_path FROM files")
            .map_err(db_err)?;
        let file_paths: Vec<String> = file_stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(db_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)?;

        let mut pkg_files: HashMap<String, usize> = HashMap::new();
        for fp in &file_paths {
            let pkg = Self::extract_package_from_path(fp);
            *pkg_files.entry(pkg).or_insert(0) += 1;
        }

        let mut sym_stmt = conn
            .prepare("SELECT file_path FROM symbols")
            .map_err(db_err)?;
        let sym_paths: Vec<String> = sym_stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(db_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)?;

        let mut pkg_symbols: HashMap<String, usize> = HashMap::new();
        for fp in &sym_paths {
            let pkg = Self::extract_package_from_path(fp);
            *pkg_symbols.entry(pkg).or_insert(0) += 1;
        }

        // SQL JOIN: fetch only cross-file caller/callee file paths (no full edge materialization)
        let cross_file_rows = self.db.query_json(
            "SELECT s1.file_path AS caller_file, s2.file_path AS callee_file \
             FROM call_edges ce \
             JOIN symbols s1 ON s1.symbol_uid = ce.caller_symbol_uid \
             JOIN symbols s2 ON s2.symbol_uid = ce.callee_symbol_uid \
             WHERE ce.caller_symbol_uid IS NOT NULL \
               AND ce.callee_symbol_uid IS NOT NULL \
               AND s1.file_path != s2.file_path",
            &[],
        )?;

        let mut pkg_fan_in: HashMap<String, usize> = HashMap::new();
        let mut pkg_fan_out: HashMap<String, usize> = HashMap::new();
        for row in &cross_file_rows {
            let from_fp = row
                .get("caller_file")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let to_fp = row
                .get("callee_file")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let from_pkg = Self::extract_package_from_path(from_fp);
            let to_pkg = Self::extract_package_from_path(to_fp);
            if from_pkg != to_pkg {
                *pkg_fan_out.entry(from_pkg).or_insert(0) += 1;
                *pkg_fan_in.entry(to_pkg).or_insert(0) += 1;
            }
        }

        let mut pkgs: Vec<cc_model::architecture::PackageInfo> = pkg_files
            .into_iter()
            .map(|(name, file_count)| cc_model::architecture::PackageInfo {
                symbol_count: *pkg_symbols.get(&name).unwrap_or(&0),
                fan_in: *pkg_fan_in.get(&name).unwrap_or(&0),
                fan_out: *pkg_fan_out.get(&name).unwrap_or(&0),
                name,
                file_count,
            })
            .collect();
        pkgs.sort_by_key(|p| std::cmp::Reverse(p.file_count));
        pkgs.truncate(limit);
        Ok(pkgs)
    }

    fn extract_package_from_path(file_path: &str) -> String {
        let skip = ["src", "lib", "app", "internal", "pkg", "cmd"];
        let parts: Vec<&str> = file_path.split('/').collect();
        for (idx, part) in parts.iter().enumerate() {
            if idx < parts.len() - 1 && !skip.contains(part) && !part.is_empty() {
                return (*part).to_string();
            }
        }
        parts
            .first()
            .filter(|p| !p.is_empty())
            .copied()
            .unwrap_or("root")
            .to_string()
    }

    pub(crate) fn architecture_entry_points(
        &self,
        limit: usize,
    ) -> CcResult<Vec<cc_model::architecture::EntryPointInfo>> {
        let conn = self.db.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT name, file_path, kind, start_line FROM symbols
                 WHERE name IN ('main', '__main__', 'app', 'server', 'index', 'run', 'start')
                    OR framework_role LIKE '%entry%'
                    OR framework_role LIKE '%handler%'
                 ORDER BY start_line
                 LIMIT ?1",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(rusqlite::params![limit as i64], |row| {
                let name: String = row.get(0)?;
                let kind: String = row.get(2)?;
                let ep_kind = if name == "main" || name == "__main__" {
                    "main".to_string()
                } else if kind.contains("route") || name == "index" {
                    "route".to_string()
                } else if kind.contains("test") {
                    "test_suite".to_string()
                } else {
                    "handler".to_string()
                };
                Ok(cc_model::architecture::EntryPointInfo {
                    name,
                    file_path: row.get(1)?,
                    kind: ep_kind,
                    line: row.get::<_, u32>(3).unwrap_or(0),
                })
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    pub(crate) fn architecture_routes(
        &self,
        limit: usize,
    ) -> CcResult<Vec<cc_model::architecture::RouteInfo>> {
        let conn = self.db.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT COALESCE(method, 'GET'), route_path, COALESCE(handler_name, ''), file_path
                 FROM routes
                 ORDER BY route_path
                 LIMIT ?1",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(rusqlite::params![limit as i64], |row| {
                Ok(cc_model::architecture::RouteInfo {
                    method: row.get(0)?,
                    path: row.get(1)?,
                    handler: row.get(2)?,
                    file_path: row.get(3)?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    pub(crate) fn architecture_hotspots(
        &self,
        limit: usize,
    ) -> CcResult<Vec<cc_model::architecture::HotspotInfo>> {
        let conn = self.db.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT s.name, s.file_path, s.kind,
                        COUNT(ce.edge_id) as fan_in
                 FROM symbols s
                 JOIN call_edges ce ON ce.callee_symbol = s.name
                 GROUP BY s.name, s.file_path, s.kind
                 ORDER BY fan_in DESC
                 LIMIT ?1",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(rusqlite::params![limit as i64], |row| {
                Ok(cc_model::architecture::HotspotInfo {
                    name: row.get(0)?,
                    file_path: row.get(1)?,
                    kind: row.get(2)?,
                    fan_in: row.get::<_, i64>(3).unwrap_or(0) as usize,
                })
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    pub(crate) fn architecture_boundaries(
        &self,
        limit: usize,
    ) -> CcResult<Vec<cc_model::architecture::BoundaryInfo>> {
        // SQL JOIN: fetch only cross-file caller/callee file paths (no full edge materialization)
        let cross_file_rows = self.db.query_json(
            "SELECT s1.file_path AS caller_file, s2.file_path AS callee_file \
             FROM call_edges ce \
             JOIN symbols s1 ON s1.symbol_uid = ce.caller_symbol_uid \
             JOIN symbols s2 ON s2.symbol_uid = ce.callee_symbol_uid \
             WHERE ce.caller_symbol_uid IS NOT NULL \
               AND ce.callee_symbol_uid IS NOT NULL \
               AND s1.file_path != s2.file_path",
            &[],
        )?;

        let mut counts: HashMap<(String, String), usize> = HashMap::new();
        for row in &cross_file_rows {
            let from_fp = row
                .get("caller_file")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let to_fp = row
                .get("callee_file")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let from_pkg = Self::extract_package_from_path(from_fp);
            let to_pkg = Self::extract_package_from_path(to_fp);
            if from_pkg != to_pkg {
                *counts.entry((from_pkg, to_pkg)).or_insert(0) += 1;
            }
        }

        let mut boundaries: Vec<cc_model::architecture::BoundaryInfo> = counts
            .into_iter()
            .map(|((source_package, target_package), call_count)| {
                cc_model::architecture::BoundaryInfo {
                    source_package,
                    target_package,
                    call_count,
                }
            })
            .collect();
        boundaries.sort_by_key(|b| std::cmp::Reverse(b.call_count));
        boundaries.truncate(limit);
        Ok(boundaries)
    }

    pub(crate) fn architecture_communities(
        &self,
    ) -> CcResult<Vec<cc_model::architecture::CommunityInfo>> {
        let rows = self.db.list_communities()?;
        Ok(rows
            .into_iter()
            .map(|c| cc_model::architecture::CommunityInfo {
                id: c.community_id as i64,
                label: c.label,
                member_count: c.member_count as usize,
            })
            .collect())
    }

    pub(crate) fn get_architecture_info(
        &self,
        aspects: &[&str],
        limit: usize,
    ) -> CcResult<cc_model::architecture::ArchitectureInfo> {
        let all = aspects.is_empty();
        Ok(cc_model::architecture::ArchitectureInfo {
            languages: if all || aspects.contains(&"languages") {
                self.architecture_languages()?
            } else {
                vec![]
            },
            packages: if all || aspects.contains(&"packages") {
                self.architecture_packages(limit)?
            } else {
                vec![]
            },
            entry_points: if all || aspects.contains(&"entry_points") {
                self.architecture_entry_points(limit)?
            } else {
                vec![]
            },
            routes: if all || aspects.contains(&"routes") {
                self.architecture_routes(limit)?
            } else {
                vec![]
            },
            hotspots: if all || aspects.contains(&"hotspots") {
                self.architecture_hotspots(limit)?
            } else {
                vec![]
            },
            boundaries: if all || aspects.contains(&"boundaries") {
                self.architecture_boundaries(limit)?
            } else {
                vec![]
            },
            communities: if all || aspects.contains(&"communities") {
                self.architecture_communities()?
            } else {
                vec![]
            },
            layers: {
                let pkgs = if all || aspects.contains(&"packages") {
                    self.architecture_packages(limit)?
                } else {
                    vec![]
                };
                pkgs.iter()
                    .map(|pkg| {
                        let (layer, reason) = if pkg.fan_in == 0 && pkg.fan_out > 0 {
                            ("entry", "no incoming calls, has outgoing calls")
                        } else if pkg.fan_in > pkg.fan_out * 2 {
                            ("api", "high fan-in relative to fan-out")
                        } else if pkg.fan_out > pkg.fan_in * 2 {
                            ("leaf", "high fan-out relative to fan-in")
                        } else if pkg.fan_in > 0 && pkg.fan_out > 0 {
                            ("core", "balanced fan-in and fan-out")
                        } else {
                            ("internal", "minimal external connections")
                        };
                        cc_model::architecture::LayerInfo {
                            package: pkg.name.clone(),
                            layer: layer.to_string(),
                            reason: reason.to_string(),
                        }
                    })
                    .collect()
            },
            adr_documents: if all || aspects.contains(&"adr") {
                self.db
                    .get_metadata("adr_documents")
                    .ok()
                    .flatten()
                    .and_then(|json_str| {
                        serde_json::from_str::<Vec<cc_model::architecture::AdrDocInfo>>(&json_str)
                            .ok()
                    })
                    .unwrap_or_default()
            } else {
                vec![]
            },
        })
    }

    pub(crate) fn infra_nodes_by_kind(
        &self,
        kind: &str,
    ) -> CcResult<Vec<cc_model::infra::InfraNode>> {
        let conn = self.db.read_conn()?;
        let mut stmt = conn.prepare(
            "SELECT node_id, file_path, kind, name, namespace, line, end_line, properties, bound_symbol_uid, binding_confidence \
             FROM infra_nodes WHERE kind = ?1"
        ).map_err(db_err)?;
        let rows = stmt
            .query_map(rusqlite::params![kind], |row| {
                let kind_str: String = row.get(2)?;
                let props_str: String = row.get::<_, String>(7).unwrap_or_default();
                let properties: serde_json::Value =
                    serde_json::from_str(&props_str).unwrap_or_default();
                let infra_kind: cc_model::infra::InfraKind =
                    serde_json::from_value(serde_json::Value::String(kind_str))
                        .unwrap_or(cc_model::infra::InfraKind::CompileTarget);
                Ok(cc_model::infra::InfraNode {
                    node_id: row.get(0)?,
                    file_path: row.get(1)?,
                    kind: infra_kind,
                    name: row.get(3)?,
                    namespace: row.get(4)?,
                    line: row.get(5)?,
                    end_line: row.get(6)?,
                    properties,
                    bound_symbol_uid: row.get(8)?,
                    binding_confidence: row.get(9)?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    pub(crate) fn adr_list(&self) -> CcResult<Vec<serde_json::Value>> {
        let conn = self.db.read_conn()?;
        let mut stmt = conn
            .prepare("SELECT adr_id, title, status, created_at, updated_at FROM adr ORDER BY created_at DESC")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(serde_json::json!({
                    "adr_id": row.get::<_, String>(0)?,
                    "title": row.get::<_, String>(1)?,
                    "status": row.get::<_, String>(2)?,
                    "created_at": row.get::<_, String>(3)?,
                    "updated_at": row.get::<_, String>(4)?,
                }))
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    pub(crate) fn adr_get(&self, adr_id: &str) -> CcResult<Option<serde_json::Value>> {
        let conn = self.db.read_conn()?;
        let result = conn
            .query_row(
                "SELECT adr_id, title, status, context, decision, created_at, updated_at FROM adr WHERE adr_id = ?1",
                [adr_id],
                |row| {
                    Ok(serde_json::json!({
                        "adr_id": row.get::<_, String>(0)?,
                        "title": row.get::<_, String>(1)?,
                        "status": row.get::<_, String>(2)?,
                        "context": row.get::<_, String>(3)?,
                        "decision": row.get::<_, String>(4)?,
                        "created_at": row.get::<_, String>(5)?,
                        "updated_at": row.get::<_, String>(6)?,
                    }))
                },
            );
        match result {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(db_err(e)),
        }
    }
}

// ADR 写操作（adr_upsert/adr_delete）紧耦合 write_conn + bump_index_epoch_on 写路径，
// 无既有 Writes 真模块模式，留 impl IndexDb（WriteOps 转发不变）。
impl IndexDb {
    pub(crate) fn adr_upsert(
        &self,
        adr_id: &str,
        title: &str,
        status: &str,
        context: &str,
        decision: &str,
        now: &str,
    ) -> CcResult<()> {
        let conn = self.write_conn.lock().map_err(db_err)?;
        let tx = conn.unchecked_transaction().map_err(db_err)?;
        tx.execute(
            "INSERT INTO adr(adr_id, title, status, context, decision, created_at, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT(adr_id) DO UPDATE SET title=?2, status=?3, context=?4, decision=?5, updated_at=?6",
            rusqlite::params![adr_id, title, status, context, decision, now],
        ).map_err(db_err)?;
        Self::bump_index_epoch_on(&tx)?;
        tx.commit().map_err(db_err)?;
        Ok(())
    }

    pub(crate) fn adr_delete(&self, adr_id: &str) -> CcResult<bool> {
        let conn = self.write_conn.lock().map_err(db_err)?;
        let tx = conn.unchecked_transaction().map_err(db_err)?;
        let affected = tx
            .execute("DELETE FROM adr WHERE adr_id = ?1", [adr_id])
            .map_err(db_err)?;
        Self::bump_index_epoch_on(&tx)?;
        tx.commit().map_err(db_err)?;
        Ok(affected > 0)
    }
}

// Read-only facet delegates (see `IndexDb::reads()`). capability boundary 保留：
// 11 个架构读委托到 ArchReads 真模块。
impl ReadOps<'_> {
    pub fn architecture_languages(&self) -> CcResult<Vec<cc_model::architecture::LanguageStat>> {
        self.0.arch().architecture_languages()
    }

    pub fn architecture_packages(
        &self,
        limit: usize,
    ) -> CcResult<Vec<cc_model::architecture::PackageInfo>> {
        self.0.arch().architecture_packages(limit)
    }

    pub fn architecture_entry_points(
        &self,
        limit: usize,
    ) -> CcResult<Vec<cc_model::architecture::EntryPointInfo>> {
        self.0.arch().architecture_entry_points(limit)
    }

    pub fn architecture_routes(
        &self,
        limit: usize,
    ) -> CcResult<Vec<cc_model::architecture::RouteInfo>> {
        self.0.arch().architecture_routes(limit)
    }

    pub fn architecture_hotspots(
        &self,
        limit: usize,
    ) -> CcResult<Vec<cc_model::architecture::HotspotInfo>> {
        self.0.arch().architecture_hotspots(limit)
    }

    pub fn architecture_boundaries(
        &self,
        limit: usize,
    ) -> CcResult<Vec<cc_model::architecture::BoundaryInfo>> {
        self.0.arch().architecture_boundaries(limit)
    }

    pub fn architecture_communities(&self) -> CcResult<Vec<cc_model::architecture::CommunityInfo>> {
        self.0.arch().architecture_communities()
    }

    pub fn get_architecture_info(
        &self,
        aspects: &[&str],
        limit: usize,
    ) -> CcResult<cc_model::architecture::ArchitectureInfo> {
        self.0.arch().get_architecture_info(aspects, limit)
    }

    pub fn infra_nodes_by_kind(&self, kind: &str) -> CcResult<Vec<cc_model::infra::InfraNode>> {
        self.0.arch().infra_nodes_by_kind(kind)
    }

    pub fn adr_list(&self) -> CcResult<Vec<serde_json::Value>> {
        self.0.arch().adr_list()
    }

    pub fn adr_get(&self, adr_id: &str) -> CcResult<Option<serde_json::Value>> {
        self.0.arch().adr_get(adr_id)
    }
}

// Write facet delegates (see `IndexDb::writes()`). adr_upsert/adr_delete 留 impl IndexDb，
// 委托不变。
impl WriteOps<'_> {
    pub fn adr_upsert(
        &self,
        adr_id: &str,
        title: &str,
        status: &str,
        context: &str,
        decision: &str,
        now: &str,
    ) -> CcResult<()> {
        self.0
            .adr_upsert(adr_id, title, status, context, decision, now)
    }

    pub fn adr_delete(&self, adr_id: &str) -> CcResult<bool> {
        self.0.adr_delete(adr_id)
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::ArchReads;
    use crate::index_db::IndexDb;

    fn setup() -> (IndexDb, TempDir) {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("test.db")).unwrap().0;
        (db, tmp)
    }

    #[test]
    fn test_architecture_languages() {
        let (db, _tmp) = setup();
        {
            let mut conn = db.write_conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            tx.execute(
                "INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    "src/main.rs",
                    "Rust",
                    "h1",
                    1.0,
                    100,
                    "2024-01-01T00:00:00Z"
                ],
            )
            .unwrap();
            tx.execute(
                "INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params!["src/lib.rs", "Rust", "h2", 1.0, 200, "2024-01-01T00:00:00Z"],
            )
            .unwrap();
            tx.execute(
                "INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params!["app.py", "Python", "h3", 1.0, 300, "2024-01-01T00:00:00Z"],
            )
            .unwrap();
            tx.commit().unwrap();
        }

        let langs = db.arch().architecture_languages().unwrap();
        assert_eq!(langs.len(), 2);

        // Sorted by count DESC, so Rust (2 files) first.
        assert_eq!(langs[0].language, "Rust");
        assert_eq!(langs[0].file_count, 2);
        assert!((langs[0].percentage - 66.666).abs() < 1.0);

        assert_eq!(langs[1].language, "Python");
        assert_eq!(langs[1].file_count, 1);
        assert!((langs[1].percentage - 33.333).abs() < 1.0);
    }

    #[test]
    fn test_architecture_routes() {
        let (db, _tmp) = setup();
        {
            let mut conn = db.write_conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            // Insert the file first (foreign key).
            tx.execute(
                "INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    "src/routes.rs",
                    "Rust",
                    "h1",
                    1.0,
                    100,
                    "2024-01-01T00:00:00Z"
                ],
            )
            .unwrap();
            tx.execute(
                "INSERT INTO routes(edge_id, file_path, route_path, handler_name, method, line)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params!["re1", "src/routes.rs", "/users", "list_users", "GET", 10],
            )
            .unwrap();
            tx.execute(
                "INSERT INTO routes(edge_id, file_path, route_path, handler_name, method, line)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params!["re2", "src/routes.rs", "/auth/login", "login", "POST", 20],
            )
            .unwrap();
            tx.commit().unwrap();
        }

        let routes = db.arch().architecture_routes(10).unwrap();
        assert_eq!(routes.len(), 2);
        // Sorted by route_path: /auth/login < /users
        assert_eq!(routes[0].path, "/auth/login");
        assert_eq!(routes[0].method, "POST");
        assert_eq!(routes[0].handler, "login");
        assert_eq!(routes[1].path, "/users");
        assert_eq!(routes[1].method, "GET");
        assert_eq!(routes[1].handler, "list_users");
    }

    #[test]
    fn test_extract_package_from_path() {
        // "services/auth/handler.rs" → first non-skip segment = "services"
        assert_eq!(
            ArchReads::extract_package_from_path("services/auth/handler.rs"),
            "services"
        );

        // "src/main.py" → "src" is in skip list, but it's the only directory; fallback to first part = "src"
        assert_eq!(ArchReads::extract_package_from_path("src/main.py"), "src");

        // "lib/utils.js" → "lib" is in skip list; fallback to first part = "lib"
        assert_eq!(ArchReads::extract_package_from_path("lib/utils.js"), "lib");

        // "single.rs" → only one part, first part is also last (filename), no non-skip dir segment → fallback to "single.rs"
        assert_eq!(
            ArchReads::extract_package_from_path("single.rs"),
            "single.rs"
        );
    }

    #[test]
    fn test_adr_crud() {
        let (db, _tmp) = setup();
        let now = "2024-06-01T12:00:00Z";

        // Insert
        db.adr_upsert(
            "ADR-001",
            "Use SQLite",
            "accepted",
            "Need embedded DB",
            "Use SQLite for index",
            now,
        )
        .unwrap();

        // List
        let list = db.arch().adr_list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["adr_id"].as_str().unwrap(), "ADR-001");
        assert_eq!(list[0]["title"].as_str().unwrap(), "Use SQLite");

        // Get
        let item = db.arch().adr_get("ADR-001").unwrap().unwrap();
        assert_eq!(item["status"].as_str().unwrap(), "accepted");
        assert_eq!(item["context"].as_str().unwrap(), "Need embedded DB");
        assert_eq!(item["decision"].as_str().unwrap(), "Use SQLite for index");

        // Get non-existent
        assert!(db.arch().adr_get("ADR-999").unwrap().is_none());

        // Delete
        let deleted = db.adr_delete("ADR-001").unwrap();
        assert!(deleted);
        assert!(db.arch().adr_get("ADR-001").unwrap().is_none());

        // Delete again returns false
        let deleted_again = db.adr_delete("ADR-001").unwrap();
        assert!(!deleted_again);
    }

    #[test]
    fn test_adr_upsert_conflict() {
        let (db, _tmp) = setup();
        let t1 = "2024-06-01T12:00:00Z";
        let t2 = "2024-06-02T12:00:00Z";

        db.adr_upsert("ADR-001", "Original title", "proposed", "ctx1", "dec1", t1)
            .unwrap();
        db.adr_upsert("ADR-001", "Updated title", "accepted", "ctx2", "dec2", t2)
            .unwrap();

        // Should still be one record, not two.
        let list = db.arch().adr_list().unwrap();
        assert_eq!(list.len(), 1);

        let item = db.arch().adr_get("ADR-001").unwrap().unwrap();
        assert_eq!(item["title"].as_str().unwrap(), "Updated title");
        assert_eq!(item["status"].as_str().unwrap(), "accepted");
        assert_eq!(item["context"].as_str().unwrap(), "ctx2");
        assert_eq!(item["decision"].as_str().unwrap(), "dec2");
        // created_at stays from original insert; updated_at changes.
        assert_eq!(item["created_at"].as_str().unwrap(), t1);
        assert_eq!(item["updated_at"].as_str().unwrap(), t2);
    }

    #[test]
    fn test_architecture_entry_points() {
        let (db, _tmp) = setup();
        {
            let mut conn = db.write_conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            tx.execute(
                "INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    "src/main.rs",
                    "Rust",
                    "h1",
                    1.0,
                    100,
                    "2024-01-01T00:00:00Z"
                ],
            )
            .unwrap();
            // Symbol named "main" — should match.
            tx.execute(
                "INSERT INTO symbols(symbol_id, file_path, name, kind, start_line, end_line, start_col, end_col)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, 0, 0)",
                rusqlite::params!["sym1", "src/main.rs", "main", "function", 1, 10],
            )
            .unwrap();
            // Symbol with framework_role containing "handler" — should match.
            tx.execute(
                "INSERT INTO symbols(symbol_id, file_path, name, kind, start_line, end_line, start_col, end_col, framework_role)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, 0, 0, ?7)",
                rusqlite::params!["sym2", "src/main.rs", "handle_request", "function", 20, 30, "http_handler"],
            )
            .unwrap();
            // Symbol that should NOT match.
            tx.execute(
                "INSERT INTO symbols(symbol_id, file_path, name, kind, start_line, end_line, start_col, end_col)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, 0, 0)",
                rusqlite::params!["sym3", "src/main.rs", "helper_func", "function", 40, 50],
            )
            .unwrap();
            tx.commit().unwrap();
        }

        let entries = db.arch().architecture_entry_points(10).unwrap();
        assert_eq!(entries.len(), 2);

        // Ordered by start_line: main (line 1) first, handle_request (line 20) second.
        assert_eq!(entries[0].name, "main");
        assert_eq!(entries[0].kind, "main");
        assert_eq!(entries[0].line, 1);

        assert_eq!(entries[1].name, "handle_request");
        assert_eq!(entries[1].kind, "handler");
        assert_eq!(entries[1].line, 20);
    }
}
