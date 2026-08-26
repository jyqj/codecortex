//! Retrieval read model: the raw-SQL retrieval queries (FTS5 bm25 file
//! summaries, trigram path/symbol token hits, chunk batch fetches, symbol
//! seed/uid lookups) consumed by cc-search's preselect layer and graph lane.
//!
//! SQL is owned here directly — not forwarded through `impl IndexDb` — so
//! `IndexDb`'s method surface stays focused on writes/maintenance and
//! [`RetrievalReadModel`] is a deep read model over a single pooled
//! connection. The borrow is zero-cost; obtained via [`IndexDb::retrieval()`]
//! (mirrors the `reads()`/`writes()`/`admin()` facets, but the SQL lives in
//! the model itself rather than in an `IndexDb` delegate — the shape the
//! sibling `GraphReads` is being brought to).
//!
//! These methods absorb the raw-SQL sites that previously lived in cc-search,
//! so FTS5 table names (`files_fts`, `file_paths_fts`, `symbols_fts`), bm25
//! scoring, and LIKE patterns stay behind the cc-db seam. SQL shape, ordering,
//! and limits follow the original call sites, with two fixes on top: LIKE
//! metacharacters in user-derived tokens and path prefixes are escaped
//! (`ESCAPE '\'`), and the `symbols_fts` path-prefix filter actually filters
//! instead of returning zero rows.

use std::collections::HashMap;
use std::sync::Arc;

use cc_model::symbol::{SymbolKind, SymbolRecord};
use cc_model::{CcResult, ParserTier};

use crate::index_db::{read_chunk_text_with_encoding, ChunkDetailRow, IndexDb};
use crate::sql_util::{db_err, escape_like, sql_in_placeholders, IN_BATCH_SIZE};

/// `(chunk_id, start_line, end_line)` spans grouped by file path.
pub type ChunkSpansByFile = HashMap<String, Vec<(String, u32, u32)>>;

/// File-scope filter for chunk-level retrieval scans — the structured form
/// of what cc-search used to render as raw scope SQL (path-prefix LIKE,
/// language IN, file-path IN). Empty strings / empty lists are treated as
/// "no filter" for that dimension.
#[derive(Debug, Clone, Default)]
pub struct ChunkScope {
    pub path_prefix: Option<String>,
    /// Language names as stored in `chunks.language` (`Language::as_str()`).
    pub languages: Option<Vec<String>>,
    pub file_paths: Option<Vec<String>>,
}

impl ChunkScope {
    /// Append the scope's WHERE clauses and their string params.
    fn push_clauses(
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
            let placeholders = vec!["?"; languages.len()].join(",");
            clauses.push(format!("{language_column} IN ({placeholders})"));
            params.extend(languages.iter().cloned());
        }

        if let Some(files) = self.file_paths.as_ref().filter(|v| !v.is_empty()) {
            let placeholders = vec!["?"; files.len()].join(",");
            clauses.push(format!("{file_path_column} IN ({placeholders})"));
            params.extend(files.iter().cloned());
        }
    }

    /// Whether the scope pins an explicit file set (bounds scan cardinality).
    fn has_file_scope(&self) -> bool {
        self.file_paths
            .as_ref()
            .map(|files| !files.is_empty())
            .unwrap_or(false)
    }
}

/// One decoded chunk row visited by [`RetrievalReadModel::scan_chunks_for_grep`].
#[derive(Debug)]
pub struct GrepChunkRow {
    pub chunk_id: String,
    pub file_path: String,
    pub language_name: String,
    /// Chunk text, already decoded from its storage encoding (zstd/plain).
    pub text: String,
    /// Base-table rowid, for recency merges across scan stages.
    pub rowid: i64,
}

/// Deep retrieval read model over [`IndexDb`]: owns the raw-SQL retrieval
/// queries (FTS5 bm25 file summaries, trigram path/symbol token hits, chunk
/// batch fetches, symbol seed/uid lookups) that cc-search's preselect layer
/// and graph lane consume. A zero-cost borrow obtained via
/// [`IndexDb::retrieval()`].
///
/// Kept distinct from the catch-all [`ReadOps`](crate::index_db::ReadOps)
/// facet so cc-search states "retrieval" intent at the call site and the
/// retrieval SQL has a single home, instead of being one more block on the
/// 130+-method `impl IndexDb`.
pub struct RetrievalReadModel<'a> {
    db: &'a IndexDb,
}

impl IndexDb {
    /// Retrieval read model: FTS5/trigram retrieval queries (file summaries,
    /// path/symbol token hits, chunk fetches, symbol seed/uid lookups) used by
    /// cc-search. See [`RetrievalReadModel`].
    pub fn retrieval(&self) -> RetrievalReadModel<'_> {
        RetrievalReadModel::new(self)
    }
}

impl<'a> RetrievalReadModel<'a> {
    /// Borrow `db` for retrieval queries (mirrors `GraphReads::new`).
    pub fn new(db: &'a IndexDb) -> Self {
        Self { db }
    }

    /// FTS file-summary search on `files_fts` with bm25 scoring.
    ///
    /// Returns `(file_path, raw bm25 score)` ordered by score ascending
    /// (bm25 is negative-better, so the best match comes first).
    ///
    /// Sanitization is the caller's responsibility: `sanitized_query` must
    /// already be a valid FTS5 MATCH expression (see `fts::sanitize_fts_query`),
    /// and callers should skip the call entirely for empty/`""` queries.
    pub fn fts_file_summaries(
        &self,
        sanitized_query: &str,
        path_prefix: Option<&str>,
        limit: usize,
    ) -> CcResult<Vec<(String, f64)>> {
        let conn = self.db.read_conn()?;
        let (sql, params_vec): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) =
            if let Some(prefix) = path_prefix {
                (
                    "SELECT files.file_path, bm25(files_fts, 1.8, 1.0) AS score \
                 FROM files_fts \
                 JOIN files ON files.file_path = files_fts.file_path \
                 WHERE files_fts MATCH ?1 AND files.file_path LIKE ?2 ESCAPE '\\' \
                 ORDER BY score LIMIT ?3",
                    vec![
                        Box::new(sanitized_query.to_string()) as Box<dyn rusqlite::types::ToSql>,
                        Box::new(format!("{}%", escape_like(prefix))),
                        Box::new(limit as i64),
                    ],
                )
            } else {
                (
                    "SELECT files.file_path, bm25(files_fts, 1.8, 1.0) AS score \
                 FROM files_fts \
                 JOIN files ON files.file_path = files_fts.file_path \
                 WHERE files_fts MATCH ?1 \
                 ORDER BY score LIMIT ?2",
                    vec![
                        Box::new(sanitized_query.to_string()) as Box<dyn rusqlite::types::ToSql>,
                        Box::new(limit as i64),
                    ],
                )
            };
        let mut stmt = conn.prepare_cached(sql).map_err(db_err)?;
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    /// Chunk candidates for the lexical lane: `chunks_fts` MATCH with bm25
    /// scoring (weights 1.0/1.0/2.0), scope-filtered, best score first.
    ///
    /// Returns `(chunk_id, file_path, language_name)` in score order.
    /// `sanitized_query` must already be a valid FTS5 MATCH expression
    /// (see `fts::sanitize_fts_query`); callers should skip the call for
    /// empty/`""` queries.
    pub fn fts_chunk_candidates(
        &self,
        sanitized_query: &str,
        scope: &ChunkScope,
        limit: usize,
    ) -> CcResult<Vec<(String, String, String)>> {
        let mut sql =
            "SELECT chunks_fts.chunk_id, chunks.file_path, chunks.language, bm25(chunks_fts, 1.0, 1.0, 2.0) AS score
             FROM chunks_fts
             JOIN chunks ON chunks.chunk_id = chunks_fts.chunk_id
             WHERE chunks_fts MATCH ?"
                .to_string();
        let mut clauses: Vec<String> = Vec::new();
        let mut params: Vec<String> = vec![sanitized_query.to_string()];

        scope.push_clauses(
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

        let conn = self.db.read_conn()?;
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(params.iter()), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    /// Streaming scan over decoded chunk text for the grep lane, in scope.
    ///
    /// Without an explicit file scope the walk is recency-ordered
    /// (`ORDER BY rowid DESC`): chunks are insert-only per write, so
    /// descending rowid puts the most recently indexed files first and a
    /// caller's scan budget is spent on the freshest code — SQLite walks the
    /// table b-tree backwards for this, no sort step. File-scoped scans keep
    /// SQLite's natural probe order.
    ///
    /// `visit` receives each decoded row and returns `false` to stop the
    /// scan (budget exhausted / enough matches). Each visited row costs one
    /// text decode; rows are decoded lazily, so stopping early skips the
    /// remaining decodes. Chunk ids in `skip` are passed over before the
    /// decode (they were already decompressed by an earlier stage, e.g. the
    /// FTS prefilter) and cost no scan budget.
    pub fn scan_chunks_for_grep(
        &self,
        scope: &ChunkScope,
        skip: Option<&std::collections::HashSet<String>>,
        mut visit: impl FnMut(GrepChunkRow) -> bool,
    ) -> CcResult<()> {
        let mut sql =
            "SELECT chunk_id, file_path, language, text, text_encoding, rowid FROM chunks"
                .to_string();
        let mut clauses: Vec<String> = Vec::new();
        let mut params: Vec<String> = Vec::new();

        scope.push_clauses(&mut clauses, &mut params, "file_path", "language");

        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }
        if !scope.has_file_scope() {
            sql.push_str(" ORDER BY rowid DESC");
        }

        let conn = self.db.read_conn()?;
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(params.iter()), |row| {
                let cid = row.get::<_, String>(0)?;
                if skip.is_some_and(|seen| seen.contains(&cid)) {
                    return Ok(None);
                }
                Ok(Some(GrepChunkRow {
                    chunk_id: cid,
                    file_path: row.get::<_, String>(1)?,
                    language_name: row.get::<_, String>(2)?,
                    text: read_chunk_text_with_encoding(row, 3, 4)?,
                    rowid: row.get::<_, i64>(5)?,
                }))
            })
            .map_err(db_err)?;

        for row in rows {
            let Some(row) = row.map_err(db_err)? else {
                continue;
            };
            if !visit(row) {
                break;
            }
        }
        Ok(())
    }

    /// FTS-prefiltered variant of [`Self::scan_chunks_for_grep`]: same
    /// columns and scope clauses, but candidates come from a `chunks_fts`
    /// MATCH (the `phrase` parameter, a token-boundary superset of the grep
    /// literal — see cc-search's `grep_prefilter_phrase`) joined back to
    /// `chunks`, instead of a full table walk. Used by the grep lane's
    /// stage-1 scan; the recency order and `scan_cap` LIMIT keep its budget
    /// semantics identical to the unscoped full scan.
    pub fn scan_chunks_for_grep_prefiltered(
        &self,
        phrase: &str,
        scan_cap: usize,
        scope: &ChunkScope,
        mut visit: impl FnMut(GrepChunkRow) -> bool,
    ) -> CcResult<()> {
        let mut sql = "SELECT chunks.chunk_id, chunks.file_path, chunks.language, chunks.text, \
                       chunks.text_encoding, chunks.rowid \
                       FROM chunks_fts JOIN chunks ON chunks.chunk_id = chunks_fts.chunk_id \
                       WHERE chunks_fts MATCH ?"
            .to_string();
        let mut clauses: Vec<String> = Vec::new();
        let mut params: Vec<String> = vec![phrase.to_string()];

        scope.push_clauses(
            &mut clauses,
            &mut params,
            "chunks.file_path",
            "chunks.language",
        );

        if !clauses.is_empty() {
            sql.push_str(" AND ");
            sql.push_str(&clauses.join(" AND "));
        }
        sql.push_str(" ORDER BY chunks.rowid DESC LIMIT ");
        sql.push_str(&scan_cap.to_string());

        let conn = self.db.read_conn()?;
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(params.iter()), |row| {
                Ok(GrepChunkRow {
                    chunk_id: row.get::<_, String>(0)?,
                    file_path: row.get::<_, String>(1)?,
                    language_name: row.get::<_, String>(2)?,
                    text: read_chunk_text_with_encoding(row, 3, 4)?,
                    rowid: row.get::<_, i64>(5)?,
                })
            })
            .map_err(db_err)?;

        for row in rows {
            let row = row.map_err(db_err)?;
            if !visit(row) {
                break;
            }
        }
        Ok(())
    }

    /// Path-token substring match via the trigram `file_paths_fts` mirror,
    /// batched over multiple tokens with a single pooled-connection checkout.
    ///
    /// Returns one result list per input token, in input order. Per-token
    /// semantics match the original single-token query exactly: `%token%`
    /// LIKE against indexed file paths, ordered by `file_path` ascending,
    /// capped at `per_token_limit`. `path_prefix` adds an additional
    /// `prefix%` LIKE filter.
    ///
    /// LIKE metacharacters (`%`, `_`, `\`) in tokens and the prefix are
    /// escaped so they match literally. Note: the `ESCAPE` clause keeps
    /// SQLite from handing the LIKE constraints to the FTS5 trigram LIKE
    /// optimization, so these queries scan `file_paths_fts` and post-filter
    /// (correct, but no trigram acceleration).
    pub fn path_token_file_hits_many(
        &self,
        tokens: &[&str],
        path_prefix: Option<&str>,
        per_token_limit: usize,
    ) -> CcResult<Vec<Vec<String>>> {
        if tokens.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.db.read_conn()?;
        let mut results = Vec::with_capacity(tokens.len());
        for token in tokens {
            let like_token = format!("%{}%", escape_like(token));
            let mut hits = Vec::new();
            if let Some(prefix) = path_prefix {
                let mut stmt = conn
                    .prepare_cached(
                        "SELECT file_path FROM file_paths_fts \
                         WHERE file_path LIKE ?1 ESCAPE '\\' \
                         AND file_path LIKE ?2 ESCAPE '\\' \
                         ORDER BY file_path LIMIT ?3",
                    )
                    .map_err(db_err)?;
                let rows = stmt
                    .query_map(
                        rusqlite::params![
                            like_token,
                            format!("{}%", escape_like(prefix)),
                            per_token_limit as i64
                        ],
                        |row| row.get::<_, String>(0),
                    )
                    .map_err(db_err)?;
                for row in rows {
                    hits.push(row.map_err(db_err)?);
                }
            } else {
                let mut stmt = conn
                    .prepare_cached(
                        "SELECT file_path FROM file_paths_fts \
                         WHERE file_path LIKE ?1 ESCAPE '\\' \
                         ORDER BY file_path LIMIT ?2",
                    )
                    .map_err(db_err)?;
                let rows = stmt
                    .query_map(
                        rusqlite::params![like_token, per_token_limit as i64],
                        |row| row.get::<_, String>(0),
                    )
                    .map_err(db_err)?;
                for row in rows {
                    hits.push(row.map_err(db_err)?);
                }
            }
            results.push(hits);
        }
        Ok(results)
    }

    /// Symbol-name substring match via the trigram `symbols_fts` mirror,
    /// batched over multiple tokens with a single pooled-connection checkout.
    ///
    /// Returns one result list per input token, in input order. Per-token
    /// semantics match the original single-token query exactly: distinct
    /// `(file_path, symbol_name)` pairs for `%token%` LIKE hits, with
    /// case-insensitive exact-name matches ordered first, then by
    /// `file_path` ascending, capped at `per_token_limit`.
    ///
    /// LIKE metacharacters (`%`, `_`, `\`) in tokens and the prefix are
    /// escaped so they match literally.
    ///
    /// The `path_prefix` variant filters on the UNINDEXED `file_path` column.
    /// The unary `+` on `file_path` (plus the `ESCAPE` clause) keeps SQLite
    /// from routing that LIKE constraint into the FTS5 trigram LIKE
    /// optimization, which cannot serve UNINDEXED columns and used to yield
    /// zero rows; instead it is evaluated as an ordinary post-filter.
    pub fn symbol_token_hits_many(
        &self,
        tokens: &[&str],
        path_prefix: Option<&str>,
        per_token_limit: usize,
    ) -> CcResult<Vec<Vec<(String, String)>>> {
        if tokens.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.db.read_conn()?;
        let mut results = Vec::with_capacity(tokens.len());
        for token in tokens {
            let like_token = format!("%{}%", escape_like(token));
            let mut hits = Vec::new();
            if let Some(prefix) = path_prefix {
                let mut stmt = conn
                    .prepare_cached(
                        "SELECT DISTINCT file_path, name \
                         FROM symbols_fts \
                         WHERE name LIKE ?1 ESCAPE '\\' AND +file_path LIKE ?2 ESCAPE '\\' \
                         ORDER BY CASE WHEN lower(name) = lower(?3) THEN 0 ELSE 1 END, file_path \
                         LIMIT ?4",
                    )
                    .map_err(db_err)?;
                let rows = stmt
                    .query_map(
                        rusqlite::params![
                            like_token,
                            format!("{}%", escape_like(prefix)),
                            token,
                            per_token_limit as i64
                        ],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                    )
                    .map_err(db_err)?;
                for row in rows {
                    hits.push(row.map_err(db_err)?);
                }
            } else {
                let mut stmt = conn
                    .prepare_cached(
                        "SELECT DISTINCT file_path, name \
                         FROM symbols_fts \
                         WHERE name LIKE ?1 ESCAPE '\\' \
                         ORDER BY CASE WHEN lower(name) = lower(?2) THEN 0 ELSE 1 END, file_path \
                         LIMIT ?3",
                    )
                    .map_err(db_err)?;
                let rows = stmt
                    .query_map(
                        rusqlite::params![like_token, token, per_token_limit as i64],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                    )
                    .map_err(db_err)?;
                for row in rows {
                    hits.push(row.map_err(db_err)?);
                }
            }
            results.push(hits);
        }
        Ok(results)
    }

    /// Most recently indexed file paths (`files` ordered by `indexed_at` DESC).
    pub fn recent_indexed_files(&self, limit: usize) -> CcResult<Vec<String>> {
        let conn = self.db.read_conn()?;
        let mut stmt = conn
            .prepare_cached("SELECT file_path FROM files ORDER BY indexed_at DESC LIMIT ?1")
            .map_err(db_err)?;
        let rows = stmt
            .query_map(rusqlite::params![limit as i64], |row| {
                row.get::<_, String>(0)
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    /// Batch-fetch full chunk rows by chunk id, queried in
    /// [`IN_BATCH_SIZE`]-sized `IN (...)` batches like the sibling
    /// batch methods.
    ///
    /// `cached_texts` lets callers supply already-decoded text per chunk id
    /// (e.g. from a decompression cache); for those rows the stored text
    /// column is not decoded again. Row order is unspecified (DB order).
    pub fn chunk_rows_by_ids(
        &self,
        chunk_ids: &[&str],
        cached_texts: &HashMap<String, Arc<str>>,
    ) -> CcResult<Vec<ChunkDetailRow>> {
        if chunk_ids.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.db.read_conn()?;
        let mut results = Vec::with_capacity(chunk_ids.len());
        for batch in chunk_ids.chunks(IN_BATCH_SIZE) {
            let sql = format!(
                "SELECT chunk_id, file_path, language, start_line, end_line, breadcrumb, \
                 symbol_name, symbol_kind, text, text_encoding \
                 FROM chunks WHERE chunk_id IN ({})",
                sql_in_placeholders(batch.len()),
            );
            let mut stmt = conn.prepare_cached(&sql).map_err(db_err)?;
            let rows = stmt
                .query_map(rusqlite::params_from_iter(batch.iter()), |row| {
                    let chunk_id: String = row.get(0)?;
                    let text = if let Some(cached) = cached_texts.get(&chunk_id) {
                        cached.to_string()
                    } else {
                        read_chunk_text_with_encoding(row, 8, 9)?
                    };
                    Ok(ChunkDetailRow {
                        chunk_id,
                        file_path: row.get(1)?,
                        language: row.get(2)?,
                        start_line: row.get(3)?,
                        end_line: row.get(4)?,
                        breadcrumb: row.get(5)?,
                        symbol_name: row.get(6)?,
                        symbol_kind: row.get(7)?,
                        text,
                    })
                })
                .map_err(db_err)?;
            for row in rows {
                results.push(row.map_err(db_err)?);
            }
        }
        Ok(results)
    }

    /// Graph-lane seed lookup: `(symbol_uid, name)` pairs whose name contains
    /// `token` (LIKE via `symbols_fts`).
    ///
    /// Case-insensitive exact-name matches sort first, then shorter names,
    /// so the row cap doesn't crowd out the best seeds. NULL-uid symbols are
    /// excluded. LIKE metacharacters (`%`, `_`, `\`) in `token` are escaped
    /// so they match literally (the `ESCAPE` clause trades the trigram LIKE
    /// acceleration for a scan-and-filter plan).
    pub fn symbol_seed_hits(&self, token: &str, limit: usize) -> CcResult<Vec<(String, String)>> {
        let conn = self.db.read_conn()?;
        let like_pattern = format!("%{}%", escape_like(token));
        let mut stmt = conn
            .prepare_cached(
                "SELECT s.symbol_uid, s.name \
                 FROM symbols_fts f \
                 JOIN symbols s ON s.symbol_id = f.symbol_id \
                 WHERE f.name LIKE ?1 ESCAPE '\\' \
                 AND s.symbol_uid IS NOT NULL \
                 ORDER BY (lower(s.name) = lower(?2)) DESC, length(s.name) ASC \
                 LIMIT ?3",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map(
                rusqlite::params![like_pattern, token, limit as i64],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    /// Symbol uids whose name equals one of `names` exactly (BINARY collation,
    /// index-served via `idx_symbols_name`). NULL-uid symbols are excluded.
    pub fn symbol_uids_by_exact_names(
        &self,
        names: &[&str],
        limit: usize,
    ) -> CcResult<Vec<String>> {
        if names.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.db.read_conn()?;
        // sql_in_placeholders yields numbered ?1..?N, so the trailing LIMIT
        // bind keeps its explicit ?(N+1) index.
        let sql = format!(
            "SELECT symbol_uid FROM symbols \
             WHERE name IN ({}) AND symbol_uid IS NOT NULL \
             LIMIT ?{}",
            sql_in_placeholders(names.len()),
            names.len() + 1,
        );
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = names
            .iter()
            .map(|name| Box::new(name.to_string()) as Box<dyn rusqlite::types::ToSql>)
            .collect();
        params_vec.push(Box::new(limit as i64));
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| row.get::<_, String>(0))
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    /// All symbols with UIDs for infra-to-code binding after a full rebuild
    /// when in-memory write units were dropped at prepare time.
    pub fn symbol_records_for_infra_binding(&self) -> CcResult<Vec<SymbolRecord>> {
        let conn = self.db.read_conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT symbol_id, file_path, name, kind, symbol_uid \
                 FROM symbols WHERE symbol_uid IS NOT NULL",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                let kind: String = row.get(3)?;
                Ok(SymbolRecord {
                    symbol_id: row.get(0)?,
                    file_path: row.get(1)?,
                    name: row.get(2)?,
                    kind: SymbolKind::from_str_lenient(&kind).unwrap_or(SymbolKind::Variable),
                    container: None,
                    start_line: 0,
                    end_line: 0,
                    start_col: 0,
                    end_col: 0,
                    signature: None,
                    doc: None,
                    parser_tier: ParserTier::Generic,
                    parser_confidence: 0.0,
                    qname: None,
                    parent_symbol_id: None,
                    scope_id: None,
                    export_name: None,
                    is_default_export: false,
                    symbol_uid: row.get(4)?,
                    framework_role: None,
                    receiver_type: None,
                    param_types: None,
                    return_type: None,
                    param_count: None,
                    base_types: None,
                    implements: None,
                })
            })
            .map_err(db_err)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_err)
    }

    /// Batch-load `(chunk_id, start_line, end_line)` spans for a set of files,
    /// keyed by file path. Queried in [`IN_BATCH_SIZE`]-sized `IN (...)`
    /// batches.
    pub fn chunk_spans_for_files(&self, file_paths: &[&str]) -> CcResult<ChunkSpansByFile> {
        let mut by_file: ChunkSpansByFile = HashMap::new();
        if file_paths.is_empty() {
            return Ok(by_file);
        }
        let conn = self.db.read_conn()?;
        for batch in file_paths.chunks(IN_BATCH_SIZE) {
            let sql = format!(
                "SELECT file_path, chunk_id, start_line, end_line \
                 FROM chunks WHERE file_path IN ({})",
                sql_in_placeholders(batch.len())
            );
            let mut stmt = conn.prepare_cached(&sql).map_err(db_err)?;
            let params: Vec<&dyn rusqlite::types::ToSql> = batch
                .iter()
                .map(|path| path as &dyn rusqlite::types::ToSql)
                .collect();
            let rows = stmt
                .query_map(params.as_slice(), |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, u32>(2)?,
                        row.get::<_, u32>(3)?,
                    ))
                })
                .map_err(db_err)?;
            for row in rows {
                let row = row.map_err(db_err)?;
                let (file, chunk_id, start, end) = row;
                by_file
                    .entry(file)
                    .or_default()
                    .push((chunk_id, start, end));
            }
        }
        Ok(by_file)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use super::ChunkScope;
    use crate::index_db::IndexDb;
    use tempfile::TempDir;

    fn setup() -> (IndexDb, TempDir) {
        let tmp = TempDir::new().unwrap();
        let db = IndexDb::open(&tmp.path().join("retrieval_test.db"))
            .unwrap()
            .0;
        (db, tmp)
    }

    /// Insert a `files` row (file_paths_fts is trigger-synced).
    fn insert_file(db: &IndexDb, file_path: &str, indexed_at: &str) {
        let conn = db.write_conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO files(file_path,language,content_hash,mtime,size,indexed_at)
             VALUES(?1,'Rust','hash1',1.0,100,?2)",
            rusqlite::params![file_path, indexed_at],
        )
        .unwrap();
    }

    /// Insert a `files` row plus its `files_fts` mirror (application-synced,
    /// rowid aligned with the `files` row).
    fn insert_file_with_summary(db: &IndexDb, file_path: &str, summary: &str) {
        insert_file(db, file_path, "2024-01-01T00:00:00Z");
        let conn = db.write_conn.lock().unwrap();
        conn.execute(
            "INSERT INTO files_fts(rowid,file_path,summary,content_excerpt) \
             SELECT rowid, file_path, ?2, '' FROM files WHERE file_path = ?1",
            rusqlite::params![file_path, summary],
        )
        .unwrap();
    }

    /// Insert a `symbols` row (symbols_fts is trigger-synced).
    fn insert_symbol(
        db: &IndexDb,
        symbol_id: &str,
        file_path: &str,
        name: &str,
        symbol_uid: Option<&str>,
    ) {
        let conn = db.write_conn.lock().unwrap();
        conn.execute(
            "INSERT INTO symbols(symbol_id,file_path,name,kind,start_line,end_line,symbol_uid)
             VALUES(?1,?2,?3,'function',1,5,?4)",
            rusqlite::params![symbol_id, file_path, name, symbol_uid],
        )
        .unwrap();
    }

    /// Insert a plain-text `chunks` row.
    fn insert_chunk(
        db: &IndexDb,
        chunk_id: &str,
        file_path: &str,
        start_line: u32,
        end_line: u32,
        text: &str,
    ) {
        let conn = db.write_conn.lock().unwrap();
        conn.execute(
            "INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,breadcrumb,text)
             VALUES(?1,?2,'rust',0,?3,?4,'root',?5)",
            rusqlite::params![chunk_id, file_path, start_line, end_line, text],
        )
        .unwrap();
    }

    /// Mirror a chunk row into chunks_fts (application-maintained,
    /// rowid-aligned with the base `chunks` row).
    fn mirror_chunk_fts(db: &IndexDb, chunk_id: &str) {
        let conn = db.write_conn.lock().unwrap();
        conn.execute(
            "INSERT INTO chunks_fts(rowid,chunk_id,file_path,breadcrumb,symbol_name,text) \
             SELECT rowid, chunk_id, file_path, breadcrumb, NULL, text FROM chunks WHERE chunk_id = ?1",
            rusqlite::params![chunk_id],
        )
        .unwrap();
    }

    fn set_chunk_language(db: &IndexDb, chunk_id: &str, language: &str) {
        let conn = db.write_conn.lock().unwrap();
        conn.execute(
            "UPDATE chunks SET language = ?2 WHERE chunk_id = ?1",
            rusqlite::params![chunk_id, language],
        )
        .unwrap();
    }

    // ── fts_chunk_candidates ───────────────────────────────────

    /// The lexical lane's scope push-down: path prefix (LIKE-escaped),
    /// language IN, file_paths IN, and LIMIT — moved here from cc-search's
    /// former scope-SQL builders.
    #[test]
    fn fts_chunk_candidates_applies_scope_and_limit() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/a.rs", "2024-01-01");
        insert_file(&db, "lib/b.py", "2024-01-01");
        insert_chunk(&db, "ck_a", "src/a.rs", 1, 5, "alpha token here");
        insert_chunk(&db, "ck_b", "lib/b.py", 1, 5, "alpha token there");
        set_chunk_language(&db, "ck_b", "python");
        mirror_chunk_fts(&db, "ck_a");
        mirror_chunk_fts(&db, "ck_b");

        let all = db
            .retrieval()
            .fts_chunk_candidates("alpha", &ChunkScope::default(), 10)
            .unwrap();
        assert_eq!(all.len(), 2);

        let prefixed = db
            .retrieval()
            .fts_chunk_candidates(
                "alpha",
                &ChunkScope {
                    path_prefix: Some("src/".into()),
                    ..Default::default()
                },
                10,
            )
            .unwrap();
        assert_eq!(prefixed.len(), 1);
        assert_eq!(prefixed[0].0, "ck_a");
        assert_eq!(prefixed[0].1, "src/a.rs");
        assert_eq!(prefixed[0].2, "rust");

        let by_language = db
            .retrieval()
            .fts_chunk_candidates(
                "alpha",
                &ChunkScope {
                    languages: Some(vec!["python".into()]),
                    ..Default::default()
                },
                10,
            )
            .unwrap();
        assert_eq!(by_language.len(), 1);
        assert_eq!(by_language[0].0, "ck_b");

        let by_files = db
            .retrieval()
            .fts_chunk_candidates(
                "alpha",
                &ChunkScope {
                    file_paths: Some(vec!["lib/b.py".into()]),
                    ..Default::default()
                },
                10,
            )
            .unwrap();
        assert_eq!(by_files.len(), 1);
        assert_eq!(by_files[0].0, "ck_b");

        let limited = db
            .retrieval()
            .fts_chunk_candidates("alpha", &ChunkScope::default(), 1)
            .unwrap();
        assert_eq!(limited.len(), 1, "LIMIT applies");
    }

    // ── scan_chunks_for_grep ───────────────────────────────────

    /// Unscoped grep scans walk newest-rowid first (scan budget is spent on
    /// the freshest code) and the visitor's `false` stops the scan early;
    /// LIKE metacharacters in the path prefix match literally.
    #[test]
    fn scan_chunks_for_grep_recency_order_early_stop_and_prefix_escaping() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/old.rs", "2024-01-01");
        insert_file(&db, "src/%special/new.rs", "2024-01-02");
        insert_chunk(&db, "ck_old", "src/old.rs", 1, 5, "needle one");
        insert_chunk(&db, "ck_new", "src/%special/new.rs", 1, 5, "needle two");

        // Recency: ck_new was inserted after ck_old → visited first.
        let mut visited = Vec::new();
        db.retrieval()
            .scan_chunks_for_grep(&ChunkScope::default(), None, |row| {
                visited.push(row.chunk_id);
                true
            })
            .unwrap();
        assert_eq!(visited, vec!["ck_new".to_string(), "ck_old".to_string()]);

        // Early stop: returning false after the first row ends the scan.
        let mut count = 0;
        db.retrieval()
            .scan_chunks_for_grep(&ChunkScope::default(), None, |_| {
                count += 1;
                false
            })
            .unwrap();
        assert_eq!(count, 1);

        // Prefix escaping: "src/%special" must match the literal directory,
        // not act as a wildcard swallowing src/old.rs.
        let mut scoped = Vec::new();
        db.retrieval()
            .scan_chunks_for_grep(
                &ChunkScope {
                    path_prefix: Some("src/%special".into()),
                    ..Default::default()
                },
                None,
                |row| {
                    scoped.push(row.chunk_id);
                    true
                },
            )
            .unwrap();
        assert_eq!(scoped, vec!["ck_new".to_string()]);

        // File-scoped scans visit exactly the scoped file's chunks.
        let mut file_scoped = Vec::new();
        db.retrieval()
            .scan_chunks_for_grep(
                &ChunkScope {
                    file_paths: Some(vec!["src/old.rs".into()]),
                    ..Default::default()
                },
                None,
                |row| {
                    file_scoped.push(row.chunk_id);
                    true
                },
            )
            .unwrap();
        assert_eq!(file_scoped, vec!["ck_old".to_string()]);
    }

    // ── fts_file_summaries ─────────────────────────────────────

    #[test]
    fn fts_file_summaries_orders_by_bm25() {
        let (db, _tmp) = setup();
        insert_file_with_summary(&db, "src/a.rs", "alpha alpha alpha alpha");
        insert_file_with_summary(&db, "src/b.rs", "alpha filler filler filler");
        insert_file_with_summary(&db, "lib/c.rs", "alpha alpha alpha alpha");

        let hits = db
            .retrieval()
            .fts_file_summaries("alpha", None, 10)
            .unwrap();
        assert_eq!(
            hits.len(),
            3,
            "all three files match 'alpha'; got {:?}",
            hits
        );
        // bm25 is negative-better; ORDER BY score ASC puts the highest-tf docs
        // first and the weakest match (src/b.rs) last.
        assert_eq!(
            hits[2].0, "src/b.rs",
            "weakest match must sort last; got {:?}",
            hits
        );
        let score_a = hits.iter().find(|(p, _)| p == "src/a.rs").unwrap().1;
        let score_b = hits.iter().find(|(p, _)| p == "src/b.rs").unwrap().1;
        assert!(
            score_a < score_b,
            "bm25(a)={} must be more negative than bm25(b)={}",
            score_a,
            score_b
        );
    }

    #[test]
    fn fts_file_summaries_applies_prefix_and_limit() {
        let (db, _tmp) = setup();
        insert_file_with_summary(&db, "src/a.rs", "alpha alpha");
        insert_file_with_summary(&db, "lib/c.rs", "alpha alpha");

        let hits = db
            .retrieval()
            .fts_file_summaries("alpha", Some("src/"), 10)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, "src/a.rs");

        let limited = db.retrieval().fts_file_summaries("alpha", None, 1).unwrap();
        assert_eq!(limited.len(), 1);
    }

    #[test]
    fn fts_file_summaries_empty_for_no_match() {
        let (db, _tmp) = setup();
        insert_file_with_summary(&db, "src/a.rs", "alpha");
        let hits = db
            .retrieval()
            .fts_file_summaries("zzznonexistent", None, 10)
            .unwrap();
        assert!(hits.is_empty());
    }

    // ── path_token_file_hits_many ──────────────────────────────

    #[test]
    fn path_token_file_hits_many_substring_match_per_token() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/widgetstore/a.rs", "2024-01-01");
        insert_file(&db, "src/b.rs", "2024-01-01");
        insert_file(&db, "vendor/widget.rs", "2024-01-01");
        insert_file(&db, "lib/storefront.rs", "2024-01-01");

        // One result list per input token, in input order; each list keeps
        // the single-token semantics (substring LIKE, file_path ASC).
        let hits = db
            .retrieval()
            .path_token_file_hits_many(&["widget", "store", "zzznope"], None, 20)
            .unwrap();
        assert_eq!(hits.len(), 3, "one entry per token");
        assert_eq!(
            hits[0],
            vec![
                "src/widgetstore/a.rs".to_string(),
                "vendor/widget.rs".to_string()
            ],
            "token 'widget': substring match ordered by file_path"
        );
        assert_eq!(
            hits[1],
            vec![
                "lib/storefront.rs".to_string(),
                "src/widgetstore/a.rs".to_string()
            ],
            "token 'store': substring match ordered by file_path"
        );
        assert!(hits[2].is_empty(), "unmatched token yields an empty list");

        let prefixed = db
            .retrieval()
            .path_token_file_hits_many(&["widget"], Some("vendor/"), 20)
            .unwrap();
        assert_eq!(prefixed, vec![vec!["vendor/widget.rs".to_string()]]);

        let limited = db
            .retrieval()
            .path_token_file_hits_many(&["widget"], None, 1)
            .unwrap();
        assert_eq!(limited[0].len(), 1, "per-token limit applies");

        let empty = db
            .retrieval()
            .path_token_file_hits_many(&[], None, 20)
            .unwrap();
        assert!(empty.is_empty());
    }

    // ── symbol_token_hits_many ─────────────────────────────────

    #[test]
    fn symbol_token_hits_many_exact_name_first_per_token() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/a.rs", "2024-01-01");
        insert_file(&db, "src/b.rs", "2024-01-01");
        insert_file(&db, "src/z.rs", "2024-01-01");
        insert_symbol(&db, "s1", "src/a.rs", "getUserById", Some("uid:guid"));
        insert_symbol(&db, "s2", "src/z.rs", "user", Some("uid:user"));
        insert_symbol(&db, "s3", "src/b.rs", "createOrder", Some("uid:co"));

        let hits = db
            .retrieval()
            .symbol_token_hits_many(&["user", "order"], None, 24)
            .unwrap();
        assert_eq!(hits.len(), 2, "one entry per token");
        // Exact (case-insensitive) name match must sort before substring hits,
        // even though src/z.rs > src/a.rs lexicographically.
        assert_eq!(
            hits[0],
            vec![
                ("src/z.rs".to_string(), "user".to_string()),
                ("src/a.rs".to_string(), "getUserById".to_string()),
            ]
        );
        assert_eq!(
            hits[1],
            vec![("src/b.rs".to_string(), "createOrder".to_string())]
        );

        // The path_prefix variant must actually filter (formerly an inherited
        // defect: SQLite routed the UNINDEXED `file_path` LIKE constraint into
        // the FTS5 trigram LIKE optimization, yielding zero rows).
        let prefixed = db
            .retrieval()
            .symbol_token_hits_many(&["user"], Some("src/a"), 24)
            .unwrap();
        assert_eq!(
            prefixed[0],
            vec![("src/a.rs".to_string(), "getUserById".to_string())],
            "prefixed query must return the in-prefix substring hit"
        );

        let empty = db
            .retrieval()
            .symbol_token_hits_many(&[], None, 24)
            .unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn symbol_token_hits_many_prefixed_keeps_exact_name_first_ordering() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/p/a.rs", "2024-01-01");
        insert_file(&db, "src/p/z.rs", "2024-01-01");
        insert_file(&db, "vendor/v.rs", "2024-01-01");
        insert_symbol(&db, "s1", "src/p/a.rs", "getUserById", Some("uid:guid"));
        insert_symbol(&db, "s2", "src/p/z.rs", "user", Some("uid:user"));
        insert_symbol(&db, "s3", "vendor/v.rs", "user", Some("uid:vuser"));

        // Same ordering contract as the unprefixed variant: case-insensitive
        // exact-name matches first, then file_path ascending — and the
        // out-of-prefix exact match must be excluded.
        let hits = db
            .retrieval()
            .symbol_token_hits_many(&["user"], Some("src/p/"), 24)
            .unwrap();
        assert_eq!(
            hits[0],
            vec![
                ("src/p/z.rs".to_string(), "user".to_string()),
                ("src/p/a.rs".to_string(), "getUserById".to_string()),
            ],
            "exact name first, then file_path ASC, prefix-filtered"
        );
    }

    // ── LIKE wildcard escaping (`_` / `%` are literals, not wildcards) ──

    #[test]
    fn symbol_token_hits_many_treats_underscore_as_literal() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/a.rs", "2024-01-01");
        insert_file(&db, "src/b.rs", "2024-01-01");
        insert_symbol(&db, "s1", "src/a.rs", "read_conn", Some("uid:rc"));
        insert_symbol(&db, "s2", "src/b.rs", "readxconn", Some("uid:rx"));

        let hits = db
            .retrieval()
            .symbol_token_hits_many(&["read_conn"], None, 24)
            .unwrap();
        assert_eq!(
            hits[0],
            vec![("src/a.rs".to_string(), "read_conn".to_string())],
            "token 'read_conn' must not LIKE-match 'readxconn'"
        );
    }

    #[test]
    fn path_token_file_hits_many_treats_underscore_as_literal() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/read_conn.rs", "2024-01-01");
        insert_file(&db, "src/readxconn.rs", "2024-01-01");
        insert_file(&db, "my_app/conn.rs", "2024-01-01");
        insert_file(&db, "myxapp/conn.rs", "2024-01-01");

        let token_hits = db
            .retrieval()
            .path_token_file_hits_many(&["read_conn"], None, 20)
            .unwrap();
        assert_eq!(
            token_hits,
            vec![vec!["src/read_conn.rs".to_string()]],
            "token 'read_conn' must not LIKE-match 'readxconn'"
        );

        let prefixed = db
            .retrieval()
            .path_token_file_hits_many(&["conn"], Some("my_app/"), 20)
            .unwrap();
        assert_eq!(
            prefixed,
            vec![vec!["my_app/conn.rs".to_string()]],
            "prefix 'my_app/' must not LIKE-match 'myxapp/'"
        );
    }

    #[test]
    fn fts_file_summaries_prefix_treats_underscore_as_literal() {
        let (db, _tmp) = setup();
        insert_file_with_summary(&db, "my_app/a.rs", "alpha alpha");
        insert_file_with_summary(&db, "myxapp/b.rs", "alpha alpha");

        let hits = db
            .retrieval()
            .fts_file_summaries("alpha", Some("my_app/"), 10)
            .unwrap();
        assert_eq!(hits.len(), 1, "prefix 'my_app/' must not match 'myxapp/'");
        assert_eq!(hits[0].0, "my_app/a.rs");
    }

    #[test]
    fn symbol_seed_hits_treats_underscore_as_literal() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/a.rs", "2024-01-01");
        insert_file(&db, "src/b.rs", "2024-01-01");
        insert_symbol(&db, "s1", "src/a.rs", "read_conn", Some("uid:rc"));
        insert_symbol(&db, "s2", "src/b.rs", "readxconn", Some("uid:rx"));

        let hits = db.retrieval().symbol_seed_hits("read_conn", 10).unwrap();
        assert_eq!(
            hits,
            vec![("uid:rc".to_string(), "read_conn".to_string())],
            "token 'read_conn' must not LIKE-match 'readxconn'"
        );
    }

    // ── recent_indexed_files ───────────────────────────────────

    #[test]
    fn recent_indexed_files_newest_first() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/old.rs", "2024-01-01T00:00:00Z");
        insert_file(&db, "src/newest.rs", "2024-03-01T00:00:00Z");
        insert_file(&db, "src/mid.rs", "2024-02-01T00:00:00Z");

        let files = db.retrieval().recent_indexed_files(2).unwrap();
        assert_eq!(
            files,
            vec!["src/newest.rs".to_string(), "src/mid.rs".to_string()]
        );
    }

    // ── chunk_rows_by_ids ──────────────────────────────────────

    #[test]
    fn chunk_rows_by_ids_fetches_batch() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/a.rs", "2024-01-01");
        insert_chunk(&db, "ch1", "src/a.rs", 1, 5, "fn alpha() {}");
        insert_chunk(&db, "ch2", "src/a.rs", 6, 10, "fn beta() {}");
        insert_chunk(&db, "ch3", "src/a.rs", 11, 15, "fn gamma() {}");

        let no_cache: HashMap<String, Arc<str>> = HashMap::new();
        let rows = db
            .retrieval()
            .chunk_rows_by_ids(&["ch1", "ch3"], &no_cache)
            .unwrap();
        assert_eq!(rows.len(), 2);
        let by_id: HashMap<&str, &str> = rows
            .iter()
            .map(|r| (r.chunk_id.as_str(), r.text.as_str()))
            .collect();
        assert_eq!(by_id["ch1"], "fn alpha() {}");
        assert_eq!(by_id["ch3"], "fn gamma() {}");
        let ch1 = rows.iter().find(|r| r.chunk_id == "ch1").unwrap();
        assert_eq!(ch1.file_path, "src/a.rs");
        assert_eq!(ch1.language, "rust");
        assert_eq!(ch1.start_line, 1);
        assert_eq!(ch1.end_line, 5);
        assert_eq!(ch1.breadcrumb, "root");
    }

    #[test]
    fn chunk_rows_by_ids_prefers_cached_text() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/a.rs", "2024-01-01");
        insert_chunk(&db, "ch1", "src/a.rs", 1, 5, "fn alpha() {}");
        insert_chunk(&db, "ch2", "src/a.rs", 6, 10, "fn beta() {}");

        let mut cached: HashMap<String, Arc<str>> = HashMap::new();
        cached.insert("ch1".to_string(), Arc::from("CACHED TEXT"));
        let rows = db
            .retrieval()
            .chunk_rows_by_ids(&["ch1", "ch2"], &cached)
            .unwrap();
        let by_id: HashMap<&str, &str> = rows
            .iter()
            .map(|r| (r.chunk_id.as_str(), r.text.as_str()))
            .collect();
        assert_eq!(
            by_id["ch1"], "CACHED TEXT",
            "cached text must win over the stored column"
        );
        assert_eq!(by_id["ch2"], "fn beta() {}");
    }

    #[test]
    fn chunk_rows_by_ids_spans_multiple_in_batches() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/a.rs", "2024-01-01");
        // More ids than one IN(...) batch (IN_BATCH_SIZE = 200) to exercise
        // the multi-batch path.
        let total = crate::sql_util::IN_BATCH_SIZE + 50;
        for i in 0..total {
            insert_chunk(
                &db,
                &format!("ch{}", i),
                "src/a.rs",
                i as u32 + 1,
                i as u32 + 1,
                &format!("text {}", i),
            );
        }
        let ids: Vec<String> = (0..total).map(|i| format!("ch{}", i)).collect();
        let id_refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();

        let no_cache: HashMap<String, Arc<str>> = HashMap::new();
        let rows = db
            .retrieval()
            .chunk_rows_by_ids(&id_refs, &no_cache)
            .unwrap();
        assert_eq!(rows.len(), total, "all ids across batches must be fetched");
        let last = rows
            .iter()
            .find(|r| r.chunk_id == format!("ch{}", total - 1))
            .unwrap();
        assert_eq!(last.text, format!("text {}", total - 1));
    }

    #[test]
    fn chunk_rows_by_ids_empty_input() {
        let (db, _tmp) = setup();
        let no_cache: HashMap<String, Arc<str>> = HashMap::new();
        let rows = db.retrieval().chunk_rows_by_ids(&[], &no_cache).unwrap();
        assert!(rows.is_empty());
    }

    // ── symbol_seed_hits ───────────────────────────────────────

    #[test]
    fn symbol_seed_hits_exact_first_and_skips_null_uid() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/a.rs", "2024-01-01");
        insert_file(&db, "src/b.rs", "2024-01-01");
        insert_file(&db, "src/c.rs", "2024-01-01");
        insert_symbol(&db, "s1", "src/a.rs", "user", Some("uid:user"));
        insert_symbol(&db, "s2", "src/b.rs", "getUserById", Some("uid:guid"));
        insert_symbol(&db, "s3", "src/c.rs", "userx", None);

        let hits = db.retrieval().symbol_seed_hits("user", 10).unwrap();
        assert_eq!(
            hits,
            vec![
                ("uid:user".to_string(), "user".to_string()),
                ("uid:guid".to_string(), "getUserById".to_string()),
            ],
            "exact name first, NULL-uid rows excluded"
        );
    }

    #[test]
    fn symbol_seed_hits_mixed_case_token_exact_first() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/a.rs", "2024-01-01");
        insert_file(&db, "src/b.rs", "2024-01-01");
        insert_symbol(&db, "s1", "src/a.rs", "user", Some("uid:user"));
        insert_symbol(&db, "s2", "src/b.rs", "getUserById", Some("uid:guid"));

        // Documented contract: exact-name matching is case-INsensitive, so a
        // mixed-case token must still rank the exact symbol first.  The SQL
        // compares lower(name) against lower(?2); binding the raw token left
        // the flag permanently false for mixed-case input.
        let hits = db.retrieval().symbol_seed_hits("User", 10).unwrap();
        assert_eq!(
            hits,
            vec![
                ("uid:user".to_string(), "user".to_string()),
                ("uid:guid".to_string(), "getUserById".to_string()),
            ],
            "mixed-case token 'User' must rank case-insensitive exact match first"
        );
    }

    // ── symbol_uids_by_exact_names ─────────────────────────────

    #[test]
    fn symbol_uids_by_exact_names_matches_only_listed_names() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/a.rs", "2024-01-01");
        insert_file(&db, "src/b.rs", "2024-01-01");
        insert_file(&db, "src/c.rs", "2024-01-01");
        insert_file(&db, "src/d.rs", "2024-01-01");
        insert_symbol(&db, "s1", "src/a.rs", "do", Some("uid:do"));
        insert_symbol(&db, "s2", "src/b.rs", "Do", Some("uid:Do"));
        insert_symbol(&db, "s3", "src/c.rs", "door", Some("uid:door"));
        insert_symbol(&db, "s4", "src/d.rs", "do", None);

        let mut uids = db
            .retrieval()
            .symbol_uids_by_exact_names(&["do", "Do"], 10)
            .unwrap();
        uids.sort();
        assert_eq!(uids, vec!["uid:Do".to_string(), "uid:do".to_string()]);

        let empty = db.retrieval().symbol_uids_by_exact_names(&[], 10).unwrap();
        assert!(empty.is_empty());
    }

    // ── chunk_spans_for_files ──────────────────────────────────

    #[test]
    fn chunk_spans_for_files_groups_by_file() {
        let (db, _tmp) = setup();
        insert_file(&db, "src/a.rs", "2024-01-01");
        insert_file(&db, "src/b.rs", "2024-01-01");
        insert_chunk(&db, "ch1", "src/a.rs", 1, 5, "x");
        insert_chunk(&db, "ch2", "src/a.rs", 6, 10, "y");
        insert_chunk(&db, "ch3", "src/b.rs", 1, 3, "z");

        let spans = db
            .retrieval()
            .chunk_spans_for_files(&["src/a.rs", "src/b.rs", "src/missing.rs"])
            .unwrap();
        assert_eq!(spans.len(), 2);
        assert_eq!(spans["src/a.rs"].len(), 2);
        assert_eq!(spans["src/b.rs"], vec![("ch3".to_string(), 1u32, 3u32)]);

        let empty = db.retrieval().chunk_spans_for_files(&[]).unwrap();
        assert!(empty.is_empty());
    }
}
