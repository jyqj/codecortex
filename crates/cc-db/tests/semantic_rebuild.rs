//! P6-014: 换库 incarnation 与缓存重用（协议侧，cc-db 门面）。
//!
//! 职责边界（TASK-BRIEFS P6-014 + ADR-0003 第 150 行）：换库（staging 重建后
//! rename）产生新 incarnation；仍持有旧连接的**另一进程**（旧 inode 幽灵库）的
//! claim/publish 必须被 fence 拒绝——P6-011 记录在案的第 8 节移交项："校验'本
//! 连接读到的 incarnation == 快照'无法覆盖另一进程持旧 inode 的场景，需 P6-014
//! 的跨实例专项测试"。fence 权威判定 = 在**权威路径上新开只读连接**做 strict
//! `ReadGeneration` 读（绝不走 legacy 双钟；幽灵进程自己的池连接读不到换库）。
//!
//! 另含 P6-006 偏差 6 的移交兑现：全量重建路径不挂接 outbox（staging 库
//! `semantic_spaces` 为空，挂接恒 no-op），重建后的 desired 集合重入队由
//! `enqueue_semantic_rebuild_plan` 门面承担（Semantic 效应按 P6-006 口径声明）。
use std::time::Duration;

use cc_db::index_db::IndexDb;
use cc_db::semantic_outbox::{supersede_and_enqueue_on, OutboxPlan, OutboxState, OutboxUpsert};
use cc_db::semantic_publish::{PublishRejection, PublishRequest};
use cc_db::semantic_rebuild::IncarnationFreshness;

// ── harness ──────────────────────────────────────────────────────────────

fn open_db(path: &std::path::Path) -> IndexDb {
    let (db, _) = IndexDb::open(path).expect("open index");
    db
}

fn raw_conn(db: &IndexDb) -> rusqlite::Connection {
    let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    conn.busy_timeout(Duration::from_secs(5)).unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    conn
}

fn seed_active_space(conn: &rusqlite::Connection, space_id: &str) {
    conn.execute(
        "INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES(?1,'{}','active')",
        [space_id],
    )
    .unwrap();
}

fn seed_world(conn: &rusqlite::Connection, space_id: &str, input_digest: &str) {
    seed_active_space(conn, space_id);
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

/// 重建闭包：把同一份 document 世界写进 staging 库（语义表从零——staging 库
/// `semantic_spaces` 为空，这是协议事实，不是测试取巧）。
fn rewrite_documents_into_staging(tmp: &rusqlite::Connection) -> cc_model::CcResult<()> {
    conn_exec(
        tmp,
        "INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) \
         VALUES('src/d1.rs','rust','hash',1.0,1,'2026-01-01')",
    )?;
    conn_exec(
        tmp,
        "INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text) \
         VALUES('c-d1','src/d1.rs','rust',0,1,2,'body')",
    )?;
    conn_exec(
        tmp,
        "INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,encoding_key,\
         reference_json,record_json) \
         VALUES('d1','v1','src/d1.rs','c-d1','enc','{}',\
         '{\"input\":{\"input_hash\":\"in-d1\"}}')",
    )?;
    Ok(())
}

fn conn_exec(conn: &rusqlite::Connection, sql: &str) -> cc_model::CcResult<()> {
    conn.execute_batch(sql)
        .map_err(|e| cc_model::CcError::Database(format!("{sql}: {e}")))
}

fn count(conn: &rusqlite::Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

// ── 1. 跨进程旧 inode：换库后旧进程的 publish/claim 被 fence 拒绝 ─────────

#[test]
fn pre_swap_process_is_fenced_out_of_publish_and_claim_after_the_swap() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.sqlite3");

    // 旧进程 A：开库、播种世界、claim 一个任务、取启动快照。
    let a = open_db(&path);
    seed_world(&raw_conn(&a), "space-1", "in-d1");
    let snapshot = a.reads().read_generation().unwrap().incarnation;
    let task = a
        .claim_semantic("ghost-worker", 60.0)
        .unwrap()
        .expect("claimed on the pre-swap database");

    // 换库进程 B：同一权威路径上完整 rebuild 协议（finalize = max(floor,live)+1
    // + renew incarnation）。
    let b = open_db(&path);
    b.admin()
        .rebuild_with_temp_db(rewrite_documents_into_staging)
        .unwrap();

    // 幽灵分叉已成立：A 的池连接仍读旧 inode（旧 incarnation），权威路径已是
    // 新 incarnation。strict ReadGeneration 两侧各读各的——这正是 P6-011 记录
    // 的覆盖边界：仅比对"本连接读到的 incarnation"无法发现换库。
    let ghost_incarnation = a.reads().read_generation().unwrap().incarnation;
    let live_incarnation = b.reads().read_generation().unwrap().incarnation;
    assert_ne!(ghost_incarnation, live_incarnation);
    assert_eq!(ghost_incarnation, snapshot);

    // fence 读的是权威路径：A 的新鲜度检查必须报 Stale。
    assert_eq!(
        a.semantic_incarnation_freshness(snapshot).unwrap(),
        IncarnationFreshness::Stale {
            snapshot,
            path_incarnation: live_incarnation
        }
    );

    // 幽灵 publish 被 fence 拒绝：拒绝码 IncarnationMismatch，且**权威库零接触**
    // （拒绝发生在任何 BEGIN 之前——向幽灵文件写 retry 只会污染旧 inode）。
    let request = PublishRequest {
        task_id: task.task_id,
        lease_token: &task.token,
        doc_key: &task.doc_key,
        doc_version: &task.doc_version,
        input_digest: &task.input_digest,
        space_id: "space-1",
        artifact_ref: "cas.v1:ns:space:in-d1:spec:deadbeef",
        expected_incarnation: snapshot,
        retry_backoff_secs: 0.0,
        max_attempts: 3,
        now_unix: 901.0,
    };
    let outcome = a.publish_semantic_fenced(&request).unwrap();
    assert!(!outcome.published);
    assert_eq!(
        outcome.rejection,
        Some(PublishRejection::IncarnationMismatch)
    );
    assert!(!outcome.visible_set_changed);

    // 幽灵 claim 同样被拒：Ok(None)（拒绝语义 = "此进程无权再做功"，零写入）。
    assert!(a
        .claim_semantic_fenced("ghost-worker", 60.0, snapshot)
        .unwrap()
        .is_none());

    // 权威库（B 视角）零接触：manifest 空、outbox 空（重建从零）、语义钟未建键。
    let live = raw_conn(&b);
    assert_eq!(count(&live, "semantic_manifest"), 0);
    assert_eq!(count(&live, "semantic_outbox"), 0);
    assert_eq!(b.reads().read_generation().unwrap().semantic_epoch, None);

    // 对照断言（fence 存在的必要性）：幽灵进程自己的池连接仍然读得到它的旧
    // outbox 行——即 fence 1 的"本连接 incarnation == 快照"在幽灵库上会通过，
    // 裸 `publish_semantic` 并不能发现换库；权威判定必须来自权威路径的新鲜读
    // （本 fence 的机制本身）。上面的 ghost_incarnation 断言已固化该分叉。
    let ghost_view = a.reads().read_generation().unwrap();
    assert_eq!(ghost_view.incarnation, snapshot);

    // 新进程（B）不受影响：fence 对快照==路径Incarnation 的正常进程透明。
    assert_eq!(
        b.semantic_incarnation_freshness(live_incarnation).unwrap(),
        IncarnationFreshness::Current
    );
}

// ── 2. 重建后 desired 集合重入队（P6-006 偏差 6 的移交兑现）──────────────

#[test]
fn enqueue_semantic_rebuild_plan_requeues_desired_set_and_bumps_semantic_once_per_change() {
    let dir = tempfile::tempdir().unwrap();
    let db = open_db(&dir.path().join("index.sqlite3"));

    // 语义未配置（无 active space）：整门面 no-op，零钟动——默认路径零付费。
    let upserts = vec![OutboxUpsert {
        doc_key: "d1".into(),
        doc_version: "v1".into(),
        input_digest: "in-d1".into(),
    }];
    let stats = db.enqueue_semantic_rebuild_plan(&upserts).unwrap();
    assert_eq!(stats, cc_db::semantic_outbox::OutboxWriteStats::default());
    assert_eq!(db.reads().read_generation().unwrap().semantic_epoch, None);

    // 配置 active space + 文档（重建闭包已写 document_manifest 的等价播种）。
    let conn = raw_conn(&db);
    seed_active_space(&conn, "space-1");
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
        rusqlite::params!["{\"input\":{\"input_hash\":\"in-d1\"}}"],
    )
    .unwrap();
    drop(conn);
    let epoch_before = db.reads().read_generation().unwrap().semantic_epoch;
    let index_before = db.reads().read_generation().unwrap().index_epoch;

    // 重入队门面：一个 IMMEDIATE 短事务 supersede + insert，且仅在期望集合实际
    // 变化时声明 Semantic 效应（P6-006 口径：非零 stat 即语义状态变化）。
    let stats = db.enqueue_semantic_rebuild_plan(&upserts).unwrap();
    assert_eq!(stats.enqueued, 1);
    assert_eq!(stats.superseded_tasks, 0, "空队列无活跃任务可 supersede");
    let gen = db.reads().read_generation().unwrap();
    assert_eq!(
        gen.semantic_epoch,
        epoch_before.map_or(Some(1), |e| Some(e + 1)),
        "首个真实变化把缺省键从 None 写到 1（绝不当作 0）"
    );
    assert_eq!(
        gen.index_epoch, index_before,
        "重建重入队不属 Index 效应：index_epoch 不动"
    );

    // 收敛性：同 desired 集合重复重入队 → supersede 旧活跃任务再插新任务，至多
    // 一个活跃任务（live_per_doc 唯一索引 + supersede-then-insert），不产生双活。
    let stats = db.enqueue_semantic_rebuild_plan(&upserts).unwrap();
    assert_eq!(stats.enqueued, 1);
    assert_eq!(stats.superseded_tasks, 1);
    let conn = raw_conn(&db);
    let live: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM semantic_outbox WHERE state IN ('pending','claimed')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    drop(conn);
    assert_eq!(live, 1);
    let state: String = raw_conn(&db)
        .query_row(
            "SELECT state FROM semantic_outbox WHERE doc_key='d1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(state, OutboxState::Pending.as_str());
}

// ── 3. 重建 epoch 向量（批次 1 交付的语义表扩展断言）──────────────────────

#[test]
fn rebuild_epoch_vector_advances_and_semantic_epoch_starts_absent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.sqlite3");
    let db = open_db(&path);
    seed_world(&raw_conn(&db), "space-1", "in-d1");
    let before = db.reads().read_generation().unwrap();

    db.admin()
        .rebuild_with_temp_db(rewrite_documents_into_staging)
        .unwrap();

    let after = db.reads().read_generation().unwrap();
    assert_ne!(before.incarnation, after.incarnation);
    assert!(after.index_epoch > before.index_epoch);
    assert!(after.evidence_epoch > before.evidence_epoch);
    // semantic_epoch 起点 = 缺席（None = 语义未就绪，绝不当作 0）：staging 库
    // metadata 为空，finalize 只写 index/evidence 向量 + renew incarnation；
    // 首个可见集合变化（P6-014 补齐侧或后续发布）才把它从 None 写到 1。
    assert_eq!(after.semantic_epoch, None);
    // 重建从零协议：可见集合与 desired 队列都不跨 swap 携带。
    let conn = raw_conn(&db);
    assert_eq!(count(&conn, "semantic_manifest"), 0);
    assert_eq!(count(&conn, "semantic_outbox"), 0);
    assert_eq!(count(&conn, "semantic_spaces"), 0);
    // 但 document 世界（eligible 分母）由重建闭包恢复。
    assert_eq!(count(&conn, "document_manifest"), 1);
}
