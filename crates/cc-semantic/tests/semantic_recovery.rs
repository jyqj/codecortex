//! P6-015 integration: 崩溃恢复扫描，端到端（kill 模拟）。
//!
//! 覆盖 cc-db/cc-semantic 门面测试看不到的部分：
//!
//! 1. **kill 模拟 + 有界多轮收敛**：worker 认领后进程死亡（P6-013 定案的
//!    Drop 语义 = 零 DB I/O，即 kill 的库内等价物）+ lease 时限被推入过去
//!    （时间流逝的确定性等价物）→ 残量按 `scan_batch` 分多轮 reclaim 收敛，
//!    恢复路径全程 0 provider 调用，worker 续跑恰好一次付费；
//! 2. **put 后 CAS 前崩溃的恢复发布**：artifact 已 durable、manifest 缺失 →
//!    recovery 以已存 artifact 优先复用重放发布（0 provider）；等值重放被
//!    Q4 幂等吸收（visible_set_changed=false，零 bump）——P6-011 "CAS 后
//!    残态不可达 + 幂等重放吸收"的恢复侧验证；
//! 3. **死信口径**：failed 终态只清点不复活；
//! 4. **换库 fence**：幽灵快照的 recover 零写入；
//! 5. **半途 reconcile 的续跑**：P6-014 重入队后 reuse 循环被打断，recovery
//!    续完，付费向量全程只付一次。
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use cc_db::index_db::IndexDb;
use cc_db::semantic_manifest_reads::SemanticManifestReads;
use cc_db::semantic_outbox::{supersede_and_enqueue_on, ClaimedTask, OutboxPlan, OutboxUpsert};
use cc_model::retrieval::HardScope;
use cc_semantic::cache::ArtifactCache;
use cc_semantic::ports::{DocumentInput, EmbeddingProvider};
use cc_semantic::providers::fake::{FakeProvider, FakeProviderConfig};
use cc_semantic::publish::Publisher;
use cc_semantic::queue::{drain_pending, EmbedHandler, LeaseGuard, WorkerLimits};
use cc_semantic::recovery::{recover_scan, RecoveryOptions, RecoveryReport, RecoveryVerdict};
use cc_semantic::spec::{DocumentEncodingSpec, VectorSpace};
use cc_semantic::types::{DocSpecDigest, InputDigest};
use cc_semantic::vector::exact::{search, space_manifest_reads, ExactSearch};

// ── harness（P6-014 reconcile 测试的同构世界） ───────────────────────────

static SEQ: AtomicU32 = AtomicU32::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "cc-semantic-p6015-{tag}-{}-{n}",
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
    VectorSpace::new("fake/model-recovery", 2).expect("space")
}

fn doc_spec(space: &VectorSpace) -> DocSpecDigest {
    DocumentEncodingSpec::new(space.clone(), None, 8_192, "fake-tokenizer")
        .expect("doc spec")
        .digest()
        .expect("doc spec digest")
}

fn options(scan_batch: usize) -> RecoveryOptions {
    RecoveryOptions {
        scan_batch,
        lease_secs: 60.0,
        retry_backoff_secs: 0.0, // 测试内即时可再 claim（revisit guard 兜底）
        // cache miss hand-back 已切换为不计 attempt 的直写原语（批次 3 收口
        // ②），不再消耗预算；revisit guard / revoke op 的 fenced retry 仍按
        // 旧口径走 attempt 记账，大预算隔离收敛性断言与该残余语义。
        max_attempts: 64,
    }
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
        let conn = fresh_conn(db.admin().db_path());
        let space = space();
        let spec = doc_spec(&space);
        let cache = ArtifactCache::open(dir.0.join("cache"), format!("ns-{tag}")).expect("cache");
        let provider = FakeProvider::new(FakeProviderConfig::new(space.clone()));
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

    fn write_projection(&self, doc_key: &str, input_digest: &str) {
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
        self.conn
            .execute(
                "INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,\
                 encoding_key,reference_json,record_json) \
                 VALUES(?1,'v1',?2,?3,'enc','{}',?4)",
                rusqlite::params![
                    doc_key,
                    format!("src/{doc_key}.rs"),
                    format!("c-{doc_key}"),
                    format!("{{\"input\":{{\"input_hash\":\"{input_digest}\"}}}}")
                ],
            )
            .unwrap();
    }

    /// 活活世界的一个文档：投影 + 期望任务（P6-006 写路径口径，含 outbox）。
    fn seed_doc(&self, doc_key: &str, input: &InputDigest) {
        self.write_projection(doc_key, input.as_str());
        let stats = supersede_and_enqueue_on(
            &self.conn,
            &OutboxPlan {
                upserts: &[OutboxUpsert {
                    doc_key: doc_key.into(),
                    doc_version: "v1".into(),
                    input_digest: input.as_str().into(),
                }],
                removals: &[],
                now_unix: 900.0,
            },
        )
        .unwrap();
        assert_eq!(stats.enqueued, 1);
    }

    fn activate_space(&self) {
        self.conn
            .execute(
                "INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES(?1,'{}','active')",
                [self.space.digest().unwrap().as_str()],
            )
            .unwrap();
    }

    /// 换库：完整 rebuild 协议。文档投影由重建闭包恢复；语义表按协议从零。
    fn rebuild(&mut self, docs: &[(&str, &InputDigest)]) {
        self.conn = fresh_conn_placeholder(); // 先释放旧 inode 连接
        self.db
            .admin()
            .rebuild_with_temp_db(|tmp| {
                for (doc_key, input) in docs {
                    write_into(tmp, doc_key, input.as_str())?;
                }
                Ok(())
            })
            .expect("rebuild swap");
        self.conn = fresh_conn(self.db.admin().db_path());
    }

    fn incarnation(&self) -> [u8; 16] {
        self.db.reads().read_generation().unwrap().incarnation
    }

    fn semantic_epoch(&self) -> Option<u64> {
        self.db.reads().read_generation().unwrap().semantic_epoch
    }

    fn limits(&self) -> WorkerLimits {
        WorkerLimits::validated(16, 60.0, 0.0, 3).expect("limits")
    }

    fn drain(&self, table: &[(String, &'static [u8])]) {
        drain_pending(&self.db, "worker", &self.limits(), &mut |guard| {
            embed_once(self, guard, table)
        })
        .expect("drain");
    }

    /// kill 模拟的第一半：认领后不做任何处置地丢弃 guard（P6-013 定案的
    /// Drop 语义 = 零 DB I/O，即进程死亡在库内的等价物：任务留在 claimed）。
    fn kill_a_claim(&self, owner: &str) -> ClaimedTask {
        let guard = LeaseGuard::claim(&self.db, owner, 60.0)
            .expect("claim")
            .expect("a task to kill");
        let task = guard.task().clone();
        drop(guard); // kill：不 ack、不 retry、零写入
        task
    }

    /// kill 模拟的第二半：把该任务的 lease 时限推入过去（wall-clock 时间
    /// 流逝的确定性等价物），使孤儿进入 reclaim 的可观测窗口。
    fn forge_expired_lease(&self, task_id: i64) {
        self.conn
            .execute(
                "UPDATE semantic_outbox SET lease_expires_at=1.0 WHERE task_id=?1",
                rusqlite::params![task_id],
            )
            .unwrap();
    }

    fn recover(&self, opts: &RecoveryOptions) -> RecoveryReport {
        match recover_scan(
            &self.db,
            &self.cache,
            &self.space,
            &self.spec,
            self.incarnation(),
            opts,
        )
        .expect("recover")
        {
            RecoveryVerdict::Applied(report) => report,
            RecoveryVerdict::Fenced { .. } => panic!("unexpected fence in a live world"),
        }
    }

    fn manifest_count(&self) -> i64 {
        self.conn
            .query_row("SELECT COUNT(*) FROM semantic_manifest", [], |r| r.get(0))
            .unwrap()
    }

    fn outbox_state(&self, doc_key: &str) -> String {
        self.conn
            .query_row(
                "SELECT state FROM semantic_outbox WHERE doc_key=?1",
                [doc_key],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn outbox_count(&self) -> i64 {
        self.conn
            .query_row("SELECT COUNT(*) FROM semantic_outbox", [], |r| r.get(0))
            .unwrap()
    }
}

fn embed_once(
    world: &World,
    guard: &LeaseGuard<'_>,
    table: &[(String, &'static [u8])],
) -> cc_model::CcResult<cc_semantic::queue::TaskExit> {
    let publisher = Publisher::new(
        &world.db,
        &world.cache,
        &world.space,
        &world.spec,
        world.incarnation(),
    )
    .expect("publisher");
    let provider = &world.provider;
    let resolver = |task: &ClaimedTask| {
        table
            .iter()
            .find(|(digest, _)| *digest == task.input_digest)
            .map(|(_, bytes)| DocumentInput::from_bytes(bytes))
            .transpose()
    };
    let handler = EmbedHandler::new(publisher, provider, &resolver);
    handler.handle(guard)
}

fn fresh_conn(path: &std::path::Path) -> rusqlite::Connection {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.busy_timeout(Duration::from_secs(5)).unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    conn
}

fn fresh_conn_placeholder() -> rusqlite::Connection {
    rusqlite::Connection::open_in_memory().unwrap()
}

/// 重建闭包内的投影写入（staging 连接上的同构 SQL）。
fn write_into(
    tmp: &rusqlite::Connection,
    doc_key: &str,
    input_digest: &str,
) -> cc_model::CcResult<()> {
    tmp.execute(
        "INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) \
         VALUES(?1,'rust','hash',1.0,1,'2026-01-01')",
        [format!("src/{doc_key}.rs")],
    )
    .map_err(|e| cc_model::CcError::Database(format!("{e}")))?;
    tmp.execute(
        "INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text) \
         VALUES(?1,?2,'rust',0,1,2,'body')",
        rusqlite::params![format!("c-{doc_key}"), format!("src/{doc_key}.rs")],
    )
    .map_err(|e| cc_model::CcError::Database(format!("{e}")))?;
    tmp.execute(
        "INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,\
         encoding_key,reference_json,record_json) \
         VALUES(?1,'v1',?2,?3,'enc','{}',?4)",
        rusqlite::params![
            doc_key,
            format!("src/{doc_key}.rs"),
            format!("c-{doc_key}"),
            format!("{{\"input\":{{\"input_hash\":\"{input_digest}\"}}}}")
        ],
    )
    .map_err(|e| cc_model::CcError::Database(format!("{e}")))?;
    Ok(())
}

fn digest(bytes: &'static [u8]) -> InputDigest {
    InputDigest::of_input(bytes).expect("input digest")
}

fn digest_table(bytes: &'static [u8]) -> Vec<(String, &'static [u8])> {
    vec![(digest(bytes).as_str().to_string(), bytes)]
}

fn retrieval_hit(world: &World, input: &InputDigest) -> Option<String> {
    let reads = SemanticManifestReads::on(&world.conn);
    let space_digest = world.space.digest().expect("space digest");
    let source = space_manifest_reads(&reads, &space_digest);
    let query_vector = match world.cache.get(&world.space, input, &world.spec).unwrap() {
        cc_semantic::cache::CacheRead::Hit(v) => v.data,
        _ => panic!("query vector must come from the paid artifact"),
    };
    let hits = search(
        &world.cache,
        &source,
        ExactSearch {
            space: &world.space,
            query: &query_vector,
            filter: &HardScope {
                path_prefix: None,
                languages: None,
                file_paths: None,
            },
            k: 10,
            batch_rows: 4,
        },
    )
    .expect("search");
    hits.first().map(|h| h.doc_key.clone())
}

// ── 1. kill 模拟：有界多轮 reclaim 收敛，恢复零付费，worker 恰一次付费 ────

#[test]
fn killed_worker_leases_reclaim_bounded_and_converge_without_paying() {
    let world = World::new("kill-bounded");
    let docs: Vec<(String, InputDigest)> = ["b1", "b2", "b3", "b4", "b5"]
        .iter()
        .map(|k| {
            let bytes: &'static [u8] =
                Box::leak(format!("body {k}").into_bytes().into_boxed_slice());
            (k.to_string(), digest(bytes))
        })
        .collect();
    world.activate_space();
    for (key, input) in &docs {
        world.seed_doc(key, input);
    }

    // kill 模拟：5 个 worker 各认领一个任务后死亡，时间流逝使 lease 全部过期。
    for (key, _) in &docs {
        let task = world.kill_a_claim("dead-worker");
        assert_eq!(&task.doc_key, key);
        world.forge_expired_lease(task.task_id);
    }
    assert_eq!(world.provider.call_count(), 0);

    // 有界多轮收敛：scan_batch=2，残量 5 → 多轮驱动至 converged。
    let opts = options(2);
    let mut total_reclaimed = 0usize;
    let mut rounds = 0usize;
    loop {
        assert!(rounds < 16, "recovery must converge in bounded rounds");
        let report = world.recover(&opts);
        assert!(
            report.reclaimed <= 2,
            "one call reclaims at most scan_batch rows"
        );
        assert_eq!(report.replayed, 0, "nothing was ever embedded: no replays");
        assert_eq!(report.dead_letters, 0);
        total_reclaimed += report.reclaimed;
        rounds += 1;
        if report.converged {
            break;
        }
    }
    assert_eq!(total_reclaimed, 5, "every orphaned lease came back");
    assert!(
        rounds >= 3,
        "存量超过批次上限时分多轮收敛（实际 {rounds} 轮）"
    );
    assert_eq!(
        world.provider.call_count(),
        0,
        "recovery never pays: reclaimed work waits for the worker"
    );

    // worker 续跑：恰一次付费，全部收敛为 done + 可见。
    let table: Vec<(String, &'static [u8])> = docs
        .iter()
        .map(|(key, input)| {
            let bytes: &'static [u8] =
                Box::leak(format!("body {key}").into_bytes().into_boxed_slice());
            (input.as_str().to_string(), bytes)
        })
        .collect();
    world.drain(&table);
    assert_eq!(
        world.provider.call_count(),
        5,
        "worker pays exactly once per document, no duplicate embeds"
    );
    assert_eq!(world.manifest_count(), 5);
    for (key, _) in &docs {
        assert_eq!(world.outbox_state(key), "done");
    }
}

// ── 1b. cache miss hand-back：不烧 attempt 预算（批次 3 收口原语②） ────────

#[test]
fn cache_miss_hand_back_spends_no_attempt_budget() {
    let world = World::new("miss-handback");
    let bytes: &'static [u8] = b"body miss-handback";
    let input = digest(bytes);
    world.activate_space();
    world.seed_doc("d1", &input);
    // cache 为空：recovery 认领后 cache.get 必 Miss，走 hand-back。

    // 判别性设置：max_attempts=1。旧语义（fenced retry）在 hand-back 时按
    // attempt_count=1 >= 1 直接折进终态 failed（恢复侧替 worker 做了死信
    // 决策）；新原语必须交还 pending，预算完整留给 worker。
    let opts = RecoveryOptions {
        max_attempts: 1,
        ..options(1) // scan_batch=1：本轮恰一次 claim，隔离 revisit guard
    };
    let report = world.recover(&opts);
    assert_eq!(report.requeued, 1, "the cache miss was handed back");
    assert_eq!(report.replayed, 0);
    assert_eq!(
        world.outbox_state("d1"),
        "pending",
        "hand-back returns pending, never a recovery-side dead-letter"
    );
    let (state, attempts, lease): (String, i64, Option<String>) = world
        .conn
        .query_row(
            "SELECT state,attempt_count,lease_token FROM semantic_outbox WHERE doc_key='d1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(state, "pending");
    assert_eq!(attempts, 1, "only the recovery claim incremented it");
    assert_eq!(lease, None, "lease cleared");
    assert!(report.converged);

    // worker 续跑：预算未被消耗，一次付费即收敛到 done。
    world.drain(&digest_table(bytes));
    assert_eq!(
        world.provider.call_count(),
        1,
        "the worker still owns the full attempt budget"
    );
    assert_eq!(world.outbox_state("d1"), "done");
}

// ── 2. put 后 CAS 前崩溃：已存 artifact 优先复用 + 幂等重放吸收 ───────────

#[test]
fn crash_after_artifact_put_recovery_replays_publish_and_absorbs_duplicates() {
    let world = World::new("put-no-cas");
    let bytes: &'static [u8] = b"body crash-after-put";
    let input = digest(bytes);
    world.activate_space();
    world.seed_doc("d1", &input);

    // 崩溃点注入：put 已完成（artifact durable），CAS 从未发生。
    // （P6-011 编排的三步中，进程死在第 1 步与第 3 步之间。）
    let killed = world.kill_a_claim("worker");
    assert_eq!(killed.doc_key, "d1");
    let doc_input = DocumentInput::from_bytes(bytes).expect("input");
    let vector = world
        .provider
        .embed_documents(&[doc_input])
        .expect("pre-crash embed")
        .into_iter()
        .next()
        .expect("one vector");
    world
        .cache
        .put(&world.space, &input, &world.spec, &vector, 1_700_000_000)
        .expect("artifact durable first");
    world.forge_expired_lease(killed.task_id);
    assert_eq!(world.provider.call_count(), 1);
    assert_eq!(world.manifest_count(), 0, "CAS never happened");
    assert_eq!(world.semantic_epoch(), None);

    // 恢复：已存 artifact 优先复用，重放发布走完整五重 fence CAS，0 新付费。
    let report = world.recover(&options(8));
    assert_eq!(
        report.reclaimed, 1,
        "the dead worker's lease was the orphan"
    );
    assert_eq!(
        report.replayed, 1,
        "the durable artifact was replay-published"
    );
    assert_eq!(
        report.replay_absorbed, 0,
        "first publication is a real visible change, not an absorption"
    );
    assert!(report.converged);
    assert_eq!(
        world.provider.call_count(),
        1,
        "recovery added zero provider calls (已存 artifact 优先复用)"
    );
    assert_eq!(world.manifest_count(), 1);
    assert_eq!(world.outbox_state("d1"), "done");
    assert_eq!(
        world.semantic_epoch(),
        Some(1),
        "exactly one visible-set change moved the clock"
    );

    // 幂等重放吸收（Q4 恢复侧验证）：等值任务再次入队并被 recovery 重放 →
    // 吸收为重复 ack，零可见变化、零 bump。（"CAS 后、ack 前"残态在 P6-011
    // 中不可达——manifest 写与 ack 同事务；此处验证的是：即便恢复路径重放
    // 撞上等值已发布内容，CAS/Q4 机制也无条件吸收。）
    let stats = supersede_and_enqueue_on(
        &world.conn,
        &OutboxPlan {
            upserts: &[OutboxUpsert {
                doc_key: "d1".into(),
                doc_version: "v1".into(),
                input_digest: input.as_str().into(),
            }],
            removals: &[],
            now_unix: 901.0,
        },
    )
    .unwrap();
    assert_eq!(stats.enqueued, 1);
    let report = world.recover(&options(8));
    assert_eq!(report.replayed, 1);
    assert_eq!(
        report.replay_absorbed, 1,
        "the duplicate replay was absorbed (visible_set_changed=false)"
    );
    assert!(report.converged);
    assert_eq!(world.semantic_epoch(), Some(1), "no double bump");
    assert_eq!(world.manifest_count(), 1, "still exactly one manifest row");
    assert_eq!(world.outbox_state("d1"), "done");
    assert_eq!(retrieval_hit(&world, &input).as_deref(), Some("d1"));
}

// ── 3. 死信口径：failed 终态只清点，绝不复活 ─────────────────────────────

#[test]
fn dead_letters_are_counted_but_never_resurrected() {
    let world = World::new("dead-letter");
    let bytes: &'static [u8] = b"body dead-letter";
    let input = digest(bytes);
    world.activate_space();
    world.seed_doc("d1", &input);

    // 预算耗尽 → 终态 failed（P6-013 fenced retry 语义）。
    let task = world.kill_a_claim("worker");
    assert!(world
        .db
        .retry_semantic_task(task.task_id, &task.token, "provider dead", 0.0, 1)
        .unwrap());
    assert_eq!(world.outbox_state("d1"), "failed");

    let report = world.recover(&options(8));
    assert_eq!(
        report.dead_letters, 1,
        "the census observes the dead letter"
    );
    assert_eq!(report.reclaimed, 0);
    assert_eq!(report.replayed, 0);
    assert!(report.converged, "a dead letter is not recoverable residue");

    // 死信保持终态：不被 reclaim、不被 claim、不被重放。
    assert_eq!(world.outbox_state("d1"), "failed", "never resurrected");
    let report = world.drain_pending_claims_zero();
    assert_eq!(report, 0, "the dead letter is not claimable");
    assert_eq!(world.provider.call_count(), 0);
    assert_eq!(world.semantic_epoch(), None, "no clock moved");
}

impl World {
    fn drain_pending_claims_zero(&self) -> usize {
        drain_pending(&self.db, "worker", &self.limits(), &mut |guard| {
            embed_once(self, guard, &[])
        })
        .expect("drain")
        .claimed
    }
}

// ── 4. 换库 fence：幽灵快照的恢复零写入 ──────────────────────────────────

#[test]
fn recovery_fences_the_pre_swap_ghost_with_zero_writes() {
    let mut world = World::new("ghost");
    let bytes: &'static [u8] = b"body ghost";
    let input = digest(bytes);
    world.activate_space();
    world.seed_doc("d1", &input);
    world.drain(&digest_table(bytes));
    let ghost_snapshot = world.incarnation();

    // 换库：incarnation 变更，语义表从零；重注册空间并手动重入队一个任务
    // （模拟继任进程已开始工作）。
    world.rebuild(&[("d1", &input)]);
    world.activate_space();
    let desired = world.db.semantic_rebuild_desired_set().unwrap();
    assert_eq!(desired.len(), 1);
    world.db.enqueue_semantic_rebuild_plan(&desired).unwrap();
    let outbox_before = world.outbox_count();
    assert_eq!(outbox_before, 1);

    // 幽灵恢复（旧快照）：fence 先行，零写入。
    let verdict = recover_scan(
        &world.db,
        &world.cache,
        &world.space,
        &world.spec,
        ghost_snapshot,
        &options(8),
    )
    .expect("recover");
    match verdict {
        RecoveryVerdict::Fenced {
            snapshot,
            path_incarnation,
        } => {
            assert_eq!(snapshot, ghost_snapshot);
            assert_ne!(path_incarnation, ghost_snapshot, "the swap moved it");
        }
        RecoveryVerdict::Applied(_) => panic!("the ghost must be fenced"),
    }
    assert_eq!(
        world.outbox_count(),
        outbox_before,
        "the ghost touched nothing"
    );
    assert_eq!(world.manifest_count(), 0);
}

// ── 5. 半途 reconcile 的续跑：recovery 续完，付费向量只付一次 ─────────────

#[test]
fn interrupted_reconcile_is_completed_by_recovery() {
    let mut world = World::new("reconcile-resume");
    let b1: &'static [u8] = b"body r1";
    let b2: &'static [u8] = b"body r2";
    let (d1, d2) = (digest(b1), digest(b2));
    world.activate_space();
    world.seed_doc("d1", &d1);
    world.seed_doc("d2", &d2);
    world.drain(&[(d1.as_str().to_string(), b1), (d2.as_str().to_string(), b2)]);
    assert_eq!(
        world.provider.call_count(),
        2,
        "pre-rebuild: paid exactly once"
    );

    // 换库：语义表从零，付费 artifact 留在 cache（Q5：namespace 不含
    // incarnation）。
    world.rebuild(&[("d1", &d1), ("d2", &d2)]);
    world.activate_space();
    assert_eq!(world.semantic_epoch(), None);
    assert_eq!(
        world.provider.call_count(),
        2,
        "the rebuild itself pays nothing"
    );

    // 模拟半途 reconcile：第 2 步（重入队）已完成，第 3 步（reuse 循环）
    // 在第一个任务认领后被打断（kill）。
    let desired = world.db.semantic_rebuild_desired_set().unwrap();
    assert_eq!(desired.len(), 2);
    world.db.enqueue_semantic_rebuild_plan(&desired).unwrap();
    assert_eq!(
        world.semantic_epoch(),
        Some(1),
        "re-enqueue moved the clock once"
    );
    let killed = world.kill_a_claim("semantic-reconcile");
    world.forge_expired_lease(killed.task_id);

    // recovery 续跑：孤儿回收 + 全部 cache 命中的任务重放发布，0 provider。
    let report = world.recover(&options(8));
    assert_eq!(report.reclaimed, 1, "the interrupted claim was the orphan");
    assert_eq!(report.replayed, 2, "both paid vectors were reused");
    assert_eq!(report.requeued, 0);
    assert!(report.converged);
    assert_eq!(
        world.provider.call_count(),
        2,
        "reconcile residue converges at zero additional cost (付费产物保留)"
    );
    assert_eq!(world.manifest_count(), 2);
    for key in ["d1", "d2"] {
        assert_eq!(world.outbox_state(key), "done");
    }
    assert_eq!(
        world.semantic_epoch(),
        Some(3),
        "1 re-enqueue + 2 visible changes (P6-006 + Q4 口径)"
    );

    // 覆盖率恢复：visible 集合回到换库前。
    let coverage = world.db.reads().semantic_coverage().unwrap().coverage;
    assert_eq!(coverage.eligible, 2);
    assert_eq!(coverage.published, 2);
    assert_eq!(coverage.uncovered, 0);
    assert_eq!(coverage.failed, 0);
    assert_eq!(retrieval_hit(&world, &d1).as_deref(), Some("d1"));
    assert_eq!(retrieval_hit(&world, &d2).as_deref(), Some("d2"));
}
