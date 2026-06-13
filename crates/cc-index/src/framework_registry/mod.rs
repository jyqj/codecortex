//! Framework detection, persistence, and query layer.
//!
//! Scans the index database (imports, file paths, route_edges) to detect which
//! frameworks are used in the repo and per-file.  Results are persisted to
//! `repo_frameworks` and `file_frameworks` tables so the runtime doesn't
//! need to re-detect on every context preparation.
//!
//! Multi-signal scoring:
//!   - import marker:     +0.40
//!   - file path:         +0.30
//!   - route framework:   +0.25
//!   - symbol pattern:    +0.20
//!   - package marker:    +0.60  (package.json / pyproject / go.mod / Cargo.toml)
//!
//! A framework is considered detected when its score exceeds `DETECTION_THRESHOLD`.
//!
//! Each detection signal lives in its own submodule and owns its literal table
//! plus its SQL/scan logic, declared once as a [`FrameworkSignalSpec`]
//! (mirroring `SynthesisPassSpec` in `dispatch_synthesis`). [`signal_registry`]
//! lists the per-file signals in execution order; the orchestration here only
//! iterates the registry, aggregates repo-level signals, persists, and queries.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use cc_db::index_db::{FileFrameworkRecord, FileFrameworkSignal, IndexDb, RepoFrameworkRecord};
use cc_model::{CcError, CcResult};

mod activation;
mod file_path;
mod import_marker;
mod package_manifest;
mod route_framework;
mod symbol_pattern;

// Shared consumers keep their existing paths: indexer.rs enrichment reads the
// import-marker table, and `check_package_markers` is part of the public API.
pub(crate) use import_marker::import_marker_table;
pub use package_manifest::check_package_markers;

use activation::activation_literals_table;
use route_framework::normalize_route_framework;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

const DETECTION_THRESHOLD: f64 = 0.35;

// ---------------------------------------------------------------------------
// Signal weights
// ---------------------------------------------------------------------------

const WEIGHT_IMPORT: f64 = 0.40;
const WEIGHT_FILE_PATH: f64 = 0.30;
const WEIGHT_ROUTE_FRAMEWORK: f64 = 0.25;
const WEIGHT_SYMBOL_PATTERN: f64 = 0.20;
const WEIGHT_PACKAGE_MARKER: f64 = 0.60;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// Result of framework detection for a single file or the repo.
#[derive(Debug, Clone)]
pub struct FileFrameworkDetection {
    pub framework_key: String,
    pub confidence: f64,
    pub signals: Vec<String>,
}

// ---------------------------------------------------------------------------
// Declarative signal seam
// ---------------------------------------------------------------------------

/// Read-only inputs shared by every per-file detection signal scan.
pub(crate) struct SignalContext<'a> {
    pub(crate) conn: &'a rusqlite::Connection,
    pub(crate) file_path: &'a str,
}

/// Single declaration point for one per-file detection signal: its identity
/// and its scan entry point. Each signal accumulates weighted detections into
/// the shared map; thresholding happens once after all signals ran.
pub(crate) struct FrameworkSignalSpec {
    /// Stable signal identifier (assertions, tests).
    pub(crate) id: &'static str,
    /// Scan one file's index data and accumulate weighted detections.
    pub(crate) detect: fn(&SignalContext, &mut HashMap<String, FileFrameworkDetection>),
}

/// All per-file detection signals in execution order. Import markers run
/// first so the `require()` fallback's already-detected snapshot covers
/// exactly the declared-import matches of the same scan.
pub(crate) fn signal_registry() -> &'static [FrameworkSignalSpec] {
    const REGISTRY: &[FrameworkSignalSpec] = &[
        import_marker::SPEC,
        file_path::SPEC,
        route_framework::SPEC,
        symbol_pattern::SPEC,
    ];
    debug_assert!(
        REGISTRY
            .iter()
            .map(|spec| spec.id)
            .collect::<HashSet<_>>()
            .len()
            == REGISTRY.len(),
        "framework signal ids must be unique"
    );
    REGISTRY
}

/// Fetch-or-create the accumulating detection entry for `fw_key`.
fn detection_entry<'a>(
    detections: &'a mut HashMap<String, FileFrameworkDetection>,
    fw_key: &str,
) -> &'a mut FileFrameworkDetection {
    detections
        .entry(fw_key.to_string())
        .or_insert_with(|| FileFrameworkDetection {
            framework_key: fw_key.to_string(),
            confidence: 0.0,
            signals: Vec::new(),
        })
}

// ---------------------------------------------------------------------------
// Per-file detection from index data
// ---------------------------------------------------------------------------

/// Detect frameworks for a single file by querying its imports, file path,
/// route_edges, and symbol patterns from the index database.
pub fn detect_file_frameworks(db: &IndexDb, file_path: &str) -> Vec<FileFrameworkDetection> {
    let conn = match db.reads().read_conn() {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    detect_file_frameworks_conn(&conn, file_path)
}

/// Detect frameworks for a single file using an existing read connection.
///
/// Split out from [`detect_file_frameworks`] so the full-repo scan can reuse one
/// pooled connection and `prepare_cached` statements across all files, instead
/// of re-acquiring a connection and recompiling 4 queries per file.
fn detect_file_frameworks_conn(
    conn: &rusqlite::Connection,
    file_path: &str,
) -> Vec<FileFrameworkDetection> {
    let mut detections: HashMap<String, FileFrameworkDetection> = HashMap::new();
    let ctx = SignalContext { conn, file_path };

    for spec in signal_registry() {
        (spec.detect)(&ctx, &mut detections);
    }

    detections
        .into_values()
        .filter(|d| d.confidence >= DETECTION_THRESHOLD)
        .collect()
}

// ---------------------------------------------------------------------------
// Repo-level detection
// ---------------------------------------------------------------------------

/// Aggregate per-file detections + repo-level signals into a repo framework set.
pub fn detect_repo_frameworks(db: &IndexDb, project_path: &Path) -> Vec<FileFrameworkDetection> {
    let conn = match db.reads().read_conn() {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut repo_scores: HashMap<String, FileFrameworkDetection> = HashMap::new();

    // 1. Aggregate from frameworks (file scope) already persisted
    if let Ok(mut stmt) = conn.prepare(
        "SELECT framework_key, COUNT(*) as cnt, MAX(confidence) as max_conf \
         FROM frameworks WHERE scope='file' GROUP BY framework_key",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, f64>(2)?,
            ))
        }) {
            for row in rows.flatten() {
                let (fw_key, count, max_conf) = row;
                // Repo confidence: max file confidence + breadth bonus
                let breadth_bonus = (count as f64 * 0.05).min(0.25);
                let confidence = (max_conf + breadth_bonus).min(0.95);
                repo_scores.insert(
                    fw_key.clone(),
                    FileFrameworkDetection {
                        framework_key: fw_key,
                        confidence,
                        signals: vec![
                            format!("file_count:{}", count),
                            format!("max_file_conf:{:.2}", max_conf),
                        ],
                    },
                );
            }
        }
    }

    // 2. Route-edge framework signal
    if let Ok(mut stmt) = conn.prepare(
        "SELECT DISTINCT framework FROM routes WHERE framework IS NOT NULL AND framework != ''",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) {
            for fw in rows.flatten() {
                if let Some(fw_key) = normalize_route_framework(&fw) {
                    let det = detection_entry(&mut repo_scores, fw_key);
                    det.confidence = (det.confidence + WEIGHT_ROUTE_FRAMEWORK).min(0.95);
                    det.signals.push(format!("route_framework:{}", fw));
                }
            }
        }
    }

    // 3. Package marker signal from filesystem
    let pkg_markers = check_package_markers(project_path);
    for (fw_key, pkg_conf) in pkg_markers {
        let det = detection_entry(&mut repo_scores, &fw_key);
        det.confidence = (det.confidence + pkg_conf).min(0.95);
        det.signals.push("package_marker".to_string());
    }

    repo_scores
        .into_values()
        .filter(|d| d.confidence >= DETECTION_THRESHOLD)
        .collect()
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

/// Run full framework detection and persist to `repo_frameworks` + `file_frameworks`.
///
/// Called after indexing completes.
pub fn detect_and_persist_frameworks(db: &IndexDb, project_path: &Path) -> CcResult<()> {
    use crate::indexer_phases::time_step;

    // 1. Gather all file paths
    let file_paths: Vec<String> = time_step("write", "fw_list_files", || -> CcResult<_> {
        let conn = db.reads().read_conn()?;
        let mut stmt = conn
            .prepare("SELECT file_path FROM files")
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| CcError::Database(e.to_string()))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    })?;

    // 2. Per-file detection — reuse one read connection across all files so each
    //    query is prepared once (cached) rather than re-acquiring a connection
    //    and recompiling 4 statements per file. Scoped so the connection is
    //    released before the write below.
    let detect_start = std::time::Instant::now();
    let mut file_records: Vec<FileFrameworkRecord> = Vec::new();
    {
        let conn = db.reads().read_conn()?;
        for fp in &file_paths {
            let detections = detect_file_frameworks_conn(&conn, fp);
            if !detections.is_empty() {
                let signals: Vec<FileFrameworkSignal> = detections
                    .into_iter()
                    .map(|d| {
                        let evidence = d.signals.join(", ");
                        (d.framework_key, d.confidence, evidence)
                    })
                    .collect();
                file_records.push((fp.clone(), signals));
            }
        }
    }
    tracing::debug!(
        phase = "write",
        step = "fw_detect_all_files",
        files = file_paths.len(),
        elapsed_ms = detect_start.elapsed().as_millis() as u64,
        "sub-phase timing"
    );

    // 3. Persist file frameworks
    time_step("write", "fw_replace_file_frameworks", || {
        db.writes().replace_file_frameworks(&file_records)
    })?;

    // 4. Repo-level aggregation (depends on file_frameworks being written)
    time_step("write", "fw_refresh_repo", || {
        refresh_repo_frameworks(db, project_path)
    })?;

    Ok(())
}

/// Recompute repo-level frameworks from persisted file detections and package markers.
pub fn refresh_repo_frameworks(db: &IndexDb, project_path: &Path) -> CcResult<()> {
    let repo_detections = detect_repo_frameworks(db, project_path);
    let repo_records: Vec<RepoFrameworkRecord> = repo_detections
        .into_iter()
        .map(|d| (d.framework_key, d.confidence, d.signals))
        .collect();

    db.writes().replace_repo_frameworks(&repo_records)
}

/// Incremental framework detection: only re-scan changed files.
///
/// Re-detecting exactly the changed files is complete for any changeset size:
/// every per-file signal (imports, chunks `require()` fallback, routes,
/// symbols, path patterns) is a pure function of that file's own index rows
/// plus its path string, so an unchanged file's detection cannot change. The
/// only global inputs — package manifests and the repo-wide route/file
/// aggregation — live exclusively at repo scope and are recomputed by the
/// unconditional [`refresh_repo_frameworks`] call below. (A `>= 20 files =>
/// full repo rescan` fallback used to live here; it re-ran all 5 signal
/// queries over every indexed file for no added guarantee.)
///
/// Files whose signals all disappeared still get an empty record pushed so
/// `replace_file_frameworks` deletes their stale rows.
pub fn detect_and_persist_frameworks_incremental(
    db: &IndexDb,
    project_path: &Path,
    changed_files: &[&str],
) -> CcResult<()> {
    if changed_files.is_empty() {
        return Ok(());
    }

    // Per-file incremental update — one pooled read connection across the
    // loop so `prepare_cached` statements are reused per file (mirrors the
    // full-scan path). Scoped so the connection is released before the write.
    let detect_start = std::time::Instant::now();
    let mut file_records: Vec<FileFrameworkRecord> = Vec::new();
    {
        let conn = db.reads().read_conn()?;
        for &fp in changed_files {
            // Check the file still exists in the index
            let exists = conn
                .prepare_cached("SELECT 1 FROM files WHERE file_path = ?1")
                .ok()
                .map(|mut stmt| stmt.query_row(rusqlite::params![fp], |_| Ok(())).is_ok())
                .unwrap_or(false);
            if !exists {
                continue;
            }

            let detections = detect_file_frameworks_conn(&conn, fp);
            let signals: Vec<FileFrameworkSignal> = detections
                .into_iter()
                .map(|d| {
                    let evidence = d.signals.join(", ");
                    (d.framework_key, d.confidence, evidence)
                })
                .collect();
            file_records.push((fp.to_string(), signals));
        }
    }
    tracing::debug!(
        phase = "write",
        step = "fw_detect_changed_files",
        files = changed_files.len(),
        elapsed_ms = detect_start.elapsed().as_millis() as u64,
        "sub-phase timing"
    );

    crate::indexer_phases::time_step("write", "fw_replace_file_frameworks", || {
        db.writes().replace_file_frameworks(&file_records)
    })?;

    // Repo-level aggregation is cheap because it reads the persisted
    // file_frameworks table plus package markers. Keep it current even for
    // small increments; otherwise a fresh incremental build can leave
    // repo_frameworks empty.
    crate::indexer_phases::time_step("write", "fw_refresh_repo", || {
        refresh_repo_frameworks(db, project_path)
    })
}

// ---------------------------------------------------------------------------
// Query helpers (used by runtime)
// ---------------------------------------------------------------------------

/// Return all detected repo-level frameworks as `(framework_key, confidence)`.
pub fn get_repo_frameworks(db: &IndexDb) -> Vec<(String, f64)> {
    db.reads().list_repo_frameworks().unwrap_or_default()
}

/// Return `{file_path: [(framework_key, confidence), ...]}` for a set of files.
pub fn get_frameworks_for_files(
    db: &IndexDb,
    file_paths: &[&str],
) -> HashMap<String, Vec<(String, f64)>> {
    if file_paths.is_empty() {
        return HashMap::new();
    }
    let conn = match db.reads().read_conn() {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };
    let placeholders: String = file_paths.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT scope_id, framework_key, confidence FROM frameworks \
         WHERE scope='file' AND scope_id IN ({}) ORDER BY confidence DESC",
        placeholders
    );
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return HashMap::new(),
    };
    let params: Vec<&dyn rusqlite::types::ToSql> = file_paths
        .iter()
        .map(|p| p as &dyn rusqlite::types::ToSql)
        .collect();
    let rows = match stmt.query_map(params.as_slice(), |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, f64>(2)?,
        ))
    }) {
        Ok(r) => r,
        Err(_) => return HashMap::new(),
    };

    let mut result: HashMap<String, Vec<(String, f64)>> = HashMap::new();
    for row in rows.flatten() {
        let (fp, fw_key, conf) = row;
        result.entry(fp).or_default().push((fw_key, conf));
    }
    result
}

/// Return just the framework keys detected at repo level.
pub fn get_repo_framework_keys(db: &IndexDb) -> Vec<String> {
    get_repo_frameworks(db)
        .into_iter()
        .map(|(k, _)| k)
        .collect()
}

// ---------------------------------------------------------------------------
// Active framework computation
// ---------------------------------------------------------------------------

/// Compute a ranked list of active frameworks from multi-signal evidence.
///
/// Signals are bucketed into 3 tiers:
///   - Strong (0.4): framework detected in `working_files`
///   - Medium (0.1): framework detected in `evidence_files`
///   - Weak   (0.01): framework in repo but not in working/evidence files
///   - Task text activation: check activation_literals
///
/// Returns `(framework_key, score)` sorted by score descending.
pub fn compute_active_frameworks(
    db: &IndexDb,
    task_text: &str,
    working_files: &[&str],
    evidence_files: &[&str],
) -> Vec<(String, f64)> {
    struct Accum {
        strong: u32,
        medium: u32,
        weak: u32,
    }

    let mut accum: HashMap<String, Accum> = HashMap::new();

    let mut bump = |fw_key: &str, tier: &str| {
        let acc = accum.entry(fw_key.to_string()).or_insert(Accum {
            strong: 0,
            medium: 0,
            weak: 0,
        });
        match tier {
            "strong" => acc.strong += 1,
            "medium" => acc.medium += 1,
            _ => acc.weak += 1,
        }
    };

    // --- Strong: working_files ---
    if !working_files.is_empty() {
        let fw_map = get_frameworks_for_files(db, working_files);
        for fw_list in fw_map.values() {
            for (fw_key, _conf) in fw_list {
                bump(fw_key, "strong");
            }
        }
    }

    // --- Medium: evidence_files ---
    if !evidence_files.is_empty() {
        let fw_map = get_frameworks_for_files(db, evidence_files);
        for fw_list in fw_map.values() {
            for (fw_key, _conf) in fw_list {
                bump(fw_key, "medium");
            }
        }
    }

    // --- Medium: task text activation literals (needs >= 2 matches) ---
    if !task_text.is_empty() {
        let task_lower = task_text.to_lowercase();
        for &(fw_key, literals) in activation_literals_table() {
            let match_count = literals
                .iter()
                .filter(|lit| task_lower.contains(&lit.to_lowercase()))
                .count();
            if match_count >= 2 {
                bump(fw_key, "medium");
            }
        }
    }

    // --- Weak: repo baseline ---
    for fw_key in get_repo_framework_keys(db) {
        bump(&fw_key, "weak");
    }

    // --- Scoring & filtering ---
    let mut results: Vec<(String, f64)> = Vec::new();
    for (fw_key, acc) in &accum {
        if acc.strong == 0 && acc.medium == 0 {
            continue; // pure weak — not worth pushing
        }
        let score = acc.strong as f64 * 0.4 + acc.medium as f64 * 0.1 + acc.weak as f64 * 0.01;
        results.push((fw_key.clone(), score));
    }

    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    results
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// The `require()` fallback inside the import-marker signal snapshots
    /// `already_detected` from the detections accumulated so far; it must see
    /// exactly the declared-import hits of the same scan and nothing from the
    /// later signals. Reordering the registry would silently change detection
    /// results, so the order is pinned here.
    #[test]
    fn test_import_marker_signal_runs_first() {
        assert_eq!(
            signal_registry()[0].id,
            "import_marker",
            "import_marker must stay first: the require() fallback snapshot \
             depends on it running before any other signal"
        );
    }

    /// Helper: create a minimal IndexDb with the schema.
    fn setup_test_db() -> (TempDir, IndexDb) {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("test_index.sqlite3");
        let db = IndexDb::open(&db_path).unwrap().0;
        (tmp, db)
    }

    /// Helper: insert a file + import records into the test DB for detection.
    fn insert_test_file(db: &IndexDb, file_path: &str, imports: &[&str]) {
        let conn = crate::test_seed::seed_conn(db);
        conn.execute(
            "INSERT OR IGNORE INTO files(file_path, language, content_hash, mtime, size, indexed_at) \
             VALUES(?1, 'typescript', 'abc', 0.0, 100, '2025-01-01')",
            rusqlite::params![file_path],
        )
        .unwrap();
        for imp in imports {
            conn.execute(
                "INSERT INTO imports(file_path, import_string) VALUES(?1, ?2)",
                rusqlite::params![file_path, imp],
            )
            .unwrap();
        }
    }

    #[test]
    fn test_detect_file_frameworks_import_signal() {
        let (_tmp, db) = setup_test_db();
        insert_test_file(&db, "src/app.ts", &["express", "cors"]);

        let detections = detect_file_frameworks(&db, "src/app.ts");
        assert!(
            detections.iter().any(|d| d.framework_key == "express"),
            "should detect express from import"
        );
        let express_det = detections
            .iter()
            .find(|d| d.framework_key == "express")
            .unwrap();
        assert!(
            express_det.confidence >= WEIGHT_IMPORT - 0.001,
            "express confidence should be at least WEIGHT_IMPORT"
        );
        assert!(
            express_det.signals.iter().any(|s| s.starts_with("import:")),
            "should have an import signal"
        );
    }

    #[test]
    fn test_detect_file_frameworks_file_path_signal() {
        let (_tmp, db) = setup_test_db();
        // Next.js path pattern + next import
        insert_test_file(&db, "app/api/users/route.ts", &["next/server"]);

        let detections = detect_file_frameworks(&db, "app/api/users/route.ts");
        assert!(
            detections.iter().any(|d| d.framework_key == "nextjs"),
            "should detect nextjs from import + file path"
        );
        let nextjs_det = detections
            .iter()
            .find(|d| d.framework_key == "nextjs")
            .unwrap();
        // Should have both import and file_path signals
        assert!(
            nextjs_det.confidence >= WEIGHT_IMPORT + WEIGHT_FILE_PATH - 0.001,
            "nextjs confidence should combine import + file_path weights, got {}",
            nextjs_det.confidence
        );
    }

    #[test]
    fn test_check_package_markers_node() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("package.json"),
            r#"{"dependencies":{"express":"^4.18","cors":"^2.8"}}"#,
        )
        .unwrap();

        let markers = check_package_markers(tmp.path());
        assert!(
            markers.contains_key("express"),
            "should detect express from package.json"
        );
        assert!(
            *markers.get("express").unwrap() >= WEIGHT_PACKAGE_MARKER - 0.001,
            "express package marker confidence should be at least WEIGHT_PACKAGE_MARKER"
        );
    }

    #[test]
    fn test_check_package_markers_python() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("pyproject.toml"),
            "[project]\ndependencies = [\"fastapi>=0.100\", \"uvicorn\"]\n",
        )
        .unwrap();

        let markers = check_package_markers(tmp.path());
        assert!(
            markers.contains_key("fastapi"),
            "should detect fastapi from pyproject.toml"
        );
    }

    #[test]
    fn test_check_package_markers_go() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("go.mod"),
            "module example.com/myapp\nrequire github.com/gin-gonic/gin v1.9.0\n",
        )
        .unwrap();

        let markers = check_package_markers(tmp.path());
        assert!(markers.contains_key("gin"), "should detect gin from go.mod");
    }

    #[test]
    fn test_compute_active_frameworks_scoring() {
        let (_tmp, db) = setup_test_db();

        // Insert files with framework associations
        insert_test_file(&db, "src/app.ts", &["express"]);
        insert_test_file(&db, "src/utils.ts", &[]);
        insert_test_file(&db, "src/routes.ts", &["express"]);

        // Persist file frameworks first
        let detections1 = detect_file_frameworks(&db, "src/app.ts");
        let detections2 = detect_file_frameworks(&db, "src/routes.ts");
        let mut file_records: Vec<FileFrameworkRecord> = Vec::new();
        for (fp, dets) in [("src/app.ts", detections1), ("src/routes.ts", detections2)] {
            if !dets.is_empty() {
                let signals: Vec<FileFrameworkSignal> = dets
                    .into_iter()
                    .map(|d| (d.framework_key, d.confidence, d.signals.join(", ")))
                    .collect();
                file_records.push((fp.to_string(), signals));
            }
        }
        db.writes().replace_file_frameworks(&file_records).unwrap();

        // Repo level
        let repo_records: Vec<RepoFrameworkRecord> =
            vec![("express".to_string(), 0.9, vec!["file_count:2".to_string()])];
        db.writes().replace_repo_frameworks(&repo_records).unwrap();

        // Compute active frameworks
        let active = compute_active_frameworks(
            &db,
            "fix the express middleware bug",
            &["src/app.ts"],
            &["src/routes.ts"],
        );

        assert!(
            !active.is_empty(),
            "should find at least one active framework"
        );
        assert_eq!(
            active[0].0, "express",
            "express should be the top active framework"
        );
        // strong(1 file) + medium(1 evidence + task_text) + weak(repo baseline)
        assert!(
            active[0].1 > 0.1,
            "express score should be significant, got {}",
            active[0].1
        );
    }

    #[test]
    fn test_below_threshold_not_detected() {
        let (_tmp, db) = setup_test_db();
        // File with no matching imports
        insert_test_file(&db, "src/plain.ts", &["lodash", "uuid"]);

        let detections = detect_file_frameworks(&db, "src/plain.ts");
        assert!(
            detections.is_empty(),
            "no framework should be detected for generic utility imports"
        );
    }

    #[test]
    fn test_detect_new_frameworks_import_signal() {
        let (_tmp, db) = setup_test_db();

        // Laravel detection via import
        insert_test_file(
            &db,
            "app/Http/Controllers/UserController.php",
            &["Illuminate\\Http"],
        );
        let dets = detect_file_frameworks(&db, "app/Http/Controllers/UserController.php");
        assert!(
            dets.iter().any(|d| d.framework_key == "laravel"),
            "should detect laravel from Illuminate import + file path, got: {:?}",
            dets.iter().map(|d| &d.framework_key).collect::<Vec<_>>()
        );

        // SvelteKit detection via import
        insert_test_file(&db, "src/routes/+page.svelte", &["@sveltejs/kit"]);
        let dets = detect_file_frameworks(&db, "src/routes/+page.svelte");
        assert!(
            dets.iter().any(|d| d.framework_key == "sveltekit"),
            "should detect sveltekit from import + file path, got: {:?}",
            dets.iter().map(|d| &d.framework_key).collect::<Vec<_>>()
        );

        // Remix detection via import
        insert_test_file(&db, "app/routes/index.tsx", &["@remix-run/react"]);
        let dets = detect_file_frameworks(&db, "app/routes/index.tsx");
        assert!(
            dets.iter().any(|d| d.framework_key == "remix"),
            "should detect remix from import + file path, got: {:?}",
            dets.iter().map(|d| &d.framework_key).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_check_package_markers_laravel() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("composer.json"),
            r#"{"require":{"laravel/framework":"^10.0","php":"^8.1"}}"#,
        )
        .unwrap();

        let markers = check_package_markers(tmp.path());
        assert!(
            markers.contains_key("laravel"),
            "should detect laravel from composer.json"
        );
    }

    #[test]
    fn test_check_package_markers_rails() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("Gemfile"),
            "source 'https://rubygems.org'\ngem 'rails', '~> 7.0'\ngem 'puma'\n",
        )
        .unwrap();

        let markers = check_package_markers(tmp.path());
        assert!(
            markers.contains_key("rails"),
            "should detect rails from Gemfile"
        );
    }

    #[test]
    fn test_check_package_markers_sveltekit() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("package.json"),
            r#"{"devDependencies":{"@sveltejs/kit":"^2.0","svelte":"^4.0"}}"#,
        )
        .unwrap();

        let markers = check_package_markers(tmp.path());
        assert!(
            markers.contains_key("sveltekit"),
            "should detect sveltekit from package.json"
        );
    }

    /// Characterization lock for the per-file signal scan: covers the signal
    /// paths the other tests miss (CommonJS `require()` fallback, route
    /// framework, symbol pattern), cross-signal score accumulation, signal
    /// ordering within a detection, and threshold filtering of weak signals.
    #[test]
    fn test_detection_signals_characterization() {
        let (_tmp, db) = setup_test_db();

        // --- require() fallback: marker only inside chunk text, no import rows ---
        insert_test_file(&db, "src/legacy_server.js", &[]);
        {
            let conn = crate::test_seed::seed_conn(&db);
            conn.execute(
                "INSERT INTO chunks(chunk_id, file_path, language, chunk_index, start_line, end_line, text) \
                 VALUES('ck_legacy', 'src/legacy_server.js', 'javascript', 0, 1, 10, ?1)",
                rusqlite::params!["const Koa = require('koa');"],
            )
            .unwrap();
        }
        let dets = detect_file_frameworks(&db, "src/legacy_server.js");
        assert_eq!(dets.len(), 1, "only koa should be detected");
        assert_eq!(dets[0].framework_key, "koa");
        assert!((dets[0].confidence - WEIGHT_IMPORT).abs() < 0.001);
        assert_eq!(dets[0].signals, vec!["require:koa".to_string()]);

        // --- combined file: import + route + symbol signals accumulate; the
        //     require() fallback must NOT double-count an already-imported
        //     framework; a lone symbol signal stays below threshold ---
        insert_test_file(&db, "src/combined_api.ts", &["express"]);
        {
            let conn = crate::test_seed::seed_conn(&db);
            conn.execute(
                "INSERT INTO chunks(chunk_id, file_path, language, chunk_index, start_line, end_line, text) \
                 VALUES('ck_combined', 'src/combined_api.ts', 'typescript', 0, 1, 10, ?1)",
                rusqlite::params!["const express = require('express');"],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO routes(edge_id, file_path, route_path, line, framework) \
                 VALUES('rt_combined', 'src/combined_api.ts', '/users', 3, 'Express')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO symbols(symbol_id, file_path, name, kind, start_line, end_line, framework_role) \
                 VALUES('sym_mw', 'src/combined_api.ts', 'authMiddleware', 'function', 1, 5, 'middleware')",
                [],
            )
            .unwrap();
            // role 'hook' maps to react, but symbol weight alone is sub-threshold
            conn.execute(
                "INSERT INTO symbols(symbol_id, file_path, name, kind, start_line, end_line, framework_role) \
                 VALUES('sym_hook', 'src/combined_api.ts', 'useThing', 'function', 6, 9, 'hook')",
                [],
            )
            .unwrap();
        }
        let dets = detect_file_frameworks(&db, "src/combined_api.ts");
        assert_eq!(
            dets.len(),
            1,
            "express only; sub-threshold react must be filtered, got: {:?}",
            dets.iter().map(|d| &d.framework_key).collect::<Vec<_>>()
        );
        let express_det = &dets[0];
        assert_eq!(express_det.framework_key, "express");
        let expected_conf = WEIGHT_IMPORT + WEIGHT_ROUTE_FRAMEWORK + WEIGHT_SYMBOL_PATTERN;
        assert!(
            (express_det.confidence - expected_conf).abs() < 0.001,
            "confidence should accumulate import + route + symbol weights, got {}",
            express_det.confidence
        );
        assert_eq!(
            express_det.signals,
            vec![
                "import:express".to_string(),
                "route_framework:Express".to_string(),
                "symbol_pattern:middleware".to_string(),
            ],
            "signal order must follow the scan order (import, route, symbol)"
        );
    }

    /// Incremental persistence must clear a file's stale framework rows once
    /// its signals disappear, for any changeset size. The changeset here has
    /// 21 entries (1 real file + 20 paths absent from the index) to pin the
    /// removal of the old `>= 20 files => full repo rescan` fallback: the
    /// full-scan path only pushes non-empty records, so the fallback would
    /// have left the stale `express` row behind.
    #[test]
    fn test_incremental_clears_stale_rows_when_signals_disappear() {
        let (tmp, db) = setup_test_db();
        insert_test_file(&db, "src/app.ts", &["express"]);

        detect_and_persist_frameworks_incremental(&db, tmp.path(), &["src/app.ts"]).unwrap();
        let fw_map = get_frameworks_for_files(&db, &["src/app.ts"]);
        assert!(
            fw_map
                .get("src/app.ts")
                .is_some_and(|fws| fws.iter().any(|(key, _)| key == "express")),
            "express row should be persisted while the import signal exists"
        );

        // Simulate a re-index where the file lost its only framework signal.
        {
            let conn = crate::test_seed::seed_conn(&db);
            conn.execute("DELETE FROM imports WHERE file_path = 'src/app.ts'", [])
                .unwrap();
        }

        let phantom_paths: Vec<String> = (0..20).map(|n| format!("src/ghost_{}.ts", n)).collect();
        let mut changed: Vec<&str> = vec!["src/app.ts"];
        changed.extend(phantom_paths.iter().map(String::as_str));
        assert!(
            changed.len() >= 20,
            "changeset must exercise the large-set path"
        );

        detect_and_persist_frameworks_incremental(&db, tmp.path(), &changed).unwrap();
        let fw_map = get_frameworks_for_files(&db, &["src/app.ts"]);
        assert!(
            fw_map.get("src/app.ts").is_none_or(|fws| fws.is_empty()),
            "stale framework rows must be cleared when signals disappear, got: {:?}",
            fw_map.get("src/app.ts")
        );
    }

    #[test]
    fn test_normalize_route_framework_new_entries() {
        assert_eq!(normalize_route_framework("laravel"), Some("laravel"));
        assert_eq!(normalize_route_framework("rails"), Some("rails"));
        assert_eq!(normalize_route_framework("aspnet"), Some("aspnet"));
        assert_eq!(normalize_route_framework("asp.net"), Some("aspnet"));
        assert_eq!(normalize_route_framework("sveltekit"), Some("sveltekit"));
        assert_eq!(normalize_route_framework("nuxt"), Some("nuxt"));
        assert_eq!(normalize_route_framework("remix"), Some("remix"));
        assert_eq!(normalize_route_framework("hono"), Some("hono"));
        assert_eq!(normalize_route_framework("vue_router"), Some("vue_router"));
        assert_eq!(normalize_route_framework("vue-router"), Some("vue_router"));
    }
}
