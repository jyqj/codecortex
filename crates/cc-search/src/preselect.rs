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

/// Result of file preselection: ordered file paths + per-file scores + per-file reason lists.
#[derive(Debug, Clone)]
pub struct PreselectResult {
    pub files: Vec<String>,
    pub scores: HashMap<String, f64>,
    pub reasons: HashMap<String, Vec<String>>,
}

/// Pre-select files that are likely relevant to the query.
///
/// Returns [`PreselectResult`] with files ranked by relevance score, up to `limit`.
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
    // If explicit file_paths given, return them directly (like Python).
    if let Some(fps) = explicit_file_paths {
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
        });
    }

    let conn = db.read_conn()?;
    let mut scores: HashMap<String, f64> = HashMap::new();
    let mut reasons: HashMap<String, Vec<String>> = HashMap::new();
    let query_tokens = tokenize_codeish(query);

    #[inline]
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

    // ── Layer 1: working-set boost ──────────────────────────────
    if let Some(paths) = boost_paths {
        for (rank, fp) in paths.iter().enumerate() {
            let rank1 = (rank + 1) as f64;
            score_file(
                &mut scores,
                &mut reasons,
                fp,
                f64::max(2.0, 5.0 / rank1),
                "working-set",
            );
        }
    }

    // ── Layer 2: recent files ───────────────────────────────────
    if let Some(paths) = recent_paths {
        for (rank, fp) in paths.iter().enumerate() {
            let rank1 = (rank + 1) as f64;
            score_file(
                &mut scores,
                &mut reasons,
                fp,
                f64::max(1.2, 3.5 / rank1),
                "recent",
            );
        }
    }

    // ── Layer 3: pinned files ───────────────────────────────────
    if let Some(paths) = pinned_paths {
        for (rank, fp) in paths.iter().enumerate() {
            let rank1 = (rank + 1) as f64;
            score_file(
                &mut scores,
                &mut reasons,
                fp,
                f64::max(2.2, 4.0 / rank1),
                "pinned",
            );
        }
    }

    // ── Layer 4: overlay (dirty-buffer) files ───────────────────
    if let Some(paths) = overlay_paths {
        for (rank, fp) in paths.iter().enumerate() {
            let rank1 = (rank + 1) as f64;
            score_file(
                &mut scores,
                &mut reasons,
                fp,
                f64::max(1.5, 3.0 / rank1),
                "dirty-buffer",
            );
        }
    }

    // ── Layer 5: FTS summary search on files_fts ────────────────
    let fts_query = sanitize_fts_query(query);
    if fts_query != r#""""# && !fts_query.is_empty() {
        let fts_limit = limit.min(80);
        // Build SQL with optional path_prefix filter.
        let (sql, params_vec): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
            if let Some(prefix) = path_prefix {
                (
                    "SELECT files.file_path, bm25(files_fts, 1.8, 1.0) AS score \
                     FROM files_fts \
                     JOIN files ON files.file_path = files_fts.file_path \
                     WHERE files_fts MATCH ?1 AND files.file_path LIKE ?2 \
                     ORDER BY score LIMIT ?3"
                        .into(),
                    vec![
                        Box::new(fts_query.clone()) as Box<dyn rusqlite::types::ToSql>,
                        Box::new(format!("{}%", prefix)),
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
                        Box::new(fts_query.clone()) as Box<dyn rusqlite::types::ToSql>,
                        Box::new(fts_limit as i64),
                    ],
                )
            };
        if let Ok(mut stmt) = conn.prepare(&sql) {
            let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                params_vec.iter().map(|p| p.as_ref()).collect();
            if let Ok(rows) = stmt.query_map(param_refs.as_slice(), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            }) {
                for row in rows.flatten() {
                    let bm25_score = row.1.abs();
                    let file_score = 1.4 + (1.0 / (1.0 + bm25_score));
                    score_file(&mut scores, &mut reasons, &row.0, file_score, "fts-summary");
                }
            }
        }
    }

    // ── Layer 6: per-token symbol name match + path token hit ───
    let candidate_tokens: Vec<&str> = query_tokens
        .iter()
        .filter(|t| t.len() >= 2)
        .take(8)
        .map(|s| s.as_str())
        .collect();

    for token in &candidate_tokens {
        // 6a. Path token match
        {
            let (sql, params_vec): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(
                prefix,
            ) =
                path_prefix
            {
                (
                        "SELECT file_path FROM files WHERE file_path LIKE ?1 AND file_path LIKE ?2 ORDER BY file_path LIMIT 20".into(),
                        vec![
                            Box::new(format!("%{}%", token)) as Box<dyn rusqlite::types::ToSql>,
                            Box::new(format!("{}%", prefix)),
                        ],
                    )
            } else {
                (
                        "SELECT file_path FROM files WHERE file_path LIKE ?1 ORDER BY file_path LIMIT 20".into(),
                        vec![Box::new(format!("%{}%", token)) as Box<dyn rusqlite::types::ToSql>],
                    )
            };
            if let Ok(mut stmt) = conn.prepare(&sql) {
                let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                    params_vec.iter().map(|p| p.as_ref()).collect();
                if let Ok(rows) =
                    stmt.query_map(param_refs.as_slice(), |row| row.get::<_, String>(0))
                {
                    for row in rows.flatten() {
                        score_file(
                            &mut scores,
                            &mut reasons,
                            &row,
                            1.0,
                            &format!("path-token:{}", token),
                        );
                    }
                }
            }
        }

        // 6b. Symbol name match
        {
            let (sql, params_vec): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(
                prefix,
            ) =
                path_prefix
            {
                (
                        "SELECT DISTINCT symbols.file_path, symbols.name \
                         FROM symbols \
                         JOIN files ON files.file_path = symbols.file_path \
                         WHERE lower(symbols.name) LIKE lower(?1) AND symbols.file_path LIKE ?2 \
                         ORDER BY CASE WHEN lower(symbols.name) = lower(?3) THEN 0 ELSE 1 END, symbols.file_path \
                         LIMIT 24"
                            .into(),
                        vec![
                            Box::new(format!("%{}%", token)) as Box<dyn rusqlite::types::ToSql>,
                            Box::new(format!("{}%", prefix)),
                            Box::new(token.to_string()),
                        ],
                    )
            } else {
                (
                        "SELECT DISTINCT symbols.file_path, symbols.name \
                         FROM symbols \
                         WHERE lower(symbols.name) LIKE lower(?1) \
                         ORDER BY CASE WHEN lower(symbols.name) = lower(?2) THEN 0 ELSE 1 END, symbols.file_path \
                         LIMIT 24"
                            .into(),
                        vec![
                            Box::new(format!("%{}%", token)) as Box<dyn rusqlite::types::ToSql>,
                            Box::new(token.to_string()),
                        ],
                    )
            };
            if let Ok(mut stmt) = conn.prepare(&sql) {
                let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                    params_vec.iter().map(|p| p.as_ref()).collect();
                if let Ok(rows) = stmt.query_map(param_refs.as_slice(), |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                }) {
                    for row in rows.flatten() {
                        let bonus = if row.1.to_lowercase() == *token {
                            2.0
                        } else {
                            1.2
                        };
                        let reason = format!("symbol:{}", row.1);
                        score_file(&mut scores, &mut reasons, &row.0, bonus, &reason);
                    }
                }
            }
        }
    }

    // ── Fallback: recently-indexed files if nothing scored ───────
    if scores.is_empty() {
        if let Ok(mut stmt) =
            conn.prepare("SELECT file_path FROM files ORDER BY indexed_at DESC LIMIT ?1")
        {
            if let Ok(rows) = stmt.query_map(rusqlite::params![limit as i64], |row| {
                row.get::<_, String>(0)
            }) {
                for row in rows.flatten() {
                    score_file(&mut scores, &mut reasons, &row, 0.2, "fallback-indexed");
                }
            }
        }
    }

    // ── Filter by path_prefix, sort, truncate ───────────────────
    let mut filtered: Vec<(String, f64)> = scores
        .into_iter()
        .filter(|(path, _)| {
            if let Some(prefix) = path_prefix {
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
    filtered.truncate(limit);

    let files: Vec<String> = filtered.iter().map(|(p, _)| p.clone()).collect();
    let final_scores: HashMap<String, f64> = filtered.into_iter().collect();
    // Deduplicate reasons per file
    let final_reasons: HashMap<String, Vec<String>> = files
        .iter()
        .map(|f| {
            let mut r = reasons.remove(f).unwrap_or_default();
            // Deduplicate preserving order
            let mut seen = std::collections::HashSet::new();
            r.retain(|item| seen.insert(item.clone()));
            (f.clone(), r)
        })
        .collect();

    Ok(PreselectResult {
        files,
        scores: final_scores,
        reasons: final_reasons,
    })
}
