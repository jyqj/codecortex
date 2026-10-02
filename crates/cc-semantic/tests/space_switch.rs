//! P6-017 integration: model space 切换端到端。
//!
//! 覆盖 cc-db 门面测试看不到的部分：
//!
//! 1. **三段协议端到端（FakeProvider）**：切 active → 新空间 embed（provider
//!    恰好对新空间调用 N 次）→ 旧空间 revoke 消费 → filtered exact 检索只见
//!    新空间（V16：切换窗口内查询只回 active space 行，分数永不混排）；
//! 2. **并存窗口（backfilling）语义**：回填任务入队后 worker 仍只认领 active
//!    空间（claim 固定 active 指针），dense lane 只读 active——新空间任务
//!    在切换前零消费；
//! 3. **回滚复用（V17）**：切回旧空间 + reconcile（P6-014 路径）——
//!    `cache.get` 校验复用，**provider 调用 0 次**；
//! 4. **revoke 后 GC 衔接（P6-016 判据的反向）**：revoke 消费删掉旧空间
//!    manifest 行后，旧对象失去 mark 保护，过期 GC pass 可回收（回收后
//!    `cache.get` = Miss）。
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use cc_db::index_db::IndexDb;
use cc_db::semantic_outbox::{supersede_and_enqueue_on, ClaimedTask, OutboxPlan, OutboxUpsert};
use cc_model::retrieval::HardScope;
use cc_semantic::cache::ArtifactCache;
use cc_semantic::gc::{run_gc_pass, GcConfig};
use cc_semantic::ports::DocumentInput;
use cc_semantic::providers::fake::{FakeProvider, FakeProviderConfig};
use cc_semantic::publish::Publisher;
use cc_semantic::queue::{drain_pending, EmbedHandler, WorkerLimits};
use cc_semantic::reconcile::{reconcile_after_rebuild, ReconcileOptions};
use cc_semantic::space_switch::{
    activate_space, drain_space_revocations, enqueue_backfill, register_backfill_space,
};
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
            "cc-semantic-p6017-{tag}-{}-{n}",
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

fn space(model: &str) -> VectorSpace {
    VectorSpace::new(model, 2).expect("space")
}

fn doc_spec(space: &VectorSpace) -> DocSpecDigest {
    DocumentEncodingSpec::new(space.clone(), None, 8_192, "fake-tokenizer")
        .expect("doc spec")
        .digest()
        .expect("doc spec digest")
}

fn options() -> ReconcileOptions {
    ReconcileOptions {
        lease_secs: 60.0,
        retry_backoff_secs: 0.0,
        max_attempts: 3,
    }
}

fn scope() -> HardScope {
    HardScope {
        path_prefix: None,
        languages: None,
        file_paths: None,
    }
}

fn run_search(
    cache: &ArtifactCache,
    conn: &rusqlite::Connection,
    space: &VectorSpace,
    query: &[f32],
) -> Vec<String> {
    let reads = cc_db::semantic_manifest_reads::SemanticManifestReads::on(conn);
    let digest = space.digest().unwrap();
    let source = space_manifest_reads(&reads, &digest);
    search(
        cache,
        &source,
        ExactSearch {
            space,
            query,
            filter: &scope(),
            k: 10,
            batch_rows: 8,
        },
    )
    .expect("exact search")
    .into_iter()
    .map(|hit| hit.doc_key)
    .collect()
}

struct World {
    _dir: TempDir,
    db: IndexDb,
    conn: rusqlite::Connection,
    cache: ArtifactCache,
    space_a: VectorSpace,
    space_b: VectorSpace,
    spec_a: DocSpecDigest,
    spec_b: DocSpecDigest,
    provider_a: FakeProvider,
    provider_b: FakeProvider,
}

impl World {
    fn new(tag: &str) -> Self {
        let dir = TempDir::new(tag);
        let (db, _) = IndexDb::open(&dir.0.join("index.sqlite3")).expect("open index");
        let conn = fresh_conn(db.admin().db_path());
        let space_a = space("fake/model-switch-a");
        let space_b = space("fake/model-switch-b");
        let spec_a = doc_spec(&space_a);
        let spec_b = doc_spec(&space_b);
        let cache = ArtifactCache::open(dir.0.join("cache"), format!("ns-{tag}")).expect("cache");
        Self {
            _dir: dir,
            db,
            conn,
            cache,
            space_a: space_a.clone(),
            space_b: space_b.clone(),
            spec_a,
            spec_b,
            provider_a: FakeProvider::new(FakeProviderConfig::new(space_a.clone())),
            provider_b: FakeProvider::new(FakeProviderConfig::new(space_b.clone())),
        }
    }

    /// A 冻结 spec 路径（register + activate，走本任务协议而非裸 SQL）。
    fn activate_a(&self, revision: &str) {
        let spec = DocumentEncodingSpec::new(
            self.space_a.clone(),
            None,
            8_192,
            "fake-tokenizer",
        )
        .unwrap();
        register_backfill_space(&self.db, &spec).unwrap();
        activate_space(&self.db, &self.space_a, revision).unwrap();
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

    /// 活文档：投影 + active 空间期望任务（P6-006 写路径口径，含 outbox）。
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

    fn incarnation(&self) -> [u8; 16] {
        self.db.reads().read_generation().unwrap().incarnation
    }

    fn limits(&self) -> WorkerLimits {
        WorkerLimits::validated(16, 60.0, 0.0, 3).expect("limits")
    }

    /// P6-013 worker drain：认领 active 空间任务，按 resolver 表喂
    /// FakeProvider，五 fence CAS 发布。`space`/`provider` 绑定当前 active。
    fn drain(&self, table: &[(String, &'static [u8])], space: &VectorSpace, provider: &FakeProvider) {
        drain_pending(&self.db, "worker", &self.limits(), &mut |guard| {
            embed_once(self, guard, table, space, provider)
        })
        .expect("drain");
    }

    fn manifest_count(&self) -> i64 {
        self.conn
            .query_row("SELECT COUNT(*) FROM semantic_manifest", [], |r| r.get(0))
            .unwrap()
    }

    fn semantic_epoch(&self) -> Option<u64> {
        self.conn
            .query_row(
                "SELECT value FROM metadata WHERE key='semantic_epoch'",
                [],
                |r| r.get::<_, String>(0),
            )
            .ok()
            .map(|v| v.parse().unwrap())
    }

    fn outbox_count(&self, space_id: &str, op: &str, state: &str) -> i64 {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM semantic_outbox WHERE space_id=?1 AND op=?2 AND state=?3",
                rusqlite::params![space_id, op, state],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn space_digest(&self, space: &VectorSpace) -> String {
        space.digest().unwrap().as_str().to_string()
    }
}

fn embed_once(
    world: &World,
    guard: &cc_semantic::queue::LeaseGuard<'_>,
    table: &[(String, &'static [u8])],
    space: &VectorSpace,
    provider: &FakeProvider,
) -> cc_model::CcResult<cc_semantic::queue::TaskExit> {
    let spec = if std::ptr::eq(space, &world.space_a) {
        &world.spec_a
    } else {
        &world.spec_b
    };
    let publisher = Publisher::new(&world.db, &world.cache, space, spec, world.incarnation())
        .expect("publisher");
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
    conn.busy_timeout(std::time::Duration::from_secs(5)).unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    conn
}

const INPUT_A: &[u8] = b"alpha body text";
const INPUT_B: &[u8] = b"beta body text";

// ── 端到端：切换 → 新空间 embed → 旧空间 revoke → 检索只见新空间 → 回滚 ──

#[test]
fn switch_end_to_end_new_embeds_old_revokes_retrieval_sees_only_new_then_rollback_reuses() {
    let world = World::new("e2e");
    let input_a = InputDigest::of_input(INPUT_A).unwrap();
    let input_b = InputDigest::of_input(INPUT_B).unwrap();

    // 0. A 激活，两文档经 worker 发布在 A 下。
    world.activate_a("rev-init");
    world.seed_doc("d1", &input_a);
    world.seed_doc("d2", &input_b);
    let table = [
        (input_a.as_str().to_string(), INPUT_A),
        (input_b.as_str().to_string(), INPUT_B),
    ];
    world.drain(&table, &world.space_a, &world.provider_a);
    assert_eq!(world.provider_a.call_count(), 2);
    assert_eq!(world.manifest_count(), 2);
    let a_digest = world.space_digest(&world.space_a);
    let b_digest = world.space_digest(&world.space_b);
    assert_eq!(world.outbox_count(&a_digest, "embed", "done"), 2);

    // 检索：A 有结果；B 无行（扫描固定空间，分数不可能混排）。
    let hits_a = run_search(&world.cache, &world.conn, &world.space_a, &[0.5, 0.5]);
    assert_eq!(hits_a.len(), 2, "A 下两文档可检索");
    assert!(run_search(&world.cache, &world.conn, &world.space_b, &[0.5, 0.5]).is_empty());
    let epoch_published = world.semantic_epoch();
    assert!(epoch_published.unwrap() >= 1, "首次发布把 semantic_epoch 从 absent 推到 >=1");

    // 1. 回填：注册 B（backfilling）+ 全量 desired 入 B 队列。
    let spec_b_full =
        DocumentEncodingSpec::new(world.space_b.clone(), None, 8_192, "fake-tokenizer").unwrap();
    register_backfill_space(&world.db, &spec_b_full).unwrap();
    let desired = world.db.semantic_rebuild_desired_set().unwrap();
    assert_eq!(desired.len(), 2);
    let stats = enqueue_backfill(&world.db, &spec_b_full, &desired).unwrap();
    assert_eq!(stats.enqueued, 2);
    assert_eq!(world.outbox_count(&b_digest, "embed", "pending"), 2);
    // epoch 口径（P6-006 Q4 约定，随已交付的 rebuild-plan 入队门面）：队列
    // 状态实际变化 → bump 一次（后续切换断言依赖该基线）。
    assert_eq!(world.semantic_epoch().unwrap(), epoch_published.unwrap() + 1);

    // 2. 并存窗口语义：worker 只认领 active（A）——A 队列空 → 零消费，
    //    B 的回填任务原封不动；dense lane 也只读 active（上面已验）。
    world.drain(&table, &world.space_a, &world.provider_a);
    assert_eq!(world.provider_a.call_count(), 2, "并存窗口内零新 provider 调用");
    assert_eq!(world.outbox_count(&b_digest, "embed", "pending"), 2, "B 回填任务切换前不被认领");

    // 3. 切 active（单事务：B→active、A→revoked、A live 任务 supersede、
    //    每 A manifest 行一个 revoke 任务、审计事件）。
    let switch = activate_space(&world.db, &world.space_b, "rev-model-b").unwrap();
    assert!(switch.activated && switch.old_revoked && switch.visible_set_switched);
    assert_eq!(switch.previous_active.as_deref(), Some(a_digest.as_str()));
    assert_eq!(switch.revoke_tasks_enqueued, 2);
    assert_eq!(switch.superseded_live_tasks, 0, "A 队列已耗尽，无 live 任务可 supersede");
    assert_eq!(
        world.semantic_epoch().unwrap(),
        epoch_published.unwrap() + 2,
        "可见集合切换 → 恰一次 bump（+1 来自回填入队，P6-006 约定）"
    );
    assert_eq!(world.db.semantic_active_space().unwrap().as_deref(), Some(b_digest.as_str()));

    // 切换瞬间检索口径：active=B，B 尚无发布行 → 查询为空；A 行虽在
    // manifest 但已不属于 active 指针下的可见集合（结构上不混排）。
    assert!(run_search(&world.cache, &world.conn, &world.space_b, &[0.5, 0.5]).is_empty());

    // 4. 撤销：消费 A 的 revoke 任务——own-space 行删除 + fenced ack。
    let a_space_id = a_digest.clone();
    let drain = drain_space_revocations(&world.db, &a_space_id, "revoker", 60.0, 0.0, 3, 64).unwrap();
    assert_eq!((drain.claimed, drain.revoked, drain.visible_changes), (2, 2, 2));
    assert_eq!(world.manifest_count(), 0, "旧空间可见集合行全部移除");
    assert_eq!(world.outbox_count(&a_digest, "revoke", "done"), 2);

    // 5. 新空间 embed：worker 认领 active=B 的回填任务，provider_b 恰好 2 次。
    world.drain(&table, &world.space_b, &world.provider_b);
    assert_eq!(world.provider_b.call_count(), 2, "新空间 embed 恰好按任务数付费");
    assert_eq!(world.manifest_count(), 2);
    assert_eq!(world.outbox_count(&b_digest, "embed", "done"), 2);

    // 6. 检索只见新空间：B 命中两文档；A 的 cache 产物虽仍在，但 A 的
    //    manifest 行已被 revoke 移除 → A 检索为空。
    let hits_b = run_search(&world.cache, &world.conn, &world.space_b, &[0.5, 0.5]);
    assert_eq!(hits_b.len(), 2, "切换后新空间可检索");
    assert!(
        run_search(&world.cache, &world.conn, &world.space_a, &[0.5, 0.5]).is_empty(),
        "旧空间 revoked 后检索为空（分数永不混排）"
    );

    // 7. 回滚：切回 A（revoked → active 边）+ desired 重推 + P6-014
    //    reconcile —— cache.get 校验复用，provider 零调用（V17）。
    let calls_before_rollback = world.provider_a.call_count() + world.provider_b.call_count();
    let rollback = activate_space(&world.db, &world.space_a, "rev-rollback").unwrap();
    assert!(rollback.activated && rollback.old_revoked && rollback.visible_set_switched);
    assert_eq!(rollback.revoke_tasks_enqueued, 2, "回滚同时为 B 的行产生 revoke 任务");
    let desired = world.db.semantic_rebuild_desired_set().unwrap();
    world.db.enqueue_semantic_rebuild_plan(&desired).unwrap();
    let verdict = reconcile_after_rebuild(
        &world.db,
        &world.cache,
        &world.space_a,
        &world.spec_a,
        world.incarnation(),
        &options(),
    )
    .unwrap();
    match verdict {
        cc_semantic::reconcile::ReconcileVerdict::Applied(report) => {
            assert_eq!(report.desired, 2);
            assert_eq!(report.reused, 2, "旧 cache 经校验全部复用");
            assert_eq!(report.visible_changes, 2);
            assert_eq!(report.requeued, 0);
        }
        other => panic!("unexpected verdict {other:?}"),
    }
    assert_eq!(
        world.provider_a.call_count() + world.provider_b.call_count(),
        calls_before_rollback,
        "回滚路径 provider 调用必须为 0（旧 cache 校验复用）"
    );
    let hits_a_after = run_search(&world.cache, &world.conn, &world.space_a, &[0.5, 0.5]);
    assert_eq!(hits_a_after.len(), 2, "回滚后旧空间可见集合恢复");
    assert!(run_search(&world.cache, &world.conn, &world.space_b, &[0.5, 0.5]).is_empty());

    // 8. 收尾（P6-006 既有语义，以代码为准）：回滚重推 d1/d2 时，rebuild
    //    计划按 doc_key 跨空间 supersede 了尚未消费的 B revoke 任务——一致性
    //    成立：同 doc 的 CAS upsert（doc_key 主键）把 B 行覆盖为 A 行，撤销
    //    实质完成，不存在悬挂的 revoked 行。
    assert_eq!(world.outbox_count(&b_digest, "revoke", "superseded"), 2);
    assert_eq!(world.outbox_count(&b_digest, "revoke", "pending"), 0);
    let drain_b = drain_space_revocations(&world.db, &b_digest, "revoker", 60.0, 0.0, 3, 64).unwrap();
    assert_eq!((drain_b.claimed, drain_b.revoked, drain_b.visible_changes), (0, 0, 0), "撤销队列已收敛");
    assert_eq!(world.manifest_count(), 2, "只剩 A 的两行");
    assert_eq!(
        world.conn.query_row("SELECT DISTINCT space_id FROM semantic_manifest", [], |r| r.get::<_, String>(0)).unwrap(),
        a_digest,
        "可见集合无 revoked 空间残留"
    );

    // 审计：4 次有效切换全部落 `semantic_space_switch_log`，pinned=false。
    let log: String = world
        .conn
        .query_row(
            "SELECT value FROM metadata WHERE key='semantic_space_switch_log'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let events: Vec<serde_json::Value> = serde_json::from_str(&log).unwrap();
    assert_eq!(events.len(), 3, "init/切换/回滚 三次有效切换");
    assert!(events.iter().all(|e| e["pinned"] == false));
    assert_eq!(events[1]["from"], a_digest.as_str());
    assert_eq!(events[1]["to"], b_digest.as_str());
    assert_eq!(events[1]["revision"], "rev-model-b");
}

// ── revoke 后 GC 衔接：旧空间对象失去 mark 保护后可回收 ─────────────────

#[test]
fn revoked_space_objects_become_gc_eligible_after_revoke_drain() {
    let world = World::new("gc");
    let input_a = InputDigest::of_input(INPUT_A).unwrap();

    // A 下发布一个文档（cache 对象 + manifest 行）。
    world.activate_a("rev-init");
    world.seed_doc("d1", &input_a);
    let table = [(input_a.as_str().to_string(), INPUT_A)];
    world.drain(&table, &world.space_a, &world.provider_a);
    assert_eq!(world.manifest_count(), 1);

    // 对照（P6-016 判据）：revoke 前，过期 GC 也不删——manifest 行保护
    // 不区分空间状态。
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let aged_cfg = GcConfig {
        min_retention_secs: 3_600,
        batch_entries: 16,
        now_unix: now + 3_600,
    };
    let (counters, _, _) = run_gc_pass(&world.db, &world.cache, &aged_cfg, None).unwrap();
    assert_eq!(counters.deleted_objects, 0, "revoke 前 manifest 行保护对象");

    // 切换 + 撤销：revoke 消费删除 A 的 manifest 行。
    let spec_b_full =
        DocumentEncodingSpec::new(world.space_b.clone(), None, 8_192, "fake-tokenizer").unwrap();
    register_backfill_space(&world.db, &spec_b_full).unwrap();
    activate_space(&world.db, &world.space_b, "rev-model-b").unwrap();
    let a_digest = world.space_digest(&world.space_a);
    let drain = drain_space_revocations(&world.db, &a_digest, "revoker", 60.0, 0.0, 3, 64).unwrap();
    assert_eq!(drain.visible_changes, 1);

    // revoke 后：旧对象成为孤儿，过期 GC pass 可回收；回收后读 = Miss。
    let (counters, _, _) = run_gc_pass(&world.db, &world.cache, &aged_cfg, None).unwrap();
    assert_eq!(counters.deleted_objects, 1, "revoke 后旧空间对象可被 GC 回收");
    let read = world
        .cache
        .get(&world.space_a, &input_a, &world.spec_a)
        .unwrap();
    assert!(
        matches!(read, cc_semantic::cache::CacheRead::Miss),
        "回收后的对象必须读为 Miss"
    );
}
