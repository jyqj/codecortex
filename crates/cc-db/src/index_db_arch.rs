//! IndexDb methods: architecture analysis, ADR (Architecture Decision Records).

use std::collections::HashMap;

use cc_model::{CcError, CcResult};

use crate::index_db::IndexDb;

impl IndexDb {
    pub fn architecture_languages(&self) -> CcResult<Vec<cc_model::architecture::LanguageStat>> {
        let dist = self.language_distribution()?;
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

    pub fn architecture_packages(
        &self,
        limit: usize,
    ) -> CcResult<Vec<cc_model::architecture::PackageInfo>> {
        let conn = self.read_conn()?;

        let mut file_stmt = conn
            .prepare("SELECT file_path FROM files")
            .map_err(|e| CcError::Database(e.to_string()))?;
        let file_paths: Vec<String> = file_stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| CcError::Database(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();

        let mut pkg_files: HashMap<String, usize> = HashMap::new();
        for fp in &file_paths {
            let pkg = Self::extract_package_from_path(fp);
            *pkg_files.entry(pkg).or_insert(0) += 1;
        }

        let mut sym_stmt = conn
            .prepare("SELECT file_path FROM symbols")
            .map_err(|e| CcError::Database(e.to_string()))?;
        let sym_paths: Vec<String> = sym_stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| CcError::Database(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();

        let mut pkg_symbols: HashMap<String, usize> = HashMap::new();
        for fp in &sym_paths {
            let pkg = Self::extract_package_from_path(fp);
            *pkg_symbols.entry(pkg).or_insert(0) += 1;
        }

        let uid_rows = self.query_json(
            "SELECT symbol_uid, file_path FROM symbols WHERE symbol_uid IS NOT NULL",
            &[],
        )?;
        let mut uid_to_pkg: HashMap<String, String> = HashMap::new();
        for row in &uid_rows {
            let uid = row.get("symbol_uid").and_then(|v| v.as_str()).unwrap_or("");
            let fp = row.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
            if !uid.is_empty() {
                uid_to_pkg.insert(uid.to_string(), Self::extract_package_from_path(fp));
            }
        }

        let all_edges = self.call_uid_edges()?;
        let mut pkg_fan_in: HashMap<String, usize> = HashMap::new();
        let mut pkg_fan_out: HashMap<String, usize> = HashMap::new();
        for (caller_uid, callee_uid) in &all_edges {
            let from_pkg = uid_to_pkg.get(caller_uid.as_str());
            let to_pkg = uid_to_pkg.get(callee_uid.as_str());
            if let (Some(from), Some(to)) = (from_pkg, to_pkg) {
                if from != to {
                    *pkg_fan_out.entry(from.clone()).or_insert(0) += 1;
                    *pkg_fan_in.entry(to.clone()).or_insert(0) += 1;
                }
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
        pkgs.sort_by(|a, b| b.file_count.cmp(&a.file_count));
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

    pub fn architecture_entry_points(
        &self,
        limit: usize,
    ) -> CcResult<Vec<cc_model::architecture::EntryPointInfo>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT name, file_path, kind, start_line FROM symbols
                 WHERE name IN ('main', '__main__', 'app', 'server', 'index', 'run', 'start')
                    OR framework_role LIKE '%entry%'
                    OR framework_role LIKE '%handler%'
                 ORDER BY start_line
                 LIMIT ?1",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
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
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn architecture_routes(
        &self,
        limit: usize,
    ) -> CcResult<Vec<cc_model::architecture::RouteInfo>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT COALESCE(method, 'GET'), route_path, COALESCE(handler_name, ''), file_path
                 FROM route_edges
                 ORDER BY route_path
                 LIMIT ?1",
            )
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![limit as i64], |row| {
                Ok(cc_model::architecture::RouteInfo {
                    method: row.get(0)?,
                    path: row.get(1)?,
                    handler: row.get(2)?,
                    file_path: row.get(3)?,
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn architecture_hotspots(
        &self,
        limit: usize,
    ) -> CcResult<Vec<cc_model::architecture::HotspotInfo>> {
        let conn = self.read_conn()?;
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
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![limit as i64], |row| {
                Ok(cc_model::architecture::HotspotInfo {
                    name: row.get(0)?,
                    file_path: row.get(1)?,
                    kind: row.get(2)?,
                    fan_in: row.get::<_, usize>(3).unwrap_or(0),
                })
            })
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn architecture_boundaries(
        &self,
        limit: usize,
    ) -> CcResult<Vec<cc_model::architecture::BoundaryInfo>> {
        let uid_rows = self.query_json(
            "SELECT symbol_uid, file_path FROM symbols WHERE symbol_uid IS NOT NULL",
            &[],
        )?;
        let mut uid_to_pkg: HashMap<String, String> = HashMap::new();
        for row in &uid_rows {
            let uid = row.get("symbol_uid").and_then(|v| v.as_str()).unwrap_or("");
            let fp = row.get("file_path").and_then(|v| v.as_str()).unwrap_or("");
            if !uid.is_empty() {
                uid_to_pkg.insert(uid.to_string(), Self::extract_package_from_path(fp));
            }
        }

        let all_edges = self.call_uid_edges()?;
        let mut counts: HashMap<(String, String), usize> = HashMap::new();
        for (caller_uid, callee_uid) in &all_edges {
            let from = uid_to_pkg.get(caller_uid.as_str());
            let to = uid_to_pkg.get(callee_uid.as_str());
            if let (Some(from_pkg), Some(to_pkg)) = (from, to) {
                if from_pkg != to_pkg {
                    *counts
                        .entry((from_pkg.clone(), to_pkg.clone()))
                        .or_insert(0) += 1;
                }
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
        boundaries.sort_by(|a, b| b.call_count.cmp(&a.call_count));
        boundaries.truncate(limit);
        Ok(boundaries)
    }

    pub fn architecture_communities(&self) -> CcResult<Vec<cc_model::architecture::CommunityInfo>> {
        let rows = self.list_communities()?;
        Ok(rows
            .into_iter()
            .map(|c| cc_model::architecture::CommunityInfo {
                id: c.community_id as i64,
                label: c.label,
                member_count: c.member_count as usize,
            })
            .collect())
    }

    pub fn get_architecture_info(
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
                pkgs.iter().map(|pkg| {
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
                }).collect()
            },
            adr_documents: if all || aspects.contains(&"adr") {
                self.get_metadata("adr_documents")
                    .ok()
                    .flatten()
                    .and_then(|json_str| {
                        serde_json::from_str::<Vec<cc_model::architecture::AdrDocInfo>>(&json_str).ok()
                    })
                    .unwrap_or_default()
            } else {
                vec![]
            },
        })
    }

    pub fn infra_nodes_by_kind(&self, kind: &str) -> CcResult<Vec<cc_model::infra::InfraNode>> {
        let conn = self.read_conn()?;
        let mut stmt = conn.prepare(
            "SELECT node_id, file_path, kind, name, namespace, line, end_line, properties, bound_symbol_uid, binding_confidence \
             FROM infra_nodes WHERE kind = ?1"
        ).map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt.query_map(rusqlite::params![kind], |row| {
            let kind_str: String = row.get(2)?;
            let props_str: String = row.get::<_, String>(7).unwrap_or_default();
            let properties: serde_json::Value = serde_json::from_str(&props_str).unwrap_or_default();
            let infra_kind: cc_model::infra::InfraKind = serde_json::from_value(
                serde_json::Value::String(kind_str),
            )
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
        }).map_err(|e| CcError::Database(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| CcError::Database(e.to_string()))
    }

    pub fn adr_list(&self) -> CcResult<Vec<serde_json::Value>> {
        let conn = self.read_conn()?;
        let mut stmt = conn
            .prepare("SELECT adr_id, title, status, created_at, updated_at FROM adr ORDER BY created_at DESC")
            .map_err(|e| CcError::Database(e.to_string()))?;
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
            .map_err(|e| CcError::Database(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| CcError::Database(e.to_string()))
    }

    pub fn adr_get(&self, adr_id: &str) -> CcResult<Option<serde_json::Value>> {
        let conn = self.read_conn()?;
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
            Err(e) => Err(CcError::Database(e.to_string())),
        }
    }

    pub fn adr_upsert(
        &self,
        adr_id: &str,
        title: &str,
        status: &str,
        context: &str,
        decision: &str,
        now: &str,
    ) -> CcResult<()> {
        let conn = self.write_conn.lock().map_err(|e| CcError::Database(e.to_string()))?;
        conn.execute(
            "INSERT INTO adr(adr_id, title, status, context, decision, created_at, updated_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT(adr_id) DO UPDATE SET title=?2, status=?3, context=?4, decision=?5, updated_at=?6",
            rusqlite::params![adr_id, title, status, context, decision, now],
        ).map_err(|e| CcError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn adr_delete(&self, adr_id: &str) -> CcResult<bool> {
        let conn = self.write_conn.lock().map_err(|e| CcError::Database(e.to_string()))?;
        let affected = conn
            .execute("DELETE FROM adr WHERE adr_id = ?1", [adr_id])
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(affected > 0)
    }
}
