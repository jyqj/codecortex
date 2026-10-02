//! P6-010 integration: filtered exact search over the real cc-db manifest
//! read path (`SemanticManifestReads` keyset scan) and the real artifact
//! cache, driven through the `ManifestCandidates` adapter.
//!
//! Covers the structural guarantees the unit tests cannot reach with the
//! in-memory source: SQL-level space isolation, keyset pagination over the
//! live table, FK-cascade deletion ("删除不可返回"), and the
//! publish → search loop end to end.

use cc_db::index_migrate::{migrate_index_db, SchemaStatus};
use cc_db::semantic_manifest_reads::SemanticManifestReads;
use cc_model::retrieval::HardScope;
use cc_model::Language;
use cc_semantic::cache::ArtifactCache;
use cc_semantic::spec::{DocumentEncodingSpec, VectorSpace};
use cc_semantic::types::{DocSpecDigest, InputDigest, SpaceDigest};
use cc_semantic::vector::exact::{search, space_manifest_reads, ExactSearch};

fn v22_conn() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    assert_eq!(migrate_index_db(&conn).unwrap(), SchemaStatus::Initialized);
    conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    conn
}

/// Insert the FK chain (files → chunks → document_manifest →
/// semantic_manifest) with a real input digest and cache ref.
fn seed_published(
    conn: &rusqlite::Connection,
    doc_key: &str,
    file_path: &str,
    language: &str,
    space_id: &str,
    input_digest: &str,
    artifact_ref: &str,
) {
    conn.execute(
        "INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) \
         VALUES(?1,?2,'hash',1.0,1,'2026-01-01')",
        rusqlite::params![file_path, language],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text) \
         VALUES(?1,?2,?3,0,1,2,'body')",
        rusqlite::params![format!("c-{doc_key}"), file_path, language],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,encoding_key,\
           reference_json,record_json) VALUES(?1,'v1',?2,?3,NULL,'{}','{}')",
        rusqlite::params![doc_key, file_path, format!("c-{doc_key}")],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO semantic_manifest(doc_key,doc_version,file_path,encoding_key,input_digest,\
           space_id,artifact_ref,published_at,published_incarnation) \
         VALUES(?1,'v1',?2,'enc',?3,?4,?5,'2026-01-01','inc')",
        rusqlite::params![doc_key, file_path, input_digest, space_id, artifact_ref],
    )
    .unwrap();
}

struct TempCache(std::path::PathBuf);

impl TempCache {
    fn open(tag: &str) -> (Self, ArtifactCache) {
        let root = std::env::temp_dir().join(format!(
            "cc-semantic-p6010-integration-{tag}-{}",
            std::process::id()
        ));
        let cache = ArtifactCache::open(&root, format!("ns-{tag}")).expect("open cache");
        (Self(root), cache)
    }
}

impl Drop for TempCache {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scope() -> HardScope {
    HardScope {
        path_prefix: None,
        languages: None,
        file_paths: None,
    }
}

fn doc_spec(space: &VectorSpace) -> DocSpecDigest {
    DocumentEncodingSpec::new(space.clone(), None, 8_192, "fake-tokenizer")
        .expect("doc spec")
        .digest()
        .expect("doc spec digest")
}

fn run_search(
    cache: &ArtifactCache,
    conn: &rusqlite::Connection,
    space_digest: &SpaceDigest,
    space: &VectorSpace,
    query: &[f32],
    filter: &HardScope,
    k: usize,
    batch_rows: usize,
) -> cc_model::CcResult<Vec<cc_semantic::vector::exact::ScoredDoc>> {
    let reads = SemanticManifestReads::on(conn);
    let source = space_manifest_reads(&reads, space_digest);
    search(
        cache,
        &source,
        ExactSearch {
            space,
            query,
            filter,
            k,
            batch_rows,
        },
    )
}

#[test]
fn published_rows_round_trip_through_manifest_scan_and_cache() {
    let conn = v22_conn();
    let (_temp, cache) = TempCache::open("roundtrip");
    let space = VectorSpace::new("fake/model-integration", 3).expect("space");
    let digest = space.digest().expect("space digest");
    let spec = doc_spec(&space);

    // Publish three documents: identical vectors, distinct doc_keys — the
    // result order must be the tie-break order (doc_key asc).
    for key in ["doc-c", "doc-a", "doc-b"] {
        let input = InputDigest::of_input(key.as_bytes()).expect("input digest");
        let reference = cache
            .put(&space, &input, &spec, &[1.0, 0.0, 0.0], 1_000)
            .expect("put");
        seed_published(
            &conn,
            key,
            &format!("src/{key}.rs"),
            "rust",
            digest.as_str(),
            input.as_str(),
            reference.as_str(),
        );
    }

    let got = run_search(
        &cache,
        &conn,
        &digest,
        &space,
        &[1.0, 0.0, 0.0],
        &scope(),
        10,
        2,
    )
    .expect("search");
    let keys: Vec<_> = got.iter().map(|d| d.doc_key.as_str()).collect();
    assert_eq!(keys, ["doc-a", "doc-b", "doc-c"]);
    assert!(got.iter().all(|d| d.score == 1.0));
}

#[test]
fn sql_scan_is_space_isolated_and_keyset_paginated() {
    let conn = v22_conn();
    let (_temp, cache) = TempCache::open("isolation-sql");
    let space = VectorSpace::new("fake/model-integration", 2).expect("space");
    let digest = space.digest().expect("space digest");
    let other = VectorSpace::new("fake/model-other", 2).expect("other space");
    let other_digest = other.digest().expect("other digest");
    let spec = doc_spec(&space);

    for key in ["a", "b", "c"] {
        let input = InputDigest::of_input(key.as_bytes()).expect("digest");
        let reference = cache
            .put(&space, &input, &spec, &[1.0, 1.0], 1_000)
            .expect("put");
        seed_published(
            &conn,
            key,
            &format!("{key}.rs"),
            "rust",
            digest.as_str(),
            input.as_str(),
            reference.as_str(),
        );
    }
    // A row of a foreign space (same cache objects would be wrong here, so it
    // carries a syntactically valid but foreign ref).
    seed_published(
        &conn,
        "foreign",
        "f.rs",
        "rust",
        other_digest.as_str(),
        "foreign-input",
        "cas.v1:ns-isolation-sql:x:x:x",
    );

    // k = 2 with batch 1 → the keyset walk must still cover exactly the space's
    // three rows, in doc_key order, skipping the foreign-space row.
    let got =
        run_search(&cache, &conn, &digest, &space, &[1.0, 1.0], &scope(), 2, 1).expect("search");
    let keys: Vec<_> = got.iter().map(|d| d.doc_key.as_str()).collect();
    assert_eq!(keys, ["a", "b"]);
}

#[test]
fn cascaded_deletion_removes_the_candidate_structurally() {
    let conn = v22_conn();
    let (_temp, cache) = TempCache::open("cascade");
    let space = VectorSpace::new("fake/model-integration", 2).expect("space");
    let digest = space.digest().expect("space digest");
    let spec = doc_spec(&space);

    for key in ["keep", "drop"] {
        let input = InputDigest::of_input(key.as_bytes()).expect("digest");
        let reference = cache
            .put(&space, &input, &spec, &[1.0, 0.0], 1_000)
            .expect("put");
        seed_published(
            &conn,
            key,
            &format!("{key}.rs"),
            "rust",
            digest.as_str(),
            input.as_str(),
            reference.as_str(),
        );
    }

    // Deleting the document_manifest row cascades the semantic_manifest row
    // away; its cached vector may still exist, but it is unreachable.
    conn.execute("DELETE FROM document_manifest WHERE doc_key='drop'", [])
        .unwrap();
    let remaining: i64 = conn
        .query_row("SELECT COUNT(*) FROM semantic_manifest", [], |r| r.get(0))
        .unwrap();
    assert_eq!(remaining, 1, "FK CASCADE removed the published row");

    let got = run_search(
        &cache,
        &conn,
        &digest,
        &space,
        &[1.0, 0.0],
        &scope(),
        10,
        10,
    )
    .expect("search");
    assert_eq!(
        got.iter().map(|d| d.doc_key.as_str()).collect::<Vec<_>>(),
        ["keep"],
        "the deleted document must not be returnable"
    );
}

#[test]
fn hard_scope_language_filter_applies_on_the_real_read_path() {
    let conn = v22_conn();
    let (_temp, cache) = TempCache::open("scope-real");
    let space = VectorSpace::new("fake/model-integration", 2).expect("space");
    let digest = space.digest().expect("space digest");
    let spec = doc_spec(&space);

    for (key, language) in [("py-doc", "python"), ("rs-doc", "rust")] {
        let input = InputDigest::of_input(key.as_bytes()).expect("digest");
        let reference = cache
            .put(&space, &input, &spec, &[1.0, 0.0], 1_000)
            .expect("put");
        seed_published(
            &conn,
            key,
            &format!("{key}.txt"),
            language,
            digest.as_str(),
            input.as_str(),
            reference.as_str(),
        );
    }
    let filter = HardScope {
        path_prefix: None,
        languages: Some(vec![Language::Rust]),
        file_paths: None,
    };
    let got =
        run_search(&cache, &conn, &digest, &space, &[1.0, 0.0], &filter, 10, 10).expect("search");
    assert_eq!(
        got.iter().map(|d| d.doc_key.as_str()).collect::<Vec<_>>(),
        ["rs-doc"]
    );
}
