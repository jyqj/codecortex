//! P6-014 integration: 换库 incarnation 与缓存重用，端到端。
//!
//! 覆盖 cc-db 门面测试看不到的部分：
//!
//! 1. **付费向量复用（V17 核心，Q5 决策的兑现点）**：rebuild 换 incarnation 后，
//!    同一 cache namespace（不含 incarnation）经 `(space, input, spec)` 校验
//!    直接复用——重建路径 reconcile 对 cache 命中的输入 **0 provider 调用**，
//!    发布走完整五重 fence CAS，manifest 从零恢复到换库前可见集合；
//! 2. **cache Miss 才付费**：未命中输入被 reconcile 留回 pending，worker
//!    （P6-013 EmbedHandler）对且仅对它们调用 provider（call_count 精确断言）；
//! 3. **跨进程旧 inode 幽灵 fence（P6-011 §8 移交专项）**：换库前打开库的进程
//!    在 swap 后变成幽灵——reconcile 判 Fenced 零写入、fenced publish 被拒、
//!    权威库全程零接触；继任进程（新快照）从 cache 复用恢复。
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use cc_db::index_db::IndexDb;
use cc_db::semantic_manifest_reads::SemanticManifestReads;
use cc_db::semantic_outbox::{supersede_and_enqueue_on, ClaimedTask, OutboxPlan, OutboxUpsert};
use cc_model::retrieval::HardScope;
use cc_semantic::cache::ArtifactCache;
use cc_semantic::ports::DocumentInput;
use cc_semantic::providers::fake::{FakeProvider, FakeProviderConfig};
use cc_semantic::publish::Publisher;
use cc_semantic::queue::{drain_pending, EmbedHandler, WorkerLimits};
use cc_semantic::reconcile::{reconcile_after_rebuild, ReconcileOptions, ReconcileVerdict};
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
            "cc-semantic-p6014-{tag}-{}-{n}",
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
    VectorSpace::new("fake/model-reconcile", 2).expect("space")
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
        retry_backoff_secs: 0.0, // 测试内即时可再 claim（revisit guard 兜底）
        max_attempts: 3,
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
        // Q5：namespace 只含项目身份，绝不含 incarnation——rebuild 前后打开
        // 同一 namespace 是复用的前提。
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

    /// 文档投影写进当前库（files → chunks → document_manifest），不挂 outbox。
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

    /// 换库：完整 rebuild 协议（staging 从零 + finalize max(floor,live)+1 +
    /// renew incarnation + 原子 swap）。文档投影由重建闭包恢复；语义表
    /// （spaces/manifest/outbox）按协议从零。raw conn 随旧 inode 一起失效，
    /// swap 后重开。
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

    fn limits(&self) -> WorkerLimits {
        WorkerLimits::validated(16, 60.0, 0.0, 3).expect("limits")
    }

    /// P6-013 worker：drain + FakeProvider + cache + 五 fence CAS。
    fn drain(&self, table: &[(String, &'static [u8])]) {
        drain_pending(&self.db, "worker", &self.limits(), &mut |guard| {
            embed_once(self, guard, table)
        })
        .expect("drain");
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
}

fn embed_once(
    world: &World,
    guard: &cc_semantic::queue::LeaseGuard<'_>,
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

// ── 1. 重建后付费向量复用：0 provider 调用，覆盖率恢复，epoch 语义正确 ────

#[test]
fn rebuild_reconcile_reuses_paid_vectors_with_zero_provider_calls() {
    let mut world = World::new("reuse");
    let d1 = digest(b"body d1");
    let d2 = digest(b"body d2");
    world.activate_space();
    world.seed_doc("d1", &d1);
    world.seed_doc("d2", &d2);

    // 换库前：worker 正常付费路径发布两个文档。
    let before_incarnation = world.incarnation();
    world.drain(&[
        (d1.as_str().to_string(), b"body d1"),
        (d2.as_str().to_string(), b"body d2"),
    ]);
    assert_eq!(world.provider.call_count(), 2);
    assert_eq!(world.manifest_count(), 2);
    let before_generation = world.db.reads().read_generation().unwrap();
    assert_eq!(before_generation.semantic_epoch, Some(2));

    // 换库：incarnation 变化；语义状态（spaces/manifest/outbox）从零；
    // semantic_epoch 起点回到缺席（staging metadata 空，绝不当作 0）。
    world.rebuild(&[("d1", &d1), ("d2", &d2)]);
    let after_incarnation = world.incarnation();
    assert_ne!(before_incarnation, after_incarnation);
    let generation = world.db.reads().read_generation().unwrap();
    assert!(generation.index_epoch > before_generation.index_epoch);
    assert_eq!(generation.semantic_epoch, None);
    assert_eq!(world.manifest_count(), 0);
    assert_eq!(world.provider.call_count(), 2, "换库本身零付费");

    // 组合根职责：重新注册 active space（协议：空间指针随配置，不随库迁移）。
    world.activate_space();

    // 补齐：同一 cache namespace 复用付费向量，0 provider 调用，发布走完整
    // 五重 fence CAS。
    let verdict = reconcile_after_rebuild(
        &world.db,
        &world.cache,
        &world.space,
        &world.spec,
        after_incarnation,
        &options(),
    )
    .expect("reconcile");
    assert_eq!(
        verdict,
        ReconcileVerdict::Applied(cc_semantic::reconcile::ReconcileReport {
            desired: 2,
            enqueued: 2,
            reused: 2,
            requeued: 0,
            visible_changes: 2,
        })
    );
    assert_eq!(
        world.provider.call_count(),
        2,
        "重建路径对 cache 命中的输入必须 0 provider 调用（付费产物保留）"
    );

    // 可见集合恢复 + epoch 语义：从缺席起点，恰 3 次 bump——1 次重入队（期望
    // 集合实际变化，P6-006 效应口径：非零 stat 即 Semantic，与增量写路径
    // apply_file_batch_on 同一约定）+ 2 次可见变化（Q4 diff 各真）。
    assert_eq!(world.manifest_count(), 2);
    let generation = world.db.reads().read_generation().unwrap();
    assert_eq!(generation.semantic_epoch, Some(3));

    // P6-012 读面：覆盖率完全恢复（分母=分子，无 failed/stale）。
    let coverage = world.db.reads().semantic_coverage().unwrap();
    assert_eq!(coverage.coverage.eligible, 2);
    assert_eq!(coverage.coverage.published, 2);
    assert_eq!(coverage.coverage.uncovered, 0);
    assert_eq!(coverage.coverage.failed, 0);
    assert_eq!(coverage.coverage.stale, 0);
    assert_eq!(coverage.coverage.reason, None);

    // 检索面：published 向量经 filtered exact 仍可召回（自匹配 score 1.0）。
    assert_eq!(retrieval_hit(&world, &d1).as_deref(), Some("d1"));
    assert_eq!(retrieval_hit(&world, &d2).as_deref(), Some("d2"));
}

// ── 2. cache Miss 才付费：reconcile 留 pending，worker 恰好补 N 次 ─────────

#[test]
fn cache_miss_inputs_are_left_pending_and_embedded_once_by_the_worker() {
    let mut world = World::new("miss");
    let d1 = digest(b"body d1");
    let d2 = digest(b"body d2");
    world.activate_space();
    world.seed_doc("d1", &d1);
    world.drain(&[(d1.as_str().to_string(), b"body d1")]);
    assert_eq!(world.provider.call_count(), 1);

    // 换库后出现新文档 d2：从未嵌入过，cache 必然 Miss。
    world.rebuild(&[("d1", &d1), ("d2", &d2)]);
    let after_incarnation = world.incarnation();
    world.activate_space();

    let verdict = reconcile_after_rebuild(
        &world.db,
        &world.cache,
        &world.space,
        &world.spec,
        after_incarnation,
        &options(),
    )
    .expect("reconcile");
    assert_eq!(
        verdict,
        ReconcileVerdict::Applied(cc_semantic::reconcile::ReconcileReport {
            desired: 2,
            enqueued: 2,
            reused: 1,
            requeued: 1,
            visible_changes: 1,
        })
    );
    assert_eq!(world.provider.call_count(), 1, "reconcile 自身零付费");
    assert_eq!(world.manifest_count(), 1);

    // d2 的任务留回 pending，由 worker（唯一付费方）消费，恰一次 embed。
    assert_eq!(world.outbox_state("d2"), "pending");
    world.drain(&[
        (d1.as_str().to_string(), b"body d1"),
        (d2.as_str().to_string(), b"body d2"),
    ]);
    assert_eq!(
        world.provider.call_count(),
        2,
        "worker 只对 cache Miss 的输入调用 provider（恰 +1）"
    );
    assert_eq!(world.manifest_count(), 2);
    assert_eq!(world.outbox_state("d2"), "done");
    let coverage = world.db.reads().semantic_coverage().unwrap();
    assert_eq!(coverage.coverage.published, 2);
    assert_eq!(coverage.coverage.uncovered, 0);
}

// ── 3. 跨进程旧 inode 幽灵：fence 拒绝零写入，继任进程从 cache 恢复 ───────

#[test]
fn pre_swap_process_is_fenced_and_the_successor_recovers_from_cache() {
    let mut world = World::new("ghost");
    let d1 = digest(b"body d1");
    world.activate_space();
    world.seed_doc("d1", &d1);
    world.drain(&[(d1.as_str().to_string(), b"body d1")]);
    assert_eq!(world.provider.call_count(), 1);

    // 幽灵进程：换库前打开库、持有连接与旧 incarnation 快照。
    let (ghost, _) = IndexDb::open(world.db.admin().db_path()).expect("ghost open");
    let ghost_snapshot = ghost.reads().read_generation().unwrap().incarnation;

    // 换库进程（world 自己的实例走完整 rebuild 协议）。
    world.rebuild(&[("d1", &d1)]);
    let live_snapshot = world.incarnation();
    assert_ne!(ghost_snapshot, live_snapshot);
    // 幽灵分叉：ghost 的池连接仍读旧 inode。
    assert_eq!(
        ghost.reads().read_generation().unwrap().incarnation,
        ghost_snapshot
    );

    // 幽灵的 reconcile：Fenced，零写入（不重入队、不 claim、不发布）。
    let verdict = reconcile_after_rebuild(
        &ghost,
        &world.cache,
        &world.space,
        &world.spec,
        ghost_snapshot,
        &options(),
    )
    .expect("ghost reconcile");
    assert_eq!(
        verdict,
        ReconcileVerdict::Fenced {
            snapshot: ghost_snapshot,
            path_incarnation: live_snapshot,
        }
    );

    // 幽灵的 publish 尝试同样被 fence 拒绝（fence 在任何事务之前——裸
    // publish_semantic 在幽灵库上连 fence 1 都会通过，这正是权威路径新鲜读
    // 存在的理由；P6-011 移交的跨实例专项即此断言）。
    let ghost_space = world.space.digest().unwrap();
    let ghost_request = cc_db::semantic_publish::PublishRequest {
        task_id: 1,
        lease_token: "ghost-token",
        doc_key: "d1",
        doc_version: "v1",
        input_digest: d1.as_str(),
        space_id: ghost_space.as_str(),
        artifact_ref: "cas.v1:ns:space:in-d1:spec:deadbeef",
        expected_incarnation: ghost_snapshot,
        retry_backoff_secs: 0.0,
        max_attempts: 3,
        now_unix: 901.0,
    };
    let outcome = ghost
        .publish_semantic_fenced(&ghost_request)
        .expect("fenced");
    assert!(!outcome.published);
    assert_eq!(
        outcome.rejection,
        Some(cc_db::semantic_publish::PublishRejection::IncarnationMismatch)
    );

    // 权威库全程零接触：幽灵的 reconcile 与 publish 都没有留下任何状态。
    let fresh_conn = fresh_conn(world.db.admin().db_path());
    let pending: i64 = fresh_conn
        .query_row("SELECT COUNT(*) FROM semantic_outbox", [], |r| r.get(0))
        .unwrap();
    assert_eq!(pending, 0, "幽灵未能在权威库写入任何任务");
    let manifest: i64 = fresh_conn
        .query_row("SELECT COUNT(*) FROM semantic_manifest", [], |r| r.get(0))
        .unwrap();
    assert_eq!(manifest, 0);
    drop(fresh_conn);

    // 继任进程（换库方，快照=新 incarnation）：重新注册空间后从 cache 复用。
    world.activate_space();
    let verdict = reconcile_after_rebuild(
        &world.db,
        &world.cache,
        &world.space,
        &world.spec,
        live_snapshot,
        &options(),
    )
    .expect("successor reconcile");
    assert_eq!(
        verdict,
        ReconcileVerdict::Applied(cc_semantic::reconcile::ReconcileReport {
            desired: 1,
            enqueued: 1,
            reused: 1,
            requeued: 0,
            visible_changes: 1,
        })
    );
    assert_eq!(
        world.provider.call_count(),
        1,
        "继任进程复用付费向量，0 新 provider 调用"
    );
    assert_eq!(world.manifest_count(), 1);
}
