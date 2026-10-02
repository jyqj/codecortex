//! P6-013 integration: the explicit worker queue end to end.
//!
//! Covers what the cc-db facade tests cannot see:
//!
//! 1. **drop policy**: an undisposed [`LeaseGuard`] leaves the claim intact
//!    and recoverable (lease expiry + reclaim; no ack, no retry, no I/O);
//! 2. **claim loop end to end**: drain → FakeProvider → artifact cache →
//!    publish CAS → retrievable manifest, with the clock audit (Auxiliary);
//! 3. **continuous-edit merge**: three rapid edits of one document coalesce
//!    into ONE live task and ONE embed (DB-layer supersede, relied on here);
//! 4. **claimed-task supersede mid-flight**: the old attempt can never
//!    publish (fenced), the waste bound is one started embed, the successor
//!    version serves;
//! 5. **lease expiry mid-work**: after expiry + reclaim the stale worker
//!    loses its voice (renew refused), the successor publishes;
//! 6. **bounded admission**: one drain claims at most `max_batch`;
//! 7. **retry budget**: exhaustion dead-letters without further claims.
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use cc_db::index_db::IndexDb;
use cc_db::semantic_manifest_reads::SemanticManifestReads;
use cc_db::semantic_outbox::{
    supersede_and_enqueue_on, ClaimFairness, ClaimedTask, OutboxPlan, OutboxUpsert,
};
use cc_model::retrieval::HardScope;
use cc_semantic::cache::{ArtifactCache, CacheRead};
use cc_semantic::ports::DocumentInput;
use cc_semantic::providers::fake::{FakeProvider, FakeProviderConfig};
use cc_semantic::publish::Publisher;
use cc_semantic::queue::{drain_pending, EmbedHandler, LeaseGuard, WorkerLimits};
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
            "cc-semantic-p6013-{tag}-{}-{n}",
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
    VectorSpace::new("fake/model-queue", 2).expect("space")
}

fn doc_spec(space: &VectorSpace) -> DocSpecDigest {
    DocumentEncodingSpec::new(space.clone(), None, 8_192, "fake-tokenizer")
        .expect("doc spec")
        .digest()
        .expect("doc spec digest")
}

fn limits(max_batch: usize) -> WorkerLimits {
    WorkerLimits::validated(max_batch, 60.0, 0.0, 3).expect("valid limits")
}

struct World {
    _dir: TempDir,
    db: IndexDb,
    conn: rusqlite::Connection,
    cache: ArtifactCache,
    space: VectorSpace,
    spec: DocSpecDigest,
    provider: FakeProvider,
}

impl World {
    fn new(tag: &str) -> Self {
        let dir = TempDir::new(tag);
        let (db, _) = IndexDb::open(&dir.0.join("index.sqlite3")).expect("open index");
        let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
        conn.busy_timeout(Duration::from_secs(5)).unwrap();
        let space = space();
        let spec = doc_spec(&space);
        let cache = ArtifactCache::open(dir.0.join("cache"), format!("ns-{tag}")).expect("cache");
        let provider = FakeProvider::new(FakeProviderConfig::new(space.clone()));
        conn.execute(
            "INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES(?1,'{}','active')",
            [space.digest().unwrap().as_str()],
        )
        .unwrap();
        Self {
            _dir: dir,
            db,
            conn,
            cache,
            space,
            spec,
            provider,
        }
    }

    /// One document write pass: manifest row at `doc_version` + its desired
    /// embed task (the P6-006 source-transaction atomic write, simulated at
    /// the same two-table seam).
    fn write_pass(&self, doc_key: &str, doc_version: &str, input_digest: &str) {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO document_manifest(doc_key,doc_version,file_path,chunk_id,\
                 encoding_key,reference_json,record_json) \
                 VALUES(?1,?2,?3,?4,'enc','{}',?5)",
                rusqlite::params![
                    doc_key,
                    doc_version,
                    format!("src/{doc_key}.rs"),
                    format!("c-{doc_key}"),
                    format!("{{\"input\":{{\"input_hash\":\"{input_digest}\"}}}}")
                ],
            )
            .unwrap();
        let upsert = OutboxUpsert {
            doc_key: doc_key.into(),
            doc_version: doc_version.into(),
            input_digest: input_digest.into(),
        };
        let stats = supersede_and_enqueue_on(
            &self.conn,
            &OutboxPlan {
                upserts: &[upsert],
                removals: &[],
                now_unix: 900.0,
            },
        )
        .unwrap();
        assert_eq!(stats.enqueued, 1);
    }

    fn seed_doc(&self, doc_key: &str, input: &InputDigest) {
        self.conn
            .execute(
                "INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) \
                 VALUES(?1,'rust','hash',1.0,1,'2026-01-01')",
                [format!("src/{doc_key}.rs")],
            )
            .unwrap();
        self.conn
            .execute(
                "INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text) \
                 VALUES(?1,?2,'rust',0,1,2,'body')",
                rusqlite::params![format!("c-{doc_key}"), format!("src/{doc_key}.rs")],
            )
            .unwrap();
        self.write_pass(doc_key, "v1", input.as_str());
    }

    fn live_tasks(&self, doc_key: &str) -> Vec<(String, String)> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT doc_version,state FROM semantic_outbox \
                 WHERE doc_key=?1 AND state IN ('pending','claimed')",
            )
            .unwrap();
        let rows = stmt
            .query_map([doc_key], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        rows.map(|r| r.unwrap()).collect()
    }

    fn task_state(&self, doc_key: &str) -> String {
        self.conn
            .query_row(
                "SELECT state FROM semantic_outbox WHERE doc_key=?1",
                [doc_key],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn manifest_version(&self, doc_key: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT doc_version FROM semantic_manifest WHERE doc_key=?1",
                [doc_key],
                |r| r.get(0),
            )
            .ok()
    }

    fn incarnation(&self) -> [u8; 16] {
        let hex: String = self
            .conn
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

    /// The reference embed handler over this world (digest-only resolver —
    /// the fake provider consumes digests, and input-bytes resolution from
    /// `record_json` is composition-root-owned).
    fn handler<'a>(
        &'a self,
        resolve: &'a dyn Fn(&ClaimedTask) -> cc_model::CcResult<Option<DocumentInput>>,
    ) -> EmbedHandler<'a> {
        let publisher = Publisher::new(
            &self.db,
            &self.cache,
            &self.space,
            &self.spec,
            self.incarnation(),
        )
        .expect("publisher");
        EmbedHandler::new(publisher, &self.provider, resolve)
    }

    fn digest_of(&self, bytes: &[u8]) -> InputDigest {
        InputDigest::of_input(bytes).expect("input digest")
    }
}

fn digest_resolver<'a>(
    table: &'a [(String, &'a [u8])],
) -> impl Fn(&ClaimedTask) -> cc_model::CcResult<Option<DocumentInput>> + 'a {
    move |task: &ClaimedTask| {
        table
            .iter()
            .find(|(digest, _)| *digest == task.input_digest)
            .map(|(_, bytes)| DocumentInput::from_bytes(bytes))
            .transpose()
    }
}

// ── 1. drop policy: undisposed claim stays recoverable ───────────────────

#[test]
fn drop_of_undisposed_lease_leaves_the_claim_recoverable() {
    let world = World::new("drop");
    let input = world.digest_of(b"body d1");
    world.seed_doc("d1", &input);
    let table = vec![(input.as_str().to_string(), &b"body d1"[..])];
    let resolver = digest_resolver(&table);

    let guard: LeaseGuard<'_> = LeaseGuard::claim(&world.db, "worker-a", 60.0)
        .unwrap()
        .expect("claimed");
    let (task_id, token) = (guard.task().task_id, guard.task().token.clone());
    drop(guard);

    // No ack, no retry, no write: still claimed under the same token, the
    // attempt budget untouched, no error recorded.
    assert_eq!(world.task_state("d1"), "claimed");
    let (state, db_token, attempts, last_error): (String, Option<String>, i64, Option<String>) =
        world
            .conn
            .query_row(
                "SELECT state,lease_token,attempt_count,last_error FROM semantic_outbox \
                 WHERE task_id=?1",
                [task_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
    assert_eq!(state, "claimed");
    assert_eq!(db_token.as_deref(), Some(token.as_str()));
    assert_eq!(attempts, 1);
    assert_eq!(last_error, None);

    // Lease expires → reclaim returns it to pending WITHOUT consuming an
    // attempt → the next claim (fresh token) serves it, and the end-to-end
    // chain publishes.
    world
        .conn
        .execute("UPDATE semantic_outbox SET lease_expires_at=1.0", [])
        .unwrap();
    assert_eq!(world.db.reclaim_expired_semantic().unwrap(), 1);
    assert_eq!(world.task_state("d1"), "pending");

    let report = drain_pending(&world.db, "worker-b", &limits(4), &mut |guard| {
        world.handler(&resolver).handle(guard)
    })
    .unwrap();
    assert_eq!(
        report.claimed, 1,
        "the dropped claim was recovered and re-claimed"
    );
    assert_eq!(report.completed, 1);
    assert_eq!(world.provider.call_count(), 1);
    assert_eq!(world.manifest_version("d1").as_deref(), Some("v1"));
}

// ── 2. claim loop end to end: FakeProvider → publish → visibility ────────

#[test]
fn drain_publishes_end_to_end_through_the_fake_provider() {
    let world = World::new("e2e");
    let d1 = world.digest_of(b"body d1");
    let d2 = world.digest_of(b"body d2");
    world.seed_doc("d1", &d1);
    world.seed_doc("d2", &d2);
    let table = vec![
        (d1.as_str().to_string(), &b"body d1"[..]),
        (d2.as_str().to_string(), &b"body d2"[..]),
    ];
    let resolver = digest_resolver(&table);

    let before = world.db.reads().read_generation().unwrap();
    assert_eq!(before.semantic_epoch, None);

    let report = drain_pending(&world.db, "worker", &limits(4), &mut |guard| {
        world.handler(&resolver).handle(guard)
    })
    .unwrap();
    assert_eq!(
        report,
        cc_semantic::queue::BatchReport {
            claimed: 2,
            completed: 2,
            retried: 0,
            lease_lost: 0,
        }
    );
    assert_eq!(world.provider.call_count(), 2, "one embed per live task");
    assert_eq!(world.manifest_version("d1").as_deref(), Some("v1"));
    assert_eq!(world.manifest_version("d2").as_deref(), Some("v1"));
    assert_eq!(world.task_state("d1"), "done");
    assert_eq!(world.task_state("d2"), "done");

    // One `Semantic` effect per real visible-set change; index/evidence and
    // incarnation never moved (Auxiliary discipline: 任何时刻 local 查询可用).
    let after = world.db.reads().read_generation().unwrap();
    assert_eq!(after.semantic_epoch, Some(2));
    assert_eq!(after.index_epoch, before.index_epoch);
    assert_eq!(after.evidence_epoch, before.evidence_epoch);
    assert_eq!(after.incarnation, before.incarnation);

    // Retrieval visibility through the real read path: the published d1
    // vector read back from the cache scores exactly 1.0 against itself.
    let CacheRead::Hit(verified) = world.cache.get(&world.space, &d1, &world.spec).unwrap() else {
        panic!("published artifact must be in the cache");
    };
    let reads = SemanticManifestReads::on(&world.conn);
    let source = space_manifest_reads(&reads, &world.space.digest().unwrap());
    let hits = search(
        &world.cache,
        &source,
        ExactSearch {
            space: &world.space,
            query: &verified.data,
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
    assert_eq!(hits.len(), 2, "both published documents are candidates");
    assert_eq!(
        hits[0].doc_key, "d1",
        "exact match ranks first (stable ties)"
    );
    assert!(
        (hits[0].score - 1.0).abs() < 1e-9,
        "self-match scores 1.0 up to f32 cosine roundtrip, got {}",
        hits[0].score
    );
}

// ── 3. continuous-edit merge: three edits, one live task, one embed ──────

#[test]
fn three_rapid_edits_coalesce_into_one_task_and_one_embed() {
    let world = World::new("merge");
    let v1 = world.digest_of(b"body v1");
    let v2 = world.digest_of(b"body v2");
    let v3 = world.digest_of(b"body v3");
    world.seed_doc("d1", &v1);
    world.write_pass("d1", "v2", v2.as_str());
    world.write_pass("d1", "v3", v3.as_str());
    let table = vec![
        (v1.as_str().to_string(), &b"body v1"[..]),
        (v2.as_str().to_string(), &b"body v2"[..]),
        (v3.as_str().to_string(), &b"body v3"[..]),
    ];
    let resolver = digest_resolver(&table);

    // The DB-layer merge (P6-005 unique index + P6-006 supersede-then-insert,
    // relied on by this module): exactly one live task, at the newest version.
    let live = world.live_tasks("d1");
    assert_eq!(live.len(), 1, "three edits must coalesce to one live task");
    assert_eq!(live[0], ("v3".into(), "pending".into()));

    // One drain, one unit of provider work, one publication — of the FINAL
    // version only.
    let report = drain_pending(&world.db, "worker", &limits(4), &mut |guard| {
        world.handler(&resolver).handle(guard)
    })
    .unwrap();
    assert_eq!(report.claimed, 1);
    assert_eq!(report.completed, 1);
    assert_eq!(
        world.provider.call_count(),
        1,
        "the worker embeds once for three edits"
    );
    assert_eq!(world.manifest_version("d1").as_deref(), Some("v3"));
    assert_eq!(world.task_state("d1"), "done");
}

// ── 4. claimed-task supersede mid-flight: fenced, bounded waste ──────────

#[test]
fn superseded_mid_flight_claim_never_publishes_the_old_version() {
    let world = World::new("supersede");
    let v1 = world.digest_of(b"body v1");
    let v2 = world.digest_of(b"body v2");
    world.seed_doc("d1", &v1);

    // The resolver performs a continuous edit (v2) while the v1 task is
    // mid-flight — after its pre-work renewal, during the embed.
    let table = vec![
        (v1.as_str().to_string(), &b"body v1"[..]),
        (v2.as_str().to_string(), &b"body v2"[..]),
    ];
    let base = digest_resolver(&table);
    let edited = AtomicBool::new(false);
    let edit_resolver = |task: &ClaimedTask| -> cc_model::CcResult<Option<DocumentInput>> {
        if !edited.swap(true, Ordering::SeqCst) {
            world.write_pass("d1", "v2", v2.as_str()); // supersedes the claimed v1 task
        }
        base(task)
    };

    // One drain converges the whole batch: the stale v1 attempt is fenced
    // out at the CAS (its row was flipped to terminal `superseded`, so the
    // fence is `LeaseLost` and the fenced retry no-ops) and the drain moves
    // straight on to the successor v2 task.
    let report = drain_pending(&world.db, "worker", &limits(4), &mut |guard| {
        world.handler(&edit_resolver).handle(guard)
    })
    .unwrap();
    assert_eq!(report.claimed, 2);
    assert_eq!(report.completed, 2);
    assert_eq!(
        world.provider.call_count(),
        2,
        "one wasted embed (v1) + one serving embed (v2)"
    );
    let superseded: i64 = world
        .conn
        .query_row(
            "SELECT COUNT(*) FROM semantic_outbox WHERE doc_key='d1' AND state='superseded'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        superseded, 1,
        "the claimed task was superseded, not retried"
    );
    assert_eq!(
        world.manifest_version("d1").as_deref(),
        Some("v2"),
        "only the final version ever became visible"
    );
    assert_eq!(world.task_state("d1"), "done");
    assert_eq!(
        world.db.reads().read_generation().unwrap().semantic_epoch,
        Some(1),
        "exactly one visible-set change (v1 never published)"
    );

    // The queue is fully consumed.
    let report = drain_pending(&world.db, "worker", &limits(4), &mut |guard| {
        world.handler(&edit_resolver).handle(guard)
    })
    .unwrap();
    assert_eq!(report.claimed, 0);
}

// ── 5. lease expiry mid-work: the stale worker loses its voice ───────────

#[test]
fn expired_and_reclaimed_lease_cannot_renew_but_the_successor_publishes() {
    let world = World::new("expiry");
    let input = world.digest_of(b"body d1");
    world.seed_doc("d1", &input);
    let table = vec![(input.as_str().to_string(), &b"body d1"[..])];
    let resolver = digest_resolver(&table);

    // A worker with an absurdly short lease "crashes" (stops renewing).
    let guard: LeaseGuard<'_> = LeaseGuard::claim(&world.db, "worker-a", 0.05)
        .unwrap()
        .expect("claimed");
    std::thread::sleep(Duration::from_millis(120));

    // Expiry + third-party reclaim → the stale holder's heartbeat is refused
    // (token no longer matches any claimed row) — 过期 worker 无法 ack/renew.
    assert_eq!(world.db.reclaim_expired_semantic().unwrap(), 1);
    assert!(!guard.renew().unwrap());
    drop(guard);

    // The successor drains and publishes; the stale worker did zero provider
    // work for it (skip-at-gate) and no duplicate embed was paid.
    let report = drain_pending(&world.db, "worker-b", &limits(4), &mut |guard| {
        world.handler(&resolver).handle(guard)
    })
    .unwrap();
    assert_eq!(report.claimed, 1);
    assert_eq!(report.completed, 1);
    assert_eq!(report.lease_lost, 0);
    assert_eq!(world.provider.call_count(), 1);
    assert_eq!(world.manifest_version("d1").as_deref(), Some("v1"));
}

// ── 6. bounded admission: one drain claims at most max_batch ─────────────

#[test]
fn one_drain_claims_at_most_max_batch() {
    let world = World::new("bounded");
    let mut table: Vec<(String, &[u8])> = Vec::new();
    for key in ["d1", "d2", "d3"] {
        let input = world.digest_of(key.as_bytes());
        world.seed_doc(key, &input);
        table.push((input.as_str().to_string(), key.as_bytes()));
    }
    let resolver = digest_resolver(&table);

    let report = drain_pending(&world.db, "worker", &limits(2), &mut |guard| {
        world.handler(&resolver).handle(guard)
    })
    .unwrap();
    assert_eq!(report.claimed, 2, "admission bound respected");
    assert_eq!(report.completed, 2);
    assert_eq!(world.live_tasks("d3").len(), 1, "the rest stays pending");
    let epoch_after_first = world.db.reads().read_generation().unwrap();
    assert_eq!(epoch_after_first.semantic_epoch, Some(2));

    // The next drain serves the remainder — consumption bounds the queue.
    let report = drain_pending(&world.db, "worker", &limits(2), &mut |guard| {
        world.handler(&resolver).handle(guard)
    })
    .unwrap();
    assert_eq!(report.claimed, 1);
    assert_eq!(
        world.db.reads().read_generation().unwrap().semantic_epoch,
        Some(3)
    );
    assert_eq!(world.provider.call_count(), 3);
}

// ── 7. retry budget: exhaustion dead-letters, dead tasks stop claiming ───

#[test]
fn retry_budget_exhaustion_dead_letters_without_further_claims() {
    let world = World::new("exhaust");
    let input = world.digest_of(b"body d1");
    world.seed_doc("d1", &input);

    // A task whose embed input can never be resolved: every attempt fails
    // before any queue-state write and is handed back through the fenced
    // retry until the attempt budget is spent.
    let no_input = |_task: &ClaimedTask| -> cc_model::CcResult<Option<DocumentInput>> { Ok(None) };
    let limits = WorkerLimits::validated(1, 60.0, 0.0, 2).expect("limits");

    let report = drain_pending(&world.db, "worker", &limits, &mut |guard| {
        world.handler(&no_input).handle(guard)
    })
    .unwrap();
    assert_eq!(
        report,
        cc_semantic::queue::BatchReport {
            claimed: 1,
            completed: 0,
            retried: 1,
            lease_lost: 0,
        }
    );

    let report = drain_pending(&world.db, "worker", &limits, &mut |guard| {
        world.handler(&no_input).handle(guard)
    })
    .unwrap();
    assert_eq!(report.claimed, 1);
    assert_eq!(report.retried, 1, "second attempt exhausts the budget");
    assert_eq!(
        world.provider.call_count(),
        0,
        "an unresolvable input never reaches the provider"
    );

    // Dead letter: terminal `failed`, never claimable through the queue.
    let report = drain_pending(&world.db, "worker", &limits, &mut |guard| {
        world.handler(&no_input).handle(guard)
    })
    .unwrap();
    assert_eq!(report.claimed, 0);
    assert_eq!(world.task_state("d1"), "failed");
    let attempts: i64 = world
        .conn
        .query_row(
            "SELECT attempt_count FROM semantic_outbox WHERE doc_key='d1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(attempts, 2);
}

// ── limits validation ────────────────────────────────────────────────────

#[test]
fn worker_limits_reject_structurally_meaningless_bounds() {
    assert!(WorkerLimits::validated(0, 60.0, 0.0, 3).is_err());
    assert!(WorkerLimits::validated(1, 0.0, 0.0, 3).is_err());
    assert!(WorkerLimits::validated(1, -1.0, 0.0, 3).is_err());
    assert!(WorkerLimits::validated(1, 60.0, -0.5, 3).is_err());
    assert!(WorkerLimits::validated(1, 60.0, 0.0, 0).is_err());
    let ok = WorkerLimits::validated(4, 60.0, 1.0, 3).expect("valid");
    assert_eq!(ok.renew_period(), 20.0, "heartbeat period = lease_secs/3");
    assert_eq!(ok.claim_order, ClaimFairness::Fifo, "default stays arrival FIFO");
    assert_eq!(
        ok,
        WorkerLimits {
            max_batch: 4,
            lease_secs: 60.0,
            backoff_secs: 1.0,
            max_attempts: 3,
            claim_order: ClaimFairness::Fifo
        }
    );
    assert_eq!(
        ok.with_claim_order(ClaimFairness::DocRoundRobin).claim_order,
        ClaimFairness::DocRoundRobin
    );
}

// ── claim fairness (P7-005, 接线轮待办 8) ─────────────────────────────────

#[test]
fn drain_claim_order_defaults_to_fifo_and_rotation_is_opt_in() {
    let no_input = |task: &ClaimedTask| {
        if task.doc_key == "doc-a" {
            Ok(None) // unresolvable: the attempt is handed back through the fenced retry
        } else {
            Ok(Some(DocumentInput::from_bytes(b"doc-b body")?))
        }
    };

    // FIFO (default): after doc-a's retry refreshes its updated_at, the next
    // drain still claims the oldest task_id — doc-a — unchanged semantics.
    let world = World::new("fairness-fifo");
    world.seed_doc("doc-a", &InputDigest::of_input(b"doc-a body").unwrap());
    world.seed_doc("doc-b", &InputDigest::of_input(b"doc-b body").unwrap());
    let report = drain_pending(&world.db, "worker", &limits(1), &mut |guard| {
        world.handler(&no_input).handle(guard)
    })
    .unwrap();
    assert_eq!((report.claimed, report.retried), (1, 1), "doc-a is served first");
    let report = drain_pending(&world.db, "worker", &limits(1), &mut |guard| {
        world.handler(&no_input).handle(guard)
    })
    .unwrap();
    assert_eq!(
        (report.claimed, report.retried),
        (1, 1),
        "FIFO ignores the fresh retry timestamp: doc-a again"
    );
    assert_eq!(world.task_state("doc-b"), "pending");

    // Rotation (opt-in): the same shape, but the retried doc-a now has the
    // freshest updated_at, so the next drain serves doc-b instead.
    let world = World::new("fairness-rotation");
    world.seed_doc("doc-a", &InputDigest::of_input(b"doc-a body").unwrap());
    world.seed_doc("doc-b", &InputDigest::of_input(b"doc-b body").unwrap());
    let rotation = limits(1).with_claim_order(ClaimFairness::DocRoundRobin);
    let report = drain_pending(&world.db, "worker", &rotation, &mut |guard| {
        world.handler(&no_input).handle(guard)
    })
    .unwrap();
    assert_eq!((report.claimed, report.retried), (1, 1), "doc-a first (only stale rows)");
    let report = drain_pending(&world.db, "worker", &rotation, &mut |guard| {
        world.handler(&no_input).handle(guard)
    })
    .unwrap();
    assert_eq!(
        (report.claimed, report.completed),
        (1, 1),
        "rotation skips the freshly touched doc-a and serves doc-b"
    );
    assert_eq!(world.task_state("doc-b"), "done");
    assert_eq!(
        world.task_state("doc-a"),
        "pending",
        "the hot doc yields; its task is untouched by this drain"
    );
}
