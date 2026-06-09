-- index.sqlite3 — Schema v3 (21 tables + 5 FTS5)

CREATE TABLE IF NOT EXISTS metadata (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS files (
    file_path         TEXT PRIMARY KEY,
    language          TEXT NOT NULL,
    content_hash      TEXT NOT NULL,
    mtime             REAL NOT NULL,
    size              INTEGER NOT NULL,
    summary           TEXT NOT NULL DEFAULT '',
    content_excerpt   TEXT NOT NULL DEFAULT '',
    parser_tier       TEXT NOT NULL DEFAULT 'generic',
    parser_confidence REAL NOT NULL DEFAULT 0.5,
    is_test_file      INTEGER NOT NULL DEFAULT 0,
    indexed_at        TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS chunks (
    chunk_id          TEXT PRIMARY KEY,
    file_path         TEXT NOT NULL REFERENCES files(file_path) ON DELETE CASCADE,
    language          TEXT NOT NULL,
    chunk_index       INTEGER NOT NULL,
    start_line        INTEGER NOT NULL,
    end_line          INTEGER NOT NULL,
    breadcrumb        TEXT NOT NULL DEFAULT '',
    symbol_name       TEXT,
    symbol_kind       TEXT,
    text              TEXT NOT NULL,
    text_encoding     TEXT NOT NULL DEFAULT 'plain',
    token_estimate    INTEGER NOT NULL DEFAULT 0,
    parser_tier       TEXT NOT NULL DEFAULT 'generic',
    parser_confidence REAL NOT NULL DEFAULT 0.5
);
CREATE INDEX IF NOT EXISTS idx_chunks_file ON chunks(file_path, chunk_index);
CREATE INDEX IF NOT EXISTS idx_chunks_symbol ON chunks(symbol_name);

CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
    chunk_id UNINDEXED, file_path UNINDEXED,
    breadcrumb, symbol_name, text,
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TABLE IF NOT EXISTS symbols (
    symbol_id         TEXT PRIMARY KEY,
    file_path         TEXT NOT NULL REFERENCES files(file_path) ON DELETE CASCADE,
    name              TEXT NOT NULL,
    kind              TEXT NOT NULL,
    container         TEXT,
    start_line        INTEGER NOT NULL,
    end_line          INTEGER NOT NULL,
    start_col         INTEGER NOT NULL DEFAULT 0,
    end_col           INTEGER NOT NULL DEFAULT 0,
    signature         TEXT,
    doc               TEXT,
    parser_tier       TEXT NOT NULL DEFAULT 'generic',
    parser_confidence REAL NOT NULL DEFAULT 0.5,
    qname             TEXT,
    parent_symbol_id  TEXT,
    export_name       TEXT,
    is_default_export INTEGER NOT NULL DEFAULT 0,
    symbol_uid        TEXT UNIQUE,
    framework_role    TEXT,
    community_id      INTEGER,
    receiver_type     TEXT,
    param_types       TEXT,
    return_type       TEXT,
    param_count       INTEGER,
    base_types        TEXT,
    implements        TEXT
);
CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file_path);
CREATE INDEX IF NOT EXISTS idx_symbols_qname ON symbols(qname);
CREATE INDEX IF NOT EXISTS idx_symbols_uid ON symbols(symbol_uid);

CREATE VIRTUAL TABLE IF NOT EXISTS symbols_fts USING fts5(
    name,
    symbol_id UNINDEXED,
    file_path UNINDEXED,
    tokenize = 'trigram'
);
CREATE TRIGGER IF NOT EXISTS symbols_fts_ai AFTER INSERT ON symbols BEGIN
    INSERT INTO symbols_fts(rowid, name, symbol_id, file_path)
    VALUES (new.rowid, new.name, new.symbol_id, new.file_path);
END;
CREATE TRIGGER IF NOT EXISTS symbols_fts_ad AFTER DELETE ON symbols BEGIN
    DELETE FROM symbols_fts WHERE rowid = old.rowid;
END;
CREATE TRIGGER IF NOT EXISTS symbols_fts_au AFTER UPDATE OF name ON symbols BEGIN
    DELETE FROM symbols_fts WHERE rowid = old.rowid;
    INSERT INTO symbols_fts(rowid, name, symbol_id, file_path)
    VALUES (new.rowid, new.name, new.symbol_id, new.file_path);
END;

CREATE TABLE IF NOT EXISTS imports (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    file_path     TEXT NOT NULL REFERENCES files(file_path) ON DELETE CASCADE,
    import_string TEXT NOT NULL,
    resolved_path TEXT,
    imported_name TEXT,
    alias         TEXT,
    is_namespace  INTEGER NOT NULL DEFAULT 0,
    is_default    INTEGER NOT NULL DEFAULT 0,
    is_reexport   INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_imports_file ON imports(file_path);
CREATE INDEX IF NOT EXISTS idx_imports_resolved ON imports(resolved_path);

CREATE TABLE IF NOT EXISTS symbol_refs (
    ref_id             TEXT PRIMARY KEY,
    file_path          TEXT NOT NULL REFERENCES files(file_path) ON DELETE CASCADE,
    symbol_name        TEXT NOT NULL,
    container          TEXT,
    ref_kind           TEXT NOT NULL,
    line               INTEGER NOT NULL,
    column_no          INTEGER NOT NULL DEFAULT 0,
    target_symbol_id   TEXT,
    target_file_path   TEXT,
    target_symbol_uid  TEXT,
    ref_name           TEXT,
    resolution_kind    TEXT NOT NULL DEFAULT 'unresolved',
    resolution_confidence REAL NOT NULL DEFAULT 0.0,
    resolution_strategy   TEXT NOT NULL DEFAULT 'unresolved',
    ref_end_line       INTEGER,
    ref_end_col        INTEGER,
    parser_tier        TEXT NOT NULL DEFAULT 'generic',
    parser_confidence  REAL NOT NULL DEFAULT 0.5
);
CREATE INDEX IF NOT EXISTS idx_refs_symbol ON symbol_refs(symbol_name);
CREATE INDEX IF NOT EXISTS idx_refs_file ON symbol_refs(file_path);
CREATE INDEX IF NOT EXISTS idx_refs_target ON symbol_refs(target_file_path, target_symbol_id);
CREATE INDEX IF NOT EXISTS idx_refs_target_uid ON symbol_refs(target_symbol_uid);

CREATE TABLE IF NOT EXISTS call_edges (
    edge_id            TEXT PRIMARY KEY,
    file_path          TEXT NOT NULL REFERENCES files(file_path) ON DELETE CASCADE,
    caller_symbol      TEXT,
    callee_symbol      TEXT NOT NULL,
    line               INTEGER NOT NULL,
    start_col          INTEGER NOT NULL DEFAULT 0,
    end_line           INTEGER,
    end_col            INTEGER NOT NULL DEFAULT 0,
    target_symbol_id   TEXT,
    target_file_path   TEXT,
    caller_symbol_id   TEXT,
    callee_ref_id      TEXT,
    caller_symbol_uid  TEXT,
    callee_symbol_uid  TEXT,
    dispatch_kind      TEXT NOT NULL DEFAULT 'direct',
    call_kind          TEXT NOT NULL DEFAULT 'direct',
    resolution_kind    TEXT NOT NULL DEFAULT 'unresolved',
    resolution_confidence REAL NOT NULL DEFAULT 0.0,
    resolution_strategy   TEXT NOT NULL DEFAULT 'unresolved',
    receiver_expr      TEXT,
    arg_count          INTEGER,
    is_optional_chain  INTEGER NOT NULL DEFAULT 0,
    is_awaited         INTEGER NOT NULL DEFAULT 0,
    is_constructor     INTEGER NOT NULL DEFAULT 0,
    parser_tier        TEXT NOT NULL DEFAULT 'generic',
    parser_confidence  REAL NOT NULL DEFAULT 0.5,
    synthesized_by     TEXT,
    synthesis_key      TEXT,
    registered_file    TEXT,
    registered_line    INTEGER
);
CREATE INDEX IF NOT EXISTS idx_ce_caller ON call_edges(caller_symbol);
CREATE INDEX IF NOT EXISTS idx_ce_callee ON call_edges(callee_symbol);
CREATE INDEX IF NOT EXISTS idx_ce_file ON call_edges(file_path);
CREATE INDEX IF NOT EXISTS idx_ce_caller_uid ON call_edges(caller_symbol_uid);
CREATE INDEX IF NOT EXISTS idx_ce_callee_uid ON call_edges(callee_symbol_uid);
CREATE INDEX IF NOT EXISTS idx_ce_caller_uid_file ON call_edges(caller_symbol_uid, file_path, line);
CREATE INDEX IF NOT EXISTS idx_ce_callee_uid_file ON call_edges(callee_symbol_uid, file_path, line);

CREATE TABLE IF NOT EXISTS test_edges (
    edge_id        TEXT PRIMARY KEY,
    test_file_path TEXT NOT NULL,
    code_file_path TEXT NOT NULL,
    reason         TEXT NOT NULL,
    confidence     REAL NOT NULL DEFAULT 0.5
);
CREATE INDEX IF NOT EXISTS idx_te_test ON test_edges(test_file_path);
CREATE INDEX IF NOT EXISTS idx_te_code ON test_edges(code_file_path);

-- routes: merged route_edges + route_nodes (normalized_path computed at index time)
CREATE TABLE IF NOT EXISTS routes (
    edge_id             TEXT PRIMARY KEY,
    file_path           TEXT NOT NULL REFERENCES files(file_path) ON DELETE CASCADE,
    route_path          TEXT NOT NULL,
    handler_name        TEXT,
    method              TEXT,
    line                INTEGER NOT NULL,
    start_col           INTEGER NOT NULL DEFAULT 0,
    end_line            INTEGER,
    end_col             INTEGER NOT NULL DEFAULT 0,
    handler_symbol_id   TEXT,
    handler_symbol_uid  TEXT,
    handler_expr        TEXT,
    router_symbol_uid   TEXT,
    framework           TEXT,
    route_kind          TEXT,
    confidence          REAL NOT NULL DEFAULT 0.5,
    parser_tier         TEXT NOT NULL DEFAULT 'generic',
    normalized_path     TEXT,
    route_id            TEXT
);
CREATE INDEX IF NOT EXISTS idx_routes_path ON routes(route_path);
CREATE INDEX IF NOT EXISTS idx_routes_handler ON routes(handler_name);
CREATE INDEX IF NOT EXISTS idx_routes_file ON routes(file_path);
CREATE INDEX IF NOT EXISTS idx_routes_handler_uid ON routes(handler_symbol_uid);
CREATE INDEX IF NOT EXISTS idx_routes_norm_path ON routes(normalized_path);
CREATE INDEX IF NOT EXISTS idx_routes_route_id ON routes(route_id);

CREATE TABLE IF NOT EXISTS literal_index (
    literal_id           TEXT PRIMARY KEY,
    file_path            TEXT NOT NULL REFERENCES files(file_path) ON DELETE CASCADE,
    literal              TEXT NOT NULL,
    literal_kind         TEXT NOT NULL,
    line                 INTEGER NOT NULL,
    container            TEXT,
    confidence           REAL NOT NULL DEFAULT 0.5,
    enclosing_symbol_uid TEXT,
    key_path             TEXT
);
CREATE INDEX IF NOT EXISTS idx_literal_kind ON literal_index(literal_kind);
CREATE INDEX IF NOT EXISTS idx_literal_file ON literal_index(file_path);
CREATE INDEX IF NOT EXISTS idx_literal_symbol ON literal_index(enclosing_symbol_uid);
CREATE VIRTUAL TABLE IF NOT EXISTS literal_fts USING fts5(
    literal_id UNINDEXED, file_path UNINDEXED,
    literal, literal_kind,
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE VIRTUAL TABLE IF NOT EXISTS files_fts USING fts5(
    file_path UNINDEXED, summary, content_excerpt,
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE VIRTUAL TABLE IF NOT EXISTS file_paths_fts USING fts5(
    file_path,
    tokenize = 'trigram'
);
CREATE TRIGGER IF NOT EXISTS file_paths_fts_ai AFTER INSERT ON files BEGIN
    INSERT INTO file_paths_fts(rowid, file_path) VALUES (new.rowid, new.file_path);
END;
CREATE TRIGGER IF NOT EXISTS file_paths_fts_ad AFTER DELETE ON files BEGIN
    DELETE FROM file_paths_fts WHERE rowid = old.rowid;
END;
CREATE TRIGGER IF NOT EXISTS file_paths_fts_au AFTER UPDATE OF file_path ON files BEGIN
    DELETE FROM file_paths_fts WHERE rowid = old.rowid;
    INSERT INTO file_paths_fts(rowid, file_path) VALUES (new.rowid, new.file_path);
END;

CREATE TABLE IF NOT EXISTS communities (
    community_id        INTEGER PRIMARY KEY,
    label               TEXT NOT NULL,
    member_count        INTEGER NOT NULL DEFAULT 0,
    representative_file TEXT,
    top_symbols_json    TEXT NOT NULL DEFAULT '[]'
);

-- frameworks: merged repo_frameworks + file_frameworks
CREATE TABLE IF NOT EXISTS frameworks (
    framework_key TEXT NOT NULL,
    scope         TEXT NOT NULL DEFAULT 'repo',
    scope_id      TEXT NOT NULL DEFAULT '',
    confidence    REAL NOT NULL DEFAULT 0.0,
    signals_json  TEXT NOT NULL DEFAULT '[]',
    updated_at    TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (framework_key, scope, scope_id)
);
CREATE INDEX IF NOT EXISTS idx_frameworks_scope ON frameworks(scope);
CREATE INDEX IF NOT EXISTS idx_frameworks_scope_id ON frameworks(scope_id);

-- data-flow edges
CREATE TABLE IF NOT EXISTS data_flow_edges (
    edge_id           TEXT PRIMARY KEY,
    file_path         TEXT NOT NULL,
    source_symbol_uid TEXT,
    target_symbol_uid TEXT,
    flow_kind         TEXT NOT NULL DEFAULT 'read',
    line              INTEGER NOT NULL DEFAULT 0,
    confidence        REAL NOT NULL DEFAULT 0.5,
    parser_tier       TEXT NOT NULL DEFAULT 'tree_sitter',
    env_key           TEXT
);
CREATE INDEX IF NOT EXISTS idx_dfe_file ON data_flow_edges(file_path);
CREATE INDEX IF NOT EXISTS idx_dfe_source ON data_flow_edges(source_symbol_uid);
CREATE INDEX IF NOT EXISTS idx_dfe_target ON data_flow_edges(target_symbol_uid);
CREATE INDEX IF NOT EXISTS idx_dfe_env_key ON data_flow_edges(env_key);

-- co-change edges
CREATE TABLE IF NOT EXISTS co_change_edges (
    edge_id         TEXT PRIMARY KEY,
    file_a          TEXT NOT NULL,
    file_b          TEXT NOT NULL,
    co_change_count INTEGER NOT NULL DEFAULT 1,
    total_commits_a INTEGER NOT NULL DEFAULT 1,
    total_commits_b INTEGER NOT NULL DEFAULT 1,
    confidence      REAL NOT NULL DEFAULT 0.5
);
CREATE INDEX IF NOT EXISTS idx_cce_file_a ON co_change_edges(file_a);
CREATE INDEX IF NOT EXISTS idx_cce_file_b ON co_change_edges(file_b);
CREATE INDEX IF NOT EXISTS idx_cce_confidence ON co_change_edges(confidence);

-- HTTP/async call edges
CREATE TABLE IF NOT EXISTS http_call_edges (
    edge_id              TEXT PRIMARY KEY,
    file_path            TEXT NOT NULL,
    caller_symbol_uid    TEXT,
    url_or_path          TEXT NOT NULL,
    normalized_path      TEXT,
    method               TEXT,
    call_kind            TEXT NOT NULL DEFAULT 'http',
    line                 INTEGER NOT NULL DEFAULT 0,
    confidence           REAL NOT NULL DEFAULT 0.5,
    parser_tier          TEXT NOT NULL DEFAULT 'tree_sitter',
    broker_type          TEXT
);
CREATE INDEX IF NOT EXISTS idx_hce_file ON http_call_edges(file_path);
CREATE INDEX IF NOT EXISTS idx_hce_caller ON http_call_edges(caller_symbol_uid);
CREATE INDEX IF NOT EXISTS idx_hce_norm_path ON http_call_edges(normalized_path);
CREATE INDEX IF NOT EXISTS idx_hce_kind ON http_call_edges(call_kind);
CREATE INDEX IF NOT EXISTS idx_hce_broker ON http_call_edges(broker_type);

-- semantic edges (inheritance, implementation, decoration, etc.)
CREATE TABLE IF NOT EXISTS semantic_edges (
    edge_id TEXT PRIMARY KEY,
    file_path TEXT NOT NULL,
    source_symbol TEXT NOT NULL,
    source_symbol_uid TEXT,
    target_symbol TEXT NOT NULL,
    target_symbol_uid TEXT,
    relation_kind TEXT NOT NULL,
    line INTEGER NOT NULL DEFAULT 0,
    confidence REAL NOT NULL DEFAULT 0.5,
    parser_tier TEXT NOT NULL DEFAULT 'generic'
);
CREATE INDEX IF NOT EXISTS idx_se_file ON semantic_edges(file_path);
CREATE INDEX IF NOT EXISTS idx_se_source ON semantic_edges(source_symbol_uid);
CREATE INDEX IF NOT EXISTS idx_se_target ON semantic_edges(target_symbol_uid);
CREATE INDEX IF NOT EXISTS idx_se_kind ON semantic_edges(relation_kind);

-- Infrastructure graph
CREATE TABLE IF NOT EXISTS infra_nodes (
    node_id     TEXT PRIMARY KEY,
    file_path   TEXT NOT NULL,
    kind        TEXT NOT NULL,
    name        TEXT NOT NULL,
    namespace   TEXT,
    line        INTEGER NOT NULL DEFAULT 0,
    end_line    INTEGER,
    properties           TEXT NOT NULL DEFAULT '{}',
    bound_symbol_uid     TEXT,
    binding_confidence   REAL
);
CREATE INDEX IF NOT EXISTS idx_infra_node_file ON infra_nodes(file_path);
CREATE INDEX IF NOT EXISTS idx_infra_node_kind ON infra_nodes(kind);
CREATE INDEX IF NOT EXISTS idx_infra_node_name ON infra_nodes(name);
CREATE INDEX IF NOT EXISTS idx_infra_bound ON infra_nodes(bound_symbol_uid);

CREATE TABLE IF NOT EXISTS infra_edges (
    edge_id         TEXT PRIMARY KEY,
    source_node_id  TEXT NOT NULL,
    target_node_id  TEXT NOT NULL,
    kind            TEXT NOT NULL,
    confidence      REAL NOT NULL DEFAULT 0.5,
    properties      TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX IF NOT EXISTS idx_infra_edge_src ON infra_edges(source_node_id);
CREATE INDEX IF NOT EXISTS idx_infra_edge_dst ON infra_edges(target_node_id);
CREATE INDEX IF NOT EXISTS idx_infra_edge_kind ON infra_edges(kind);

-- dispatch sites (event/callback/JSX/state registration & invocation)
CREATE TABLE IF NOT EXISTS dispatch_sites (
    site_id              TEXT PRIMARY KEY,
    file_path            TEXT NOT NULL,
    line                 INTEGER NOT NULL,
    col                  INTEGER NOT NULL,
    enclosing_symbol_uid TEXT,
    receiver_expr        TEXT,
    site_kind            TEXT NOT NULL,
    key                  TEXT NOT NULL,
    handler_expr         TEXT,
    handler_symbol_uid   TEXT,
    confidence           REAL NOT NULL DEFAULT 0.5
);
CREATE INDEX IF NOT EXISTS idx_dispatch_sites_file ON dispatch_sites(file_path);
CREATE INDEX IF NOT EXISTS idx_dispatch_sites_kind_key ON dispatch_sites(site_kind, key);

-- runtime evidence: OTLP trace observations that validate HTTP/async edges
CREATE TABLE IF NOT EXISTS runtime_evidence (
    evidence_id     TEXT PRIMARY KEY,
    http_edge_id    TEXT,
    route_id        TEXT,
    service_name    TEXT NOT NULL,
    method          TEXT,
    path            TEXT NOT NULL,
    status_code     TEXT,
    observed_count  INTEGER NOT NULL DEFAULT 1,
    p95_ms          REAL,
    first_seen      TEXT NOT NULL,
    last_seen       TEXT NOT NULL,
    trace_source    TEXT NOT NULL DEFAULT 'otlp'
);
CREATE INDEX IF NOT EXISTS idx_runtime_evidence_path ON runtime_evidence(path);
CREATE INDEX IF NOT EXISTS idx_runtime_evidence_edge ON runtime_evidence(http_edge_id);

-- architecture decision records
CREATE TABLE IF NOT EXISTS adr (
    adr_id      TEXT PRIMARY KEY,
    title       TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'accepted',
    context     TEXT NOT NULL DEFAULT '',
    decision    TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_adr_status ON adr(status);
