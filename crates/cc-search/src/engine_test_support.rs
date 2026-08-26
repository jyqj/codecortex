//! Shared `#[cfg(test)]` fixtures for the `engine*` test modules: a scoped
//! [`SearchEngine`] over a temp DB, plus chunk/symbol/call-edge seeding
//! helpers.  Function bodies are unchanged from their original home in
//! `engine.rs`'s test module; only visibility (`pub(crate)`) was added so
//! the split-out test modules can keep using them.

use std::sync::Arc;

use cc_db::index_db::{FileWriteUnit, IndexDb};
use cc_model::config::{ProjectConfig, SearchConfig};
use cc_model::{CallEdgeRecord, ChunkRecord, Language, ParseOutcome, ParserTier, SymbolRecord};

use crate::engine::SearchEngine;

pub(crate) fn scoped_test_engine() -> (SearchEngine, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let db = IndexDb::open(&tmp.path().join("index.sqlite3")).unwrap().0;
    let config = ProjectConfig {
        search: SearchConfig {
            lexical_top_k: 3,
            grep_top_k: 3,
            rrf_k: 50,
            lexical_weight: 1.0,
            grep_weight: 0.0,
            rerank_window: 3,
            ..Default::default()
        },
        ..Default::default()
    };
    (SearchEngine::new(Arc::new(db), &config, None), tmp)
}

pub(crate) fn insert_chunk_file(
    engine: &SearchEngine,
    file_path: &str,
    language: Language,
    text: &str,
) {
    let chunk = ChunkRecord {
        chunk_id: format!("chunk:{}", file_path),
        file_path: file_path.to_string(),
        language,
        chunk_index: 0,
        start_line: 1,
        end_line: 1,
        breadcrumb: "root".to_string(),
        text: text.to_string(),
        symbol_name: None,
        symbol_kind: None,
        token_estimate: 8,
        parser_tier: ParserTier::TreeSitter,
        parser_confidence: 1.0,
    };
    let mut outcome = ParseOutcome {
        summary: text.to_string(),
        chunks: vec![chunk],
        parser_tier: ParserTier::TreeSitter,
        parser_confidence: 1.0,
        ..Default::default()
    };
    outcome.is_test_file = false;

    let conn = crate::test_seed::seed_conn(&engine.db);
    IndexDb::insert_file_data(
        &conn,
        &FileWriteUnit {
            rel_path: file_path.to_string(),
            language,
            content_hash: format!("hash-{file_path}"),
            mtime: 0.0,
            size: text.len() as u64,
            outcome,
        },
    )
    .unwrap();
}

pub(crate) fn chunk_write_unit(file_path: &str, text: &str) -> FileWriteUnit {
    let chunk = ChunkRecord {
        chunk_id: format!("chunk:{}", file_path),
        file_path: file_path.to_string(),
        language: Language::Rust,
        chunk_index: 0,
        start_line: 1,
        end_line: 1,
        breadcrumb: "root".to_string(),
        text: text.to_string(),
        symbol_name: None,
        symbol_kind: None,
        token_estimate: 8,
        parser_tier: ParserTier::TreeSitter,
        parser_confidence: 1.0,
    };
    FileWriteUnit {
        rel_path: file_path.to_string(),
        language: Language::Rust,
        content_hash: format!("hash-{file_path}-{}", text.len()),
        mtime: 0.0,
        size: text.len() as u64,
        outcome: ParseOutcome {
            summary: text.to_string(),
            chunks: vec![chunk],
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 1.0,
            ..Default::default()
        },
    }
}

/// Insert a single-chunk file together with one symbol (lines 1-1) and
/// optional call edges, so the graph lane has seeds and hops to follow.
pub(crate) fn insert_graph_file(
    engine: &SearchEngine,
    file_path: &str,
    text: &str,
    symbol_name: &str,
    symbol_uid: &str,
    call_edges: Vec<CallEdgeRecord>,
) {
    let chunk = ChunkRecord {
        chunk_id: format!("chunk:{}", file_path),
        file_path: file_path.to_string(),
        language: Language::Rust,
        chunk_index: 0,
        start_line: 1,
        end_line: 1,
        breadcrumb: "root".to_string(),
        text: text.to_string(),
        symbol_name: None,
        symbol_kind: None,
        token_estimate: 8,
        parser_tier: ParserTier::TreeSitter,
        parser_confidence: 1.0,
    };
    let symbol = SymbolRecord {
        symbol_id: format!("sym:{file_path}:{symbol_name}"),
        file_path: file_path.to_string(),
        name: symbol_name.to_string(),
        kind: cc_model::SymbolKind::Function,
        container: None,
        start_line: 1,
        end_line: 1,
        start_col: 0,
        end_col: 0,
        signature: None,
        doc: None,
        parser_tier: ParserTier::TreeSitter,
        parser_confidence: 1.0,
        qname: None,
        parent_symbol_id: None,
        scope_id: None,
        export_name: None,
        is_default_export: false,
        symbol_uid: Some(symbol_uid.to_string()),
        framework_role: None,
        receiver_type: None,
        param_types: None,
        return_type: None,
        param_count: None,
        base_types: None,
        implements: None,
    };
    let mut outcome = ParseOutcome {
        summary: text.to_string(),
        chunks: vec![chunk],
        symbols: vec![symbol],
        call_edges,
        parser_tier: ParserTier::TreeSitter,
        parser_confidence: 1.0,
        ..Default::default()
    };
    outcome.is_test_file = false;

    let conn = crate::test_seed::seed_conn(&engine.db);
    IndexDb::insert_file_data(
        &conn,
        &FileWriteUnit {
            rel_path: file_path.to_string(),
            language: Language::Rust,
            content_hash: format!("hash-{file_path}"),
            mtime: 0.0,
            size: text.len() as u64,
            outcome,
        },
    )
    .unwrap();
}
