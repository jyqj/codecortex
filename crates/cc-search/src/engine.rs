//! SearchEngine — lexical + grep retrieval with RRF fusion.
//!
//! Extended with graph/navigation queries.

use std::collections::HashMap;
use std::sync::Arc;

use cc_db::fts::{expand_query_text, sanitize_fts_query, tokenize_codeish};
use cc_db::index_db::{read_chunk_text_with_encoding, IndexDb};
use cc_model::config::{ProjectStats, SearchConfig};
use cc_model::search::{SearchHit, SearchRequest};
use cc_model::{CcError, CcResult, Language};

use crate::rrf::rrf_accumulate;

pub struct SearchEngine {
    pub(crate) db: Arc<IndexDb>,
    pub(crate) config: SearchConfig,
}

fn augmented_query_text(request: &SearchRequest) -> String {
    let mut parts = Vec::new();
    let primary = request.query.trim();
    if !primary.is_empty() {
        parts.push(primary.to_string());
    }
    if let Some(extra) = &request.conversation_queries {
        for query in extra.iter().rev().take(4) {
            let q = query.trim();
            if !q.is_empty() && !parts.iter().any(|existing| existing == q) {
                parts.push(q.to_string());
            }
        }
    }
    parts.join("\n")
}

impl SearchEngine {
    pub fn new(db: Arc<IndexDb>, config: &cc_model::ProjectConfig) -> Self {
        Self {
            db,
            config: config.search.clone(),
        }
    }

    pub fn status(&self) -> CcResult<ProjectStats> {
        self.db.stats(std::path::Path::new(""))
    }

    /// Core search — FTS5 + grep with RRF fusion and reranking.
    pub fn search(&self, request: &SearchRequest) -> CcResult<Vec<SearchHit>> {
        let conn = self.db.read_conn()?;

        // ── DSL filter extraction ──────────────────────────────
        let parsed = crate::dsl::parse_search_dsl(&request.query);
        let mut request = request.clone();

        // Apply DSL path_filter as path_prefix (if not already set).
        if parsed.path_filter.is_some() && request.path_prefix.is_none() {
            request.path_prefix = parsed.path_filter.clone();
        }

        // Apply DSL lang_filter as languages constraint (if not already set).
        if let Some(ref lang_str) = parsed.lang_filter {
            if request.languages.is_none() {
                let lang = cc_model::Language::from_name(lang_str);
                if lang != cc_model::Language::Unknown {
                    request.languages = Some(vec![lang]);
                }
            }
        }

        // Replace query with cleaned text (filters stripped).
        if !parsed.text.is_empty() {
            request.query = parsed.text.clone();
        }
        let query_text = augmented_query_text(&request);
        let expanded = expand_query_text(&query_text);
        let top_k = if request.top_k == 0 {
            10
        } else {
            request.top_k
        };

        // ── Stage A: preselect candidate files ──────────────────
        let preselect_limit = request
            .file_preselect_limit
            .unwrap_or_else(|| 60usize.max(top_k * 12));
        let preselect_result = crate::preselect::preselect_files(
            &self.db,
            &query_text,
            request.path_prefix.as_deref(),
            request.boost_file_paths.as_deref(),
            request.recent_file_paths.as_deref(),
            request.pinned_file_paths.as_deref(),
            request.overlay_file_paths.as_deref(),
            request.file_paths.as_deref(),
            preselect_limit,
        )?;
        if !preselect_result.files.is_empty() && request.file_paths.is_none() {
            request.file_paths = Some(preselect_result.files.clone());
        }
        let request = &request;

        let lexical_limit = self.config.lexical_top_k.max(top_k);
        let grep_limit = self.config.grep_top_k.max(top_k);

        // Lexical search (FTS5)
        let lexical_hits = self.lexical_search(&conn, &expanded, lexical_limit, request)?;

        // Grep search
        let grep_hits = if request.include_grep {
            self.grep_search(&conn, &request.query, grep_limit, request)?
        } else {
            Vec::new()
        };

        // RRF fusion
        let mut fused: HashMap<String, f64> = HashMap::new();
        let k = self.config.rrf_k;
        rrf_accumulate(
            &mut fused,
            &lexical_hits
                .iter()
                .map(|h| h.0.as_str())
                .collect::<Vec<_>>(),
            self.config.lexical_weight,
            k,
        );
        rrf_accumulate(
            &mut fused,
            &grep_hits.iter().map(|h| h.0.as_str()).collect::<Vec<_>>(),
            self.config.grep_weight,
            k,
        );

        // Get top candidates
        let mut candidates: Vec<(String, f64)> = fused.into_iter().collect();
        candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        candidates.truncate(self.config.rerank_window.max(top_k));

        if candidates.is_empty() {
            return Ok(Vec::new());
        }

        // Fetch chunk data
        let query_tokens = tokenize_codeish(&query_text);

        let boost_set: std::collections::HashSet<&str> = request
            .boost_file_paths
            .as_ref()
            .map(|v| v.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();
        let recent_set: std::collections::HashSet<&str> = request
            .recent_file_paths
            .as_ref()
            .map(|v| v.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();
        let pinned_set: std::collections::HashSet<&str> = request
            .pinned_file_paths
            .as_ref()
            .map(|v| v.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();
        let overlay_set: std::collections::HashSet<&str> = request
            .overlay_file_paths
            .as_ref()
            .map(|v| v.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();

        let lexical_rank: HashMap<&str, usize> = lexical_hits
            .iter()
            .enumerate()
            .map(|(i, (id, _))| (id.as_str(), i + 1))
            .collect();
        let grep_rank: HashMap<&str, usize> = grep_hits
            .iter()
            .enumerate()
            .map(|(i, (id, _))| (id.as_str(), i + 1))
            .collect();

        // ── Batch-fetch all candidate chunks in one query ─────
        type ChunkData = (
            String,
            String,
            String,
            u32,
            u32,
            String,
            Option<String>,
            Option<String>,
            String,
        );
        let mut chunk_map: HashMap<String, ChunkData> = {
            let placeholders = (1..=candidates.len())
                .map(|i| format!("?{}", i))
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "SELECT chunk_id, file_path, language, start_line, end_line, breadcrumb, \
                 symbol_name, symbol_kind, text, text_encoding \
                 FROM chunks WHERE chunk_id IN ({})",
                placeholders,
            );
            let chunk_ids_refs: Vec<&str> =
                candidates.iter().map(|(cid, _)| cid.as_str()).collect();
            let mut stmt = conn
                .prepare(&sql)
                .map_err(|e| CcError::Database(e.to_string()))?;
            let rows = stmt
                .query_map(rusqlite::params_from_iter(chunk_ids_refs.iter()), |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, u32>(3)?,
                        row.get::<_, u32>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, Option<String>>(7)?,
                        read_chunk_text_with_encoding(row, 8, 9)?,
                    ))
                })
                .map_err(|e| CcError::Database(e.to_string()))?;
            let mut map = HashMap::with_capacity(candidates.len());
            for data in rows.flatten() {
                map.insert(data.0.clone(), data);
            }
            map
        };

        let mut results = Vec::new();
        // Iterate candidates in fused-score order, looking up from batch result
        for (chunk_id, fused_score) in &candidates {
            let (cid, fp, lang, sl, el, bc, sn, sk, text) = match chunk_map.remove(chunk_id) {
                Some(data) => data,
                None => continue,
            };
            let language = parse_language_name(&lang);
            if !passes_filters(&fp, language, request) {
                continue;
            }
            let path_text = format!("{} {} {}", fp, bc, sn.as_deref().unwrap_or(""));
            let overlap =
                crate::rrf::overlap_score(&query_tokens, &format!("{}\n{}", path_text, text));
            let mut rerank = fused_score + overlap * 0.35;

            let mut reasons = Vec::new();
            let lexical_score = lexical_rank
                .get(cid.as_str())
                .map(|rank| 1.0 / *rank as f64)
                .unwrap_or(0.0);
            let grep_score = grep_rank
                .get(cid.as_str())
                .map(|rank| 1.0 / *rank as f64)
                .unwrap_or(0.0);

            // Reasons with rank info: "lexical@5", "grep@3"
            if let Some(&rank) = lexical_rank.get(cid.as_str()) {
                reasons.push(format!("lexical@{}", rank));
            }
            if let Some(&rank) = grep_rank.get(cid.as_str()) {
                reasons.push(format!("grep@{}", rank));
            }

            // Symbol exact match boost (+0.18)
            if let Some(ref sym_name) = sn {
                let sym_lower = sym_name.to_lowercase();
                if query_tokens.contains(&sym_lower) {
                    rerank += 0.18;
                    reasons.push("symbol-exact".into());
                }
            }

            // Path prefix boost (+0.05)
            if let Some(ref prefix) = request.path_prefix {
                if fp.starts_with(prefix.as_str()) {
                    rerank += 0.05;
                }
            }

            // Doc file boost (+0.08 for README/DESIGN/CHANGELOG/docs/)
            if is_project_doc(&fp) {
                rerank += 0.08;
                reasons.push("doc-file".into());
            }

            // Working-set / recent / pinned / overlay boosts
            if boost_set.contains(fp.as_str()) {
                rerank += 0.22;
                reasons.push("working-set-boost".into());
            }
            if recent_set.contains(fp.as_str()) {
                rerank += 0.12;
                reasons.push("recent-file".into());
            }
            if pinned_set.contains(fp.as_str()) {
                rerank += 0.20;
                reasons.push("pinned-context".into());
            }
            if overlay_set.contains(fp.as_str()) {
                rerank += 0.10;
                reasons.push("overlay-neighbor".into());
            }

            // Stage A file score contribution
            let stage_a_score = preselect_result.scores.get(&fp).copied().unwrap_or(0.0);
            if stage_a_score > 0.0 {
                rerank += (stage_a_score * 0.04).min(0.25);
                if let Some(file_reasons) = preselect_result.reasons.get(&fp) {
                    for r in file_reasons.iter().take(3) {
                        reasons.push(r.clone());
                    }
                }
            }

            // Deduplicate reasons preserving order
            let mut seen = std::collections::HashSet::new();
            reasons.retain(|r| seen.insert(r.clone()));

            // Build stage_a metadata
            let metadata = serde_json::json!({
                "stage_a_file_score": stage_a_score,
                "stage_a_files_considered": preselect_result.files.len(),
                "stage_a_file_reasons": preselect_result.reasons.get(&fp).cloned().unwrap_or_default(),
            });

            results.push(SearchHit {
                chunk_id: cid,
                file_path: fp,
                language,
                start_line: sl,
                end_line: el,
                breadcrumb: bc,
                symbol_name: sn,
                symbol_kind: sk.and_then(|s| cc_model::symbol::SymbolKind::from_str_lenient(&s)),
                text,
                fused_score: *fused_score,
                lexical_score,
                grep_score,
                graph_score: 0.0,
                rerank_score: rerank,
                reasons,
                source: "index".into(),
                lane: None,
                metadata,
            });
        }

        // ── DSL post-filters: kind, name ────────────────────────
        if let Some(ref kind_filter) = parsed.kind_filter {
            results.retain(|hit| {
                match &hit.symbol_kind {
                    Some(sk) => crate::dsl::matches_kind(sk, kind_filter),
                    None => false, // no symbol_kind => filtered out
                }
            });
        }
        if let Some(ref name_filter) = parsed.name_filter {
            let nf_lower = name_filter.to_lowercase();
            // Boost hits whose symbol_name matches; filter out those that don't.
            for hit in &mut results {
                if let Some(ref sn) = hit.symbol_name {
                    if sn.to_lowercase().contains(&nf_lower) {
                        hit.rerank_score += 0.25;
                        hit.reasons.push(format!("dsl-name:{}", name_filter));
                    }
                }
            }
            // Keep only hits that have a matching symbol name.
            results.retain(|hit| {
                hit.symbol_name
                    .as_ref()
                    .map(|sn| sn.to_lowercase().contains(&nf_lower))
                    .unwrap_or(false)
            });
        }

        results.sort_by(|a, b| {
            b.rerank_score
                .partial_cmp(&a.rerank_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(top_k);
        Ok(results)
    }

    /// Lexical search via FTS5.
    fn lexical_search(
        &self,
        conn: &rusqlite::Connection,
        query: &str,
        limit: usize,
        request: &SearchRequest,
    ) -> CcResult<Vec<(String, f64)>> {
        let fts_q = sanitize_fts_query(query);
        if fts_q == r#""""# {
            return Ok(Vec::new());
        }
        let mut stmt = conn.prepare(
            "SELECT chunks_fts.chunk_id, chunks.file_path, chunks.language, bm25(chunks_fts, 1.0, 1.0, 2.0) AS score
             FROM chunks_fts
             JOIN chunks ON chunks.chunk_id = chunks_fts.chunk_id
             WHERE chunks_fts MATCH ?1
             ORDER BY score LIMIT ?2"
        ).map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![fts_q, limit], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, f64>(3)?,
                ))
            })
            .map_err(|e| CcError::Database(e.to_string()))?;

        let mut results = Vec::new();
        for row in rows {
            let (cid, file_path, language_name, _score) =
                row.map_err(|e| CcError::Database(e.to_string()))?;
            let language = parse_language_name(&language_name);
            if !passes_filters(&file_path, language, request) {
                continue;
            }
            results.push(cid);
        }
        Ok(results
            .into_iter()
            .enumerate()
            .map(|(i, id)| (id, 1.0 / (i + 1) as f64))
            .collect())
    }

    /// Grep search — regex match on chunk text with file-level filtering.
    fn grep_search(
        &self,
        conn: &rusqlite::Connection,
        query: &str,
        limit: usize,
        request: &SearchRequest,
    ) -> CcResult<Vec<(String, f64)>> {
        // Build a simple case-insensitive regex from the query
        let escaped = regex::escape(query);
        let re = match regex::RegexBuilder::new(&escaped)
            .case_insensitive(true)
            .build()
        {
            Ok(r) => r,
            Err(_) => return Ok(Vec::new()),
        };

        let (sql, params) = grep_chunk_scope_sql(request);
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| CcError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(params.iter()), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    read_chunk_text_with_encoding(row, 3, 4)?,
                ))
            })
            .map_err(|e| CcError::Database(e.to_string()))?;

        let mut matches = Vec::new();
        for row in rows {
            let (cid, file_path, language_name, text) =
                row.map_err(|e| CcError::Database(e.to_string()))?;
            let language = parse_language_name(&language_name);
            // File-level filtering: path_prefix, languages, file_paths
            if !passes_filters(&file_path, language, request) {
                continue;
            }
            if re.is_match(&text) {
                matches.push(cid);
                if matches.len() >= limit {
                    break;
                }
            }
        }
        Ok(matches
            .into_iter()
            .enumerate()
            .map(|(i, id)| (id, 1.0 / (i + 1) as f64))
            .collect())
    }
}

fn grep_chunk_scope_sql(request: &SearchRequest) -> (String, Vec<String>) {
    let mut sql =
        "SELECT chunk_id, file_path, language, text, text_encoding FROM chunks".to_string();
    let mut clauses: Vec<String> = Vec::new();
    let mut params = Vec::new();

    if let Some(prefix) = request.path_prefix.as_ref().filter(|p| !p.is_empty()) {
        clauses.push("file_path LIKE ? ESCAPE '\\'".to_string());
        params.push(format!("{}%", escape_like(prefix)));
    }

    if let Some(languages) = request.languages.as_ref().filter(|v| !v.is_empty()) {
        let placeholders = sql_placeholders(languages.len());
        clauses.push(format!("language IN ({})", placeholders));
        params.extend(languages.iter().map(|lang| lang.as_str().to_string()));
    }

    if let Some(files) = request.file_paths.as_ref().filter(|v| !v.is_empty()) {
        let placeholders = sql_placeholders(files.len());
        clauses.push(format!("file_path IN ({})", placeholders));
        params.extend(files.iter().cloned());
    }

    if !clauses.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&clauses.join(" AND "));
    }

    (sql, params)
}

fn sql_placeholders(count: usize) -> String {
    vec!["?"; count].join(",")
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

pub(crate) fn parse_language_name(value: &str) -> Language {
    Language::from_name(value)
}

/// Return true if the file path looks like a project documentation file.
///
/// Public so other crates can reuse the heuristic (e.g. for role tagging).
///
/// Matches: README.md, DESIGN.md, CHANGELOG.md, CONTRIBUTING.md, docs/*.md,
/// and similar top-level or docs-directory markdown files commonly used for
/// project documentation.
pub fn is_project_doc(file_path: &str) -> bool {
    let lower = file_path.to_lowercase();
    if !lower.ends_with(".md") {
        return false;
    }
    // Top-level doc files (no directory separator or single-level path)
    let segments: Vec<&str> = file_path.split('/').collect();
    if segments.len() <= 2 {
        let name = segments.last().unwrap_or(&"").to_uppercase();
        if matches!(
            name.trim_end_matches(".MD").trim_end_matches(".md"),
            "README"
                | "DESIGN"
                | "ARCHITECTURE"
                | "CHANGELOG"
                | "CONTRIBUTING"
                | "LICENSE"
                | "ADR"
                | "DECISIONS"
        ) {
            return true;
        }
    }
    // Files under docs/ or doc/ directory
    if lower.starts_with("docs/") || lower.starts_with("doc/") {
        return true;
    }
    // ADR directory pattern
    if lower.contains("/adr/") || lower.contains("/adrs/") {
        return true;
    }
    false
}

fn passes_filters(file_path: &str, language: Language, request: &SearchRequest) -> bool {
    if let Some(prefix) = &request.path_prefix {
        if !file_path.starts_with(prefix) {
            return false;
        }
    }
    if let Some(languages) = &request.languages {
        if !languages.contains(&language) {
            return false;
        }
    }
    if let Some(files) = &request.file_paths {
        if !files.iter().any(|file| file == file_path) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_is_project_doc_top_level_files() {
        assert!(is_project_doc("README.md"));
        assert!(is_project_doc("DESIGN.md"));
        assert!(is_project_doc("CHANGELOG.md"));
        assert!(is_project_doc("CONTRIBUTING.md"));
        assert!(is_project_doc("ARCHITECTURE.md"));
        // Case-insensitive file name matching
        assert!(is_project_doc("readme.md"));
        assert!(is_project_doc("Readme.md"));
    }

    #[test]
    fn test_is_project_doc_docs_directory() {
        assert!(is_project_doc("docs/getting-started.md"));
        assert!(is_project_doc("docs/adr/0001-use-sqlite.md"));
        assert!(is_project_doc("doc/api.md"));
    }

    #[test]
    fn test_is_project_doc_adr_directory() {
        assert!(is_project_doc("architecture/adr/0002-rrf-fusion.md"));
        assert!(is_project_doc("decisions/adrs/0003-index-cache.md"));
    }

    #[test]
    fn test_is_project_doc_non_doc_files() {
        assert!(!is_project_doc("src/main.rs"));
        assert!(!is_project_doc("src/lib.rs"));
        assert!(!is_project_doc("tests/test_main.rs"));
        // Non-doc markdown in src
        assert!(!is_project_doc("src/deep/nested/notes.md"));
        // Non-md files
        assert!(!is_project_doc("README.txt"));
    }

    #[test]
    fn grep_chunk_scope_sql_pushes_filters_into_db_query() {
        let request = SearchRequest {
            path_prefix: Some("src/%special".into()),
            languages: Some(vec![Language::Rust, Language::Python]),
            file_paths: Some(vec!["src/lib.rs".into(), "src/main.py".into()]),
            ..Default::default()
        };

        let (sql, params) = grep_chunk_scope_sql(&request);

        assert!(sql.contains("file_path LIKE ? ESCAPE '\\'"));
        assert!(sql.contains("language IN (?,?)"));
        assert!(sql.contains("file_path IN (?,?)"));
        assert_eq!(
            params,
            vec![
                "src/\\%special%".to_string(),
                "rust".to_string(),
                "python".to_string(),
                "src/lib.rs".to_string(),
                "src/main.py".to_string()
            ]
        );
    }
}
