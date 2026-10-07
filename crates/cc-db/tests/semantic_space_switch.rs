//! P6-017: model space 切换协议 —— `semantic_spaces` 三态状态机守卫、
//! 单事务切换 + revoke 生产者、revoke 消费（第二生产消费者）、切换审计与
//! epoch 口径。
//!
//! 端到端（FakeProvider：切换→新空间 embed→旧空间 revoke→检索只见新空间→
//! 回滚 0 provider 调用→revoke 后 GC 可回收旧对象）在
//! `crates/cc-semantic/tests/space_switch.rs`。
use cc_db::index_db::IndexDb;
use cc_db::semantic_outbox::OutboxOp;
use cc_db::semantic_space_switch::{
    register_space_on, space_state_on, switch_active_space_on, SpaceState,
};
use cc_model::CcError;

/// Fresh in-memory database at the current schema, foreign keys enforced.
fn v22_conn() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    assert_eq!(
        cc_db::index_migrate::migrate_index_db(&conn).unwrap(),
        cc_db::index_migrate::SchemaStatus::Initialized
    );
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

fn seed_manifest_row(conn: &rusqlite::Connection, doc_key: &str, space_id: &str) {
    let file_path = format!("src/{doc_key}.rs");
    conn.execute_batch(&format!(
        "INSERT INTO files(file_path,language,content_hash,mtime,size,indexed_at) \
         VALUES('{file_path}','rust','hash',1.0,1,'2026-01-01');
         INSERT INTO chunks(chunk_id,file_path,language,chunk_index,start_line,end_line,text) \
         VALUES('c-{doc_key}','{file_path}','rust',0,1,2,'body');
         INSERT INTO document_manifest(doc_key,doc_version,file_path,chunk_id,encoding_key,\
           reference_json,record_json) \
         VALUES('{doc_key}','v1','{file_path}','c-{doc_key}','enc','{{}}','{{}}');
         INSERT INTO semantic_manifest(doc_key,doc_version,file_path,encoding_key,input_digest,\
           space_id,artifact_ref,published_at,published_incarnation) \
         VALUES('{doc_key}','v1','{file_path}','enc','in-{doc_key}','{space_id}','art-{doc_key}',\
           '2026-01-01','inc');"
    ))
    .unwrap();
}

fn seed_outbox_task(
    conn: &rusqlite::Connection,
    doc_key: &str,
    space_id: &str,
    state: &str,
    op: &str,
) {
    conn.execute(
        "INSERT INTO semantic_outbox(doc_key,doc_version,input_digest,space_id,op,state,\
           available_at,created_at,updated_at) \
         VALUES(?1,'v1','in',?2,?3,?4,0,'2026-01-01','2026-01-01')",
        rusqlite::params![doc_key, space_id, op, state],
    )
    .unwrap();
}

fn count(conn: &rusqlite::Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

fn semantic_epoch(conn: &rusqlite::Connection) -> Option<u64> {
    conn.query_row(
        "SELECT value FROM metadata WHERE key='semantic_epoch'",
        [],
        |r| r.get::<_, String>(0),
    )
    .ok()
    .map(|v| v.parse().unwrap())
}

/// In-tx wrapper: run a `*_on` primitive inside one explicit transaction the
/// way the IndexDb facades do.
fn in_tx<T>(
    conn: &rusqlite::Connection,
    body: impl FnOnce(&rusqlite::Connection) -> cc_model::CcResult<T>,
) -> cc_model::CcResult<T> {
    conn.execute_batch("BEGIN IMMEDIATE;").unwrap();
    let outcome = body(conn);
    match outcome {
        Ok(v) => {
            conn.execute_batch("COMMIT;").unwrap();
            Ok(v)
        }
        Err(e) => {
            conn.execute_batch("ROLLBACK;").unwrap();
            Err(e)
        }
    }
}

// ── 三态状态机：闭转换表 ─────────────────────────────────────────────────

#[test]
fn space_transition_table_admits_exactly_the_three_edges() {
    use SpaceState::*;
    let legal = [(Backfilling, Active), (Active, Revoked), (Revoked, Active)];
    for (from, to) in legal {
        assert!(
            from.can_transition_to(to),
            "{from:?} -> {to:?} must be legal"
        );
    }
    for from in [Backfilling, Active, Revoked] {
        for to in [Backfilling, Active, Revoked] {
            if !legal.contains(&(from, to)) {
                assert!(
                    !from.can_transition_to(to),
                    "{from:?} -> {to:?} must be illegal"
                );
            }
        }
    }
}

// ── register_space_on 守卫 ───────────────────────────────────────────────

#[test]
fn register_inserts_backfilling_and_refuses_any_existing_row() {
    let conn = v22_conn();
    register_space_on(&conn, "sp-new", "{}", 1.0).unwrap();
    assert_eq!(
        space_state_on(&conn, "sp-new").unwrap(),
        Some((SpaceState::Backfilling, None))
    );
    for existing_state in ["backfilling", "active", "revoked"] {
        conn.execute("DELETE FROM semantic_spaces WHERE space_id='sp-x'", [])
            .unwrap();
        conn.execute(
            "INSERT INTO semantic_spaces(space_id,spec_json,state) VALUES('sp-x','{}',?1)",
            [existing_state],
        )
        .unwrap();
        let err = register_space_on(&conn, "sp-x", "{}", 2.0).unwrap_err();
        assert!(matches!(err, CcError::InvalidParams(_)), "{err:?}");
    }
    // 注册是 Auxiliary：不产生 semantic_epoch 键（absent = not ready）。
    assert_eq!(semantic_epoch(&conn), None);
}

// ── switch_active_space_on 守卫与生产者 ─────────────────────────────────

#[test]
fn switch_requires_known_space_and_legal_target_state() {
    let conn = v22_conn();
    // 未知空间拒绝。
    let err = in_tx(&conn, |c| switch_active_space_on(c, "ghost", "rev-1", 1.0)).unwrap_err();
    assert!(matches!(err, CcError::InvalidParams(_)), "{err:?}");
    // 目标已是 active：幂等 no-op（activated=false，零写入）。
    seed_active_space(&conn, "sp");
    let stats = in_tx(&conn, |c| switch_active_space_on(c, "sp", "rev-1", 1.0)).unwrap();
    assert!(!stats.activated && !stats.visible_set_switched);
    assert_eq!(
        space_state_on(&conn, "sp").unwrap().unwrap().0,
        SpaceState::Active
    );
    // 撤销态之外的守卫本身在转换表测试覆盖；这里补 "active → active 走 no-op"
    // 已验，`backfilling|revoked → active` 由下方切换测试覆盖。
}

#[test]
fn first_activation_has_no_previous_active_and_no_epoch_bump() {
    let conn = v22_conn();
    register_space_on(&conn, "sp-1", "{}", 1.0).unwrap();
    let stats = in_tx(&conn, |c| {
        switch_active_space_on(c, "sp-1", "rev-init", 2.0)
    })
    .unwrap();
    assert_eq!(stats.previous_active, None);
    assert!(stats.activated && !stats.old_revoked);
    assert!(
        !stats.visible_set_switched,
        "首次激活不可见集合为空→空，非切换"
    );
    assert_eq!(
        space_state_on(&conn, "sp-1").unwrap(),
        Some((SpaceState::Active, Some("1970-01-01T00:00:02+00:00".into())))
    );
    // 首次激活不触碰 epoch 键（absent = not ready，P6-004）。
    assert_eq!(semantic_epoch(&conn), None);
}

#[test]
fn switch_revokes_old_activates_new_and_produces_revoke_tasks() {
    let conn = v22_conn();
    seed_active_space(&conn, "sp-old");
    register_space_on(&conn, "sp-new", "{}", 1.0).unwrap();
    // 旧空间两个已发布行 + 一个 live embed 任务（应被 supersede）+ 新空间一个
    // live 任务（不得被动）。
    seed_manifest_row(&conn, "d1", "sp-old");
    seed_manifest_row(&conn, "d2", "sp-old");
    seed_manifest_row(&conn, "d3", "sp-new");
    seed_outbox_task(&conn, "d1", "sp-old", "pending", "embed");
    seed_outbox_task(&conn, "d2", "sp-old", "claimed", "embed");
    seed_outbox_task(&conn, "d3", "sp-new", "pending", "embed");

    let stats = in_tx(&conn, |c| switch_active_space_on(c, "sp-new", "rev-2", 2.0)).unwrap();
    assert_eq!(stats.previous_active.as_deref(), Some("sp-old"));
    assert!(stats.activated && stats.old_revoked);
    assert!(stats.visible_set_switched);
    assert_eq!(
        stats.superseded_live_tasks, 2,
        "旧空间 live embed 全部 supersede"
    );
    assert_eq!(
        stats.revoke_tasks_enqueued, 2,
        "每个旧空间 manifest 行一个 revoke 任务"
    );

    assert_eq!(
        space_state_on(&conn, "sp-old").unwrap().unwrap().0,
        SpaceState::Revoked
    );
    assert_eq!(
        space_state_on(&conn, "sp-new").unwrap().unwrap().0,
        SpaceState::Active
    );

    // revoke 生产者：op/state/space 全对，且每个 doc_key 一条。
    let revokes: Vec<(String, String)> = conn
        .prepare("SELECT doc_key,state FROM semantic_outbox WHERE op='revoke' ORDER BY doc_key")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(revokes.len(), 2);
    assert!(revokes.contains(&("d1".into(), "pending".into())));
    assert!(revokes.contains(&("d2".into(), "pending".into())));
    // 旧空间 live embed 已 supersede；新空间 live 任务不受影响。
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM semantic_outbox WHERE space_id='sp-old' AND state='superseded'"
        ),
        2
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM semantic_outbox WHERE space_id='sp-new' AND state='pending'"
        ),
        1
    );
    // 可见集合切换：Semantic 效应由 facade 在 stats 上声明；裸 *_on 不动钟。
    assert_eq!(semantic_epoch(&conn), None);
}

#[test]
fn switch_appends_pinned_false_audit_event_to_metadata_log() {
    let conn = v22_conn();
    seed_active_space(&conn, "sp-old");
    register_space_on(&conn, "sp-new", "{}", 1.0).unwrap();
    in_tx(&conn, |c| {
        switch_active_space_on(c, "sp-new", "user-rev-7", 2.0)
    })
    .unwrap();
    let log: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE key='semantic_space_switch_log'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let events: serde_json::Value = serde_json::from_str(&log).unwrap();
    let arr = events.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["from"], "sp-old");
    assert_eq!(arr[0]["to"], "sp-new");
    assert_eq!(arr[0]["revision"], "user-rev-7");
    assert_eq!(arr[0]["pinned"], false, "未 pin 限制必须随事件记录");
}

#[test]
fn re_switch_of_a_revoked_space_supersedes_stale_live_revokes_before_reenqueue() {
    // 切走后 revoke 任务未被消费（回滚先发生）→ 再次切走时旧 live revoke 任务
    // 必须先被 supersede，INSERT..SELECT 才不撞 semantic_outbox_live_per_doc
    // 唯一索引。
    let conn = v22_conn();
    seed_active_space(&conn, "sp-a");
    register_space_on(&conn, "sp-b", "{}", 1.0).unwrap();
    seed_manifest_row(&conn, "d1", "sp-a");
    in_tx(&conn, |c| switch_active_space_on(c, "sp-b", "rev-1", 2.0)).unwrap();
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM semantic_outbox WHERE op='revoke' AND state='pending'"
        ),
        1
    );
    // 回滚：b → a（revoked → active 边）。
    in_tx(&conn, |c| switch_active_space_on(c, "sp-a", "rev-2", 3.0)).unwrap();
    // 再切走：a → b。d1 的旧 revoke 任务仍是 pending，必须被 supersede 后重产。
    let stats = in_tx(&conn, |c| switch_active_space_on(c, "sp-b", "rev-3", 4.0)).unwrap();
    assert_eq!(
        stats.superseded_live_tasks, 1,
        "stale live revoke 被 supersede"
    );
    assert_eq!(stats.revoke_tasks_enqueued, 1);
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM semantic_outbox WHERE op='revoke' AND state IN ('pending','claimed')"),
        1,
        "同 doc 同空间至多一个 live revoke"
    );
}

// ── consume_revoke_on / facade：fenced 消费与 Q4 口径 ────────────────────

fn facade_db() -> (tempdir::TempDirGuard, IndexDb) {
    let dir = tempdir::TempDirGuard::new("switch");
    let (db, _) = IndexDb::open(&dir.path().join("index.sqlite3")).expect("open index");
    (dir, db)
}

mod tempdir {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    pub struct TempDirGuard(PathBuf);
    impl TempDirGuard {
        pub fn new(tag: &str) -> Self {
            // pid + monotonic per-process counter: two guards created in the
            // same nanosecond (parallel test cases) used to collide on one
            // directory and share an index.sqlite3; the counter cannot.
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let seq = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("cc-db-p6017-{tag}-{}-{seq}", std::process::id(),));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        pub fn path(&self) -> &std::path::Path {
            &self.0
        }
    }
    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

/// Facade world: sp-old active, d1/d2 published under sp-old.
struct RevokeWorld {
    _dir: tempdir::TempDirGuard,
    db: IndexDb,
    conn: rusqlite::Connection,
}

impl RevokeWorld {
    fn new() -> Self {
        let (dir, db) = facade_db();
        let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        seed_manifest_row(&conn, "d1", "sp-old");
        seed_manifest_row(&conn, "d2", "sp-old");
        Self {
            _dir: dir,
            db,
            conn,
        }
    }
}

#[test]
fn consume_revoke_deletes_own_space_row_and_bumps_exactly_on_change() {
    let world = RevokeWorld::new();
    world.db.register_semantic_space("sp-old", "{}").unwrap();
    // register 拒绝已存在行 → 直接用 SQL 置 active（facade 只提供切换）。
    world
        .conn
        .execute(
            "UPDATE semantic_spaces SET state='active' WHERE space_id='sp-old'",
            [],
        )
        .unwrap();
    // 造一条 revoke 任务（切换生产者的等价行）并 claim。
    seed_outbox_task(&world.conn, "d1", "sp-old", "pending", "revoke");
    let task = world
        .db
        .claim_semantic_space("sp-old", "w", 60.0)
        .unwrap()
        .unwrap();
    assert_eq!(task.op, OutboxOp::Revoke);

    let outcome = world
        .db
        .consume_semantic_revoke(task.task_id, &task.token, &task.doc_key, "sp-old")
        .unwrap();
    assert!(outcome.acked && outcome.visible_set_changed);
    // 行被删 + 任务 done + epoch 精确 +1（可见集合变化 → Semantic 效应）。
    assert_eq!(
        count(
            &world.conn,
            "SELECT COUNT(*) FROM semantic_manifest WHERE doc_key='d1'"
        ),
        0
    );
    assert_eq!(
        count(
            &world.conn,
            "SELECT COUNT(*) FROM semantic_outbox WHERE task_id=1 AND state='done'"
        ),
        1
    );
    let epoch_after_first = semantic_epoch(&world.conn);
    assert_eq!(epoch_after_first, Some(1));

    // 幂等方向：对已无行的 doc 再消费一次 → ack 成功但可见集合未变，不 bump。
    seed_outbox_task(&world.conn, "d1", "sp-old", "pending", "revoke");
    let task2 = world
        .db
        .claim_semantic_space("sp-old", "w", 60.0)
        .unwrap()
        .unwrap();
    let outcome2 = world
        .db
        .consume_semantic_revoke(task2.task_id, &task2.token, &task2.doc_key, "sp-old")
        .unwrap();
    assert!(outcome2.acked && !outcome2.visible_set_changed);
    assert_eq!(
        semantic_epoch(&world.conn),
        epoch_after_first,
        "Q4：无变化不 bump"
    );
}

#[test]
fn consume_revoke_never_touches_another_space_row_and_loses_to_stale_token() {
    let world = RevokeWorld::new();
    world.db.register_semantic_space("sp-old", "{}").unwrap();
    world
        .conn
        .execute(
            "UPDATE semantic_spaces SET state='active' WHERE space_id='sp-old'",
            [],
        )
        .unwrap();
    // 文档已被新空间重新发布：revoke 任务必须只删自己空间的行（删不到）。
    world
        .conn
        .execute(
            "UPDATE semantic_manifest SET space_id='sp-new' WHERE doc_key='d1'",
            [],
        )
        .unwrap();
    seed_outbox_task(&world.conn, "d1", "sp-old", "pending", "revoke");
    let task = world
        .db
        .claim_semantic_space("sp-old", "w", 60.0)
        .unwrap()
        .unwrap();

    // 1) 陈旧 token：零写入（行仍在，任务仍 claimed）。
    let outcome = world
        .db
        .consume_semantic_revoke(task.task_id, "forged-token", &task.doc_key, "sp-old")
        .unwrap();
    assert!(!outcome.acked && !outcome.visible_set_changed);
    assert_eq!(
        count(
            &world.conn,
            "SELECT COUNT(*) FROM semantic_manifest WHERE doc_key='d1'"
        ),
        1
    );
    assert_eq!(
        count(
            &world.conn,
            "SELECT COUNT(*) FROM semantic_outbox WHERE task_id=1 AND state='claimed'"
        ),
        1,
        "lost lease 必须零写入"
    );
    assert_eq!(semantic_epoch(&world.conn), None);

    // 2) 正 token：own-space guard 使 delete 计 0，ack 成功但可见集合未变。
    let outcome2 = world
        .db
        .consume_semantic_revoke(task.task_id, &task.token, &task.doc_key, "sp-old")
        .unwrap();
    assert!(outcome2.acked && !outcome2.visible_set_changed);
    assert_eq!(
        count(
            &world.conn,
            "SELECT COUNT(*) FROM semantic_manifest WHERE doc_key='d1'"
        ),
        1
    );
    assert_eq!(semantic_epoch(&world.conn), None, "Q4：未删行不 bump");
}

#[test]
fn space_switch_facades_end_to_end_epoch_and_states() {
    let dir = tempdir::TempDirGuard::new("facade");
    let (db, _) = IndexDb::open(&dir.path().join("index.sqlite3")).expect("open index");
    let conn = rusqlite::Connection::open(db.admin().db_path()).unwrap();
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    db.register_semantic_space("sp-a", "{}").unwrap();
    db.register_semantic_space("sp-b", "{}").unwrap();
    db.switch_semantic_active_space("sp-a", "rev-1").unwrap();
    assert_eq!(db.semantic_active_space().unwrap().as_deref(), Some("sp-a"));
    assert_eq!(semantic_epoch(&conn), None, "首次激活无可见集合变化");

    seed_manifest_row(&conn, "d1", "sp-a");
    db.switch_semantic_active_space("sp-b", "rev-2").unwrap();
    assert_eq!(db.semantic_active_space().unwrap().as_deref(), Some("sp-b"));
    assert_eq!(
        semantic_epoch(&conn),
        Some(1),
        "可见集合切换 → Semantic 效应 bump"
    );
    assert_eq!(
        db.semantic_space_state("sp-a").unwrap().map(|(s, _)| s),
        Some(SpaceState::Revoked)
    );
    assert_eq!(db.live_semantic_revokes("sp-a").unwrap(), 1);

    // 回滚：revoked → active 边（可见集合切回 → 再 bump）。
    db.switch_semantic_active_space("sp-a", "rev-3").unwrap();
    assert_eq!(db.semantic_active_space().unwrap().as_deref(), Some("sp-a"));
    assert_eq!(semantic_epoch(&conn), Some(2));

    // 非法目标：未知空间拒绝（facade 同样走状态机守卫）。
    let err = db
        .switch_semantic_active_space("ghost", "rev-4")
        .unwrap_err();
    assert!(matches!(err, CcError::InvalidParams(_)));
}
