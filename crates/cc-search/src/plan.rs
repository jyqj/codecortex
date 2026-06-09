//! Search planning — normalizes caller requests into a compact execution plan.
//!
//! `SearchEngine` owns execution (lane queries, fusion, DB fetch).  `SearchPlan`
//! owns the caller-facing search semantics that should stay consistent across
//! those execution steps: DSL normalization, file preselection, lane limits,
//! materialized filters, and rerank metadata/reasons.

use std::collections::{HashMap, HashSet};

use cc_db::fts::{expand_query_text, tokenize_codeish};
use cc_db::index_db::{read_chunk_text_with_encoding, IndexDb};
use cc_model::config::SearchConfig;
use cc_model::search::{SearchHit, SearchRequest};
use cc_model::{CcResult, Language};

use crate::preselect::{PreselectRequest, PreselectResult};

#[derive(Debug)]
pub(crate) struct SearchPlan {
    request: SearchRequest,
    dsl: crate::dsl::ParsedQuery,
    expanded_query: String,
    query_tokens: Vec<String>,
    limits: LaneLimits,
    filters: MaterializedFilters,
    preselect: PreselectResult,
    rerank_inputs: RerankInputs,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LaneLimits {
    pub top_k: usize,
    pub lexical: usize,
    pub grep: usize,
    pub rerank_window: usize,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct MaterializedFilters {
    path_prefix: Option<String>,
    languages: Option<Vec<Language>>,
    file_paths: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default)]
struct RerankInputs {
    boost_files: HashSet<String>,
    recent_files: HashSet<String>,
    pinned_files: HashSet<String>,
    overlay_files: HashSet<String>,
}

#[derive(Debug)]
pub(crate) struct LaneRanks<'a> {
    lexical: HashMap<&'a str, usize>,
    grep: HashMap<&'a str, usize>,
}

#[derive(Debug)]
pub(crate) struct CandidateChunk {
    pub chunk_id: String,
    pub file_path: String,
    pub language_name: String,
    pub start_line: u32,
    pub end_line: u32,
    pub breadcrumb: String,
    pub symbol_name: Option<String>,
    pub symbol_kind: Option<String>,
    pub text: String,
}

impl SearchPlan {
    pub(crate) fn build(
        db: &IndexDb,
        config: &SearchConfig,
        request: &SearchRequest,
    ) -> CcResult<Self> {
        let dsl = crate::dsl::parse_search_dsl(&request.query);
        let mut request = request.clone();

        normalize_request_from_dsl(&mut request, &dsl);

        let query_text = augmented_query_text(&request);
        let expanded_query = expand_query_text(&query_text);
        let top_k = if request.top_k == 0 {
            10
        } else {
            request.top_k
        };

        let preselect_limit = request
            .file_preselect_limit
            .unwrap_or_else(|| 60usize.max(top_k * 12));
        let preselect = crate::preselect::preselect(
            db,
            &PreselectRequest {
                query: &query_text,
                path_prefix: request.path_prefix.as_deref(),
                boost_paths: request.boost_file_paths.as_deref(),
                recent_paths: request.recent_file_paths.as_deref(),
                pinned_paths: request.pinned_file_paths.as_deref(),
                overlay_paths: request.overlay_file_paths.as_deref(),
                explicit_file_paths: request.file_paths.as_deref(),
                limit: preselect_limit,
            },
        )?;
        if !preselect.files.is_empty() && request.file_paths.is_none() {
            request.file_paths = Some(preselect.files.clone());
        }

        let filters = MaterializedFilters::from_request(&request);
        let rerank_inputs = RerankInputs::from_request(&request);
        let query_tokens = tokenize_codeish(&query_text);
        let limits = LaneLimits {
            top_k,
            lexical: config.lexical_top_k.max(top_k),
            grep: config.grep_top_k.max(top_k),
            rerank_window: config.rerank_window.max(top_k),
        };

        Ok(Self {
            request,
            dsl,
            expanded_query,
            query_tokens,
            limits,
            filters,
            preselect,
            rerank_inputs,
        })
    }

    pub(crate) fn request(&self) -> &SearchRequest {
        &self.request
    }

    pub(crate) fn lexical_query(&self) -> &str {
        &self.expanded_query
    }

    pub(crate) fn grep_query(&self) -> &str {
        &self.request.query
    }

    pub(crate) fn limits(&self) -> LaneLimits {
        self.limits
    }

    pub(crate) fn passes_filters(&self, file_path: &str, language: Language) -> bool {
        self.filters.passes(file_path, language)
    }

    pub(crate) fn grep_scope_sql(&self) -> (String, Vec<String>) {
        self.filters.grep_chunk_scope_sql()
    }

    pub(crate) fn lexical_scope_sql(&self, limit: usize) -> (String, Vec<String>) {
        self.filters.fts_chunk_scope_sql(limit)
    }

    pub(crate) fn lane_ranks<'a>(
        &self,
        lexical_hits: &'a [(String, f64)],
        grep_hits: &'a [(String, f64)],
    ) -> LaneRanks<'a> {
        LaneRanks::from_lanes(lexical_hits, grep_hits)
    }

    pub(crate) fn hit_from_chunk(
        &self,
        chunk: CandidateChunk,
        fused_score: f64,
        lane_ranks: &LaneRanks<'_>,
    ) -> Option<SearchHit> {
        let CandidateChunk {
            chunk_id,
            file_path,
            language_name,
            start_line,
            end_line,
            breadcrumb,
            symbol_name,
            symbol_kind,
            text,
        } = chunk;

        let language = parse_language_name(&language_name);
        if !self.filters.passes(&file_path, language) {
            return None;
        }

        let path_text = format!(
            "{} {} {}",
            file_path,
            breadcrumb,
            symbol_name.as_deref().unwrap_or("")
        );
        let overlap =
            crate::rrf::overlap_score(&self.query_tokens, &format!("{}\n{}", path_text, text));
        let mut rerank = fused_score + overlap * 0.35;

        let mut reasons = Vec::new();
        let lexical_score = lane_ranks
            .lexical_rank(&chunk_id)
            .map(|rank| 1.0 / rank as f64)
            .unwrap_or(0.0);
        let grep_score = lane_ranks
            .grep_rank(&chunk_id)
            .map(|rank| 1.0 / rank as f64)
            .unwrap_or(0.0);

        if let Some(rank) = lane_ranks.lexical_rank(&chunk_id) {
            reasons.push(format!("lexical@{}", rank));
        }
        if let Some(rank) = lane_ranks.grep_rank(&chunk_id) {
            reasons.push(format!("grep@{}", rank));
        }

        if let Some(ref sym_name) = symbol_name {
            let sym_lower = sym_name.to_lowercase();
            if self.query_tokens.contains(&sym_lower) {
                rerank += 0.18;
                reasons.push("symbol-exact".into());
            }
        }

        if let Some(prefix) = self.filters.path_prefix() {
            if file_path.starts_with(prefix) {
                rerank += 0.05;
            }
        }

        if is_project_doc(&file_path) {
            rerank += 0.08;
            reasons.push("doc-file".into());
        }

        if self.rerank_inputs.boost_files.contains(file_path.as_str()) {
            rerank += 0.22;
            reasons.push("working-set-boost".into());
        }
        if self.rerank_inputs.recent_files.contains(file_path.as_str()) {
            rerank += 0.12;
            reasons.push("recent-file".into());
        }
        if self.rerank_inputs.pinned_files.contains(file_path.as_str()) {
            rerank += 0.20;
            reasons.push("pinned-context".into());
        }
        if self
            .rerank_inputs
            .overlay_files
            .contains(file_path.as_str())
        {
            rerank += 0.10;
            reasons.push("overlay-neighbor".into());
        }

        let stage_a_score = self
            .preselect
            .scores
            .get(&file_path)
            .copied()
            .unwrap_or(0.0);
        if stage_a_score > 0.0 {
            rerank += (stage_a_score * 0.04).min(0.25);
            if let Some(file_reasons) = self.preselect.reasons.get(&file_path) {
                for r in file_reasons.iter().take(3) {
                    reasons.push(r.clone());
                }
            }
        }

        dedupe_reasons(&mut reasons);
        let metadata = self.rerank_metadata(&file_path, stage_a_score);

        Some(SearchHit {
            chunk_id,
            file_path,
            language,
            start_line,
            end_line,
            breadcrumb,
            symbol_name,
            symbol_kind: symbol_kind
                .and_then(|s| cc_model::symbol::SymbolKind::from_str_lenient(&s)),
            text,
            fused_score,
            lexical_score,
            grep_score,
            graph_score: 0.0,
            rerank_score: rerank,
            reasons,
            source: "index".into(),
            lane: None,
            metadata,
        })
    }

    pub(crate) fn finalize_results(&self, results: &mut Vec<SearchHit>) {
        if let Some(ref kind_filter) = self.dsl.kind_filter {
            results.retain(|hit| match &hit.symbol_kind {
                Some(sk) => crate::dsl::matches_kind(sk, kind_filter),
                None => false,
            });
        }

        if let Some(ref name_filter) = self.dsl.name_filter {
            let nf_lower = name_filter.to_lowercase();
            for hit in results.iter_mut() {
                if let Some(ref sn) = hit.symbol_name {
                    if sn.to_lowercase().contains(&nf_lower) {
                        hit.rerank_score += 0.25;
                        hit.reasons.push(format!("dsl-name:{}", name_filter));
                    }
                }
            }
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
        results.truncate(self.limits.top_k);
    }

    fn rerank_metadata(&self, file_path: &str, stage_a_score: f64) -> serde_json::Value {
        serde_json::json!({
            "stage_a_file_score": stage_a_score,
            "stage_a_files_considered": self.preselect.files.len(),
            "stage_a_file_reasons": self.preselect.reasons.get(file_path).cloned().unwrap_or_default(),
        })
    }
}

impl MaterializedFilters {
    pub(crate) fn from_request(request: &SearchRequest) -> Self {
        Self {
            path_prefix: request.path_prefix.clone(),
            languages: request.languages.clone(),
            file_paths: request.file_paths.clone(),
        }
    }

    pub(crate) fn passes(&self, file_path: &str, language: Language) -> bool {
        if let Some(prefix) = &self.path_prefix {
            if !file_path.starts_with(prefix) {
                return false;
            }
        }
        if let Some(languages) = &self.languages {
            if !languages.contains(&language) {
                return false;
            }
        }
        if let Some(files) = &self.file_paths {
            if !files.iter().any(|file| file == file_path) {
                return false;
            }
        }
        true
    }

    pub(crate) fn grep_chunk_scope_sql(&self) -> (String, Vec<String>) {
        let mut sql =
            "SELECT chunk_id, file_path, language, text, text_encoding FROM chunks".to_string();
        let mut clauses: Vec<String> = Vec::new();
        let mut params = Vec::new();

        self.push_chunk_scope_clauses(&mut clauses, &mut params, "file_path", "language");

        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }

        (sql, params)
    }

    pub(crate) fn fts_chunk_scope_sql(&self, limit: usize) -> (String, Vec<String>) {
        let mut sql =
            "SELECT chunks_fts.chunk_id, chunks.file_path, chunks.language, bm25(chunks_fts, 1.0, 1.0, 2.0) AS score
             FROM chunks_fts
             JOIN chunks ON chunks.chunk_id = chunks_fts.chunk_id
             WHERE chunks_fts MATCH ?"
                .to_string();
        let mut clauses: Vec<String> = Vec::new();
        let mut params = Vec::new();

        self.push_chunk_scope_clauses(
            &mut clauses,
            &mut params,
            "chunks.file_path",
            "chunks.language",
        );

        if !clauses.is_empty() {
            sql.push_str(" AND ");
            sql.push_str(&clauses.join(" AND "));
        }
        sql.push_str(" ORDER BY score LIMIT ");
        sql.push_str(&limit.to_string());

        (sql, params)
    }

    fn push_chunk_scope_clauses(
        &self,
        clauses: &mut Vec<String>,
        params: &mut Vec<String>,
        file_path_column: &str,
        language_column: &str,
    ) {
        if let Some(prefix) = self.path_prefix.as_ref().filter(|p| !p.is_empty()) {
            clauses.push(format!("{file_path_column} LIKE ? ESCAPE '\\'"));
            params.push(format!("{}%", escape_like(prefix)));
        }

        if let Some(languages) = self.languages.as_ref().filter(|v| !v.is_empty()) {
            let placeholders = sql_placeholders(languages.len());
            clauses.push(format!("{language_column} IN ({placeholders})"));
            params.extend(languages.iter().map(|lang| lang.as_str().to_string()));
        }

        if let Some(files) = self.file_paths.as_ref().filter(|v| !v.is_empty()) {
            let placeholders = sql_placeholders(files.len());
            clauses.push(format!("{file_path_column} IN ({placeholders})"));
            params.extend(files.iter().cloned());
        }
    }

    fn path_prefix(&self) -> Option<&str> {
        self.path_prefix.as_deref()
    }
}

impl RerankInputs {
    fn from_request(request: &SearchRequest) -> Self {
        Self {
            boost_files: collect_paths(request.boost_file_paths.as_ref()),
            recent_files: collect_paths(request.recent_file_paths.as_ref()),
            pinned_files: collect_paths(request.pinned_file_paths.as_ref()),
            overlay_files: collect_paths(request.overlay_file_paths.as_ref()),
        }
    }
}

impl<'a> LaneRanks<'a> {
    fn from_lanes(lexical_hits: &'a [(String, f64)], grep_hits: &'a [(String, f64)]) -> Self {
        Self {
            lexical: lexical_hits
                .iter()
                .enumerate()
                .map(|(i, (id, _))| (id.as_str(), i + 1))
                .collect(),
            grep: grep_hits
                .iter()
                .enumerate()
                .map(|(i, (id, _))| (id.as_str(), i + 1))
                .collect(),
        }
    }

    fn lexical_rank(&self, chunk_id: &str) -> Option<usize> {
        self.lexical.get(chunk_id).copied()
    }

    fn grep_rank(&self, chunk_id: &str) -> Option<usize> {
        self.grep.get(chunk_id).copied()
    }
}

impl CandidateChunk {
    pub(crate) fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            chunk_id: row.get::<_, String>(0)?,
            file_path: row.get::<_, String>(1)?,
            language_name: row.get::<_, String>(2)?,
            start_line: row.get::<_, u32>(3)?,
            end_line: row.get::<_, u32>(4)?,
            breadcrumb: row.get::<_, String>(5)?,
            symbol_name: row.get::<_, Option<String>>(6)?,
            symbol_kind: row.get::<_, Option<String>>(7)?,
            text: read_chunk_text_with_encoding(row, 8, 9)?,
        })
    }

    /// Build from a DB row but use a pre-cached text instead of decompressing.
    pub(crate) fn from_row_with_text(
        row: &rusqlite::Row<'_>,
        cached_text: String,
    ) -> rusqlite::Result<Self> {
        Ok(Self {
            chunk_id: row.get::<_, String>(0)?,
            file_path: row.get::<_, String>(1)?,
            language_name: row.get::<_, String>(2)?,
            start_line: row.get::<_, u32>(3)?,
            end_line: row.get::<_, u32>(4)?,
            breadcrumb: row.get::<_, String>(5)?,
            symbol_name: row.get::<_, Option<String>>(6)?,
            symbol_kind: row.get::<_, Option<String>>(7)?,
            text: cached_text,
        })
    }
}

fn normalize_request_from_dsl(request: &mut SearchRequest, dsl: &crate::dsl::ParsedQuery) {
    if dsl.path_filter.is_some() && request.path_prefix.is_none() {
        request.path_prefix = dsl.path_filter.clone();
    }

    if let Some(ref lang_str) = dsl.lang_filter {
        if request.languages.is_none() {
            let lang = Language::from_name(lang_str);
            if lang != Language::Unknown {
                request.languages = Some(vec![lang]);
            }
        }
    }

    if !dsl.text.is_empty() {
        request.query = dsl.text.clone();
    }
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

fn collect_paths(paths: Option<&Vec<String>>) -> HashSet<String> {
    paths
        .map(|v| v.iter().cloned().collect())
        .unwrap_or_default()
}

fn dedupe_reasons(reasons: &mut Vec<String>) {
    let mut seen = HashSet::new();
    reasons.retain(|r| seen.insert(r.clone()));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn materialized_grep_scope_sql_pushes_filters_into_db_query() {
        let request = SearchRequest {
            path_prefix: Some("src/%special".into()),
            languages: Some(vec![Language::Rust, Language::Python]),
            file_paths: Some(vec!["src/lib.rs".into(), "src/main.py".into()]),
            ..Default::default()
        };

        let (sql, params) = MaterializedFilters::from_request(&request).grep_chunk_scope_sql();

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

    #[test]
    fn materialized_fts_scope_sql_pushes_filters_before_limit() {
        let request = SearchRequest {
            path_prefix: Some("src/%special".into()),
            languages: Some(vec![Language::Rust, Language::Python]),
            file_paths: Some(vec!["src/lib.rs".into(), "src/main.py".into()]),
            ..Default::default()
        };

        let (sql, params) = MaterializedFilters::from_request(&request).fts_chunk_scope_sql(7);

        assert!(sql.contains("WHERE chunks_fts MATCH ? AND"));
        assert!(sql.contains("chunks.file_path LIKE ? ESCAPE '\\'"));
        assert!(sql.contains("chunks.language IN (?,?)"));
        assert!(sql.contains("chunks.file_path IN (?,?)"));
        assert!(sql.ends_with("ORDER BY score LIMIT 7"));
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

    #[test]
    fn test_is_project_doc_top_level_files() {
        assert!(is_project_doc("README.md"));
        assert!(is_project_doc("DESIGN.md"));
        assert!(is_project_doc("CHANGELOG.md"));
        assert!(is_project_doc("CONTRIBUTING.md"));
        assert!(is_project_doc("ARCHITECTURE.md"));
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
        assert!(!is_project_doc("src/deep/nested/notes.md"));
        assert!(!is_project_doc("README.txt"));
    }
}
