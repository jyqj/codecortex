//! P6-011 integration: the artifact → manifest publish loop end to end.
//!
//! Covers what the cc-db CAS tests cannot see from inside the library:
//!
//! 1. **artifact-before-manifest** (ADR-0003): an unverifiable artifact
//!    (`put` failure injection) refuses the publish before the database is
//!    touched — no manifest row, no ack, no epoch movement;
//! 2. **retrieval visibility**: a published document is immediately a
//!    candidate of the filtered exact backend over the real cc-db read path;
//! 3. **Q4 duplicate ack**: republishing identical content through a fresh
//!    claim acks the task with zero visible-set change and zero
//!    `semantic_epoch` bump (P6-004 钟读);
//! 4. **fencing through the orchestrator**: a swapped incarnation and an
//!    expired-then-reclaimed lease are both rejected by the publish CAS.
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use cc_db::index_db::IndexDb;
use cc_db::semantic_manifest_reads::SemanticManifestReads;
use cc_db::semantic_outbox::{
    claim_next_on, reclaim_expired_on, supersede_and_enqueue_on, ClaimedTask, OutboxPlan,
    OutboxUpsert,
};
use cc_model::retrieval::HardScope;
use cc_semantic::cache::{ArtifactCache, CacheRead};
use cc_semantic::publish::{PublishVerdict, Publisher};
use cc_semantic::spec::{DocumentEncodingSpec, VectorSpace};
use cc_semantic::types::{DocSpecDigest, InputDigest};
use cc_semantic::vector::exact::{search, space_manifest_reads, ExactSearch};

// ── harness ──────────────────────────────────────────────────────────────

static SEQ: AtomicU32 = AtomicU32::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "cc-semantic-p6011-{tag}-{}-{n}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn space() -> VectorSpace {
    VectorSpace::new("fake/model-publish", 2).expect("space")
}

fn doc_spec(space: &VectorSpace) -> DocSpecDigest {
    DocumentEncodingSpec::new(space.clone(), None, 8_192, "fake-tokenizer")
        .expect("doc spec")
        .digest()
        .expect("doc spec digest")
}

/// One seeded document ('d1' at 'src/d1.rs') plus its active space and
/// desired embed task, all through the real cc-db tables.
fn seed_world(conn: &rusqlite::Connection, space_id: &str, input_digest: &str) {
    conn.execute(
        "INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES(?1,'{}','active')",
        [space_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) \
         VALUES('src/d1.rs','rust','hash',1.0,1,'2026-01-01')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text) \
         VALUES('c-d1','src/d1.rs','rust',0,1,2,'body')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,encoding_key,\
           reference_json,record_json) \
         VALUES('d1','v1','src/d1.rs','c-d1','enc','{}',?1)",
        rusqlite::params![format!(
            "{{\"input\":{{\"input_hash\":\"{input_digest}\"}}}}"
        )],
    )
    .unwrap();
    let stats = supersede_and_enqueue_on(
        conn,
        &OutboxPlan {
            upserts: &[OutboxUpsert {
                doc_key: "d1".into(),
                doc_version: "v1".into(),
                input_digest: input_digest.into(),
            }],
            removals: &[],
            now_unix: 900.0,
        },
    )
    .unwrap();
    assert_eq!(stats.enqueued, 1);
}

fn incarnation_of(conn: &rusqlite::Connection) -> [u8; 16] {
    let hex: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE key='index_incarnation'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let mut bytes = [0u8; 16];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
    }
    bytes
}

fn manifest_row(conn: &rusqlite::Connection) -> Option<String> {
    conn.query_row(
        "SELECT artifact_ref FROM semantic_manifest WHERE doc_key='d1'",
        [],
        |r| r.get(0),
    )
    .ok()
}

fn task_state(conn: &rusqlite::Connection) -> String {
    conn.query_row(
        "SELECT state FROM semantic_outbox WHERE doc_key='d1'",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

struct World {
    _dir: TempDir,
    db: IndexDb,
    conn: rusqlite::Connection,
    cache: ArtifactCache,
    space: VectorSpace,
    spec: DocSpecDigest,
    input: InputDigest,
}

impl World {
    fn new(tag: &str) -> Self {
        let dir = TempDir::new(tag);
        let (db, _) = IndexDb::open(&dir.0.join("index.sqlite3")).expect("open index");
        let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let space = space();
        let spec = doc_spec(&space);
        let cache = ArtifactCache::open(dir.0.join("cache"), format!("ns-{tag}")).expect("cache");
        let input = InputDigest::of_input(b"body text").expect("input digest");
        seed_world(&conn, space.digest().unwrap().as_str(), input.as_str());
        Self {
            _dir: dir,
            db,
            conn,
            cache,
            space,
            spec,
            input,
        }
    }

    fn publisher(&self) -> Publisher<'_> {
        Publisher::new(
            &self.db,
            &self.cache,
            &self.space,
            &self.spec,
            incarnation_of(&self.conn),
        )
        .expect("publisher")
    }

    fn claim(&self, now: f64) -> ClaimedTask {
        claim_next_on(
            &self.conn,
            self.space.digest().unwrap().as_str(),
            "worker",
            now,
            60.0,
        )
        .unwrap()
        .expect("a ready task")
    }
}

// ── artifact-before-manifest: no verified artifact, no database touch ────

#[test]
fn unverifiable_artifact_refuses_the_publish_before_touching_the_db() {
    let world = World::new("gate");
    let task = world.claim(1000.0);

    // Fault injection (brief: "put 失败 → manifest 无变化"): the vector does
    // not match the frozen space dimension, so cache.put fails inside the
    // orchestration — before the database is ever involved.
    let verdict = world
        .publisher()
        .publish_embedding(&task, &[1.0, 0.0, 0.0], 1_000)
        .expect("orchestration");
    assert_eq!(
        verdict,
        PublishVerdict::ArtifactNotVerified {
            reason: "cache put failed: vector length 3 does not match space dimension 2".into()
        }
    );

    // Second injection: NaN payload — the validity gate of the cache layer.
    let verdict = world
        .publisher()
        .publish_embedding(&task, &[f32::NAN, 0.0], 1_000)
        .expect("orchestration");
    assert!(matches!(
        verdict,
        PublishVerdict::ArtifactNotVerified { .. }
    ));

    // The manifest gained nothing, the task is still claimed (no ack, no
    // retry write), and no clock moved (P6-004 钟读).
    assert_eq!(manifest_row(&world.conn), None);
    assert_eq!(task_state(&world.conn), "claimed");
    let generation = world.db.reads().read_generation().unwrap();
    assert_eq!(generation.semantic_epoch, None);

    // Nothing half-entered the cache either.
    assert_eq!(
        world
            .cache
            .get(&world.space, &world.input, &world.spec)
            .unwrap(),
        CacheRead::Miss
    );
}

// ── publish → retrieval visibility → Q4 duplicate ack ────────────────────

#[test]
fn published_document_is_retrievable_and_duplicate_ack_does_not_bump() {
    let world = World::new("visibility");
    let task = world.claim(1000.0);
    let before = world.db.reads().read_generation().unwrap();
    assert_eq!(before.semantic_epoch, None);

    let verdict = world
        .publisher()
        .publish_embedding(&task, &[1.0, 0.0], 1_000)
        .expect("orchestration");
    assert_eq!(
        verdict,
        PublishVerdict::Published {
            visible_set_changed: true
        }
    );
    let after_first = world.db.reads().read_generation().unwrap();
    assert_eq!(
        after_first.semantic_epoch,
        Some(1),
        "None → 1, exactly once"
    );
    assert_eq!(after_first.index_epoch, before.index_epoch);
    assert_eq!(after_first.evidence_epoch, before.evidence_epoch);

    // Retrieval visibility: the published document is a real candidate of
    // the filtered exact backend over the cc-db read path — query equal to
    // the published vector scores 1.0.
    let reads = SemanticManifestReads::on(&world.conn);
    let source = space_manifest_reads(&reads, &world.space.digest().unwrap());
    let hits = search(
        &world.cache,
        &source,
        ExactSearch {
            space: &world.space,
            query: &[1.0, 0.0],
            filter: &HardScope {
                path_prefix: None,
                languages: None,
                file_paths: None,
            },
            k: 10,
            batch_rows: 2,
        },
    )
    .expect("search");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].doc_key, "d1");
    assert_eq!(hits[0].score, 1.0);

    // Q4: the same content republished under a fresh claim — the visible
    // set did not change, so the epoch stays put (duplicate-ack absorption).
    supersede_and_enqueue_on(
        &world.conn,
        &OutboxPlan {
            upserts: &[OutboxUpsert {
                doc_key: "d1".into(),
                doc_version: "v1".into(),
                input_digest: world.input.as_str().into(),
            }],
            removals: &[],
            now_unix: 1500.0,
        },
    )
    .unwrap();
    let replay = world.claim(1510.0);
    let replay_verdict = world
        .publisher()
        .publish_embedding(&replay, &[1.0, 0.0], 1_510)
        .expect("orchestration");
    assert_eq!(
        replay_verdict,
        PublishVerdict::Published {
            visible_set_changed: false
        }
    );
    assert_eq!(task_state(&world.conn), "done");
    let after_replay = world.db.reads().read_generation().unwrap();
    assert_eq!(
        after_replay.semantic_epoch,
        Some(1),
        "duplicate ack of an unchanged visible set must not bump"
    );
    assert_eq!(after_replay.index_epoch, after_first.index_epoch);
    assert!(
        manifest_row(&world.conn).is_some(),
        "manifest keeps its single row"
    );
}

// ── fencing through the orchestrator ─────────────────────────────────────

#[test]
fn swapped_incarnation_rejects_the_publish() {
    let world = World::new("incarnation");
    let task = world.claim(1000.0);
    let stale = incarnation_of(&world.conn);

    // Simulate the rebuild swap visible to this database (P6-014's
    // "renew on the completed staging DB"): every result computed against
    // the old incarnation must be fenced out.
    world
        .conn
        .execute(
            "UPDATE metadata SET value=lower(hex(randomblob(16))) \
             WHERE key='index_incarnation'",
            [],
        )
        .unwrap();

    let publisher = Publisher::new(&world.db, &world.cache, &world.space, &world.spec, stale)
        .expect("publisher");
    let verdict = publisher
        .publish_embedding(&task, &[1.0, 0.0], 1_000)
        .expect("orchestration");
    assert_eq!(
        verdict,
        PublishVerdict::Rejected(cc_db::semantic_publish::PublishRejection::IncarnationMismatch)
    );

    // The artifact stayed in the cache (paid work is not destroyed), the
    // manifest gained nothing, and the task went back to the queue.
    assert_eq!(manifest_row(&world.conn), None);
    assert_eq!(task_state(&world.conn), "pending");
    assert_eq!(
        world.db.reads().read_generation().unwrap().semantic_epoch,
        None
    );
}

#[test]
fn expired_reclaimed_lease_cannot_publish_but_successor_can() {
    let world = World::new("lease");
    let stale_task = world.claim(1000.0);

    // Lease expires, third party reclaims, worker B claims with a new token.
    assert_eq!(reclaim_expired_on(&world.conn, 1061.0).unwrap(), 1);
    let fresh_task = world.claim(1062.0);
    assert_ne!(fresh_task.token, stale_task.token);

    // The stale holder's publish is fenced out; the database is untouched.
    let stale_verdict = world
        .publisher()
        .publish_embedding(&stale_task, &[1.0, 0.0], 1_062)
        .expect("orchestration");
    assert_eq!(
        stale_verdict,
        PublishVerdict::Rejected(cc_db::semantic_publish::PublishRejection::LeaseLost)
    );
    assert_eq!(manifest_row(&world.conn), None);
    assert_eq!(
        world.db.reads().read_generation().unwrap().semantic_epoch,
        None
    );

    // The current holder publishes; the document becomes retrievable.
    let verdict = world
        .publisher()
        .publish_embedding(&fresh_task, &[1.0, 0.0], 1_063)
        .expect("orchestration");
    assert_eq!(
        verdict,
        PublishVerdict::Published {
            visible_set_changed: true
        }
    );
    assert_eq!(task_state(&world.conn), "done");
    assert_eq!(
        world.db.reads().read_generation().unwrap().semantic_epoch,
        Some(1)
    );
}
