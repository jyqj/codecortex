//! Self-authored finite synthetic fixtures under authorized /tmp; no fault/GC/WAL tests.
use cc_db::{
    index_db::IndexDb,
    semantic_outbox::{supersede_and_enqueue_on, OutboxPlan, OutboxUpsert},
    semantic_publish::LifecycleFence,
};
use cc_model::{CcError, CcResult};
use cc_semantic::{
    cache::ArtifactCache,
    ports::{DocumentInput, EmbeddingProvider, ProviderError, QueryInput},
    providers::fake::{FakeProvider, FakeProviderConfig},
    publish::Publisher,
    queue::{
        drain_pending_parallel_with_lifecycle as drain, EmbedHandler, LeaseGuard, TaskExit,
        WorkerLimits,
    },
    spec::{DocumentEncodingSpec, VectorSpace},
    types::DocSpecDigest,
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Condvar, Mutex,
    },
    time::Duration,
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    root: PathBuf,
    db: Arc<IndexDb>,
    cache: ArtifactCache,
    spec: DocumentEncodingSpec,
    digest: DocSpecDigest,
    input: DocumentInput,
}
impl Fixture {
    fn new(n: usize) -> Self {
        let root = PathBuf::from("/tmp").join(format!(
            "cc-bounded-authorized-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let db = Arc::new(IndexDb::open(&root.join("index.db")).unwrap().0);
        let space = VectorSpace::new("fake/bounded", 2).unwrap();
        let spec = DocumentEncodingSpec::new(space, None, 8192, "fake-tokenizer").unwrap();
        cc_semantic::space_switch::register_backfill_space(&db, &spec).unwrap();
        cc_semantic::space_switch::activate_space(&db, spec.space(), "synthetic initial").unwrap();
        let input = DocumentInput::from_bytes(b"finite synthetic input").unwrap();
        let cache = ArtifactCache::open(
            root.join("cache"),
            format!("ns-{}", NEXT.load(Ordering::Relaxed)),
        )
        .unwrap();
        let f = Self {
            root,
            db,
            cache,
            digest: spec.digest().unwrap(),
            spec,
            input,
        };
        for i in 0..n {
            f.seed(i, "v1", true);
        }
        f
    }
    fn seed(&self, i: usize, version: &str, fresh: bool) {
        let c = rusqlite::Connection::open(self.db.admin().db_path()).unwrap();
        if fresh {
            c.execute("INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) VALUES(?1,'rust','hash',1,1,'2026-01-01')",[format!("src/d{i}.rs")]).unwrap();
            c.execute("INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text) VALUES(?1,?2,'rust',0,1,2,'body')",rusqlite::params![format!("c{i}"),format!("src/d{i}.rs")]).unwrap();
        }
        c.execute("INSERT OR REPLACE INTO document_manifest(doc_key,doc_version,file_path,chunk_id,encoding_key,reference_json,record_json) VALUES(?1,?2,?3,?4,'enc','{}',?5)",rusqlite::params![format!("d{i}"),version,format!("src/d{i}.rs"),format!("c{i}"),format!("{{\"input\":{{\"input_hash\":\"{}\"}}}}",self.input.input_digest.as_str())]).unwrap();
        supersede_and_enqueue_on(
            &c,
            &OutboxPlan {
                upserts: &[OutboxUpsert {
                    doc_key: format!("d{i}"),
                    doc_version: version.into(),
                    input_digest: self.input.input_digest.as_str().into(),
                }],
                removals: &[],
                now_unix: 900.0,
            },
        )
        .unwrap();
    }
    fn limits(&self, n: usize) -> WorkerLimits {
        WorkerLimits::validated(n, 60.0, 30.0, 3).unwrap()
    }
    fn handle(
        &self,
        p: &dyn EmbeddingProvider,
        g: &LeaseGuard<'_>,
        f: Option<&LifecycleFence>,
    ) -> CcResult<TaskExit> {
        let incarnation = self.db.reads().read_generation()?.incarnation;
        let publisher = Publisher::new(
            &self.db,
            &self.cache,
            self.spec.space(),
            &self.digest,
            incarnation,
        )?
        .with_lifecycle(f);
        EmbedHandler::new(publisher, p, &|_| Ok(Some(self.input.clone()))).handle(g)
    }
    fn rows(&self) -> Vec<(i64, String, u32)> {
        let c = self.db.read_conn().unwrap();
        let mut s = c
            .prepare("SELECT task_id,state,attempt_count FROM semantic_outbox ORDER BY task_id")
            .unwrap();
        s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
#[derive(Default)]
struct Hold {
    state: Mutex<(usize, bool)>,
    wake: Condvar,
}
impl Hold {
    fn enter(&self) {
        let mut s = self.state.lock().unwrap();
        s.0 += 1;
        self.wake.notify_all();
        while !s.1 {
            let (next, timeout) = self.wake.wait_timeout(s, Duration::from_secs(5)).unwrap();
            s = next;
            assert!(!timeout.timed_out(), "finite synthetic hold exceeded");
        }
    }
    fn wait(&self, n: usize) {
        let mut s = self.state.lock().unwrap();
        while s.0 < n {
            let (next, timeout) = self.wake.wait_timeout(s, Duration::from_secs(5)).unwrap();
            s = next;
            assert!(!timeout.timed_out(), "workers failed to enter");
        }
    }
    fn release(&self) {
        self.state.lock().unwrap().1 = true;
        self.wake.notify_all();
    }
}
struct Release<'a>(&'a Hold);
impl Drop for Release<'_> {
    fn drop(&mut self) {
        self.0.release();
    }
}
struct Provider {
    fake: FakeProvider,
    active: AtomicUsize,
    peak: AtomicUsize,
    calls: AtomicUsize,
    hold: Option<Arc<Hold>>,
    fail: bool,
}
impl Provider {
    fn new(f: &Fixture, hold: Option<Arc<Hold>>, fail: bool) -> Self {
        Self {
            fake: FakeProvider::new(FakeProviderConfig::new(f.spec.space().clone())),
            active: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            calls: AtomicUsize::new(0),
            hold,
            fail,
        }
    }
}
impl EmbeddingProvider for Provider {
    fn space(&self) -> &VectorSpace {
        self.fake.space()
    }
    fn embed_documents(&self, input: &[DocumentInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(active, Ordering::SeqCst);
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(hold) = &self.hold {
            hold.enter();
        } else {
            std::thread::sleep(Duration::from_millis(3));
        }
        let r = if self.fail {
            Err(ProviderError::ServerError)
        } else {
            self.fake.embed_documents(input)
        };
        self.active.fetch_sub(1, Ordering::SeqCst);
        r
    }
    fn embed_queries(&self, input: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        self.fake.embed_queries(input)
    }
}
#[test]
fn width_zero_and_one_are_serial_two_and_larger_are_bounded_and_fifo_budget_is_shared() {
    for (width, peak) in [(0, 1), (1, 1), (2, 4), (4, 4), (usize::MAX, 4)] {
        let f = Fixture::new(40);
        let h = Arc::new(Hold::default());
        let p = Provider::new(&f, Some(h.clone()), false);
        let seen = Mutex::new(Vec::new());
        std::thread::scope(|s| {
            let _release = Release(&h);
            let job = s.spawn(|| {
                drain(&f.db, "finite", &f.limits(16), None, width, &|g| {
                    seen.lock()
                        .unwrap()
                        .push((g.task().task_id, g.task().token.clone()));
                    f.handle(&p, g, None)
                })
            });
            h.wait(peak);
            f.db.reads().read_generation().unwrap();
            f.db.enqueue_semantic_rebuild_plan(&[]).unwrap();
            h.release();
            let report = job.join().unwrap().unwrap();
            assert_eq!(report.claimed, 16);
            assert_eq!(report.completed, 16);
        });
        assert_eq!(p.calls.load(Ordering::SeqCst), 16);
        assert_eq!(p.peak.load(Ordering::SeqCst), peak);
        assert_eq!(p.active.load(Ordering::SeqCst), 0);
        let mut seen = seen.into_inner().unwrap();
        seen.sort();
        assert_eq!(
            seen.iter().map(|r| r.0).collect::<Vec<_>>(),
            (1..=16).collect::<Vec<_>>()
        );
        let tokens: std::collections::HashSet<_> = seen.iter().map(|r| &r.1).collect();
        assert_eq!(tokens.len(), 16);
        assert!(f.rows()[..16].iter().all(|r| r.1 == "done" && r.2 == 1));
        assert!(f.rows()[16..].iter().all(|r| r.1 == "pending" && r.2 == 0));
        println!("bounded width={width} provider_peak={peak} claims=16 unique_tokens=16 done=16 pending=24");
    }
}
#[test]
fn close_joins_four_started_attempts_and_restores_pending_with_spent_attempts() {
    let f = Fixture::new(20);
    let fence = LifecycleFence::default();
    let h = Arc::new(Hold::default());
    let p = Provider::new(&f, Some(h.clone()), false);
    std::thread::scope(|s| {
        let _release = Release(&h);
        let job = s.spawn(|| {
            drain(&f.db, "close", &f.limits(16), Some(&fence), 2, &|g| {
                f.handle(&p, g, Some(&fence))
            })
        });
        h.wait(4);
        fence.close();
        assert_eq!(p.active.load(Ordering::SeqCst), 4);
        h.release();
        let r = job.join().unwrap().unwrap();
        assert_eq!(r.claimed, 4);
        assert_eq!(r.completed, 0);
    });
    assert_eq!(p.active.load(Ordering::SeqCst), 0);
    assert!(f.rows()[..4].iter().all(|r| r.1 == "pending" && r.2 == 1));
    assert!(f.rows()[4..].iter().all(|r| r.2 == 0));
    println!("close joined=4 provider_peak=4 started_attempts=4 subsequent_claims=0");
}
#[test]
fn unstarted_cancel_does_not_consume_attempt_and_preclosed_has_zero_calls() {
    let f = Fixture::new(20);
    let fence = LifecycleFence::default();
    let p = Provider::new(&f, None, false);
    let r = drain(&f.db, "unstarted", &f.limits(16), Some(&fence), 1, &|_| {
        fence.close();
        Ok(TaskExit::Cancelled { started: false })
    })
    .unwrap();
    assert_eq!(r.claimed, 1);
    assert!(f.rows().iter().all(|r| r.1 == "pending" && r.2 == 0));
    let r = drain(&f.db, "preclosed", &f.limits(16), Some(&fence), 2, &|g| {
        f.handle(&p, g, Some(&fence))
    })
    .unwrap();
    assert_eq!(r.claimed, 0);
    assert_eq!(p.calls.load(Ordering::SeqCst), 0);
}
#[test]
fn all_four_parallel_errors_propagate_after_join_and_retry_each_independent_token() {
    let f = Fixture::new(20);
    let h = Arc::new(Hold::default());
    let error = std::thread::scope(|s| {
        let _release = Release(&h);
        let job = s.spawn(|| {
            drain(&f.db, "errors", &f.limits(16), None, 2, &|g| {
                h.enter();
                Err(CcError::InvalidParams(format!(
                    "synthetic error {}",
                    g.task().task_id
                )))
            })
        });
        h.wait(4);
        h.release();
        job.join().unwrap().unwrap_err()
    });
    let text = error.to_string();
    assert!(
        (1..=4).all(|id| text.contains(&format!("synthetic error {id}"))),
        "{text}"
    );
    assert!(f.rows()[..4].iter().all(|r| r.1 == "pending" && r.2 == 1));
    assert!(f.rows()[4..].iter().all(|r| r.2 == 0));
}
#[test]
fn closed_lifecycle_during_handled_retry_still_stops_claims() {
    let f = Fixture::new(4);
    let fence = LifecycleFence::default();
    let report = drain(
        &f.db,
        "retry-close",
        &f.limits(16),
        Some(&fence),
        1,
        &|_| {
            fence.close();
            Ok(TaskExit::NeedsRetry {
                reason: "ordinary late provider timeout".into(),
            })
        },
    )
    .unwrap();
    assert_eq!(report.claimed, 1);
    assert_eq!(report.retried, 0);
    assert!(f.rows().iter().all(|r| r.1 == "pending"));
    assert_eq!(f.rows()[0].2, 1);
    assert!(f.rows()[1..].iter().all(|r| r.2 == 0));
}
#[test]
fn handled_retry_keeps_shared_budget_and_independent_tokens() {
    for width in [0, 1, 2, 4] {
        let f = Fixture::new(20);
        let bad = Provider::new(&f, None, true);
        let good = Provider::new(&f, None, false);
        let seen = Mutex::new(Vec::new());
        let handler = |g: &LeaseGuard<'_>| {
            seen.lock()
                .unwrap()
                .push((g.task().task_id, g.task().token.clone()));
            f.handle(if g.task().task_id == 1 { &bad } else { &good }, g, None)
        };
        let first = drain(
            &f.db,
            "retry-progress",
            &f.limits(16),
            None,
            width,
            &handler,
        )
        .unwrap();
        assert_eq!(first.claimed, 16);
        assert_eq!(first.retried, 1);
        assert_eq!(first.completed, 15);
        assert_eq!(bad.calls.load(Ordering::SeqCst), 1);
        assert_eq!(good.calls.load(Ordering::SeqCst), 15);
        assert_eq!(f.rows()[0].1, "pending");
        assert_eq!(f.rows()[0].2, 1);
        assert!(f.rows()[16..].iter().all(|r| r.1 == "pending" && r.2 == 0));
        let second = drain(
            &f.db,
            "retry-progress-next",
            &f.limits(16),
            None,
            width,
            &handler,
        )
        .unwrap();
        assert_eq!(second.claimed, 4);
        assert_eq!(second.completed, 4);
        assert_eq!(second.retried, 0);
        assert_eq!(
            bad.calls.load(Ordering::SeqCst),
            1,
            "backoff is not immediately retried"
        );
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 20);
        assert_eq!(
            seen.iter()
                .map(|r| r.0)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            20
        );
        assert_eq!(
            seen.iter()
                .map(|r| &r.1)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            20
        );
        let conn = f.db.read_conn().unwrap();
        let ready: i64 = conn.query_row("SELECT COUNT(*) FROM semantic_outbox WHERE state='pending' AND available_at<=unixepoch('now','subsec')", [], |r| r.get(0)).unwrap();
        assert_eq!(ready, 0);
        assert!(f.rows().iter().all(|r| r.2 == 1));
        println!("retry progress width={width} shared_claims=16+4 unique_tokens=20 retried=1 ready_stranded=0");
    }
}
#[test]
fn ordinary_provider_failure_retries_within_the_shared_finite_budget() {
    let f = Fixture::new(20);
    let h = Arc::new(Hold::default());
    let p = Provider::new(&f, Some(h.clone()), true);
    let r = std::thread::scope(|s| {
        let _release = Release(&h);
        let job = s.spawn(|| {
            drain(&f.db, "provider-error", &f.limits(16), None, 2, &|g| {
                f.handle(&p, g, None)
            })
        });
        h.wait(2);
        h.release();
        job.join().unwrap().unwrap()
    });
    assert_eq!(r.claimed, 16);
    assert_eq!(r.retried, 16);
    assert_eq!(p.calls.load(Ordering::SeqCst), 16);
    assert!(f.rows()[..16].iter().all(|r| r.1 == "pending" && r.2 == 1));
    assert!(f.rows()[16..].iter().all(|r| r.2 == 0));
}
#[test]
fn ordinary_space_switch_fences_started_publications_and_stops_claiming_new_space() {
    let f = Fixture::new(20);
    let h = Arc::new(Hold::default());
    let p = Provider::new(&f, Some(h.clone()), false);
    let r = std::thread::scope(|s| {
        let _release = Release(&h);
        let job = s.spawn(|| {
            drain(&f.db, "switch", &f.limits(16), None, 2, &|g| {
                f.handle(&p, g, None)
            })
        });
        h.wait(4);
        let spec = DocumentEncodingSpec::new(
            VectorSpace::new("fake/new-space", 2).unwrap(),
            None,
            8192,
            "fake-tokenizer",
        )
        .unwrap();
        cc_semantic::space_switch::register_backfill_space(&f.db, &spec).unwrap();
        cc_semantic::space_switch::activate_space(&f.db, spec.space(), "ordinary synthetic switch")
            .unwrap();
        h.release();
        job.join().unwrap().unwrap()
    });
    assert_eq!(r.claimed, 4);
    assert_eq!(p.calls.load(Ordering::SeqCst), 4);
    assert_eq!(
        f.db.read_conn()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM semantic_manifest", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[test]
fn ordinary_document_edit_fences_old_attempt_while_neighbor_publishes() {
    let f = Fixture::new(2);
    let h = Arc::new(Hold::default());
    let p = Provider::new(&f, Some(h.clone()), false);
    std::thread::scope(|s| {
        let _release = Release(&h);
        let job = s.spawn(|| {
            drain(&f.db, "edit", &f.limits(2), None, 2, &|g| {
                f.handle(&p, g, None)
            })
        });
        h.wait(2);
        f.seed(0, "v2", false);
        h.release();
        job.join().unwrap().unwrap();
    });
    let c = f.db.read_conn().unwrap();
    let count: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM semantic_manifest WHERE doc_key='d0' AND doc_version='v1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    let v: String = c
        .query_row(
            "SELECT doc_version FROM semantic_manifest WHERE doc_key='d1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(v, "v1");
    let attempts: u32 = c
        .query_row(
            "SELECT attempt_count FROM semantic_outbox WHERE doc_key='d0' AND doc_version='v2'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(attempts, 0);
}

#[test]
fn normal_enqueue_during_one_inflight_attempt_is_drained_despite_peer_empty_claim() {
    let f = Fixture::new(1);
    let h = Arc::new(Hold::default());
    let p = Provider::new(&f, Some(h.clone()), false);
    let report = std::thread::scope(|scope| {
        let _release = Release(&h);
        let job = scope.spawn(|| {
            drain(&f.db, "normal-enqueue", &f.limits(16), None, 2, &|g| {
                f.handle(&p, g, None)
            })
        });
        h.wait(1);
        // The queue contains only the held claim; another worker's None branch
        // exits locally without consuming the shared budget or setting stop.
        assert_eq!(f.rows(), vec![(1, "claimed".into(), 1)]);
        f.seed(1, "v1", true); // ordinary enqueue, no clock/fault/backoff changes.
        h.release();
        job.join().unwrap().unwrap()
    });
    assert_eq!(report.claimed, 2);
    assert_eq!(report.completed, 2);
    assert!(report.claimed < 16);
    assert!(f.rows().iter().all(|r| r.1 == "done" && r.2 == 1));
    assert_eq!(p.calls.load(Ordering::SeqCst), 2);
    println!("bounded partial drain initial_ready=1 normal_enqueued=1 claimed=2 completed=2 ready_stranded=0 empty_claim_budget=0");
}

// New candidate checks use only normal claim/publish/reclaim APIs and synthetic data.
#[test]
fn pipeline_width_is_capped_by_small_claim_budget() {
    for batch in [1, 2, 3] {
        let f = Fixture::new(8);
        let h = Arc::new(Hold::default());
        let p = Provider::new(&f, Some(h.clone()), false);
        std::thread::scope(|scope| {
            let _release = Release(&h);
            let job = scope.spawn(|| {
                drain(
                    &f.db,
                    "small-budget",
                    &f.limits(batch),
                    None,
                    usize::MAX,
                    &|g| f.handle(&p, g, None),
                )
            });
            h.wait(batch);
            assert_eq!(p.active.load(Ordering::SeqCst), batch);
            h.release();
            let report = job.join().unwrap().unwrap();
            assert_eq!((report.claimed, report.completed), (batch, batch));
        });
        assert_eq!(p.peak.load(Ordering::SeqCst), batch);
    }
}

#[test]
#[ignore = "parent ruling: stronger strict-clock policy is not the existing token-fencing contract; original failed result retained in checkpoint"]
fn slow_provider_beyond_lease_cannot_publish_and_old_tokens_cannot_ack() {
    let f = Fixture::new(4);
    let h = Arc::new(Hold::default());
    let p = Provider::new(&f, Some(h.clone()), false);
    let tokens = Mutex::new(Vec::new());
    let limits = WorkerLimits::validated(4, 0.1, 30.0, 3).unwrap();
    std::thread::scope(|scope| {
        let _release = Release(&h);
        let job = scope.spawn(|| {
            drain(&f.db, "slow", &limits, None, 2, &|g| {
                tokens
                    .lock()
                    .unwrap()
                    .push((g.task().task_id, g.task().token.clone()));
                f.handle(&p, g, None)
            })
        });
        h.wait(4);
        std::thread::sleep(Duration::from_millis(200));
        h.release();
        assert_eq!(job.join().unwrap().unwrap().claimed, 4);
    });
    let conn = f.db.read_conn().unwrap();
    let publications: i64 = conn
        .query_row("SELECT COUNT(*) FROM semantic_manifest", [], |r| r.get(0))
        .unwrap();
    assert_eq!(publications, 0);
    drop(conn);
    f.db.reclaim_expired_semantic().unwrap();
    for (id, token) in tokens.into_inner().unwrap() {
        assert!(!f.db.renew_semantic_lease(id, &token, 60.0).unwrap());
    }
    assert!(f.rows().iter().all(|r| r.1 == "pending" && r.2 == 1));
}

#[test]
fn frozen_input_byte_budget_is_not_increased_by_pipeline() {
    let max = cc_semantic::spec::MAX_INPUT_BYTES;
    assert_eq!(max, 1_048_576);
    assert!(DocumentInput::from_bytes(&vec![b'x'; max]).is_ok());
    assert!(DocumentInput::from_bytes(&vec![b'x'; max + 1]).is_err());
}
