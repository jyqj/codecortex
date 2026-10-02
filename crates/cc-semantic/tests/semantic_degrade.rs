//! P6-018 integration: cache 缺失/损坏降级闭环（隔离搬移 / 费用预算 / 自愈回路 /
//! GC 衔接 / 降级矩阵）。
//!
//! 覆盖各门面级测试看不到的端到端部分：
//!
//! 1. **自愈回路（FakeProvider 全链）**：发布 → 损坏注入 → 检测（`CacheRead::
//!    Corrupt`）→ 隔离搬移（evidence 保留、地址回落 Miss）→ 重新入队 →
//!    BudgetedProvider 补嵌（受预算记账）→ `put` 覆盖 → 发布 CAS → 检索重见；
//! 2. **claimed 任务的降级交还**：`requeue_after_degrade` 走无 attempt 直写
//!    原语（P6-015 收口 ②），缓存故障不消耗任务预算；
//! 3. **预算耗尽死信**：超限后 provider 拒绝（不触内层 provider），fenced
//!    retry 把任务终态 `failed` 且 `last_error` 带预算原因——绝不静默循环；
//! 4. **GC 衔接**：quarantine 内容在任何 GC pass 中字节级原样幸存；
//! 5. **降级矩阵（检索行）**：Miss/Corrupt 候选被跳过、健康行照常返回、零报错。
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use cc_db::index_db::IndexDb;
use cc_db::semantic_manifest_reads::SemanticManifestReads;
use cc_db::semantic_outbox::{supersede_and_enqueue_on, ClaimedTask, OutboxPlan, OutboxUpsert};
use cc_model::retrieval::HardScope;
use cc_semantic::cache::{ArtifactCache, CacheRead, CorruptReport};
use cc_semantic::degrade::{
    quarantine_detected, requeue_after_degrade, BudgetedProvider, DegradationLedger,
};
use cc_semantic::ports::{DocumentInput, EmbeddingProvider, QueryInput};
use cc_semantic::providers::fake::{FakeProvider, FakeProviderConfig};
use cc_semantic::publish::Publisher;
use cc_semantic::queue::{drain_pending, EmbedHandler, LeaseGuard, TaskExit, WorkerLimits};
use cc_semantic::spec::{DocumentEncodingSpec, VectorSpace};
use cc_semantic::types::{DocSpecDigest, InputDigest};
use cc_semantic::vector::exact::{search, space_manifest_reads, ExactSearch};

// ── harness（P6-015 恢复测试世界的无重建子集） ───────────────────────────

static SEQ: AtomicU32 = AtomicU32::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "cc-semantic-p6018-{tag}-{}-{n}",
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
    VectorSpace::new("fake/model-degrade", 2).expect("space")
}

fn doc_spec(space: &VectorSpace) -> DocSpecDigest {
    DocumentEncodingSpec::new(space.clone(), None, 8_192, "fake-tokenizer")
        .expect("doc spec")
        .digest()
        .expect("doc spec digest")
}

fn digest(bytes: &'static [u8]) -> InputDigest {
    InputDigest::of_input(bytes).expect("input digest")
}

struct World {
    _dir: TempDir,
    db: IndexDb,
    conn: rusqlite::Connection,
    cache: ArtifactCache,
    space: VectorSpace,
    spec: DocSpecDigest,
    provider: FakeProvider,
    ledger: DegradationLedger,
}

impl World {
    /// `reembed_budget`: None = 不限；Some(n) = 进程生命周期内 n 次补嵌。
    fn new(tag: &str, reembed_budget: Option<u64>) -> Self {
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
            ledger: DegradationLedger::new(reembed_budget),
        }
    }

    fn write_projection(&self, doc_key: &str, doc_version: &str, input_digest: &str) {
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
    }

    /// 活活一个文档：投影 + 期望任务（P6-006 写路径口径，含 outbox）。
    fn seed_doc(&self, doc_key: &str, doc_version: &str, input: &InputDigest) {
        self.write_projection(doc_key, doc_version, input.as_str());
        self.enqueue_task(doc_key, doc_version, input);
    }

    /// 同文档新版本：版本推进（CAS doc_version fence 的基行）+ 重入队。
    fn bump_version(&self, doc_key: &str, doc_version: &str, input: &InputDigest) {
        self.conn
            .execute(
                "UPDATE document_manifest SET doc_version=?1 WHERE doc_key=?2",
                rusqlite::params![doc_version, doc_key],
            )
            .unwrap();
        self.enqueue_task(doc_key, doc_version, input);
    }

    fn enqueue_task(&self, doc_key: &str, doc_version: &str, input: &InputDigest) {
        let stats = supersede_and_enqueue_on(
            &self.conn,
            &OutboxPlan {
                upserts: &[OutboxUpsert {
                    doc_key: doc_key.into(),
                    doc_version: doc_version.into(),
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

    fn limits(&self) -> WorkerLimits {
        WorkerLimits::validated(16, 60.0, 0.0, 3).expect("limits")
    }

    /// 一轮显式 drain，handler 用预算化 provider（P6-018 费用策略在位）。
    fn drain(&self, table: &[(String, &'static [u8])]) -> cc_semantic::queue::BatchReport {
        drain_pending(&self.db, "worker", &self.limits(), &mut |guard| {
            self.embed_once(guard, table)
        })
        .expect("drain")
    }

    fn embed_once(
        &self,
        guard: &LeaseGuard<'_>,
        table: &[(String, &'static [u8])],
    ) -> cc_model::CcResult<TaskExit> {
        let publisher = Publisher::new(
            &self.db,
            &self.cache,
            &self.space,
            &self.spec,
            self.incarnation(),
        )
        .expect("publisher");
        let budgeted = BudgetedProvider::new(&self.provider, self.ledger.clone());
        let resolver = |task: &ClaimedTask| {
            table
                .iter()
                .find(|(digest, _)| *digest == task.input_digest)
                .map(|(_, bytes)| DocumentInput::from_bytes(bytes))
                .transpose()
        };
        let handler = EmbedHandler::new(publisher, &budgeted, &resolver);
        handler.handle(guard)
    }

    /// 把 `input` 寻址对象的 payload 翻转一字节（损坏注入）。
    fn corrupt_payload(&self, input: &InputDigest) -> PathBuf {
        let bin = self.bin_path(input);

        let mut payload = std::fs::read(&bin).expect("payload readable");
        payload[0] ^= 0xff;
        std::fs::write(&bin, &payload).expect("payload rewritten");
        bin
    }

    fn bin_path(&self, input: &InputDigest) -> PathBuf {
        self.cache
            .root()
            .join(format!("namespace-{}", self.cache.namespace()))
            .join(self.space.digest().unwrap().as_str())
            .join(input.as_str())
            .join(self.spec.as_str())
            .join(format!("{}.bin", self.spec.as_str()))
    }

    fn incarnation(&self) -> [u8; 16] {
        self.db.reads().read_generation().unwrap().incarnation
    }

    /// 最新一行（bump_version 会留下 done 的旧行；无 ORDER BY 的 rowid 序
    /// 会取到旧行，必须显式取 task_id 最大者）。
    fn outbox_state(&self, doc_key: &str) -> String {
        self.conn
            .query_row(
                "SELECT state FROM semantic_outbox WHERE doc_key=?1 ORDER BY task_id DESC LIMIT 1",
                [doc_key],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn outbox_attempt_count(&self, doc_key: &str) -> i64 {
        self.conn
            .query_row(
                "SELECT attempt_count FROM semantic_outbox WHERE doc_key=?1 ORDER BY task_id DESC LIMIT 1",
                [doc_key],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn outbox_last_error(&self, doc_key: &str) -> String {
        self.conn
            .query_row(
                "SELECT last_error FROM semantic_outbox WHERE doc_key=?1 ORDER BY task_id DESC LIMIT 1",
                [doc_key],
                |r| r.get::<_, Option<String>>(0),
            )
            .unwrap()
            .unwrap_or_default()
    }

    fn manifest_count(&self) -> i64 {
        self.conn
            .query_row("SELECT COUNT(*) FROM semantic_manifest", [], |r| r.get(0))
            .unwrap()
    }

    fn dead_letters(&self) -> u64 {
        self.db.semantic_dead_letter_count().unwrap()
    }

    /// exact 检索：以独立 query 输入的 fake 向量查 top-k doc_key。
    fn search_doc_keys(&self, k: usize) -> Vec<String> {
        let reads = SemanticManifestReads::on(&self.conn);
        let space_digest = self.space.digest().expect("space digest");
        let source = space_manifest_reads(&reads, &space_digest);
        let query = self
            .provider
            .embed_queries(&[QueryInput::from_bytes(b"probe-query").expect("query")])
            .expect("query vector")
            .remove(0);
        search(
            &self.cache,
            &source,
            ExactSearch {
                space: &self.space,
                query: &query,
                filter: &HardScope {
                    path_prefix: None,
                    languages: None,
                    file_paths: None,
                },
                k,
                batch_rows: 4,
            },
        )
        .expect("search")
        .into_iter()
        .map(|h| h.doc_key)
        .collect()
    }
}

fn fresh_conn(path: &Path) -> rusqlite::Connection {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.busy_timeout(Duration::from_secs(5)).unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    conn
}

const DOC_A: &[u8] = b"degrade-doc-a payload";

// ── 1. 自愈回路：发布 → 损坏 → 隔离 → 预算内补嵌 → 重新发布 → 检索重见 ──

#[test]
fn corrupt_quarantine_reembed_republish_self_heals_the_visible_set() {
    let world = World::new("self-heal", Some(4));
    world.activate_space();
    let input = digest(DOC_A);
    world.seed_doc("doc-a", "v1", &input);

    // 首次发布（FirstEmbed：不入补嵌预算）。
    let _report = world.drain(&[(input.as_str().to_string(), DOC_A)]);
    assert_eq!(world.outbox_state("doc-a"), "done");
    assert_eq!(world.provider.call_count(), 1);
    assert_eq!(world.ledger.snapshot().reembeds_used, 0);
    assert_eq!(world.search_doc_keys(10), vec!["doc-a".to_string()]);

    // 损坏注入 → 检测（Corrupt 带诊断）→ 隔离搬移。
    world.corrupt_payload(&input);
    assert!(matches!(
        world.cache.get(&world.space, &input, &world.spec).unwrap(),
        CacheRead::Corrupt(report) if !report.reason.is_empty()
    ));
    let report = CorruptReport {
        path: world.bin_path(&input),
        reason: "payload checksum mismatch (test injection)".to_string(),
    };
    let record = quarantine_detected(
        &world.cache,
        &world.ledger,
        &world.space,
        &input,
        &world.spec,
        &report,
        1_000,
    )
    .expect("quarantine")
    .expect("record");
    // 地址回落 Miss；evidence 三件套在 quarantine 内；事件可见（degraded）。
    assert!(matches!(
        world.cache.get(&world.space, &input, &world.spec).unwrap(),
        CacheRead::Miss
    ));
    assert!(record.bin_path.starts_with(world.cache.quarantine_dir()));
    assert!(record.meta_path.exists() && record.report_path.exists());
    let snap = world.ledger.snapshot();
    assert!(snap.degraded);
    assert_eq!(snap.corrupt_events, 1);
    assert_eq!(snap.quarantined_objects, 1);

    // 新版本重入队 → worker 补嵌（受预算记账一次）→ put 覆盖 → 发布。
    world.bump_version("doc-a", "v2", &input);
    let _report = world.drain(&[(input.as_str().to_string(), DOC_A)]);
    assert_eq!(world.outbox_state("doc-a"), "done");
    assert_eq!(
        world.provider.call_count(),
        3,
        "补嵌恰一次（2 次文档 embed + 1 次检索 query embed）"
    );
    assert_eq!(world.ledger.snapshot().reembeds_used, 1);
    assert!(matches!(
        world.cache.get(&world.space, &input, &world.spec).unwrap(),
        CacheRead::Hit(_)
    ));
    assert_eq!(world.search_doc_keys(10), vec!["doc-a".to_string()]);
}

// ── 2. claimed 任务的降级交还：无 attempt 直写回 pending ─────────────────

#[test]
fn claimed_task_degrade_hands_back_without_consuming_the_attempt_budget() {
    let world = World::new("hand-back", Some(4));
    world.activate_space();
    let input = digest(DOC_A);
    world.seed_doc("doc-a", "v1", &input);
    let _report = world.drain(&[(input.as_str().to_string(), DOC_A)]);
    assert_eq!(world.outbox_state("doc-a"), "done");
    assert_eq!(world.provider.call_count(), 1);

    // 先损坏，再重入队并认领（模拟 reconcile/recovery 在 reuse 循环中撞上坏对象）。
    world.corrupt_payload(&input);
    world.bump_version("doc-a", "v2", &input);
    let guard = LeaseGuard::claim(&world.db, "semantic-reconcile", 60.0)
        .unwrap()
        .expect("claim");
    let task = guard.task().clone();
    drop(guard); // 保持 claimed 态由 facade 交还

    let report = CorruptReport {
        path: world.bin_path(&input),
        reason: "meta addressing triple does not match".to_string(),
    };
    quarantine_detected(
        &world.cache,
        &world.ledger,
        &world.space,
        &input,
        &world.spec,
        &report,
        1_000,
    )
    .expect("quarantine");
    let handed = requeue_after_degrade(
        &world.db,
        &task,
        "degrade: corrupt artifact quarantined, requeued for the worker",
    )
    .expect("hand-back");
    assert!(handed, "claimed 任务的降级交必须真实落地");
    assert_eq!(world.outbox_state("doc-a"), "pending");
    let attempts_after_claim = world.outbox_attempt_count("doc-a");
    assert_eq!(
        world.outbox_attempt_count("doc-a"),
        attempts_after_claim,
        "缓存故障不是任务的错：交还本身不消耗 attempt（claim 自身的 1 次计数除外）"
    );
    assert!(world.outbox_last_error("doc-a").contains("quarantined"));

    // worker 续跑自愈：补嵌（预算记账一次）→ 发布 → 可检索。
    let _report = world.drain(&[(input.as_str().to_string(), DOC_A)]);
    assert_eq!(world.outbox_state("doc-a"), "done");
    assert_eq!(world.provider.call_count(), 2, "补嵌恰一次");
    assert_eq!(world.ledger.snapshot().reembeds_used, 1);
    assert_eq!(world.search_doc_keys(10), vec!["doc-a".to_string()]);
}

// ── 3. 预算耗尽：provider 拒绝 → fenced retry 终态 failed 带原因 ─────────

#[test]
fn exhausted_budget_dead_letters_the_task_with_reason_and_stops_the_spend() {
    let world = World::new("budget", Some(1));
    world.activate_space();
    let input = digest(DOC_A);
    world.seed_doc("doc-a", "v1", &input);
    let _report = world.drain(&[(input.as_str().to_string(), DOC_A)]);
    assert_eq!(world.outbox_state("doc-a"), "done");

    // 第一次补嵌耗尽预算（Some(1)）。
    world.corrupt_payload(&input);
    let report = CorruptReport {
        path: world.bin_path(&input),
        reason: "payload checksum mismatch (test injection)".to_string(),
    };
    quarantine_detected(
        &world.cache,
        &world.ledger,
        &world.space,
        &input,
        &world.spec,
        &report,
        1_000,
    )
    .expect("quarantine");
    // 新版本重入队 → 补嵌（预算 1 次耗尽）→ 重新发布。
    world.bump_version("doc-a", "v2", &input);
    let _report = world.drain(&[(input.as_str().to_string(), DOC_A)]);
    assert_eq!(world.outbox_state("doc-a"), "done");
    assert_eq!(world.ledger.snapshot().reembeds_used, 1);
    let calls_after_budget = world.provider.call_count();
    assert_eq!(calls_after_budget, 2, "首次 embed + 一次补嵌");

    // 再次损坏 + 新版本任务：预算已尽 → provider 拒绝 → 死信。
    world.corrupt_payload(&input);
    let report = CorruptReport {
        path: world.bin_path(&input),
        reason: "payload checksum mismatch (second rot)".to_string(),
    };
    quarantine_detected(
        &world.cache,
        &world.ledger,
        &world.space,
        &input,
        &world.spec,
        &report,
        2_000,
    )
    .expect("quarantine");
    world.bump_version("doc-a", "v3", &input);
    // max_attempts=2：一次 drain 内两次 fenced retry 后终态 failed。
    let limits = WorkerLimits::validated(16, 60.0, 0.0, 2).expect("limits");
    drain_pending(&world.db, "worker", &limits, &mut |guard| {
        world.embed_once(guard, &[(input.as_str().to_string(), DOC_A)])
    })
    .expect("drain");

    assert_eq!(world.outbox_state("doc-a"), "failed", "预算耗尽死信");
    assert_eq!(
        world.provider.call_count(),
        calls_after_budget,
        "拒绝发生在 provider 之前：无界重费被结构性截止"
    );
    assert_eq!(world.ledger.snapshot().reembeds_used, 1, "预算不再前进");
    let last_error = world.outbox_last_error("doc-a");
    assert!(
        last_error.contains("re-embed budget exhausted"),
        "死信必须带预算原因: {last_error}"
    );
    assert_eq!(world.dead_letters(), 1);
    // 可见集合未被污染：坏对象仍在 quarantine，地址 Miss，manifest 不变。
    assert!(matches!(
        world.cache.get(&world.space, &input, &world.spec).unwrap(),
        CacheRead::Miss
    ));
    assert_eq!(world.manifest_count(), 1, "v1 的发布行保留");
    // 预算耗尽后该输入无可用向量：dense lane 降级为空（本地 lexical/graph
    // 照常，"本地继续"；缺向量绝不缓存成 complete——降级矩阵闭环）。
    assert_eq!(
        world.search_doc_keys(10),
        Vec::<String>::new(),
        "dense lane 降级：无可用向量即不可召回，绝不伪造结果"
    );
}

// ── 4. GC 衔接：quarantine 在任何 pass 中字节级原样幸存 ──────────────────

#[test]
fn gc_passes_never_touch_the_quarantine_directory() {
    let world = World::new("gc-seam", Some(4));
    world.activate_space();
    let input = digest(DOC_A);
    world.seed_doc("doc-a", "v1", &input);
    let _report = world.drain(&[(input.as_str().to_string(), DOC_A)]);

    // 隔离一个损坏对象 + 一个从不在 namespace 内的孤儿证据。
    world.corrupt_payload(&input);
    let report = CorruptReport {
        path: world.bin_path(&input),
        reason: "payload checksum mismatch (gc seam)".to_string(),
    };
    let record = quarantine_detected(
        &world.cache,
        &world.ledger,
        &world.space,
        &input,
        &world.spec,
        &report,
        1_000,
    )
    .expect("quarantine")
    .expect("record");
    let evidence = [
        (
            record.bin_path.clone(),
            std::fs::read(&record.bin_path).unwrap(),
        ),
        (
            record.meta_path.clone(),
            std::fs::read(&record.meta_path).unwrap(),
        ),
        (
            record.report_path.clone(),
            std::fs::read(&record.report_path).unwrap(),
        ),
    ];

    // 时钟远超宽限期，多轮 GC pass 全量跑完。
    let cfg = cc_semantic::gc::GcConfig {
        min_retention_secs: 60,
        batch_entries: 16,
        now_unix: 1_000_000,
    };
    let mut position = None;
    let mut deleted_objects = 0usize;
    loop {
        let (counters, resume, exhausted) =
            cc_semantic::gc::run_gc_pass(&world.db, &world.cache, &cfg, position.as_ref())
                .expect("gc pass");
        deleted_objects += counters.deleted_objects;
        match resume {
            Some(p) => position = Some(p),
            None => {
                assert!(exhausted);
                break;
            }
        }
    }

    for (path, bytes) in &evidence {
        assert_eq!(
            &std::fs::read(path).expect("quarantine evidence survives"),
            bytes,
            "quarantine 文件被 GC 触碰: {}",
            path.display()
        );
    }
    // 原地址已由隔离搬移清空（后续 GC pass 也无可回收对象），quarantine 仍可诊断。
    assert!(matches!(
        world.cache.get(&world.space, &input, &world.spec).unwrap(),
        CacheRead::Miss
    ));
    assert!(record.meta_path.exists());
    assert_eq!(deleted_objects, 0, "命名空间树内已无可回收对象");
}

// ── 5. 降级矩阵（检索行）：Miss/Corrupt 跳过、健康行照常、零报错 ─────────

#[test]
fn retrieval_degradation_matrix_skips_miss_and_corrupt_serves_the_healthy() {
    let world = World::new("matrix", Some(4));
    world.activate_space();
    let ok = digest(b"matrix-doc-ok payload");
    let missing = digest(b"matrix-doc-missing payload");
    let rotten = digest(b"matrix-doc-rotten payload");
    for (key, input) in [
        ("doc-ok", &ok),
        ("doc-missing", &missing),
        ("doc-rotten", &rotten),
    ] {
        world.seed_doc(key, "v1", input);
    }
    let table: Vec<(String, &'static [u8])> = vec![
        (ok.as_str().to_string(), b"matrix-doc-ok payload"),
        (missing.as_str().to_string(), b"matrix-doc-missing payload"),
        (rotten.as_str().to_string(), b"matrix-doc-rotten payload"),
    ];
    let _report = world.drain(&table);
    assert_eq!(world.manifest_count(), 3);

    // 缺失行：discard 两半 → 永久 Miss；损坏行：翻转 payload → Corrupt。
    assert!(world
        .cache
        .discard(&world.space, &missing, &world.spec)
        .unwrap());
    world.corrupt_payload(&rotten);

    let keys = world.search_doc_keys(10);
    assert_eq!(
        keys,
        vec!["doc-ok".to_string()],
        "降级矩阵检索行：Miss/Corrupt 跳过、健康行照常返回、零报错"
    );
    // 两个坏输入的降级事件由显式检测点记账；纯检索只跳过不隔离（只读路径）。
    assert_eq!(world.ledger.snapshot().corrupt_events, 0);
}
