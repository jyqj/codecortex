//! ImpactAnalyzer — BFS reverse-caller expansion + community boundary detection.

use cc_db::index_db::IndexDb;
use cc_model::impact::*;
use cc_model::{CcError, CcResult};
use std::collections::{HashMap, HashSet};
use std::process::Command;
use std::sync::Arc;

pub struct ImpactAnalyzer {
    db: Arc<IndexDb>,
    project_root: Option<String>,
}

impl ImpactAnalyzer {
    pub fn new(db: Arc<IndexDb>) -> Self {
        Self {
            db,
            project_root: None,
        }
    }

    pub fn with_project_root(mut self, root: impl Into<String>) -> Self {
        self.project_root = Some(root.into());
        self
    }

    // ── Git integration ──────────────────────────────────────────

    /// Detect changed files via git. Combines:
    /// 1. unstaged changes (`git diff --name-only`)
    /// 2. staged changes (`git diff --cached --name-only`)
    /// 3. untracked files (`git ls-files --others --exclude-standard`)
    /// 4. branch diff if `base_ref` is provided (`base...HEAD`)
    pub fn git_changed_files(&self, base_ref: Option<&str>) -> Vec<String> {
        let cwd = match &self.project_root {
            Some(p) => p.clone(),
            None => ".".to_string(),
        };
        let mut files: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        let mut cmds: Vec<Vec<String>> = vec![
            // unstaged
            vec!["git".into(), "diff".into(), "--name-only".into()],
            // staged
            vec![
                "git".into(),
                "diff".into(),
                "--cached".into(),
                "--name-only".into(),
            ],
            // untracked
            vec![
                "git".into(),
                "ls-files".into(),
                "--others".into(),
                "--exclude-standard".into(),
            ],
        ];

        if let Some(base) = base_ref {
            cmds.push(vec![
                "git".into(),
                "diff".into(),
                "--name-only".into(),
                format!("{}...HEAD", base),
            ]);
        }

        for cmd_parts in &cmds {
            let result = Command::new(&cmd_parts[0])
                .args(&cmd_parts[1..])
                .current_dir(&cwd)
                .output();
            if let Ok(output) = result {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    for line in stdout.trim().lines() {
                        let line = line.trim();
                        if !line.is_empty() && seen.insert(line.to_string()) {
                            files.push(line.to_string());
                        }
                    }
                }
            }
        }
        files
    }

    // ── Main analysis ────────────────────────────────────────────

    pub fn analyze(&self, changed_files: &[String], max_depth: usize) -> CcResult<ImpactReport> {
        self.analyze_with_options(changed_files, max_depth, None)
    }

    pub fn analyze_with_options(
        &self,
        changed_files: &[String],
        max_depth: usize,
        confidence_threshold: Option<f64>,
    ) -> CcResult<ImpactReport> {
        if changed_files.is_empty() {
            return Ok(ImpactReport {
                changed_files: Vec::new(),
                impacted_symbols: Vec::new(),
                suggested_tests: Vec::new(),
                boundary_crossings: Vec::new(),
                risk_summary: RiskSummary {
                    risk: "none".into(),
                    ..Default::default()
                },
                confidence_weighted_risk: 0.0,
                cross_service_impacts: Vec::new(),
                historical_impacts: Vec::new(),
            });
        }

        let conn = self.db.read_conn()?;

        // 1. Find symbols in changed files (hop 0 — critical)
        let mut seed_uids: Vec<(String, String, String, String, Option<u32>)> = Vec::new();
        for file in changed_files {
            let mut stmt = conn
                .prepare(
                    "SELECT symbol_uid, name, file_path, kind, community_id \
                     FROM symbols WHERE file_path=?1 AND symbol_uid IS NOT NULL",
                )
                .map_err(|e| CcError::Database(e.to_string()))?;
            let rows = stmt
                .query_map(rusqlite::params![file], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, Option<u32>>(4)?,
                    ))
                })
                .map_err(|e| CcError::Database(e.to_string()))?;
            for row in rows.flatten() {
                seed_uids.push(row);
            }
        }

        // Build seed impacted symbols
        let mut impacted: Vec<ImpactedSymbol> = Vec::new();
        let mut visited: HashSet<String> = HashSet::new();

        for (uid, name, fp, kind, cid) in &seed_uids {
            visited.insert(uid.clone());
            impacted.push(ImpactedSymbol {
                symbol_uid: uid.clone(),
                name: name.clone(),
                file_path: fp.clone(),
                kind: kind.clone(),
                risk_level: RiskLevel::Critical,
                hop_depth: 0,
                community_id: *cid,
                confidence: 1.0,
            });
        }

        // 2. BFS reverse callers with batch queries and optional confidence filtering
        let mut current_layer: Vec<String> =
            seed_uids.iter().map(|(uid, ..)| uid.clone()).collect();

        for hop in 1..=max_depth {
            if current_layer.is_empty() {
                break;
            }
            let mut next_layer: Vec<String> = Vec::new();
            let batch_size = 200;

            for batch_start in (0..current_layer.len()).step_by(batch_size) {
                let batch_end = (batch_start + batch_size).min(current_layer.len());
                let batch = &current_layer[batch_start..batch_end];
                let placeholders: String = (0..batch.len())
                    .map(|i| format!("?{}", i + 1))
                    .collect::<Vec<_>>()
                    .join(",");

                // Build query with optional confidence filter
                let conf_clause = if confidence_threshold.is_some() {
                    format!("AND ce.parser_confidence >= ?{}", batch.len() + 1)
                } else {
                    String::new()
                };

                let sql = format!(
                    "SELECT DISTINCT ce.caller_symbol_uid, s.name, s.file_path, s.kind, s.community_id \
                     FROM call_edges ce \
                     JOIN symbols s ON s.symbol_uid = ce.caller_symbol_uid \
                     WHERE ce.callee_symbol_uid IN ({}) \
                     AND ce.caller_symbol_uid IS NOT NULL \
                     {}",
                    placeholders, conf_clause
                );

                let result = conn.prepare(&sql).and_then(|mut stmt| {
                    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
                    for uid in batch {
                        params.push(Box::new(uid.clone()));
                    }
                    if let Some(threshold) = confidence_threshold {
                        params.push(Box::new(threshold));
                    }
                    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                        params.iter().map(|p| p.as_ref()).collect();
                    let rows = stmt
                        .query_map(param_refs.as_slice(), |r| {
                            Ok((
                                r.get::<_, String>(0)?,
                                r.get::<_, String>(1)?,
                                r.get::<_, String>(2)?,
                                r.get::<_, String>(3)?,
                                r.get::<_, Option<u32>>(4)?,
                            ))
                        })?
                        .filter_map(|r| r.ok())
                        .collect::<Vec<_>>();
                    Ok(rows)
                });

                if let Ok(rows) = result {
                    for (caller_uid, name, fp, kind, cid) in rows {
                        if visited.insert(caller_uid.clone()) {
                            impacted.push(ImpactedSymbol {
                                symbol_uid: caller_uid.clone(),
                                name,
                                file_path: fp,
                                kind,
                                risk_level: RiskLevel::from_hop_depth(hop as u32),
                                hop_depth: hop as u32,
                                community_id: cid,
                                confidence: 0.8f64.powi(hop as i32),
                            });
                            next_layer.push(caller_uid);
                        }
                    }
                }
            }
            current_layer = next_layer;
        }

        // 3. Collect suggested tests — cover both changed files and impacted files
        let mut all_files: Vec<String> = changed_files.to_vec();
        for sym in &impacted {
            if !all_files.contains(&sym.file_path) {
                all_files.push(sym.file_path.clone());
            }
        }

        let mut suggested_tests: Vec<String> = Vec::new();
        let mut seen_tests: HashSet<String> = HashSet::new();
        for file in &all_files {
            let tests = conn
                .prepare("SELECT DISTINCT test_file_path FROM test_edges WHERE code_file_path=?1")
                .and_then(|mut s| {
                    s.query_map(rusqlite::params![file], |r| r.get::<_, String>(0))
                        .map(|rows| rows.filter_map(|r| r.ok()).collect::<Vec<_>>())
                });
            if let Ok(test_files) = tests {
                for tf in test_files {
                    if seen_tests.insert(tf.clone()) {
                        suggested_tests.push(tf);
                    }
                }
            }
        }

        // 4. Boundary crossing detection
        let boundary_crossings = Self::detect_boundary_crossings(&impacted);

        // 4a. Advisory Pass A: cross-service reverse impact
        let cross_service_impacts = self.collect_cross_service_impacts(&impacted);

        // 4b. Advisory Pass B: historical co-change impact
        let historical_impacts = self.collect_historical_impacts(changed_files);

        // 5. Risk summary
        let mut by_level: HashMap<&str, usize> = HashMap::new();
        by_level.insert("critical", 0);
        by_level.insert("high", 0);
        by_level.insert("medium", 0);
        by_level.insert("low", 0);

        for sym in &impacted {
            let key = match sym.risk_level {
                RiskLevel::Critical => "critical",
                RiskLevel::High => "high",
                RiskLevel::Medium => "medium",
                RiskLevel::Low => "low",
            };
            *by_level.entry(key).or_insert(0) += 1;
        }

        let total = impacted.len();
        let critical_count = *by_level.get("critical").unwrap_or(&0);
        let risk = if critical_count > 10 || total > 50 {
            "high"
        } else if critical_count > 3 || total > 15 {
            "medium"
        } else {
            "low"
        };

        // 6. Confidence-weighted risk with advisory bonus
        let confidence_weighted_risk = if total > 0 {
            let weights: HashMap<&str, f64> = [
                ("critical", 1.0),
                ("high", 0.7),
                ("medium", 0.4),
                ("low", 0.2),
            ]
            .into_iter()
            .collect();

            let weighted_sum: f64 = by_level
                .iter()
                .map(|(level, count)| {
                    let w = weights.get(level).copied().unwrap_or(0.2);
                    w * (*count as f64)
                })
                .sum();

            let primary_weighted = weighted_sum / total.max(1) as f64;

            // Advisory bonus: cross-service weight 0.6, co-change weight 0.25, capped at 0.15
            let cross_service_weighted = cross_service_impacts.len() as f64 * 0.6;
            let historical_weighted: f64 =
                historical_impacts.iter().map(|h| h.confidence * 0.25).sum();
            let advisory_bonus = ((cross_service_weighted + historical_weighted) / 10.0).min(0.15);

            let raw = (primary_weighted + advisory_bonus).min(1.0);
            (raw * 10000.0).round() / 10000.0
        } else {
            0.0
        };

        let summary = RiskSummary {
            critical: critical_count,
            high: *by_level.get("high").unwrap_or(&0),
            medium: *by_level.get("medium").unwrap_or(&0),
            low: *by_level.get("low").unwrap_or(&0),
            total_impacted: total,
            risk: risk.to_string(),
            boundary_crossing_count: boundary_crossings.len(),
            suggested_test_count: suggested_tests.len(),
            cross_service_count: cross_service_impacts.len(),
            historical_count: historical_impacts.len(),
        };

        Ok(ImpactReport {
            changed_files: changed_files.to_vec(),
            impacted_symbols: impacted,
            suggested_tests,
            boundary_crossings,
            risk_summary: summary,
            confidence_weighted_risk,
            cross_service_impacts,
            historical_impacts,
        })
    }

    // ── Advisory Pass A: cross-service reverse impact ───────────

    fn collect_cross_service_impacts(
        &self,
        impacted_symbols: &[ImpactedSymbol],
    ) -> Vec<CrossServiceImpact> {
        let mut results = Vec::new();

        for seed in impacted_symbols.iter().filter(|s| s.hop_depth == 0) {
            // 1. Check if this symbol is a route handler
            let routes = match self.db.route_rows_by_handler_uid(&seed.symbol_uid, 10) {
                Ok(r) => r,
                Err(_) => continue,
            };

            for route in &routes {
                // 2. Normalize route path
                let norm_path = cc_model::route_normalize::normalize_route_path(&route.route_path);

                // 3. Find callers via HTTP call edges
                let method = route.method.as_deref();
                let callers = match self
                    .db
                    .http_callers_by_normalized_path_and_method(&norm_path, method, 10)
                {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                for caller in &callers {
                    // 4. Resolve caller name: try symbol table lookup, fallback to file_path
                    let (caller_uid, caller_name) = if let Some(ref uid) = caller.caller_symbol_uid
                    {
                        // Try to look up the symbol name from the DB
                        let name = self
                            .db
                            .read_conn()
                            .ok()
                            .and_then(|conn| {
                                conn.prepare("SELECT name FROM symbols WHERE symbol_uid = ?1")
                                    .ok()
                                    .and_then(|mut stmt| {
                                        stmt.query_row(rusqlite::params![uid], |r| {
                                            r.get::<_, String>(0)
                                        })
                                        .ok()
                                    })
                            })
                            .unwrap_or_else(|| caller.file_path.clone());
                        (uid.clone(), name)
                    } else {
                        (String::new(), caller.file_path.clone())
                    };

                    results.push(CrossServiceImpact {
                        caller_symbol_uid: caller_uid,
                        caller_name,
                        caller_file: caller.file_path.clone(),
                        route_path: route.route_path.clone(),
                        method: route.method.clone(),
                        handler_symbol_uid: Some(seed.symbol_uid.clone()),
                        handler_name: Some(seed.name.clone()),
                        handler_file: Some(seed.file_path.clone()),
                        confidence: 0.65,
                        source: "http_call_reverse".to_string(),
                    });
                }
            }
        }
        results
    }

    // ── Advisory Pass B: historical co-change impact ─────────────

    fn collect_historical_impacts(&self, changed_files: &[String]) -> Vec<HistoricalImpact> {
        let mut seen_files: HashSet<String> = changed_files.iter().cloned().collect();
        let mut results = Vec::new();

        for file in changed_files {
            let co_changes = match self.db.get_co_changes_for_file(file, 0.35) {
                Ok(c) => c,
                Err(_) => continue,
            };

            for cc in co_changes.iter().take(8) {
                let other_file = if cc.file_a == *file {
                    &cc.file_b
                } else {
                    &cc.file_a
                };

                // Skip already-changed files
                if seen_files.contains(other_file) {
                    continue;
                }

                // Skip test files
                if other_file.contains("test")
                    || other_file.contains("spec")
                    || other_file.contains("__tests__")
                {
                    continue;
                }

                // Skip config files (simple heuristic)
                if other_file.ends_with(".json")
                    || other_file.ends_with(".yaml")
                    || other_file.ends_with(".yml")
                    || other_file.ends_with(".toml")
                    || other_file.ends_with(".ini")
                {
                    continue;
                }

                seen_files.insert(other_file.to_string());
                results.push(HistoricalImpact {
                    file_path: other_file.to_string(),
                    co_change_count: cc.co_change_count,
                    confidence: cc.confidence,
                    source: "co_change".to_string(),
                });
            }
        }

        // Sort by confidence descending, keep top 5 overall
        results.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(5);
        results
    }

    // ── Boundary crossing detection ──────────────────────────────

    fn detect_boundary_crossings(impacted: &[ImpactedSymbol]) -> Vec<BoundaryCrossing> {
        if impacted.is_empty() {
            return Vec::new();
        }

        // Collect community IDs of seed symbols (hop 0)
        let seed_communities: HashSet<u32> = impacted
            .iter()
            .filter(|s| s.hop_depth == 0 && s.community_id.is_some())
            .map(|s| s.community_id.unwrap())
            .collect();

        if seed_communities.is_empty() {
            return Vec::new();
        }

        let mut crossings = Vec::new();
        for sym in impacted {
            if sym.hop_depth > 0 {
                if let Some(cid) = sym.community_id {
                    if !seed_communities.contains(&cid) {
                        // Pick any seed community as the "from" for reporting
                        let from_community = *seed_communities.iter().next().unwrap();
                        crossings.push(BoundaryCrossing {
                            from_community,
                            to_community: cid,
                            edge_symbol: sym.name.clone(),
                            edge_file: sym.file_path.clone(),
                        });
                    }
                }
            }
        }
        crossings
    }
}
