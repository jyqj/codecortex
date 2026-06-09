//! File preselection — narrows candidate files before chunk-level search.
//!
//! Implements the full 6-layer scoring strategy ported from Python's
//! `_preselect_files()`:
//!   1. working-set boost   max(2.0, 5.0 / rank)
//!   2. recent files         max(1.2, 3.5 / rank)
//!   3. pinned files         max(2.2, 4.0 / rank)
//!   4. overlay (dirty)      max(1.5, 3.0 / rank)
//!   5. FTS summary search   1.4 + 1.0 / (1.0 + |score|)
//!   6. per-token: symbol name match (exact=2.0, fuzzy=1.2) + path token hit (1.0)
//!      fallback: recently-indexed files if nothing scored

use std::collections::HashMap;

use cc_db::fts::{sanitize_fts_query, tokenize_codeish};
use cc_db::index_db::IndexDb;
use cc_model::CcResult;

// ── Public types ───────────────────────────────────────────────

/// Bundles the 9 parameters that `preselect_files` used to take individually.
pub struct PreselectRequest<'a> {
    pub query: &'a str,
    pub path_prefix: Option<&'a str>,
    pub boost_paths: Option<&'a [String]>,
    pub recent_paths: Option<&'a [String]>,
    pub pinned_paths: Option<&'a [String]>,
    pub overlay_paths: Option<&'a [String]>,
    pub explicit_file_paths: Option<&'a [String]>,
    pub limit: usize,
}

/// Statistics about which scoring lanes fired during preselection.
#[derive(Debug, Clone, Default)]
pub struct LaneStats {
    pub fts_hits: usize,
    pub token_hits: usize,
    pub used_fallback: bool,
}

/// Result of file preselection: ordered file paths + per-file scores + per-file reason lists.
#[derive(Debug, Clone)]
pub struct PreselectResult {
    pub files: Vec<String>,
    pub scores: HashMap<String, f64>,
    pub reasons: HashMap<String, Vec<String>>,
    pub lane_stats: LaneStats,
}

// ── Internal helpers ───────────────────────────────────────────

/// Add `score` to a file and record the reason.
fn score_file(
    scores: &mut HashMap<String, f64>,
    reasons: &mut HashMap<String, Vec<String>>,
    file_path: &str,
    score: f64,
    reason: &str,
) {
    let normalized = file_path.replace('\\', "/");
    *scores.entry(normalized.clone()).or_insert(0.0) += score;
    reasons
        .entry(normalized)
        .or_default()
        .push(reason.to_string());
}

// ── Layer functions ────────────────────────────────────────────

/// Layer 1: working-set boost — `max(2.0, 5.0 / rank)`
fn score_working_set(
    paths: &[String],
    scores: &mut HashMap<String, f64>,
    reasons: &mut HashMap<String, Vec<String>>,
) {
    for (rank, fp) in paths.iter().enumerate() {
        let rank1 = (rank + 1) as f64;
        score_file(scores, reasons, fp, f64::max(2.0, 5.0 / rank1), "working-set");
    }
}

/// Layer 2: recent files — `max(1.2, 3.5 / rank)`
fn score_recent(
    paths: &[String],
    scores: &mut HashMap<String, f64>,
    reasons: &mut HashMap<String, Vec<String>>,
) {
    for (rank, fp) in paths.iter().enumerate() {
        let rank1 = (rank + 1) as f64;
        score_file(scores, reasons, fp, f64::max(1.2, 3.5 / rank1), "recent");
    }
}

/// Layer 3: pinned files — `max(2.2, 4.0 / rank)`
fn score_pinned(
    paths: &[String],
    scores: &mut HashMap<String, f64>,
    reasons: &mut HashMap<String, Vec<String>>,
) {
    for (rank, fp) in paths.iter().enumerate() {
        let rank1 = (rank + 1) as f64;
        score_file(scores, reasons, fp, f64::max(2.2, 4.0 / rank1), "pinned");
    }
}

/// Layer 4: overlay (dirty-buffer) files — `max(1.5, 3.0 / rank)`
fn score_overlay(
    paths: &[String],
    scores: &mut HashMap<String, f64>,
    reasons: &mut HashMap<String, Vec<String>>,
) {
    for (rank, fp) in paths.iter().enumerate() {
        let rank1 = (rank + 1) as f64;
        score_file(
            scores,
            reasons,
            fp,
            f64::max(1.5, 3.0 / rank1),
            "dirty-buffer",
        );
    }
}

/// Layer 5: FTS summary search on `files_fts`. Returns the number of hits.
fn score_fts_summary(
    db: &IndexDb,
    query: &str,
    prefix: Option<&str>,
    limit: usize,
    scores: &mut HashMap<String, f64>,
    reasons: &mut HashMap<String, Vec<String>>,
) -> usize {
    let fts_query = sanitize_fts_query(query);
    if fts_query == r#""""# || fts_query.is_empty() {
        return 0;
    }

    let conn = match db.read_conn() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("preselect: FTS summary read_conn failed: {}", e);
            return 0;
        }
    };

    let fts_limit = limit.min(80);
    let (sql, params_vec): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(pfx) =
        prefix
    {
        (
            "SELECT files.file_path, bm25(files_fts, 1.8, 1.0) AS score \
             FROM files_fts \
             JOIN files ON files.file_path = files_fts.file_path \
             WHERE files_fts MATCH ?1 AND files.file_path LIKE ?2 \
             ORDER BY score LIMIT ?3"
                .into(),
            vec![
                Box::new(fts_query) as Box<dyn rusqlite::types::ToSql>,
                Box::new(format!("{}%", pfx)),
                Box::new(fts_limit as i64),
            ],
        )
    } else {
        (
            "SELECT files.file_path, bm25(files_fts, 1.8, 1.0) AS score \
             FROM files_fts \
             JOIN files ON files.file_path = files_fts.file_path \
             WHERE files_fts MATCH ?1 \
             ORDER BY score LIMIT ?2"
                .into(),
            vec![
                Box::new(fts_query) as Box<dyn rusqlite::types::ToSql>,
                Box::new(fts_limit as i64),
            ],
        )
    };

    let mut stmt = match conn.prepare_cached(&sql) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("preselect: FTS summary prepare failed: {}", e);
            return 0;
        }
    };
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        params_vec.iter().map(|p| p.as_ref()).collect();
    let rows = match stmt.query_map(param_refs.as_slice(), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
    }) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("preselect: FTS summary query failed: {}", e);
            return 0;
        }
    };

    let mut hits = 0usize;
    for row in rows.flatten() {
        let bm25_score = row.1.abs();
        let file_score = 1.4 + (1.0 / (1.0 + bm25_score));
        score_file(scores, reasons, &row.0, file_score, "fts-summary");
        hits += 1;
    }
    hits
}

/// Layer 6: per-token symbol name match + path token hit. Returns the total
/// number of hits across all tokens.
fn score_token_search(
    db: &IndexDb,
    query: &str,
    prefix: Option<&str>,
    scores: &mut HashMap<String, f64>,
    reasons: &mut HashMap<String, Vec<String>>,
) -> usize {
    let query_tokens = tokenize_codeish(query);
    let candidate_tokens: Vec<&str> = query_tokens
        .iter()
        .filter(|t| t.len() >= 3)
        .take(8)
        .map(|s| s.as_str())
        .collect();

    if candidate_tokens.is_empty() {
        return 0;
    }

    let conn = match db.read_conn() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("preselect: token search read_conn failed: {}", e);
            return 0;
        }
    };

    let mut hits = 0usize;

    for token in &candidate_tokens {
        // 6a. Path token match
        hits += score_path_token(&conn, token, prefix, scores, reasons);
        // 6b. Symbol name match
        hits += score_symbol_token(&conn, token, prefix, scores, reasons);
    }

    hits
}

/// Layer 6a helper: path-token substring match via `file_paths_fts`.
fn score_path_token(
    conn: &rusqlite::Connection,
    token: &str,
    prefix: Option<&str>,
    scores: &mut HashMap<String, f64>,
    reasons: &mut HashMap<String, Vec<String>>,
) -> usize {
    let (sql, params_vec): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(pfx) =
        prefix
    {
        (
            "SELECT file_path FROM file_paths_fts WHERE file_path LIKE ?1 AND file_path LIKE ?2 ORDER BY file_path LIMIT 20".into(),
            vec![
                Box::new(format!("%{}%", token)) as Box<dyn rusqlite::types::ToSql>,
                Box::new(format!("{}%", pfx)),
            ],
        )
    } else {
        (
            "SELECT file_path FROM file_paths_fts WHERE file_path LIKE ?1 ORDER BY file_path LIMIT 20".into(),
            vec![Box::new(format!("%{}%", token)) as Box<dyn rusqlite::types::ToSql>],
        )
    };

    let mut stmt = match conn.prepare_cached(&sql) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("preselect: path-token prepare failed: {}", e);
            return 0;
        }
    };
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        params_vec.iter().map(|p| p.as_ref()).collect();
    let rows = match stmt.query_map(param_refs.as_slice(), |row| row.get::<_, String>(0)) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("preselect: path-token query failed: {}", e);
            return 0;
        }
    };

    let mut hits = 0usize;
    for row in rows.flatten() {
        score_file(
            scores,
            reasons,
            &row,
            1.0,
            &format!("path-token:{}", token),
        );
        hits += 1;
    }
    hits
}

/// Layer 6b helper: symbol-name substring match via `symbols_fts`.
fn score_symbol_token(
    conn: &rusqlite::Connection,
    token: &str,
    prefix: Option<&str>,
    scores: &mut HashMap<String, f64>,
    reasons: &mut HashMap<String, Vec<String>>,
) -> usize {
    let (sql, params_vec): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(pfx) =
        prefix
    {
        (
            "SELECT DISTINCT file_path, name \
             FROM symbols_fts \
             WHERE name LIKE ?1 AND file_path LIKE ?2 \
             ORDER BY CASE WHEN lower(name) = lower(?3) THEN 0 ELSE 1 END, file_path \
             LIMIT 24"
                .into(),
            vec![
                Box::new(format!("%{}%", token)) as Box<dyn rusqlite::types::ToSql>,
                Box::new(format!("{}%", pfx)),
                Box::new(token.to_string()),
            ],
        )
    } else {
        (
            "SELECT DISTINCT file_path, name \
             FROM symbols_fts \
             WHERE name LIKE ?1 \
             ORDER BY CASE WHEN lower(name) = lower(?2) THEN 0 ELSE 1 END, file_path \
             LIMIT 24"
                .into(),
            vec![
                Box::new(format!("%{}%", token)) as Box<dyn rusqlite::types::ToSql>,
                Box::new(token.to_string()),
            ],
        )
    };

    let mut stmt = match conn.prepare_cached(&sql) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("preselect: symbol-token prepare failed: {}", e);
            return 0;
        }
    };
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        params_vec.iter().map(|p| p.as_ref()).collect();
    let rows = match stmt.query_map(param_refs.as_slice(), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    }) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("preselect: symbol-token query failed: {}", e);
            return 0;
        }
    };

    let mut hits = 0usize;
    for row in rows.flatten() {
        let bonus = if row.1.to_lowercase() == *token {
            2.0
        } else {
            1.2
        };
        let reason = format!("symbol:{}", row.1);
        score_file(scores, reasons, &row.0, bonus, &reason);
        hits += 1;
    }
    hits
}

/// Fallback: recently-indexed files when nothing scored.
fn score_fallback(
    db: &IndexDb,
    limit: usize,
    scores: &mut HashMap<String, f64>,
    reasons: &mut HashMap<String, Vec<String>>,
) {
    let conn = match db.read_conn() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("preselect: fallback read_conn failed: {}", e);
            return;
        }
    };
    let mut stmt = match conn
        .prepare_cached("SELECT file_path FROM files ORDER BY indexed_at DESC LIMIT ?1")
    {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("preselect: fallback prepare failed: {}", e);
            return;
        }
    };
    let rows = match stmt.query_map(rusqlite::params![limit as i64], |row| {
        row.get::<_, String>(0)
    }) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("preselect: fallback query failed: {}", e);
            return;
        }
    };
    for row in rows.flatten() {
        score_file(scores, reasons, &row, 0.2, "fallback-indexed");
    }
}

// ── Main entry point ───────────────────────────────────────────

/// Pre-select files that are likely relevant to the query (new interface).
///
/// Accepts a [`PreselectRequest`] and returns [`PreselectResult`] with files
/// ranked by relevance score, up to `req.limit`.
pub fn preselect(db: &IndexDb, req: &PreselectRequest) -> CcResult<PreselectResult> {
    // If explicit file_paths given, return them directly (like Python).
    if let Some(fps) = req.explicit_file_paths {
        let files: Vec<String> = fps.to_vec();
        let scores: HashMap<String, f64> = files.iter().map(|f| (f.clone(), 10.0)).collect();
        let reasons: HashMap<String, Vec<String>> = files
            .iter()
            .map(|f| (f.clone(), vec!["explicit-scope".into()]))
            .collect();
        return Ok(PreselectResult {
            files,
            scores,
            reasons,
            lane_stats: LaneStats::default(),
        });
    }

    let mut scores: HashMap<String, f64> = HashMap::new();
    let mut reasons: HashMap<String, Vec<String>> = HashMap::new();
    let mut lane_stats = LaneStats::default();

    // ── Layers 1-4: context-boost layers ───────────────────────
    if let Some(paths) = req.boost_paths {
        score_working_set(paths, &mut scores, &mut reasons);
    }
    if let Some(paths) = req.recent_paths {
        score_recent(paths, &mut scores, &mut reasons);
    }
    if let Some(paths) = req.pinned_paths {
        score_pinned(paths, &mut scores, &mut reasons);
    }
    if let Some(paths) = req.overlay_paths {
        score_overlay(paths, &mut scores, &mut reasons);
    }

    // ── Layer 5: FTS summary search ────────────────────────────
    lane_stats.fts_hits =
        score_fts_summary(db, req.query, req.path_prefix, req.limit, &mut scores, &mut reasons);

    // ── Layer 6: per-token symbol + path match ─────────────────
    lane_stats.token_hits =
        score_token_search(db, req.query, req.path_prefix, &mut scores, &mut reasons);

    // ── Fallback: recently-indexed files if nothing scored ──────
    if scores.is_empty() {
        score_fallback(db, req.limit, &mut scores, &mut reasons);
        lane_stats.used_fallback = true;
    }

    // ── Filter by path_prefix, sort, truncate ──────────────────
    let mut filtered: Vec<(String, f64)> = scores
        .into_iter()
        .filter(|(path, _)| {
            if let Some(prefix) = req.path_prefix {
                path.starts_with(prefix)
            } else {
                true
            }
        })
        .collect();
    filtered.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    filtered.truncate(req.limit);

    let files: Vec<String> = filtered.iter().map(|(p, _)| p.clone()).collect();
    let final_scores: HashMap<String, f64> = filtered.into_iter().collect();
    // Deduplicate reasons per file
    let final_reasons: HashMap<String, Vec<String>> = files
        .iter()
        .map(|f| {
            let mut r = reasons.remove(f).unwrap_or_default();
            let mut seen = std::collections::HashSet::new();
            r.retain(|item| seen.insert(item.clone()));
            (f.clone(), r)
        })
        .collect();

    Ok(PreselectResult {
        files,
        scores: final_scores,
        reasons: final_reasons,
        lane_stats,
    })
}

/// Backward-compatible wrapper: delegates to [`preselect`] via [`PreselectRequest`].
#[allow(clippy::too_many_arguments)]
pub fn preselect_files(
    db: &IndexDb,
    query: &str,
    path_prefix: Option<&str>,
    boost_paths: Option<&[String]>,
    recent_paths: Option<&[String]>,
    pinned_paths: Option<&[String]>,
    overlay_paths: Option<&[String]>,
    explicit_file_paths: Option<&[String]>,
    limit: usize,
) -> CcResult<PreselectResult> {
    preselect(
        db,
        &PreselectRequest {
            query,
            path_prefix,
            boost_paths,
            recent_paths,
            pinned_paths,
            overlay_paths,
            explicit_file_paths,
            limit,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cc_db::index_db::IndexDb;
    use tempfile::TempDir;

    /// Build an IndexDb whose file paths deliberately do NOT contain the search
    /// token, so a hit can only come from the symbol-name (Layer 6b) path —
    /// isolating the trigram `symbols_fts` substring lookup.
    fn db_with_symbols() -> (TempDir, IndexDb) {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("preselect_test.db"))
            .unwrap()
            .0;
        let conn = db.read_conn().unwrap();
        conn.execute_batch(
            "INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at) \
                 VALUES('src/a.rs', 'Rust', 'h1', 1.0, 100, '2024-01-01');\
             INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at) \
                 VALUES('src/b.rs', 'Rust', 'h2', 1.0, 100, '2024-01-01');\
             INSERT INTO symbols(symbol_id, file_path, name, kind, start_line, end_line) \
                 VALUES('s1', 'src/a.rs', 'getUserById', 'function', 1, 5);\
             INSERT INTO symbols(symbol_id, file_path, name, kind, start_line, end_line) \
                 VALUES('s2', 'src/b.rs', 'createOrder', 'function', 1, 5);",
        )
        .unwrap();
        (tmp, db)
    }

    /// A token that appears only mid-identifier (camelCase) must still recall the
    /// symbol — the property that forbids degrading `%token%` to a prefix match.
    #[test]
    fn preselect_recalls_substring_symbol_match() {
        let (_tmp, db) = db_with_symbols();

        let result = preselect_files(&db, "user", None, None, None, None, None, None, 10).unwrap();
        assert!(
            result.files.contains(&"src/a.rs".to_string()),
            "substring token 'user' must recall 'getUserById' in src/a.rs (no path-token hit possible); got {:?}",
            result.files
        );
        assert!(
            !result.files.contains(&"src/b.rs".to_string()),
            "'user' must not recall 'createOrder'; got {:?}",
            result.files
        );

        let result = preselect_files(&db, "order", None, None, None, None, None, None, 10).unwrap();
        assert!(
            result.files.contains(&"src/b.rs".to_string()),
            "substring token 'order' must recall 'createOrder' in src/b.rs; got {:?}",
            result.files
        );
    }

    /// A token that appears only inside a file *path* (not in any symbol name or
    /// file summary) must still be recalled via Layer 6a — exercising the
    /// trigram `file_paths_fts` mirror that replaced the `files` full scan.
    #[test]
    fn preselect_recalls_path_token_match() {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("preselect_path_test.db"))
            .unwrap()
            .0;
        let conn = db.read_conn().unwrap();
        // Path contains "widget"; symbol names deliberately do not, so the only
        // possible hit is the path-token (file_paths_fts) lookup.
        conn.execute_batch(
            "INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at) \
                 VALUES('src/widgetstore/a.rs', 'Rust', 'h1', 1.0, 100, '2024-01-01');\
             INSERT INTO files(file_path, language, content_hash, mtime, size, indexed_at) \
                 VALUES('src/b.rs', 'Rust', 'h2', 1.0, 100, '2024-01-01');\
             INSERT INTO symbols(symbol_id, file_path, name, kind, start_line, end_line) \
                 VALUES('s1', 'src/widgetstore/a.rs', 'alpha', 'function', 1, 5);\
             INSERT INTO symbols(symbol_id, file_path, name, kind, start_line, end_line) \
                 VALUES('s2', 'src/b.rs', 'beta', 'function', 1, 5);",
        )
        .unwrap();

        let result =
            preselect_files(&db, "widget", None, None, None, None, None, None, 10).unwrap();
        assert!(
            result.files.contains(&"src/widgetstore/a.rs".to_string()),
            "path-substring token 'widget' must recall 'src/widgetstore/a.rs' via file_paths_fts; got {:?}",
            result.files
        );
        assert!(
            !result.files.contains(&"src/b.rs".to_string()),
            "'widget' must not recall unrelated 'src/b.rs'; got {:?}",
            result.files
        );
    }

    /// Test via PreselectRequest directly — verifies the new interface works.
    #[test]
    fn preselect_request_interface() {
        let (_tmp, db) = db_with_symbols();

        let req = PreselectRequest {
            query: "user",
            path_prefix: None,
            boost_paths: None,
            recent_paths: None,
            pinned_paths: None,
            overlay_paths: None,
            explicit_file_paths: None,
            limit: 10,
        };
        let result = preselect(&db, &req).unwrap();
        assert!(
            result.files.contains(&"src/a.rs".to_string()),
            "PreselectRequest interface: 'user' must recall 'getUserById'; got {:?}",
            result.files
        );
        assert!(
            result.lane_stats.token_hits > 0,
            "lane_stats.token_hits should be > 0"
        );
        assert!(!result.lane_stats.used_fallback);
    }

    /// Test explicit_file_paths short-circuit via PreselectRequest.
    #[test]
    fn preselect_request_explicit_paths() {
        let (_tmp, db) = db_with_symbols();
        let explicit = vec!["src/a.rs".to_string(), "src/b.rs".to_string()];
        let req = PreselectRequest {
            query: "anything",
            path_prefix: None,
            boost_paths: None,
            recent_paths: None,
            pinned_paths: None,
            overlay_paths: None,
            explicit_file_paths: Some(&explicit),
            limit: 10,
        };
        let result = preselect(&db, &req).unwrap();
        assert_eq!(result.files.len(), 2);
        assert_eq!(*result.scores.get("src/a.rs").unwrap(), 10.0);
        assert_eq!(result.lane_stats.fts_hits, 0);
        assert!(!result.lane_stats.used_fallback);
    }

    /// Verify LaneStats reports fallback when query matches nothing.
    #[test]
    fn preselect_reports_fallback() {
        let (_tmp, db) = db_with_symbols();
        let req = PreselectRequest {
            query: "zzznonexistent",
            path_prefix: None,
            boost_paths: None,
            recent_paths: None,
            pinned_paths: None,
            overlay_paths: None,
            explicit_file_paths: None,
            limit: 10,
        };
        let result = preselect(&db, &req).unwrap();
        assert!(result.lane_stats.used_fallback, "should use fallback for unmatched query");
    }
}
