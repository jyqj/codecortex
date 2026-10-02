//! Model space 切换的三段编排（P6-017）：回填 → 切 active → 撤销/回滚。
//!
//! 边界权威：`docs/adr/0003-semantic-persistence-single-db-boundary.md`
//! （P6-017 行："新空间回填/切 active/撤销三段；不同空间分数永不混排；旧
//! cache 经校验可回滚复用"）与 TASK-BRIEFS P6-017（steps 1-3 + "记录用户
//! revision 与未 pin 限制"）。本模块只做编排与冻结：三态状态机、切换事务、
//! revoke 生产者/消费者全部是 cc-db 门面（`cc_db::semantic_space_switch`），
//! 这里把它们与冻结 spec（[`VectorSpace`]/[`DocumentEncodingSpec`] digest →
//! `space_id`）组合起来。
//!
//! 模块归属偏差（按批次红线记录，先例同 P6-016 sweep）：简报把编排归
//! `spec.rs + reconcile.rs`，但两者分别是 P6-003/P6-014 的已封存交付物，
//! 红线要求"不改既有交付物（只调用/组合）"，故编排落本新模块；`reconcile.rs`
//! 对 revoke op 的"不在此消费"行为（P6-014 交付注记）保持不变——本模块的
//! [`drain_space_revocations`] 才是 revoke 任务的第二生产消费者。
//!
//! ## 三段协议（简报 steps 1-3）
//!
//! 1. **回填**：[`register_backfill_space`] 冻结新 spec 为 `backfilling` 行；
//!    [`enqueue_backfill`] 把全量 desired 集按新 space 入 outbox。期间 dense
//!    lane / worker 只读 active 空间（`claim_semantic` 固定 active 指针、
//!    `scan_space` 固定 space_id），不同空间分数永不混排在读取层结构性成立。
//! 2. **切 active**：[`activate_space`] 单短事务完成 旧 active→revoked +
//!    新空间→active + 旧空间 live 任务 supersede + 每 manifest 行一个
//!    `op='revoke'` 任务 + 审计事件（revision、`pinned:false`）；
//!    `semantic_epoch` 恰在可见集合切换时 bump（Q4 口径，cc-db 门面声明）。
//! 3. **撤销/回滚**：撤销 = [`drain_space_revocations`] 消费 revoke 任务
//!    （own-space manifest 行删除 + fenced ack，删行才 bump）；回滚 = 再次
//!    [`activate_space`] 指回旧空间（`revoked → active` 边），随后照常走
//!    rebuild reconcile（P6-014）：desired 重推 + `cache.get` 校验复用，
//!    缺失部分才回到 worker 付费回填。
//!
//! 未 pin 限制：本模块提供的协议不保证"未 pin 的配置 revision"跨版本行为
//! 不变；每次有效切换把 `{at, from, to, revision, pinned:false}` 追加进
//! `semantic_space_switch_log`（metadata 键，schema 红线内唯一载体）。
//!
//! 无常驻进程（ADR 红线）：三段都是显式调用；何时触发切换/回填/回收归
//! 组合根（接线轮），本模块是库层协议。

use cc_db::index_db::IndexDb;
use cc_db::semantic_outbox::{OutboxOp, OutboxUpsert, OutboxWriteStats};
use cc_db::semantic_space_switch::SpaceSwitchStats;
use cc_model::CcResult;

use crate::spec::{DocumentEncodingSpec, VectorSpace};
use crate::types::SpaceDigest;

/// 冻结 spec 的 `spec_json` 载荷（`semantic_spaces.spec_json` 列）：
/// `serde_json` 序列化 struct——字段序即定义序、无 map，满足 C03 canonical
/// 序列化约定（固定字段序、长度分隔/版本化由 digest 公式另行保证）。
pub fn space_spec_json(spec: &DocumentEncodingSpec) -> CcResult<String> {
    spec.validate()?;
    serde_json::to_string(spec)
        .map_err(|e| cc_model::CcError::InvalidParams(format!("spec not serializable: {e}")))
}

/// 三段之第 1 段（状态载体）：校验并冻结新 spec，注册 `backfilling` 行，
/// 返回其 `SpaceDigest`（即 `space_id`）。已存在的行（任意状态）拒绝——
/// 回滚复用走 `revoked → active` 边，不重注册。
pub fn register_backfill_space(db: &IndexDb, spec: &DocumentEncodingSpec) -> CcResult<SpaceDigest> {
    spec.validate()?;
    let digest = spec.space().digest()?;
    db.register_semantic_space(digest.as_str(), &space_spec_json(spec)?)?;
    Ok(digest)
}

/// 三段之第 1 段（队列半）：把回填 desired 集按新 space 入 outbox
/// （embed 任务落新空间；切换前 worker 永不认领它们）。epoch 口径随 cc-db
/// 门面：计划改变了队列状态才 bump（P6-006 Q4 约定）。
pub fn enqueue_backfill(
    db: &IndexDb,
    spec: &DocumentEncodingSpec,
    desired: &[OutboxUpsert],
) -> CcResult<OutboxWriteStats> {
    let digest = spec.space().digest()?;
    db.enqueue_semantic_backfill_plan(digest.as_str(), desired)
}

/// 三段之第 2 段：切 active（单事务，见 `cc_db::semantic_space_switch` 模块
/// 文档）。`trigger_revision` 是调用方的配置 revision，随审计事件落库。
pub fn activate_space(
    db: &IndexDb,
    space: &VectorSpace,
    trigger_revision: &str,
) -> CcResult<SpaceSwitchStats> {
    space.validate()?;
    let digest = space.digest()?;
    db.switch_semantic_active_space(digest.as_str(), trigger_revision)
}

/// [`drain_space_revocations`] 一轮的产出。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RevocationDrainReport {
    pub claimed: usize,
    /// fenced 消费成功（ack 落地）的 revoke 任务数。
    pub revoked: usize,
    /// 其中真正删除了 manifest 行（可见集合变化，各 bump 一次）的数目。
    pub visible_changes: usize,
    /// claim 后 pre-work renew 失败 / ack 遭遇 lease lost（跳过，零写入）。
    pub lease_lost: usize,
    /// 非 revoke 任务或消费错误经 fenced retry 交还队列的数目。
    pub retried: usize,
}

/// 三段之第 3 段（撤销）：显式驱动一个（通常已 `revoked` 的）空间的 revoke
/// 任务直到批界或队列耗尽。每任务：reclaim 过期 lease → 按空间认领 →
/// pre-work renew（活性门）→ fenced 消费（own-space manifest 删除 + ack +
/// 删行才 bump，全在一个 `IMMEDIATE` 事务）。 Auxiliary 之外唯一可能的
/// epoch 运动在消费事务内、且只由实际删行声明（Q4 口径）。
///
/// 与 P6-013 [`crate::queue::drain_pending`] 的分工：embed 消费永远只对
/// active 空间；本函数只为撤销窗口服务，认领的是显式给定的空间。
pub fn drain_space_revocations(
    db: &IndexDb,
    space_id: &str,
    owner: &str,
    lease_secs: f64,
    backoff_secs: f64,
    max_attempts: u32,
    max_batch: usize,
) -> CcResult<RevocationDrainReport> {
    if max_batch == 0 {
        return Err(cc_model::CcError::InvalidParams(
            "revocation drain max_batch must be at least 1".into(),
        ));
    }
    if lease_secs <= 0.0 {
        return Err(cc_model::CcError::InvalidParams(
            "revocation drain lease_secs must be positive".into(),
        ));
    }
    let mut report = RevocationDrainReport::default();
    for _ in 0..max_batch {
        db.reclaim_expired_semantic()?;
        let Some(task) = db.claim_semantic_space(space_id, owner, lease_secs)? else {
            break;
        };
        report.claimed += 1;
        // 活性门：claim 与消费之间被 supersede/reclaim 的任务直接跳过。
        if !db.renew_semantic_lease(task.task_id, &task.token, lease_secs)? {
            report.lease_lost += 1;
            continue;
        }
        if task.op != OutboxOp::Revoke {
            // 防御：撤销窗口里该空间的队列只应含 revoke；出现其他 op 交还
            // 队列（fenced retry），绝不误消费。
            db.retry_semantic_task(
                task.task_id,
                &task.token,
                "space-switch: non-revoke task in a revocation drain",
                backoff_secs,
                max_attempts,
            )?;
            report.retried += 1;
            continue;
        }
        match db.consume_semantic_revoke(task.task_id, &task.token, &task.doc_key, space_id) {
            Ok(outcome) if outcome.acked => {
                report.revoked += 1;
                report.visible_changes += usize::from(outcome.visible_set_changed);
            }
            Ok(_lost) => report.lease_lost += 1,
            Err(e) => {
                db.retry_semantic_task(
                    task.task_id,
                    &task.token,
                    &format!("revoke consume failed: {e}"),
                    backoff_secs,
                    max_attempts,
                )?;
                report.retried += 1;
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::VectorSpace;

    fn spec() -> DocumentEncodingSpec {
        let space = VectorSpace::new("fake/model-switch", 2).expect("space");
        DocumentEncodingSpec::new(space, None, 8_192, "fake-tokenizer").expect("spec")
    }

    #[test]
    fn spec_json_is_canonical_struct_serialization_and_stable() {
        let a = space_spec_json(&spec()).expect("json a");
        let b = space_spec_json(&spec()).expect("json b");
        assert_eq!(a, b, "冻结序列化必须逐字节稳定");
        // 字段序固定、无 map：space 在前，tokenizer/max_tokens 在后。
        let expected_prefix = "{\"space\":{\"model_id\":\"fake/model-switch\"";
        assert!(
            a.starts_with(expected_prefix),
            "unexpected canonical form: {a}"
        );
    }

    #[test]
    fn spec_json_digest_pairing_is_stable_across_reconstruction() {
        // spec_json 稳定 ⇒ space_id（digest）与冻结载荷一一对应：切换协议
        // 靠这一点保证"同一 spec 重新注册必然解析到同一 space_id"。
        let first = space_spec_json(&spec()).expect("json first");
        let second = space_spec_json(&spec()).expect("json second");
        assert_eq!(first, second);
        assert_eq!(
            spec().space().digest().expect("digest a"),
            spec().space().digest().expect("digest b")
        );
    }
}
