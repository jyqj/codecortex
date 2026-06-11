//! ImpactAnalyzer — BFS reverse-caller expansion + community boundary detection.

use cc_db::index_db::IndexDb;
use cc_model::impact::*;
use cc_model::CcResult;
use std::collections::{HashMap, HashSet};
use std::process::Command;
use std::sync::Arc;

use crate::graph_read_model::GraphReadModel;

/// Tunable bounds for impact BFS expansion.
///
/// `max_nodes`, `max_per_layer` and `result_limit` are all optional; `None`
/// preserves the historical unbounded behaviour. The capped MCP path supplies
/// concrete values so a hub callee cannot fan out into an unbounded report.
#[derive(Debug, Clone)]
pub struct ImpactOptions {
    pub max_depth: usize,
    pub confidence_threshold: Option<f64>,
    /// Upper bound on the deduplicated impacted-symbol count expanded by BFS.
    /// When reached, expansion stops and `truncated` is set.
    pub max_nodes: Option<usize>,
    /// Upper bound on callers fetched per BFS layer; pushed down as the
    /// `reverse_callers` SQL LIMIT. When a layer hits this cap, `truncated`
    /// is set.
    pub max_per_layer: Option<usize>,
    /// Final cap on the returned `impacted_symbols`. Seeds (hop 0) are kept
    /// first; the remainder are clipped to this size when exceeded.
    pub result_limit: Option<usize>,
}

impl ImpactOptions {
    /// Multiplier on `result_limit` for the default BFS node cap.
    pub const DEFAULT_NODE_CAP_FACTOR: usize = 10;
    /// Ceiling on the default BFS node cap.
    pub const DEFAULT_NODE_CAP_MAX: usize = 5000;
    /// Default per-layer caller cap (pushed down as the `reverse_callers`
    /// SQL LIMIT).
    pub const DEFAULT_MAX_PER_LAYER: usize = 500;
    /// Default BFS depth used by the MCP impact path.
    pub const DEFAULT_MAX_DEPTH: usize = 3;

    /// Adaptive default budget used by the MCP `impact` handler: result cap =
    /// `result_limit`, node cap = `result_limit × 10` capped at 5000, layer
    /// cap = 500, depth 3. Explicit `max_nodes` / `max_per_layer` override
    /// the corresponding default unchanged (no extra clamping), so a caller
    /// who asks for a wider BFS gets exactly what they asked for.
    pub fn default_for(
        result_limit: usize,
        confidence_threshold: Option<f64>,
        max_nodes: Option<usize>,
        max_per_layer: Option<usize>,
    ) -> Self {
        let node_cap = max_nodes.unwrap_or_else(|| {
            result_limit
                .saturating_mul(Self::DEFAULT_NODE_CAP_FACTOR)
                .min(Self::DEFAULT_NODE_CAP_MAX)
        });
        let layer_cap = max_per_layer.unwrap_or(Self::DEFAULT_MAX_PER_LAYER);
        Self {
            max_depth: Self::DEFAULT_MAX_DEPTH,
            confidence_threshold,
            max_nodes: Some(node_cap),
            max_per_layer: Some(layer_cap),
            result_limit: Some(result_limit),
        }
    }

    /// Legacy/unbounded options: only `max_depth` + optional confidence apply.
    #[cfg(test)]
    pub fn unbounded(max_depth: usize, confidence_threshold: Option<f64>) -> Self {
        Self {
            max_depth,
            confidence_threshold,
            max_nodes: None,
            max_per_layer: None,
            result_limit: None,
        }
    }
}

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
            match result {
                Ok(output) if output.status.success() => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    for line in stdout.trim().lines() {
                        let line = line.trim();
                        if !line.is_empty() && seen.insert(line.to_string()) {
                            files.push(line.to_string());
                        }
                    }
                }
                Ok(output) => {
                    tracing::debug!(
                        cmd = ?cmd_parts,
                        code = output.status.code(),
                        stderr = %String::from_utf8_lossy(&output.stderr).trim(),
                        "git command returned non-zero exit; skipping (not a git repo or unknown ref?)"
                    );
                }
                Err(e) => {
                    tracing::debug!(cmd = ?cmd_parts, error = %e, "failed to spawn git; skipping");
                }
            }
        }
        files
    }

    // ── Main analysis ────────────────────────────────────────────

    #[cfg(test)]
    pub fn analyze(&self, changed_files: &[String], max_depth: usize) -> CcResult<ImpactReport> {
        self.analyze_with(changed_files, &ImpactOptions::unbounded(max_depth, None))
    }

    #[cfg(test)]
    pub fn analyze_with_options(
        &self,
        changed_files: &[String],
        max_depth: usize,
        confidence_threshold: Option<f64>,
    ) -> CcResult<ImpactReport> {
        self.analyze_with(
            changed_files,
            &ImpactOptions::unbounded(max_depth, confidence_threshold),
        )
    }

    pub fn analyze_with(
        &self,
        changed_files: &[String],
        opts: &ImpactOptions,
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
                truncated: false,
                returned_symbol_count: 0,
                total_impacted_discovered: 0,
            });
        }

        let max_depth = opts.max_depth;
        let confidence_threshold = opts.confidence_threshold;
        let read_model = GraphReadModel::without_http_bridges(Arc::clone(&self.db));

        // 1. Find symbols in changed files (hop 0 — critical)
        let seed_symbols = read_model.symbols_in_files(changed_files)?;

        // Build seed impacted symbols
        let mut impacted: Vec<ImpactedSymbol> = Vec::new();
        let mut visited: HashSet<String> = HashSet::new();
        let mut truncated = false;

        for seed in &seed_symbols {
            visited.insert(seed.symbol_uid.clone());
            impacted.push(ImpactedSymbol {
                symbol_uid: seed.symbol_uid.clone(),
                name: seed.name.clone(),
                file_path: seed.file_path.clone(),
                kind: seed.kind.clone(),
                risk_level: RiskLevel::Critical,
                hop_depth: 0,
                community_id: seed.community_id,
                confidence: 1.0,
            });
        }

        // 2. BFS reverse callers with batch queries, optional confidence filtering
        //    and safety caps (max_per_layer pushed down as SQL LIMIT, max_nodes
        //    bounding total expansion). Seeds always remain in `impacted`.
        //    Deliberately NOT on graph_walk::bfs_visit: that kernel expands one
        //    node at a time, while this walk batches a whole layer into a single
        //    reverse_callers SQL query (IN(...) + LIMIT pushdown).
        let mut current_layer: Vec<String> = seed_symbols
            .iter()
            .map(|seed| seed.symbol_uid.clone())
            .collect();

        'bfs: for hop in 1..=max_depth {
            if current_layer.is_empty() {
                break;
            }
            if let Some(cap) = opts.max_nodes {
                if impacted.len() >= cap {
                    truncated = true;
                    break;
                }
            }
            let mut next_layer: Vec<String> = Vec::new();

            let layer_callers = read_model
                .reverse_callers(&current_layer, confidence_threshold, opts.max_per_layer)
                .unwrap_or_default();
            // A full layer at the per-layer cap means callers were dropped.
            if let Some(per_layer) = opts.max_per_layer {
                if layer_callers.len() >= per_layer {
                    truncated = true;
                }
            }

            for caller in layer_callers {
                if let Some(cap) = opts.max_nodes {
                    if impacted.len() >= cap {
                        truncated = true;
                        break 'bfs;
                    }
                }
                if visited.insert(caller.symbol_uid.clone()) {
                    impacted.push(ImpactedSymbol {
                        symbol_uid: caller.symbol_uid.clone(),
                        name: caller.name,
                        file_path: caller.file_path,
                        kind: caller.kind,
                        risk_level: RiskLevel::from_hop_depth(hop as u32),
                        hop_depth: hop as u32,
                        community_id: caller.community_id,
                        confidence: 0.8f64.powi(hop as i32),
                    });
                    next_layer.push(caller.symbol_uid);
                }
            }
            current_layer = next_layer;
        }

        // 2a. Record the deduplicated discovery total before any result clipping.
        let total_impacted_discovered = impacted.len();

        // 2b. Clip the returned set to `result_limit`, keeping seeds (hop 0)
        //     first so the critical changed symbols are never dropped.
        if let Some(limit) = opts.result_limit {
            if impacted.len() > limit {
                impacted.sort_by_key(|sym| sym.hop_depth);
                impacted.truncate(limit);
                truncated = true;
            }
        }

        // 3. Collect suggested tests — cover both changed files and impacted files
        let mut all_files: Vec<String> = changed_files.to_vec();
        for sym in &impacted {
            if !all_files.contains(&sym.file_path) {
                all_files.push(sym.file_path.clone());
            }
        }

        let suggested_tests = read_model
            .suggested_tests_for_files(&all_files)
            .unwrap_or_default();

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

        let returned_symbol_count = impacted.len();

        Ok(ImpactReport {
            changed_files: changed_files.to_vec(),
            impacted_symbols: impacted,
            suggested_tests,
            boundary_crossings,
            risk_summary: summary,
            confidence_weighted_risk,
            cross_service_impacts,
            historical_impacts,
            truncated,
            returned_symbol_count,
            total_impacted_discovered,
        })
    }

    // ── Advisory Pass A: cross-service reverse impact ───────────

    fn collect_cross_service_impacts(
        &self,
        impacted_symbols: &[ImpactedSymbol],
    ) -> Vec<CrossServiceImpact> {
        // Phase 1: gather every (seed, route, caller) tuple and the full set of
        // caller symbol UIDs that need a name lookup.
        struct PendingImpact {
            seed_uid: String,
            seed_name: String,
            seed_file: String,
            route_path: String,
            method: Option<String>,
            caller_uid: Option<String>,
            caller_file: String,
        }
        let mut pending: Vec<PendingImpact> = Vec::new();
        let mut caller_uids: Vec<String> = Vec::new();
        let mut seen_uids: HashSet<String> = HashSet::new();

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
                    if let Some(ref uid) = caller.caller_symbol_uid {
                        if seen_uids.insert(uid.clone()) {
                            caller_uids.push(uid.clone());
                        }
                    }
                    pending.push(PendingImpact {
                        seed_uid: seed.symbol_uid.clone(),
                        seed_name: seed.name.clone(),
                        seed_file: seed.file_path.clone(),
                        route_path: route.route_path.clone(),
                        method: route.method.clone(),
                        caller_uid: caller.caller_symbol_uid.clone(),
                        caller_file: caller.file_path.clone(),
                    });
                }
            }
        }

        // Phase 2: resolve all caller UIDs to names in one batched pass instead
        // of opening a read connection + single-row query per caller.
        let name_by_uid = self.symbol_names_by_uid(&caller_uids);

        // Phase 3: assemble results, falling back to caller_file when the UID is
        // absent or unresolved.
        pending
            .into_iter()
            .map(|p| {
                let (caller_symbol_uid, caller_name) = match p.caller_uid {
                    Some(uid) => {
                        let name = name_by_uid
                            .get(&uid)
                            .cloned()
                            .unwrap_or_else(|| p.caller_file.clone());
                        (uid, name)
                    }
                    None => (String::new(), p.caller_file.clone()),
                };
                CrossServiceImpact {
                    caller_symbol_uid,
                    caller_name,
                    caller_file: p.caller_file,
                    route_path: p.route_path,
                    method: p.method,
                    handler_symbol_uid: Some(p.seed_uid),
                    handler_name: Some(p.seed_name),
                    handler_file: Some(p.seed_file),
                    confidence: 0.65,
                    source: "http_call_reverse".to_string(),
                }
            })
            .collect()
    }

    /// Resolve a batch of symbol UIDs to their names with a single connection
    /// and `WHERE symbol_uid IN (...)` query (chunked to bound placeholder count).
    /// UIDs without a matching symbol row are simply absent from the map.
    fn symbol_names_by_uid(&self, uids: &[String]) -> HashMap<String, String> {
        GraphReadModel::without_http_bridges(Arc::clone(&self.db))
            .symbol_names_by_uid(uids)
            .unwrap_or_default()
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
            .filter(|s| s.hop_depth == 0)
            .filter_map(|s| s.community_id)
            .collect();

        if seed_communities.is_empty() {
            return Vec::new();
        }

        let mut crossings = Vec::new();
        for sym in impacted {
            if sym.hop_depth > 0 {
                if let Some(cid) = sym.community_id {
                    if !seed_communities.contains(&cid) {
                        let Some(&from_community) = seed_communities.iter().next() else {
                            continue;
                        };
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Helper: open a fresh IndexDb in a temp directory.
    fn temp_db() -> (TempDir, Arc<IndexDb>) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test_index.sqlite3");
        let (db, _status) = IndexDb::open(&db_path).unwrap();
        (dir, Arc::new(db))
    }

    /// Helper: insert a minimal file row (needed for FK on symbols/call_edges).
    fn insert_file(db: &IndexDb, file_path: &str) {
        let conn = db.read_conn().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO files(file_path, language, content_hash, mtime, size, indexed_at) \
             VALUES(?1, 'rust', 'hash', 0.0, 100, '2025-01-01')",
            rusqlite::params![file_path],
        )
        .unwrap();
    }

    /// Helper: insert a symbol row with the given fields.
    fn insert_symbol(
        db: &IndexDb,
        symbol_uid: &str,
        name: &str,
        file_path: &str,
        kind: &str,
        community_id: Option<u32>,
    ) {
        let conn = db.read_conn().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO symbols(symbol_id, file_path, name, kind, start_line, end_line, symbol_uid, community_id) \
             VALUES(?1, ?2, ?3, ?4, 1, 10, ?5, ?6)",
            rusqlite::params![
                format!("sid_{}", symbol_uid),
                file_path,
                name,
                kind,
                symbol_uid,
                community_id,
            ],
        )
        .unwrap();
    }

    /// Helper: insert a call edge from caller_uid to callee_uid.
    fn insert_call_edge(
        db: &IndexDb,
        edge_id: &str,
        file_path: &str,
        caller_uid: &str,
        callee_uid: &str,
    ) {
        let conn = db.read_conn().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO call_edges(edge_id, file_path, callee_symbol, line, caller_symbol_uid, callee_symbol_uid) \
             VALUES(?1, ?2, 'callee', 1, ?3, ?4)",
            rusqlite::params![edge_id, file_path, caller_uid, callee_uid],
        )
        .unwrap();
    }

    // ── analyze: empty changed_files ────────────────────────────────

    #[test]
    fn analyze_empty_changed_files_returns_none_risk() {
        let (_dir, db) = temp_db();
        let analyzer = ImpactAnalyzer::new(db);
        let report = analyzer.analyze(&[], 3).unwrap();

        assert!(report.changed_files.is_empty());
        assert!(report.impacted_symbols.is_empty());
        assert!(report.suggested_tests.is_empty());
        assert!(report.boundary_crossings.is_empty());
        assert_eq!(report.risk_summary.risk, "none");
        assert_eq!(report.confidence_weighted_risk, 0.0);
    }

    // ── analyze: seeds found in changed files ───────────────────────

    #[test]
    fn analyze_finds_seeds_from_changed_files() {
        let (_dir, db) = temp_db();
        insert_file(&db, "src/lib.rs");
        insert_symbol(&db, "uid_a", "FuncA", "src/lib.rs", "function", Some(1));

        let analyzer = ImpactAnalyzer::new(db);
        let report = analyzer.analyze(&["src/lib.rs".to_string()], 3).unwrap();

        assert_eq!(report.changed_files, vec!["src/lib.rs"]);
        assert_eq!(report.impacted_symbols.len(), 1);
        let sym = &report.impacted_symbols[0];
        assert_eq!(sym.symbol_uid, "uid_a");
        assert_eq!(sym.name, "FuncA");
        assert_eq!(sym.hop_depth, 0);
        assert_eq!(sym.risk_level, RiskLevel::Critical);
        assert_eq!(sym.confidence, 1.0);
    }

    // ── analyze: BFS expansion across call edges ────────────────────

    #[test]
    fn analyze_bfs_expands_callers() {
        let (_dir, db) = temp_db();
        // seed: FuncA in src/lib.rs
        insert_file(&db, "src/lib.rs");
        insert_file(&db, "src/caller.rs");
        insert_symbol(&db, "uid_a", "FuncA", "src/lib.rs", "function", Some(1));
        // caller: FuncB calls FuncA
        insert_symbol(&db, "uid_b", "FuncB", "src/caller.rs", "function", Some(1));
        insert_call_edge(&db, "edge_ba", "src/caller.rs", "uid_b", "uid_a");

        let analyzer = ImpactAnalyzer::new(db);
        let report = analyzer.analyze(&["src/lib.rs".to_string()], 3).unwrap();

        assert_eq!(report.impacted_symbols.len(), 2);
        let hop1: Vec<_> = report
            .impacted_symbols
            .iter()
            .filter(|s| s.hop_depth == 1)
            .collect();
        assert_eq!(hop1.len(), 1);
        assert_eq!(hop1[0].name, "FuncB");
        assert_eq!(hop1[0].risk_level, RiskLevel::High);
    }

    // ── analyze: BFS respects max_depth ─────────────────────────────

    #[test]
    fn analyze_bfs_respects_max_depth() {
        let (_dir, db) = temp_db();
        insert_file(&db, "src/a.rs");
        insert_file(&db, "src/b.rs");
        insert_file(&db, "src/c.rs");

        insert_symbol(&db, "uid_a", "A", "src/a.rs", "function", None);
        insert_symbol(&db, "uid_b", "B", "src/b.rs", "function", None);
        insert_symbol(&db, "uid_c", "C", "src/c.rs", "function", None);
        // C -> B -> A (call chain)
        insert_call_edge(&db, "edge_ba", "src/b.rs", "uid_b", "uid_a");
        insert_call_edge(&db, "edge_cb", "src/c.rs", "uid_c", "uid_b");

        let analyzer = ImpactAnalyzer::new(db);

        // max_depth=1: should find A (seed) + B (hop 1), but NOT C (hop 2)
        let report = analyzer.analyze(&["src/a.rs".to_string()], 1).unwrap();
        let names: Vec<&str> = report
            .impacted_symbols
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert!(names.contains(&"A"));
        assert!(names.contains(&"B"));
        assert!(!names.contains(&"C"));
    }

    // ── analyze: no file match yields low risk ──────────────────────

    #[test]
    fn analyze_nonexistent_file_yields_empty_symbols() {
        let (_dir, db) = temp_db();
        let analyzer = ImpactAnalyzer::new(db);
        let report = analyzer
            .analyze(&["nonexistent/file.rs".to_string()], 3)
            .unwrap();

        assert!(report.impacted_symbols.is_empty());
        assert_eq!(report.risk_summary.risk, "low");
    }

    // ── risk_summary thresholds ─────────────────────────────────────

    #[test]
    fn risk_summary_low_when_few_symbols() {
        let (_dir, db) = temp_db();
        insert_file(&db, "src/a.rs");
        insert_symbol(&db, "uid_1", "F1", "src/a.rs", "function", None);

        let analyzer = ImpactAnalyzer::new(db);
        let report = analyzer.analyze(&["src/a.rs".to_string()], 0).unwrap();

        assert_eq!(report.risk_summary.critical, 1);
        assert_eq!(report.risk_summary.total_impacted, 1);
        assert_eq!(report.risk_summary.risk, "low");
    }

    #[test]
    fn risk_summary_medium_when_critical_exceeds_3() {
        let (_dir, db) = temp_db();
        insert_file(&db, "src/a.rs");
        for i in 0..5 {
            insert_symbol(
                &db,
                &format!("uid_{}", i),
                &format!("F{}", i),
                "src/a.rs",
                "function",
                None,
            );
        }

        let analyzer = ImpactAnalyzer::new(db);
        let report = analyzer.analyze(&["src/a.rs".to_string()], 0).unwrap();

        // 5 critical symbols → exceeds threshold of 3 → "medium"
        assert_eq!(report.risk_summary.critical, 5);
        assert_eq!(report.risk_summary.risk, "medium");
    }

    // ── detect_boundary_crossings ───────────────────────────────────

    #[test]
    fn boundary_crossings_empty_for_empty_input() {
        let crossings = ImpactAnalyzer::detect_boundary_crossings(&[]);
        assert!(crossings.is_empty());
    }

    #[test]
    fn boundary_crossings_empty_when_no_community_ids() {
        let symbols = vec![make_impacted("A", 0, None), make_impacted("B", 1, None)];
        let crossings = ImpactAnalyzer::detect_boundary_crossings(&symbols);
        assert!(crossings.is_empty());
    }

    #[test]
    fn boundary_crossings_detected_across_communities() {
        let symbols = vec![
            make_impacted("Seed", 0, Some(1)),
            make_impacted("SameCommunity", 1, Some(1)),
            make_impacted("OtherCommunity", 2, Some(2)),
        ];
        let crossings = ImpactAnalyzer::detect_boundary_crossings(&symbols);

        assert_eq!(crossings.len(), 1);
        assert_eq!(crossings[0].from_community, 1);
        assert_eq!(crossings[0].to_community, 2);
        assert_eq!(crossings[0].edge_symbol, "OtherCommunity");
    }

    #[test]
    fn boundary_crossings_not_reported_for_hop0() {
        // Even if hop-0 symbols span multiple communities, no crossing is reported
        // because crossings are only from non-seed to different community.
        let symbols = vec![
            make_impacted("SeedA", 0, Some(1)),
            make_impacted("SeedB", 0, Some(2)),
        ];
        let crossings = ImpactAnalyzer::detect_boundary_crossings(&symbols);
        assert!(crossings.is_empty());
    }

    // ── confidence_weighted_risk ─────────────────────────────────────

    #[test]
    fn confidence_weighted_risk_is_zero_for_empty_symbols() {
        let (_dir, db) = temp_db();
        let analyzer = ImpactAnalyzer::new(db);
        let report = analyzer.analyze(&[], 3).unwrap();
        assert_eq!(report.confidence_weighted_risk, 0.0);
    }

    #[test]
    fn confidence_weighted_risk_is_1_for_single_critical() {
        let (_dir, db) = temp_db();
        insert_file(&db, "src/a.rs");
        insert_symbol(&db, "uid_1", "F1", "src/a.rs", "function", None);

        let analyzer = ImpactAnalyzer::new(db);
        let report = analyzer.analyze(&["src/a.rs".to_string()], 0).unwrap();

        // 1 critical symbol: weight 1.0 / 1 = 1.0 (no advisory bonus)
        assert_eq!(report.confidence_weighted_risk, 1.0);
    }

    // ── with_project_root builder ───────────────────────────────────

    #[test]
    fn with_project_root_sets_root() {
        let (_dir, db) = temp_db();
        let analyzer = ImpactAnalyzer::new(db).with_project_root("/some/path");
        assert_eq!(analyzer.project_root.as_deref(), Some("/some/path"));
    }

    // ── analyze_with_options: confidence threshold ──────────────────

    #[test]
    fn analyze_with_high_confidence_threshold_filters_edges() {
        let (_dir, db) = temp_db();
        insert_file(&db, "src/a.rs");
        insert_file(&db, "src/b.rs");
        insert_symbol(&db, "uid_a", "A", "src/a.rs", "function", None);
        insert_symbol(&db, "uid_b", "B", "src/b.rs", "function", None);

        // Insert edge with low parser_confidence (default is 0.5)
        insert_call_edge(&db, "edge_ba", "src/b.rs", "uid_b", "uid_a");

        let analyzer = ImpactAnalyzer::new(db);

        // threshold=0.9 should filter out the edge (parser_confidence=0.5)
        let report = analyzer
            .analyze_with_options(&["src/a.rs".to_string()], 3, Some(0.9))
            .unwrap();

        let hop1: Vec<_> = report
            .impacted_symbols
            .iter()
            .filter(|s| s.hop_depth > 0)
            .collect();
        assert!(
            hop1.is_empty(),
            "high confidence threshold should filter low-confidence edges"
        );
    }

    // ── analyze_with: result_limit clips returned symbols ───────────

    #[test]
    fn analyze_with_result_limit_truncates_and_keeps_seeds() {
        let (_dir, db) = temp_db();
        insert_file(&db, "src/lib.rs");
        insert_file(&db, "src/callers.rs");
        // One seed callee with many hop-1 callers.
        insert_symbol(&db, "uid_seed", "Seed", "src/lib.rs", "function", None);
        for i in 0..10 {
            let uid = format!("uid_c{}", i);
            insert_symbol(
                &db,
                &uid,
                &format!("C{}", i),
                "src/callers.rs",
                "function",
                None,
            );
            insert_call_edge(
                &db,
                &format!("edge_{}", i),
                "src/callers.rs",
                &uid,
                "uid_seed",
            );
        }

        let analyzer = ImpactAnalyzer::new(db);
        // Unbounded discovers seed + 10 callers = 11.
        let full = analyzer
            .analyze_with(
                &["src/lib.rs".to_string()],
                &ImpactOptions::unbounded(3, None),
            )
            .unwrap();
        assert_eq!(full.impacted_symbols.len(), 11);
        assert!(!full.truncated);
        assert_eq!(full.total_impacted_discovered, 11);
        assert_eq!(full.returned_symbol_count, 11);

        // result_limit=5 clips to 5 and flags truncation.
        let opts = ImpactOptions {
            max_depth: 3,
            confidence_threshold: None,
            max_nodes: None,
            max_per_layer: None,
            result_limit: Some(5),
        };
        let clipped = analyzer
            .analyze_with(&["src/lib.rs".to_string()], &opts)
            .unwrap();
        assert!(clipped.truncated);
        assert_eq!(clipped.impacted_symbols.len(), 5);
        assert_eq!(clipped.returned_symbol_count, 5);
        // Discovered count reflects pre-clip total.
        assert_eq!(clipped.total_impacted_discovered, 11);
        // The seed (hop 0) must survive the clip.
        assert!(clipped
            .impacted_symbols
            .iter()
            .any(|s| s.hop_depth == 0 && s.name == "Seed"));
    }

    // ── analyze_with: max_per_layer pushes down SQL LIMIT ───────────

    #[test]
    fn analyze_with_max_per_layer_caps_callers() {
        let (_dir, db) = temp_db();
        insert_file(&db, "src/lib.rs");
        insert_file(&db, "src/callers.rs");
        insert_symbol(&db, "uid_seed", "Seed", "src/lib.rs", "function", None);
        for i in 0..10 {
            let uid = format!("uid_c{}", i);
            insert_symbol(
                &db,
                &uid,
                &format!("C{}", i),
                "src/callers.rs",
                "function",
                None,
            );
            insert_call_edge(
                &db,
                &format!("edge_{}", i),
                "src/callers.rs",
                &uid,
                "uid_seed",
            );
        }

        let analyzer = ImpactAnalyzer::new(db);
        let opts = ImpactOptions {
            max_depth: 3,
            confidence_threshold: None,
            max_nodes: None,
            max_per_layer: Some(3),
            result_limit: None,
        };
        let report = analyzer
            .analyze_with(&["src/lib.rs".to_string()], &opts)
            .unwrap();

        // Only 3 of the 10 hop-1 callers are fetched (SQL LIMIT).
        let hop1 = report
            .impacted_symbols
            .iter()
            .filter(|s| s.hop_depth == 1)
            .count();
        assert_eq!(hop1, 3);
        assert!(report.truncated);
    }

    // ── analyze_with: max_nodes stops BFS expansion ─────────────────

    #[test]
    fn analyze_with_max_nodes_stops_expansion() {
        let (_dir, db) = temp_db();
        insert_file(&db, "src/lib.rs");
        insert_file(&db, "src/callers.rs");
        insert_symbol(&db, "uid_seed", "Seed", "src/lib.rs", "function", None);
        for i in 0..10 {
            let uid = format!("uid_c{}", i);
            insert_symbol(
                &db,
                &uid,
                &format!("C{}", i),
                "src/callers.rs",
                "function",
                None,
            );
            insert_call_edge(
                &db,
                &format!("edge_{}", i),
                "src/callers.rs",
                &uid,
                "uid_seed",
            );
        }

        let analyzer = ImpactAnalyzer::new(db);
        // max_nodes=4: seed + a few callers before the cap halts expansion.
        let opts = ImpactOptions {
            max_depth: 3,
            confidence_threshold: None,
            max_nodes: Some(4),
            max_per_layer: None,
            result_limit: None,
        };
        let report = analyzer
            .analyze_with(&["src/lib.rs".to_string()], &opts)
            .unwrap();

        assert!(report.truncated);
        assert!(report.impacted_symbols.len() <= 4);
        assert!(!report.impacted_symbols.is_empty());
    }

    // ── analyze_with: empty changed_files carries truncation fields ──

    #[test]
    fn analyze_with_empty_changed_files_reports_zero_counts() {
        let (_dir, db) = temp_db();
        let analyzer = ImpactAnalyzer::new(db);
        let report = analyzer
            .analyze_with(&[], &ImpactOptions::unbounded(3, None))
            .unwrap();
        assert!(!report.truncated);
        assert_eq!(report.returned_symbol_count, 0);
        assert_eq!(report.total_impacted_discovered, 0);
    }

    // ── suggested_tests via test_edges ───────────────────────────────

    #[test]
    fn suggested_tests_from_test_edges() {
        let (_dir, db) = temp_db();
        insert_file(&db, "src/a.rs");

        // Insert a test_edge linking src/a.rs to tests/test_a.rs
        let conn = db.read_conn().unwrap();
        conn.execute(
            "INSERT INTO test_edges(edge_id, test_file_path, code_file_path, reason) \
             VALUES('te1', 'tests/test_a.rs', 'src/a.rs', 'import')",
            [],
        )
        .unwrap();
        drop(conn);

        insert_symbol(&db, "uid_a", "A", "src/a.rs", "function", None);

        let analyzer = ImpactAnalyzer::new(db);
        let report = analyzer.analyze(&["src/a.rs".to_string()], 0).unwrap();

        assert_eq!(report.suggested_tests, vec!["tests/test_a.rs"]);
        assert_eq!(report.risk_summary.suggested_test_count, 1);
    }

    // ── symbol_names_by_uid: batched name resolution ────────────────

    #[test]
    fn symbol_names_by_uid_resolves_batch_and_skips_missing() {
        let (_dir, db) = temp_db();
        insert_file(&db, "src/a.rs");
        insert_symbol(&db, "uid_a", "Alpha", "src/a.rs", "function", None);
        insert_symbol(&db, "uid_b", "Beta", "src/a.rs", "function", None);

        let analyzer = ImpactAnalyzer::new(db);
        let map = analyzer.symbol_names_by_uid(&[
            "uid_a".to_string(),
            "uid_b".to_string(),
            "uid_missing".to_string(),
        ]);
        assert_eq!(map.get("uid_a").map(String::as_str), Some("Alpha"));
        assert_eq!(map.get("uid_b").map(String::as_str), Some("Beta"));
        assert!(!map.contains_key("uid_missing"));
    }

    #[test]
    fn symbol_names_by_uid_empty_returns_empty() {
        let (_dir, db) = temp_db();
        let analyzer = ImpactAnalyzer::new(db);
        assert!(analyzer.symbol_names_by_uid(&[]).is_empty());
    }

    // ── ImpactOptions::default_for: formula lock ────────────────────

    #[test]
    fn default_for_applies_handler_formulas() {
        let opts = ImpactOptions::default_for(50, None, None, None);
        assert_eq!(opts.max_depth, 3);
        assert_eq!(opts.confidence_threshold, None);
        assert_eq!(opts.max_nodes, Some(500)); // 50 × 10
        assert_eq!(opts.max_per_layer, Some(500));
        assert_eq!(opts.result_limit, Some(50));
    }

    #[test]
    fn default_for_node_cap_is_capped_at_5000() {
        // 600 × 10 = 6000 → capped at the ceiling.
        let over = ImpactOptions::default_for(600, None, None, None);
        assert_eq!(over.max_nodes, Some(5000));
        // Exactly at the ceiling: 500 × 10 = 5000.
        let exact = ImpactOptions::default_for(500, None, None, None);
        assert_eq!(exact.max_nodes, Some(5000));
        // Saturating multiply cannot overflow past the ceiling.
        let huge = ImpactOptions::default_for(usize::MAX, None, None, None);
        assert_eq!(huge.max_nodes, Some(5000));
    }

    #[test]
    fn default_for_explicit_caps_pass_through_unclamped() {
        let opts = ImpactOptions::default_for(50, Some(0.7), Some(9999), Some(7));
        // Explicit max_nodes is used as-is: no ×10 formula, no 5000 clamp.
        assert_eq!(opts.max_nodes, Some(9999));
        assert_eq!(opts.max_per_layer, Some(7));
        assert_eq!(opts.confidence_threshold, Some(0.7));
        assert_eq!(opts.result_limit, Some(50));
    }

    // ── Helper: create an ImpactedSymbol for boundary tests ─────────

    fn make_impacted(name: &str, hop_depth: u32, community_id: Option<u32>) -> ImpactedSymbol {
        ImpactedSymbol {
            symbol_uid: format!("uid_{}", name),
            name: name.to_string(),
            file_path: format!("src/{}.rs", name.to_lowercase()),
            kind: "function".to_string(),
            risk_level: RiskLevel::from_hop_depth(hop_depth),
            hop_depth,
            community_id,
            confidence: 1.0,
        }
    }
}
