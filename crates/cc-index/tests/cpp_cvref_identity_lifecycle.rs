//! Owned cv/ref persistence fixtures; no user cache, server, or C++ execution.
//! The v27 bytes and audit must come from the accepted v27 producer, unchanged.
use cc_db::{
    index_db::{FileWriteUnit, IndexDb},
    index_migrate::{SchemaStatus, CURRENT_SCHEMA_VERSION},
};
use cc_index::Indexer;
use cc_model::{
    config::IndexingConfig,
    cpp_owner::CppQualifiedOwnerState,
    generation::ReadGeneration,
    id::StableId,
    source::{ByteSpan, SourceSnapshot},
    symbol_identity::ChunkSymbolIdentity,
    Language, ParseOutcome, SymbolRecord,
};
use cc_parsers::ParserRegistry;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};

const V27_DB: &[u8] = include_bytes!("fixtures/cpp-cvref-v27/index-v27.dbfixture");
const SOURCES: &str = include_str!("fixtures/cpp-cvref-v27/source-manifest.json");
const AUDIT: &str = include_str!("fixtures/cpp-cvref-v27/baseline-audit.json");

fn config() -> IndexingConfig {
    IndexingConfig {
        max_concurrent_parse: Some(1),
        dispatch_synthesis: false,
        ..Default::default()
    }
}

fn sources() -> Vec<Value> {
    serde_json::from_str(SOURCES).unwrap()
}

fn audit() -> Value {
    serde_json::from_str(AUDIT).unwrap()
}

fn text<'a>(record: &'a Value, key: &str) -> &'a str {
    record[key]
        .as_str()
        .unwrap_or_else(|| panic!("missing {key}: {record}"))
}

fn materialize(root: &Path, legacy: bool) -> (PathBuf, BTreeMap<String, String>) {
    let mut active = BTreeMap::new();
    for source in sources() {
        let name = text(&source, "file_path");
        let content = text(&source, "source");
        assert_eq!(source["size"].as_u64().unwrap(), content.len() as u64);
        assert_eq!(
            text(&source, "content_hash"),
            blake3::hash(content.as_bytes()).to_hex().as_str()
        );
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        let mtime = UNIX_EPOCH
            + Duration::new(
                source["mtime_seconds"].as_u64().unwrap(),
                source["mtime_nanos"].as_u64().unwrap() as u32,
            );
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(mtime))
            .unwrap();
        active.insert(name.into(), content.into());
    }
    let path = root.join(".codecortex/index.sqlite3");
    if legacy {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, V27_DB).unwrap();
    }
    (path, active)
}

fn metadata(
    root: &Path,
    active: &BTreeMap<String, String>,
) -> Vec<(String, Vec<u8>, String, u128)> {
    active
        .keys()
        .map(|name| {
            let path = root.join(name);
            let bytes = std::fs::read(&path).unwrap();
            let hash = blake3::hash(&bytes).to_hex().to_string();
            let mtime = std::fs::metadata(path)
                .unwrap()
                .modified()
                .unwrap()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            (name.clone(), bytes, hash, mtime)
        })
        .collect()
}

fn strings(conn: &Connection, sql: &str) -> Vec<String> {
    conn.prepare(sql)
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn count(conn: &Connection, sql: &str, value: &str) -> i64 {
    conn.query_row(sql, [value], |r| r.get(0)).unwrap()
}

fn symbol_value(symbol: &SymbolRecord) -> Value {
    json!({
        "symbol_id": symbol.symbol_id, "file_path": symbol.file_path,
        "name": symbol.name, "kind": symbol.kind.as_str(), "container": symbol.container,
        "qname": symbol.qname, "signature": symbol.signature, "symbol_uid": symbol.symbol_uid,
        "cpp_qualified_owner": symbol.cpp_qualified_owner.as_str(),
        "start_line": symbol.start_line, "start_col": symbol.start_col,
        "end_line": symbol.end_line, "end_col": symbol.end_col,
    })
}

fn sql_symbols(conn: &Connection) -> BTreeMap<String, Value> {
    strings(conn, "SELECT json_object('symbol_id',symbol_id,'file_path',file_path,'name',name,
        'kind',kind,'container',container,'qname',qname,'signature',signature,'symbol_uid',symbol_uid,
        'cpp_qualified_owner',cpp_qualified_owner,'start_line',start_line,'start_col',start_col,
        'end_line',end_line,'end_col',end_col) FROM symbols ORDER BY symbol_id")
        .into_iter().map(|row| {
            let value: Value = serde_json::from_str(&row).unwrap();
            (text(&value, "symbol_id").into(), value)
        }).collect()
}

fn expected_symbols(active: &BTreeMap<String, String>) -> BTreeMap<String, SymbolRecord> {
    let registry = ParserRegistry::new();
    let mut result = BTreeMap::new();
    let mut last_uid = BTreeMap::new();
    for (path, content) in active {
        for symbol in registry
            .parse(path, content, Language::Cpp)
            .unwrap()
            .symbols
        {
            // Preserve the ordinary inline overload debt's existing OR REPLACE
            // survivor. The independent cv/ref assertions below forbid folding
            // either newly supported B1 pair into this historical behavior.
            if let Some(uid) = &symbol.symbol_uid {
                if let Some(previous) = last_uid.insert(uid.clone(), symbol.symbol_id.clone()) {
                    result.remove(&previous);
                }
            }
            assert!(result.insert(symbol.symbol_id.clone(), symbol).is_none());
        }
    }
    result
}

fn assert_fields(actual: &Value, expected: &Value) {
    for key in [
        "symbol_id",
        "file_path",
        "name",
        "kind",
        "container",
        "qname",
        "signature",
        "symbol_uid",
        "cpp_qualified_owner",
        "start_line",
        "start_col",
        "end_line",
        "end_col",
    ] {
        if let Some(value) = expected.get(key) {
            assert_eq!(&actual[key], value, "{key}: {expected}");
        }
    }
}

fn payload_snapshot(path: &Path) -> Vec<Vec<String>> {
    let conn = Connection::open(path).unwrap();
    vec![
        sql_symbols(&conn).into_values().map(|v| v.to_string()).collect(),
        strings(&conn, "SELECT chunk_id||char(0)||record_json FROM chunk_symbol_identity ORDER BY chunk_id"),
        strings(&conn, "SELECT doc_key||char(0)||record_json FROM document_manifest ORDER BY doc_key"),
        strings(&conn, "SELECT chunk_id||char(0)||file_path||char(0)||hex(text)||char(0)||coalesce(source_json,'') FROM chunks ORDER BY chunk_id"),
    ]
}

fn assert_legacy(path: &Path, active: &BTreeMap<String, String>) -> ReadGeneration {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let audit = audit();
    assert_eq!(V27_DB.len(), 778_240);
    assert_eq!(audit["schema_version"], 27);
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        27
    );
    let columns: i64 = conn
        .query_row(
            "SELECT count(*) FROM pragma_table_info('symbols') WHERE name='cpp_qualified_owner'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        columns, 1,
        "genuine v27 schema, not a relabeled older fixture"
    );
    for source in sources() {
        let row: (String, u64, f64) = conn
            .query_row(
                "SELECT content_hash,size,mtime FROM files WHERE file_path=?1",
                [text(&source, "file_path")],
                |r| {
                    Ok((
                        r.get(0)?,
                        u64::try_from(r.get::<_, i64>(1)?).unwrap(),
                        r.get(2)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(row.0, text(&source, "content_hash"));
        assert_eq!(row.1, source["size"].as_u64().unwrap());
        let expected = Duration::new(
            source["mtime_seconds"].as_u64().unwrap(),
            source["mtime_nanos"].as_u64().unwrap() as u32,
        )
        .as_secs_f64();
        assert_eq!(row.2, expected);
    }
    let rows = sql_symbols(&conn);
    for overload in audit["overloads"].as_array().unwrap() {
        let name = text(overload, "name");
        let definitions = overload["definitions"].as_array().unwrap();
        assert_eq!(definitions.len(), 2);
        assert!(definitions
            .iter()
            .all(|d| text(d, "signature") == format!("int {name}()")));
        let mut definition_text = BTreeSet::new();
        let mut definition_ids = BTreeSet::new();
        for definition in definitions {
            let file = text(definition, "file_path");
            let source = SourceSnapshot::new(active.get(file).unwrap().as_bytes());
            let owner: ByteSpan = serde_json::from_value(definition["owner"].clone()).unwrap();
            assert_eq!(
                source.point(owner.start).unwrap(),
                (
                    definition["start_line"].as_u64().unwrap() as usize,
                    definition["start_col"].as_u64().unwrap() as usize
                )
            );
            assert_eq!(
                source.point(owner.end).unwrap(),
                (
                    definition["end_line"].as_u64().unwrap() as usize,
                    definition["end_col"].as_u64().unwrap() as usize
                )
            );
            assert_eq!(
                StableId::edge_id(
                    "sym",
                    file,
                    definition["start_line"].as_u64().unwrap() as u32,
                    definition["start_col"].as_u64().unwrap() as u32
                ),
                text(definition, "symbol_id")
            );
            assert_eq!(
                StableId::symbol_uid(
                    file,
                    text(definition, "qname"),
                    "method",
                    Some(text(definition, "signature"))
                ),
                text(overload, "old_uid")
            );
            definition_ids.insert(text(definition, "symbol_id"));
            definition_text.insert(source.slice(owner).unwrap().to_string());
        }
        assert_eq!(definition_ids.len(), 2);
        let frozen_definitions = match name {
            "cv" => ["int C::cv(){return 1;}", "int C::cv() const{return 2;}"],
            "ref" => ["int C::ref() &{return 3;}", "int C::ref() &&{return 4;}"],
            _ => panic!("unexpected baseline name: {name}"),
        };
        assert_eq!(
            definition_text,
            frozen_definitions.map(String::from).into_iter().collect()
        );
        let survivor = &overload["stored_survivor"];
        assert_fields(rows.get(text(survivor, "symbol_id")).unwrap(), survivor);
        assert_eq!(
            count(&conn, "SELECT count(*) FROM symbols WHERE name=?1", name),
            1,
            "both parsed definitions really collided in v27 SQL"
        );
        assert_eq!(
            count(
                &conn,
                "SELECT count(*) FROM symbols WHERE symbol_uid=?1",
                text(overload, "old_uid")
            ),
            1
        );
        let records = {
            let mut stmt = conn
                .prepare(
                    "SELECT i.record_json FROM chunk_symbol_identity i
                JOIN symbols s ON s.symbol_id=i.symbol_id WHERE s.name=?1 ORDER BY i.chunk_id",
                )
                .unwrap();
            let records = stmt
                .query_map([name], |r| r.get::<_, String>(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            records
        };
        assert!(
            !records.is_empty(),
            "v27 SQL survivor had durable public authority"
        );
        let mut associated_definitions = BTreeSet::new();
        for record in records {
            let identity: ChunkSymbolIdentity = serde_json::from_str(&record).unwrap();
            assert_eq!(identity.symbol_uid, text(overload, "old_uid"));
            assert_eq!(identity.symbol_id, text(survivor, "symbol_id"));
            assert_eq!(identity.qname, text(survivor, "qname"));
            assert_eq!(
                (
                    identity.start_line,
                    identity.start_col,
                    identity.end_line,
                    identity.end_col
                ),
                (
                    survivor["start_line"].as_u64().unwrap() as u32,
                    survivor["start_col"].as_u64().unwrap() as u32,
                    survivor["end_line"].as_u64().unwrap() as u32,
                    survivor["end_col"].as_u64().unwrap() as u32
                )
            );
            let source = SourceSnapshot::new(active.get(&identity.file_path).unwrap().as_bytes());
            assert_eq!(&identity.source, source.identity());
            assert_eq!(
                source.point(identity.owner.start).unwrap(),
                (identity.start_line as usize, identity.start_col as usize)
            );
            assert_eq!(
                source.point(identity.owner.end).unwrap(),
                (identity.end_line as usize, identity.end_col as usize)
            );
            let (chunk_text, proof_json, document_json): (String, String, String) = conn
                .query_row(
                    "SELECT c.text,c.text_encoding,c.source_json,d.reference_json FROM chunks c
                 JOIN document_manifest d ON d.chunk_id=c.chunk_id WHERE c.chunk_id=?1",
                    [&identity.chunk_id],
                    |r| {
                        Ok((
                            cc_db::index_db::read_chunk_text_with_encoding(r, 0, 1)?,
                            r.get(2)?,
                            r.get(3)?,
                        ))
                    },
                )
                .unwrap();
            let proof: cc_model::source::ChunkSource = serde_json::from_str(&proof_json).unwrap();
            assert_eq!(proof.owner, Some(identity.owner));
            assert_eq!(proof.source, identity.source);
            assert!(proof.validate(&chunk_text));
            assert_eq!(source.slice(proof.span).unwrap(), chunk_text);
            assert_eq!(
                serde_json::from_str::<cc_model::identity::DocumentRef>(&document_json).unwrap(),
                identity.document
            );
            associated_definitions.insert(identity.symbol_id);
        }
        assert_eq!(
            associated_definitions.len(),
            1,
            "one public definition per collided name"
        );
    }
    for control in audit["controls"].as_array().unwrap() {
        assert_fields(rows.get(text(control, "symbol_id")).unwrap(), control);
    }
    let dangling: i64 = conn
        .query_row(
            "SELECT count(*) FROM chunk_symbol_identity i
        LEFT JOIN symbols s ON s.symbol_id=i.symbol_id LEFT JOIN chunks c ON c.chunk_id=i.chunk_id
        LEFT JOIN document_manifest d ON d.doc_key=i.doc_key AND d.doc_version=i.doc_version
        WHERE s.symbol_id IS NULL OR c.chunk_id IS NULL OR d.doc_key IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(dangling, 0);
    // These are accepted-producer public hydration results. The current binary
    // must not open v27 through IndexDb before the raw collision audit is done.
    let public = audit["public"]
        .as_array()
        .expect("frozen producer public results");
    for (chunk, qname) in [
        ("chunk:overload.cpp:3", "C::cv"),
        ("chunk:overload.cpp:5", "C::ref"),
    ] {
        let hydrated = public.iter().find(|r| r["chunk_id"] == chunk).unwrap();
        assert_eq!(hydrated["qname"], qname);
        let record: String = conn
            .query_row(
                "SELECT record_json FROM chunk_symbol_identity WHERE chunk_id=?1",
                [chunk],
                |r| r.get(0),
            )
            .unwrap();
        let identity: ChunkSymbolIdentity = serde_json::from_str(&record).unwrap();
        assert_eq!(identity.qname, qname);
        assert_eq!(
            serde_json::to_value(&identity.document).unwrap(),
            hydrated["document"]
        );
        assert_eq!(
            serde_json::to_value(identity.owner).unwrap(),
            hydrated["source_evidence"]["owner"]
        );
    }
    let generation: ReadGeneration = serde_json::from_value(audit["generation"].clone()).unwrap();
    let incarnation: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE key='index_incarnation'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let expected_hex: String = generation
        .incarnation
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(incarnation, expected_hex);
    for (key, expected) in [
        ("index_epoch", Some(generation.index_epoch)),
        ("evidence_epoch", Some(generation.evidence_epoch)),
        ("semantic_epoch", generation.semantic_epoch),
    ] {
        let stored: Option<String> = conn
            .query_row("SELECT value FROM metadata WHERE key=?1", [key], |r| {
                r.get(0)
            })
            .optional()
            .unwrap();
        let actual = stored.map(|s| s.parse::<u64>().unwrap());
        if key == "semantic_epoch" {
            assert_eq!(actual, expected);
        } else {
            assert_eq!(actual.unwrap_or(0), expected.unwrap());
        }
    }
    generation
}

fn assert_current(db: &IndexDb, path: &Path, active: &BTreeMap<String, String>, edited: bool) {
    let conn = Connection::open(path).unwrap();
    assert_eq!(
        CURRENT_SCHEMA_VERSION, 28,
        "semantic identity changes require cache invalidation"
    );
    assert_eq!(db.reads().schema_version().unwrap(), 28);
    assert_eq!(
        strings(&conn, "SELECT file_path FROM files ORDER BY file_path"),
        active.keys().cloned().collect::<Vec<_>>()
    );
    for (file, content) in active {
        let stored: (String, u64) = conn
            .query_row(
                "SELECT content_hash,size FROM files WHERE file_path=?1",
                [file],
                |r| Ok((r.get(0)?, u64::try_from(r.get::<_, i64>(1)?).unwrap())),
            )
            .unwrap();
        assert_eq!(
            stored,
            (
                blake3::hash(content.as_bytes()).to_hex().to_string(),
                content.len() as u64
            )
        );
    }
    let expected = expected_symbols(active);
    let rows = sql_symbols(&conn);
    assert_eq!(
        rows,
        expected
            .iter()
            .map(|(id, s)| (id.clone(), symbol_value(s)))
            .collect()
    );
    for control in audit()["controls"].as_array().unwrap() {
        if active.contains_key(text(control, "file_path")) {
            assert_fields(rows.get(text(control, "symbol_id")).unwrap(), control);
        }
    }
    if let Some(content) = active.get("controls.cpp") {
        let source = SourceSnapshot::new(content.as_bytes());
        let mut stmt = conn
            .prepare(
                "SELECT line,start_col,end_line,end_col FROM call_edges
            WHERE file_path='controls.cpp' AND caller_symbol='run' ORDER BY start_col",
            )
            .unwrap();
        let coordinates: Vec<(u32, u32, u32, u32)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let shapes: BTreeSet<_> = coordinates
            .into_iter()
            .map(|(sl, sc, el, ec)| {
                let span = ByteSpan::new(
                    source.line_start(sl as usize).unwrap() + sc as usize,
                    source.line_start(el as usize).unwrap() + ec as usize,
                )
                .unwrap();
                source.slice(span).unwrap().to_string()
            })
            .collect();
        assert_eq!(
            shapes,
            ["C::cv", "c.cv", "C::ref", "cv", "Other::cv"]
                .map(String::from)
                .into_iter()
                .collect()
        );
        assert_eq!(count(&conn, "SELECT count(*) FROM call_edges WHERE file_path=?1 AND caller_symbol='run'
            AND (target_symbol_id IS NOT NULL OR target_file_path IS NOT NULL OR callee_symbol_uid IS NOT NULL)", "controls.cpp"), 0);
        assert_eq!(count(&conn, "SELECT count(*) FROM symbol_refs WHERE file_path=?1 AND symbol_name IN ('cv','ref','orphan')
            AND (target_symbol_id IS NOT NULL OR target_file_path IS NOT NULL OR target_symbol_uid IS NOT NULL)", "controls.cpp"), 0);
    }
    for overload in audit()["overloads"].as_array().unwrap() {
        let name = text(overload, "name");
        let file = text(overload, "file_path");
        let matching: Vec<_> = expected
            .values()
            .filter(|s| {
                s.name == name
                    && s.qname.as_deref() == overload["stored_survivor"]["qname"].as_str()
            })
            .collect();
        if matching.is_empty() {
            assert!(!active.contains_key(file));
            assert_uid_absent(&conn, text(overload, "old_uid"));
            continue;
        }
        // Independent canonical expectations supplement the parser full-row oracle.
        let canonical = match name {
            "cv" => vec![
                "int cv()",
                if edited {
                    "int cv() volatile"
                } else {
                    "int cv() const"
                },
            ],
            "ref" => vec!["int ref() &", "int ref() &&"],
            _ => panic!("unexpected audited overload: {name}"),
        };
        let signatures: BTreeSet<_> = matching
            .iter()
            .map(|s| s.signature.as_deref().unwrap())
            .collect();
        assert_eq!(signatures, canonical.into_iter().collect());
        assert_eq!(matching.len(), 2);
        let uids: BTreeSet<_> = matching
            .iter()
            .map(|s| s.symbol_uid.as_deref().unwrap())
            .collect();
        assert_eq!(uids.len(), 2);
        for symbol in matching {
            assert_eq!(
                symbol.cpp_qualified_owner,
                CppQualifiedOwnerState::ProvenType
            );
            assert_eq!(symbol.kind.as_str(), "method");
            assert_eq!(
                symbol.symbol_uid.as_deref(),
                Some(
                    StableId::symbol_uid(
                        &symbol.file_path,
                        symbol.qname.as_deref().unwrap(),
                        "method",
                        symbol.signature.as_deref()
                    )
                    .as_str()
                )
            );
            assert!(
                count(
                    &conn,
                    "SELECT count(*) FROM chunk_symbol_identity WHERE symbol_id=?1",
                    &symbol.symbol_id
                ) > 0
            );
        }
        if name == "cv" && active.contains_key(file) {
            let bare = expected
                .values()
                .find(|s| s.file_path == file && s.signature.as_deref() == Some("int cv()"))
                .unwrap();
            assert_eq!(
                bare.symbol_uid.as_deref(),
                Some(text(overload, "old_uid")),
                "bare cv legitimately retains v27 UID"
            );
        } else if name == "ref" || !active.contains_key(file) {
            assert_uid_absent(&conn, text(overload, "old_uid"));
        }
    }
    let records = strings(
        &conn,
        "SELECT record_json FROM chunk_symbol_identity ORDER BY chunk_id",
    );
    for record in records {
        let identity: ChunkSymbolIdentity = serde_json::from_str(&record).unwrap();
        let symbol = expected
            .get(&identity.symbol_id)
            .expect("association must have a live exact symbol");
        assert!(identity.matches_symbol(symbol));
        let source = SourceSnapshot::new(active.get(&identity.file_path).unwrap().as_bytes());
        assert_eq!(&identity.source, source.identity());
        assert_eq!(
            source.point(identity.owner.start).unwrap(),
            (identity.start_line as usize, identity.start_col as usize)
        );
        assert_eq!(
            source.point(identity.owner.end).unwrap(),
            (identity.end_line as usize, identity.end_col as usize)
        );
        let public = db
            .retrieval()
            .chunk_rows_by_ids(&[identity.chunk_id.as_str()], &Default::default())
            .unwrap();
        assert_eq!(public.len(), 1);
        let public = &public[0];
        assert_eq!(public.qname.as_deref(), Some(identity.qname.as_str()));
        assert_eq!(public.document.as_ref(), Some(&identity.document));
        let proof = public.source_evidence.as_ref().unwrap();
        assert_eq!(proof.owner, Some(identity.owner));
        assert_eq!(&proof.source, source.identity());
        assert!(proof.validate(&public.text));
        assert_eq!(source.slice(proof.span).unwrap(), public.text);
    }
    assert_nonbinding(db, &conn);
}

fn assert_nonbinding(db: &IndexDb, conn: &Connection) {
    for sql in [
        "SELECT count(*) FROM call_edges e JOIN symbols s ON s.symbol_id=e.target_symbol_id OR s.symbol_uid=e.callee_symbol_uid WHERE s.cpp_qualified_owner!='non_b1'",
        "SELECT count(*) FROM symbol_refs r JOIN symbols s ON s.symbol_id=r.target_symbol_id OR s.symbol_uid=r.target_symbol_uid WHERE s.cpp_qualified_owner!='non_b1'",
        "SELECT count(*) FROM semantic_edges e JOIN symbols s ON s.symbol_uid=e.target_symbol_uid WHERE s.cpp_qualified_owner!='non_b1' AND e.relation_kind IN ('defines','defines_method')",
    ] {
        assert_eq!(conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    }
    let tagged: BTreeSet<_> = db
        .reads()
        .list_symbol_targets()
        .unwrap()
        .into_iter()
        .filter(|s| s.cpp_qualified_owner.is_b1())
        .filter_map(|s| s.symbol_uid)
        .collect();
    assert!(db
        .symbol_graph_reads()
        .symbol_dispatch_rows()
        .unwrap()
        .iter()
        .all(|s| !tagged.contains(&s.symbol_uid)));
    for symbol in db.retrieval().symbol_records_for_infra_binding().unwrap() {
        if tagged.contains(symbol.symbol_uid.as_ref().unwrap()) {
            assert!(
                symbol.cpp_qualified_owner.is_b1(),
                "infra reload must preserve the negative binding guard"
            );
        }
    }
}

fn assert_uid_absent(conn: &Connection, uid: &str) {
    for sql in [
        "SELECT count(*) FROM symbols WHERE symbol_uid=?1",
        "SELECT count(*) FROM chunk_symbol_identity WHERE json_extract(record_json,'$.symbol_uid')=?1",
        "SELECT count(*) FROM call_edges WHERE caller_symbol_uid=?1 OR callee_symbol_uid=?1",
        "SELECT count(*) FROM symbol_refs WHERE target_symbol_uid=?1",
        "SELECT count(*) FROM semantic_edges WHERE source_symbol_uid=?1 OR target_symbol_uid=?1",
    ] {
        assert_eq!(count(conn, sql, uid), 0, "stale UID: {uid}");
    }
}

fn assert_path_absent(db: &IndexDb, path: &Path, old: &str, old_uids: &[String]) {
    let conn = Connection::open(path).unwrap();
    for table in [
        "files",
        "symbols",
        "chunks",
        "document_manifest",
        "chunk_symbol_identity",
        "files_fts",
        "file_paths_fts",
        "symbols_fts",
        "chunks_fts",
        "public_surfaces",
        "resolution_manifests",
        "resolution_dependencies",
        "call_edges",
        "symbol_refs",
    ] {
        assert_eq!(
            count(
                &conn,
                &format!("SELECT count(*) FROM {table} WHERE file_path=?1"),
                old
            ),
            0,
            "{table}: {old}"
        );
    }
    assert_eq!(
        count(
            &conn,
            "SELECT count(*) FROM call_edges WHERE target_file_path=?1",
            old
        ),
        0
    );
    assert_eq!(
        count(
            &conn,
            "SELECT count(*) FROM symbol_refs WHERE target_file_path=?1",
            old
        ),
        0
    );
    for uid in old_uids {
        assert_uid_absent(&conn, uid);
    }
    assert!(db
        .reads()
        .list_symbol_targets()
        .unwrap()
        .iter()
        .all(|s| s.file_path != old));
    assert!(db
        .retrieval()
        .symbol_records_for_infra_binding()
        .unwrap()
        .iter()
        .all(|s| s.file_path != old));
}

fn unit(path: &str, content: &str) -> FileWriteUnit {
    let source = SourceSnapshot::new(content.as_bytes());
    let mut outcome = ParserRegistry::new()
        .parse(path, content, Language::Cpp)
        .unwrap();
    outcome.document_spec = Some(cc_index::documents::delta::spec_fingerprint().into());
    outcome.documents = Some(cc_index::documents::delta::prepare(&source, &outcome, &[]).unwrap());
    outcome.symbol_identities =
        cc_index::documents::symbol_identity::prepare(&source, &outcome).unwrap();
    FileWriteUnit {
        rel_path: path.into(),
        language: Language::Cpp,
        content_hash: source.identity().content_digest.clone(),
        mtime: 0.0,
        size: content.len() as u64,
        outcome,
    }
}

fn dirty_unit(db: &IndexDb, path: &str, content: &str) -> FileWriteUnit {
    let loaded = db.reads().load_file_edges_for_reresolve(path).unwrap();
    // Writer-only exercise: reload unchanged, already-unbound B1 edges. Do not
    // synthesize current-snapshot proofs or try to publish new associations.
    let outcome = ParseOutcome {
        symbols: loaded.symbols,
        imports: loaded.imports,
        call_edges: loaded.call_edges,
        symbol_refs: loaded.symbol_refs,
        semantic_edges: loaded.semantic_edges,
        dispatch_sites: loaded.dispatch_sites,
        route_edges: loaded.route_edges,
        ..Default::default()
    };
    assert!(outcome.cpp_qualified_owner_proofs.is_empty());
    assert!(outcome.symbol_identities.is_empty());
    FileWriteUnit {
        rel_path: path.into(),
        language: Language::Unknown,
        content_hash: blake3::hash(content.as_bytes()).to_hex().to_string(),
        mtime: 0.0,
        size: content.len() as u64,
        outcome,
    }
}

#[test]
fn real_v27_rebuilds_unchanged_bytes_hashes_mtimes_then_edits_renames_deletes() {
    let root = tempfile::tempdir().unwrap();
    let (path, mut active) = materialize(root.path(), true);
    let before = metadata(root.path(), &active);
    let old = assert_legacy(&path, &active);
    let (db, status) = IndexDb::open(&path).unwrap();
    assert_eq!(status, SchemaStatus::Initialized);
    let opened = db.reads().read_generation().unwrap();
    assert_ne!(opened.incarnation, old.incarnation);
    assert!(opened.index_epoch > old.index_epoch);
    assert!(opened.evidence_epoch > old.evidence_epoch);
    let db = Arc::new(db);
    let cfg = config();
    let indexer = Indexer::new(Arc::clone(&db), root.path(), &cfg);
    assert_eq!(
        indexer
            .build_index(root.path(), false)
            .unwrap()
            .files_parsed,
        active.len()
    );
    assert_eq!(metadata(root.path(), &active), before);
    assert_current(&db, &path, &active, false);
    let generation = db.reads().read_generation().unwrap();
    assert_eq!(
        indexer
            .build_index(root.path(), false)
            .unwrap()
            .files_parsed,
        0
    );
    assert_eq!(db.reads().read_generation().unwrap(), generation);
    assert_current(&db, &path, &active, false);
    drop(indexer);
    drop(db);
    let (db, status) = IndexDb::open(&path).unwrap();
    assert_eq!(status, SchemaStatus::UpToDate);
    assert_eq!(db.reads().read_generation().unwrap(), generation);
    let db = Arc::new(db);
    let indexer = Indexer::new(Arc::clone(&db), root.path(), &cfg);
    assert_eq!(
        indexer
            .build_index(root.path(), false)
            .unwrap()
            .files_parsed,
        0
    );
    assert_eq!(metadata(root.path(), &active), before);
    assert_current(&db, &path, &active, false);

    let parsed = expected_symbols(&active);
    let qualified = parsed
        .values()
        .find(|s| {
            s.signature.as_deref() == Some("int cv() const")
                && s.cpp_qualified_owner == CppQualifiedOwnerState::ProvenType
        })
        .unwrap();
    let edited_path = qualified.file_path.clone();
    let stale_uid = qualified.symbol_uid.clone().unwrap();
    let content = active.get(&edited_path).unwrap();
    let source = SourceSnapshot::new(content.as_bytes());
    let span = ByteSpan::new(
        source.line_start(qualified.start_line as usize).unwrap() + qualified.start_col as usize,
        source.line_start(qualified.end_line as usize).unwrap() + qualified.end_col as usize,
    )
    .unwrap();
    let definition = source.slice(span).unwrap();
    let replacement = definition.replacen(" const", " volatile", 1);
    assert_ne!(replacement, definition);
    let mut edited = content.clone();
    edited.replace_range(span.start..span.end, &replacement);
    std::fs::write(root.path().join(&edited_path), &edited).unwrap();
    active.insert(edited_path.clone(), edited);
    assert_eq!(
        indexer
            .build_index(root.path(), false)
            .unwrap()
            .files_parsed,
        1
    );
    assert_current(&db, &path, &active, true);
    assert_uid_absent(&Connection::open(&path).unwrap(), &stale_uid);

    let old_uids: Vec<_> = expected_symbols(&active)
        .values()
        .filter(|s| s.file_path == edited_path)
        .filter_map(|s| s.symbol_uid.clone())
        .collect();
    let renamed = "renamed-cvref.cpp";
    std::fs::rename(root.path().join(&edited_path), root.path().join(renamed)).unwrap();
    let renamed_source = active.remove(&edited_path).unwrap();
    active.insert(renamed.into(), renamed_source);
    assert_eq!(
        indexer
            .build_index(root.path(), false)
            .unwrap()
            .files_parsed,
        1
    );
    assert_path_absent(&db, &path, &edited_path, &old_uids);
    assert_current(&db, &path, &active, true);
    let renamed_uids: Vec<_> = expected_symbols(&active)
        .values()
        .filter(|s| s.file_path == renamed)
        .filter_map(|s| s.symbol_uid.clone())
        .collect();
    std::fs::remove_file(root.path().join(renamed)).unwrap();
    active.remove(renamed);
    assert_eq!(
        indexer
            .build_index(root.path(), false)
            .unwrap()
            .files_parsed,
        0
    );
    assert_path_absent(&db, &path, renamed, &renamed_uids);
    assert_current(&db, &path, &active, true);
}

#[test]
fn fresh_hot_noop_reopen_and_both_dirty_writers_preserve_identity_and_guard() {
    let root = tempfile::tempdir().unwrap();
    let (path, active) = materialize(root.path(), false);
    let db = Arc::new(IndexDb::open(&path).unwrap().0);
    let cfg = config();
    let indexer = Indexer::new(Arc::clone(&db), root.path(), &cfg);
    assert_eq!(
        indexer
            .build_index(root.path(), false)
            .unwrap()
            .files_parsed,
        active.len()
    );
    assert_current(&db, &path, &active, false);
    let before = payload_snapshot(&path);
    let generation = db.reads().read_generation().unwrap();
    assert_eq!(
        indexer
            .build_index(root.path(), false)
            .unwrap()
            .files_parsed,
        0
    );
    assert_eq!(db.reads().read_generation().unwrap(), generation);
    assert_eq!(payload_snapshot(&path), before);
    let dirty: Vec<_> = active.iter().map(|(p, s)| dirty_unit(&db, p, s)).collect();
    db.writes().replace_reresolved_edges_only(&dirty).unwrap();
    assert_current(&db, &path, &active, false);
    assert_eq!(payload_snapshot(&path), before);
    db.writes()
        .write_incremental_batch(&[], &[], &dirty, &[], &[], &Default::default())
        .unwrap();
    assert_current(&db, &path, &active, false);
    assert_eq!(payload_snapshot(&path), before);
    let generation = db.reads().read_generation().unwrap();
    drop(indexer);
    drop(db);
    let (db, status) = IndexDb::open(&path).unwrap();
    assert_eq!(status, SchemaStatus::UpToDate);
    assert_eq!(db.reads().read_generation().unwrap(), generation);
    assert_current(&db, &path, &active, false);
    assert_eq!(payload_snapshot(&path), before);
}

#[test]
fn current_snapshot_proofs_are_required_and_invalid_full_writes_roll_back() {
    let root = tempfile::tempdir().unwrap();
    let (path, active) = materialize(root.path(), false);
    let db = IndexDb::open(&path).unwrap().0;
    let units: Vec<_> = active.iter().map(|(p, s)| unit(p, s)).collect();
    db.writes().replace_files_batch(&units).unwrap();
    assert_current(&db, &path, &active, false);
    let valid = units
        .iter()
        .find(|u| {
            u.outcome
                .symbols
                .iter()
                .any(|s| s.signature.as_deref() == Some("int cv() const"))
        })
        .unwrap();
    let content = active.get(&valid.rel_path).unwrap();
    let before = payload_snapshot(&path);
    let generation = db.reads().read_generation().unwrap();
    let mut missing = valid.clone();
    missing.outcome.cpp_qualified_owner_proofs.clear();
    let prepared = cc_index::documents::symbol_identity::prepare(
        &SourceSnapshot::new(content.as_bytes()),
        &missing.outcome,
    )
    .unwrap();
    assert!(prepared
        .iter()
        .all(|i| !["cv", "ref"].contains(&i.name.as_str())));
    let mut duplicate = valid.clone();
    let proof = duplicate
        .outcome
        .cpp_qualified_owner_proofs
        .iter()
        .find(|p| p.qname.ends_with("::cv"))
        .unwrap()
        .clone();
    duplicate
        .outcome
        .cpp_qualified_owner_proofs
        .push(proof.clone());
    let prepared = cc_index::documents::symbol_identity::prepare(
        &SourceSnapshot::new(content.as_bytes()),
        &duplicate.outcome,
    )
    .unwrap();
    assert!(prepared.iter().all(|i| i.symbol_id != proof.symbol_id));
    let mut mismatched = valid.clone();
    mismatched.outcome.cpp_qualified_owner_proofs[0]
        .source
        .content_digest = "0".repeat(64);
    let mut downgraded = valid.clone();
    downgraded
        .outcome
        .symbols
        .iter_mut()
        .find(|s| s.symbol_id == proof.symbol_id)
        .unwrap()
        .cpp_qualified_owner = CppQualifiedOwnerState::NonB1;
    for invalid in [missing, duplicate, mismatched, downgraded] {
        assert!(db.writes().replace_files_batch(&[invalid]).is_err());
        assert_eq!(db.reads().read_generation().unwrap(), generation);
        assert_eq!(payload_snapshot(&path), before);
        assert_current(&db, &path, &active, false);
    }
}

#[test]
fn ordinary_owner_controls_and_b1_target_shapes_keep_their_binding_policy() {
    let root = tempfile::tempdir().unwrap();
    let content = "int global() { return 1; }\n\
        namespace lexical { int nsleaf() { return 2; } }\n\
        struct Inline { int inside() const { return 3; } };\n\
        namespace Owner { int qualified(); }\n\
        int Owner::qualified() { return 4; }\n\
        struct Complete { int member() const; };\n\
        int Complete::member() const { return 5; }\n\
        int Unknown::unknown() const { return 6; }\n\
        struct Forward;\n\
        int Forward::forward() const { return 7; }\n\
        int caller() { Complete object; Inline local; global(); local.inside();\n\
          member(); qualified(); unknown(); forward(); object.member();\n\
          Complete::member(); Owner::qualified(); Unknown::unknown(); Forward::forward();\n\
          auto pointer = &Complete::member; return 0; }\n";
    let active = BTreeMap::from([("controls.cpp".into(), content.into())]);
    std::fs::write(root.path().join("controls.cpp"), content).unwrap();
    let path = root.path().join(".codecortex/index.sqlite3");
    let db = Arc::new(IndexDb::open(&path).unwrap().0);
    let cfg = config();
    let indexer = Indexer::new(Arc::clone(&db), root.path(), &cfg);
    indexer.build_index(root.path(), false).unwrap();
    let conn = Connection::open(&path).unwrap();
    let symbols = expected_symbols(&active);
    assert_eq!(
        sql_symbols(&conn),
        symbols
            .iter()
            .map(|(id, s)| (id.clone(), symbol_value(s)))
            .collect()
    );
    for (name, state, kind, signature, qname) in [
        (
            "global",
            "non_b1",
            "function",
            "int global()",
            Some("global"),
        ),
        (
            "nsleaf",
            "non_b1",
            "function",
            "int nsleaf()",
            Some("lexical::nsleaf"),
        ),
        (
            "inside",
            "non_b1",
            "method",
            "int inside()",
            Some("Inline::inside"),
        ),
        (
            "qualified",
            "proven_namespace",
            "function",
            "int qualified()",
            Some("Owner::qualified"),
        ),
        (
            "member",
            "proven_type",
            "method",
            "int member() const",
            Some("Complete::member"),
        ),
        ("unknown", "unproven", "function", "int unknown()", None),
        ("forward", "unproven", "function", "int forward()", None),
    ] {
        let s = symbols.values().find(|s| s.name == name).unwrap();
        assert_eq!(s.cpp_qualified_owner.as_str(), state);
        assert_eq!(s.kind.as_str(), kind);
        assert_eq!(s.signature.as_deref(), Some(signature));
        assert_eq!(s.qname.as_deref(), qname);
        assert_eq!(s.symbol_uid.is_some(), qname.is_some());
    }
    for name in ["member", "qualified", "unknown", "forward"] {
        assert!(
            count(
                &conn,
                "SELECT count(*) FROM call_edges WHERE callee_symbol=?1",
                name
            ) > 0
        );
        assert_eq!(count(&conn, "SELECT count(*) FROM call_edges WHERE callee_symbol=?1 AND
            (target_symbol_id IS NOT NULL OR target_file_path IS NOT NULL OR callee_symbol_uid IS NOT NULL
             OR resolution_strategy!='cpp_qualified_owner_unproven')", name), 0);
        assert_eq!(count(&conn, "SELECT count(*) FROM symbol_refs WHERE symbol_name=?1 AND
            (target_symbol_id IS NOT NULL OR target_file_path IS NOT NULL OR target_symbol_uid IS NOT NULL
             OR resolution_strategy!='cpp_qualified_owner_unproven')", name), 0);
    }
    let global = symbols
        .values()
        .find(|s| s.name == "global")
        .unwrap()
        .symbol_uid
        .as_ref()
        .unwrap();
    assert!(
        count(
            &conn,
            "SELECT count(*) FROM call_edges WHERE callee_symbol_uid=?1",
            global
        ) > 0
    );
    assert_nonbinding(&db, &conn);
    assert_eq!(
        db.reads()
            .find_classes_with_method_names(&["member"])
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        db.reads()
            .find_classes_with_method_names(&["inside"])
            .unwrap(),
        vec![("Inline".into(), "controls.cpp".into())]
    );
    let dirty = dirty_unit(&db, "controls.cpp", content);
    let before = payload_snapshot(&path);
    db.writes()
        .replace_reresolved_edges_only(&[dirty.clone()])
        .unwrap();
    db.writes()
        .write_incremental_batch(&[], &[], &[dirty], &[], &[], &Default::default())
        .unwrap();
    assert_eq!(payload_snapshot(&path), before);
    assert_nonbinding(&db, &conn);
}
