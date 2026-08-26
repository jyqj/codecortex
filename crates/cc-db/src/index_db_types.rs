//! Row/value types of the `index_db` surface: epoch vector, file state,
//! write units, and the lite row projections shared by the read models.
//!
//! Everything here is re-exported from [`crate::index_db`], so external
//! paths (`cc_db::index_db::CallEdgeLite`, …) are unchanged.

use std::collections::HashMap;

use cc_model::parse::ParseOutcome;
use cc_model::Language;
use rusqlite::Connection;

/// Metadata key for the index-content epoch counter.
pub const INDEX_EPOCH_KEY: &str = "index_epoch";
/// Metadata key for the runtime-evidence epoch counter.
pub const EVIDENCE_EPOCH_KEY: &str = "evidence_epoch";

/// Monotonic epoch vector persisted in the metadata KV table.
///
/// `index_epoch` advances whenever index content is committed (file batches,
/// postprocess edge rebuilds, full rebuilds). `evidence_epoch` advances on
/// runtime-evidence writes only, so evidence ingestion never invalidates
/// caches that depend solely on index content. Consumers key their caches on
/// these values and compare on read; any observed change forces a recompute.
/// Databases created before the epochs existed read as `0` until first write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct IndexGeneration {
    pub index_epoch: u64,
    pub evidence_epoch: u64,
}

/// Persisted file metadata used to decide whether an incremental scan can skip
/// reading and hashing a file.
#[derive(Debug, Clone, PartialEq)]
pub struct FileState {
    pub content_hash: String,
    pub mtime: f64,
    pub size: u64,
}

/// A single file's worth of data to write into the index.
#[derive(Clone)]
pub struct FileWriteUnit {
    pub rel_path: String,
    pub language: Language,
    pub content_hash: String,
    pub mtime: f64,
    pub size: u64,
    pub outcome: ParseOutcome,
}

/// Pre-compressed chunk payloads computed off the write lock, keyed by
/// `FileWriteUnit::rel_path` and index-aligned with `outcome.chunks`.
/// `Some(blob)` stores the zstd blob, `None` stores plain text — exactly the
/// decision [`compress_chunk_text`] would make inside the transaction, so the
/// on-disk bytes are identical whether or not a side-car entry is present.
pub type PrecompressedChunks = HashMap<String, Vec<Option<Vec<u8>>>>;

/// The `symbols_seed` aggregate observed inside one incremental batch
/// transaction, before and after its mutations. `None` halves mean the
/// database carries no aggregate baseline (or the batch was a no-op that
/// never opened a transaction); consumers must treat that as "no proof".
/// Returned by [`WriteOps::write_incremental_batch`] so seed-derived caches
/// layered above cc-db can validate their fold basis the same way the
/// in-crate seed cache does.
#[derive(Debug, Clone, Copy)]
pub struct SeedTokenSpan {
    pub pre: Option<crate::signature_agg::RowAgg>,
    pub post: Option<crate::signature_agg::RowAgg>,
}

/// Deterministic chunk compression policy: zstd level 3, only for payloads
/// larger than 128 bytes, and only when compression actually saves space.
/// Returns `None` when the chunk should be stored as plain text. Shared by
/// the prepare-phase precompression (cc-index) and the in-transaction
/// fallback in [`IndexDb::insert_file_data`], so both produce byte-identical
/// rows.
pub fn compress_chunk_text(text: &str) -> Option<Vec<u8>> {
    let text_bytes = text.as_bytes();
    if text_bytes.len() <= 128 {
        return None;
    }
    match zstd::encode_all(std::io::Cursor::new(text_bytes), 3) {
        Ok(compressed) if compressed.len() < text_bytes.len() => Some(compressed),
        _ => None,
    }
}

pub type RepoFrameworkRecord = (String, f64, Vec<String>);
pub type FileFrameworkSignal = (String, f64, String);
pub type FileFrameworkRecord = (String, Vec<FileFrameworkSignal>);

#[derive(Debug, Clone)]
pub struct SymbolCoverRow {
    pub symbol_id: String,
    pub symbol_uid: Option<String>,
    pub name: String,
    pub file_path: String,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CallEdgeLite {
    pub file_path: String,
    pub line: u32,
    pub caller_symbol: Option<String>,
    pub callee_symbol: String,
    pub caller_symbol_uid: Option<String>,
    pub callee_symbol_uid: Option<String>,
    pub resolution_kind: String,
    pub confidence: f64,
    pub dispatch_kind: String,
    pub synthesized_by: Option<String>,
    pub synthesis_key: Option<String>,
    pub registered_file: Option<String>,
    pub registered_line: Option<u32>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SymbolRow {
    pub symbol_id: String,
    pub symbol_uid: Option<String>,
    pub name: String,
    pub kind: String,
    pub file_path: String,
    pub container: Option<String>,
    pub start_line: u32,
    pub end_line: u32,
    pub qname: Option<String>,
    pub signature: Option<String>,
}

/// Lean `call_edges` projection feeding dispatch synthesis: both endpoint
/// UIDs are guaranteed present (SQL filters `IS NOT NULL` on both).
#[derive(Debug, Clone)]
pub struct DispatchCallEdgeRow {
    pub edge_id: String,
    pub caller_uid: String,
    pub callee_uid: String,
    pub file_path: String,
    pub line: u32,
}

/// `(symbol_uid, name, kind, container)` projection of `symbols` rows with a
/// UID — the lookup-table input of dispatch synthesis.
#[derive(Debug, Clone)]
pub struct SymbolDispatchRow {
    pub symbol_uid: String,
    pub name: String,
    pub kind: String,
    pub container: Option<String>,
}

/// BFS-friendly edge info returned by call_uid_edges_lite.
#[derive(Debug, Clone)]
pub struct EdgeLiteBfs {
    pub caller_uid: String,
    pub callee_uid: String,
    pub dispatch_kind: String,
    pub synthesized_by: Option<String>,
    pub synthesis_key: Option<String>,
    pub confidence: f64,
    pub file_path: String,
    pub line: u32,
    pub registered_file: Option<String>,
    pub registered_line: Option<u32>,
    pub resolution_kind: Option<String>,
    pub parser_tier: Option<String>,
    pub resolution_strategy: Option<String>,
    pub parser_confidence: Option<f64>,
}

/// Symbol degree metrics.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SymbolDegreeInfo {
    pub in_degree: u32,
    pub out_degree: u32,
    pub caller_count: u32,
    pub callee_count: u32,
    pub ref_count: u32,
}

#[derive(Debug, Clone)]
pub struct SymbolTargetRow {
    pub symbol_id: String,
    pub symbol_uid: Option<String>,
    pub name: String,
    pub qname: Option<String>,
    pub file_path: String,
}

/// Full chunk row with decoded text, returned by `chunk_rows_by_ids`.
#[derive(Debug, Clone)]
pub struct ChunkDetailRow {
    pub chunk_id: String,
    pub file_path: String,
    pub language: String,
    pub start_line: u32,
    pub end_line: u32,
    pub breadcrumb: String,
    pub symbol_name: Option<String>,
    pub symbol_kind: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FileInfoRow {
    pub file_path: String,
    pub language: String,
    pub size: u64,
    pub parser_tier: String,
    pub indexed_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CommunityRow {
    pub community_id: u32,
    pub label: String,
    pub member_count: u32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SymbolRefLite {
    pub file_path: String,
    pub line: u32,
    pub symbol_name: String,
    pub target_symbol_uid: Option<String>,
    pub resolution_kind: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ResolutionAttemptRow {
    pub attempt_id: String,
    pub source_table: String,
    pub source_id: String,
    pub file_path: String,
    pub reference_name: String,
    pub reference_kind: String,
    pub line: u32,
    pub column_no: u32,
    pub container: Option<String>,
    pub candidates: serde_json::Value,
    pub failure_reason: String,
    pub resolution_strategy: String,
    pub parser_tier: String,
    pub parser_confidence: f64,
    pub language: Option<String>,
}

/// Register a `REGEXP(pattern, text)` scalar function on a SQLite connection.
///
/// This enables `column REGEXP ?` syntax in SQL (used by Cypher `=~` expressions).
/// The compiled `Regex` is cached as SQLite auxiliary data keyed on the pattern
/// argument, so a constant pattern is compiled once per statement rather than once
/// per row.
pub(crate) fn register_regexp_function(conn: &Connection) -> rusqlite::Result<()> {
    use rusqlite::functions::FunctionFlags;

    conn.create_scalar_function(
        "regexp",
        2,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let re: std::sync::Arc<regex::Regex> = ctx.get_or_create_aux(
                0,
                |vr| -> Result<_, Box<dyn std::error::Error + Send + Sync + 'static>> {
                    Ok(regex::Regex::new(vr.as_str()?)?)
                },
            )?;
            let text: String = ctx.get(1)?;
            Ok(re.is_match(&text))
        },
    )
}

/// Lightweight route edge row for frontier expansion.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RouteEdgeLite {
    pub edge_id: String,
    pub file_path: String,
    pub route_path: String,
    pub handler_name: Option<String>,
    pub method: Option<String>,
    pub line: u32,
    pub end_line: Option<u32>,
    pub handler_symbol_uid: Option<String>,
    pub framework: Option<String>,
    pub confidence: f64,
}

/// Lightweight route node row.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RouteNodeLite {
    pub route_id: String,
    pub file_path: String,
    pub route_path: String,
    pub method: Option<String>,
    pub handler_symbol_uid: Option<String>,
    pub handler_name: Option<String>,
    pub framework: Option<String>,
    pub line: u32,
    pub end_line: Option<u32>,
    pub confidence: f64,
    /// Normalized route path for matching against HTTP call edges.
    /// Only populated by `all_route_nodes_lite`; other queries leave it as None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normalized_path: Option<String>,
}

/// Lightweight co-change edge row.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CoChangeLite {
    pub edge_id: String,
    pub file_a: String,
    pub file_b: String,
    pub co_change_count: u32,
    pub total_commits_a: u32,
    pub total_commits_b: u32,
    pub confidence: f64,
}

/// Lightweight HTTP call edge row for frontier expansion.
#[derive(Debug, Clone, serde::Serialize)]
pub struct HttpCallEdgeLite {
    pub edge_id: String,
    pub file_path: String,
    pub caller_symbol_uid: Option<String>,
    pub url_or_path: String,
    pub normalized_path: Option<String>,
    pub method: Option<String>,
    pub call_kind: String,
    pub line: u32,
    pub confidence: f64,
}

/// Lightweight symbol row for graph projections (impact seeds, reverse
/// callers): just the identity/location fields plus community membership.
#[derive(Debug, Clone)]
pub struct SymbolLiteRow {
    pub symbol_uid: String,
    pub name: String,
    pub file_path: String,
    pub kind: String,
    pub community_id: Option<u32>,
}

/// Raw symbol row from the dead-code scan; UID may be empty for symbols
/// without a stable identity (callers filter those out).
#[derive(Debug, Clone)]
pub struct DeadCodeSymbolRow {
    pub name: String,
    pub symbol_uid: String,
    pub file_path: String,
    pub kind: String,
}

/// One resolved import edge from a specific file: target path plus the
/// original import string (cycle witness reporting).
#[derive(Debug, Clone)]
pub struct ImportWitnessRow {
    pub resolved_path: String,
    pub import_string: Option<String>,
}

/// Infra nodes, routes, and connecting edges matched for a service/route
/// query. Rows keep the JSON projection shape used by the MCP handlers.
#[derive(Debug, Clone)]
pub struct ServiceBindingRows {
    pub matched_infra_nodes: Vec<serde_json::Value>,
    pub matched_routes: Vec<serde_json::Value>,
    pub related_edges: Vec<serde_json::Value>,
}

/// Aggregated provenance counters over `call_edges`, grouped by
/// dispatch/resolution/synthesis dimensions. Each failed sub-query degrades
/// to an empty breakdown (matching the previous best-effort behavior).
#[derive(Debug, Clone, Default)]
pub struct CallEdgeProvenanceCounts {
    pub by_dispatch_kind: Vec<(Option<String>, i64)>,
    pub synthesized_total: i64,
    pub by_synthesized_by: Vec<(Option<String>, i64)>,
    pub by_resolution_kind: Vec<(Option<String>, i64)>,
}

/// 增量重解析场景的文件边数据载体。
/// 包含重新 resolve 所需的所有边类型，不含 chunk / literal 等无需重解析的数据。
pub struct FileEdgesForReresolve {
    pub symbols: Vec<cc_model::SymbolRecord>,
    pub imports: Vec<cc_model::ImportRecord>,
    pub call_edges: Vec<cc_model::CallEdgeRecord>,
    pub symbol_refs: Vec<cc_model::SymbolRefRecord>,
    pub semantic_edges: Vec<cc_model::SemanticEdgeRecord>,
    pub dispatch_sites: Vec<cc_model::DispatchSiteRecord>,
    pub route_edges: Vec<cc_model::edge::RouteEdgeRecord>,
}
