# Bounded DB publication facade — 独立实现与专项 TODO

状态：DB facade 已实现、专项测试通过；未接生产 queue/wiring，性能未知，未验证 cache 或跨层接线。用户已接受此新 API 的组内同时可见、单项 SQL 失败全组回滚、close 等待至多四项 DB publication 的语义；旧单项接口保持不变。

## 固定来源与范围

- 生产 source：`11f5b76273a16b520e6b6e01ad32a0a6743173fb`。
- 本实现直接基于指定 head：`513a98c9a94b15ec77153df41af26fa3c8c0b5e8`，source 为其祖先。
- 设计已从 `9689afbef998cfa8063b4781cc173e47b78b3df8:docs/design/2026-10-03-cache-durability-publication.md` 读取。未把未 accepted 的四 attempt 当成实现或测试依据。
- 本次新增生产代码仅在 `crates/cc-db/src/semantic_publish.rs`；直属新测试 `semantic_publish_group_tests.rs` 与本专项文档/测试日志。`.agents` 为空；仓库无 AGENTS.md / SKILL.md。未委派子 agent。
- `publish_and_ack_on`、原 LifecycleFence、原单项两个 facade 与基线逐字节相同（静态比较通过）。无 schema/dependency/Cargo.lock/config/cache/queue/wiring/runtime 修改。

## API 契约

`IndexDb::publish_semantic_group(&[PublishRequest], &LifecycleFence)` 返回 `CcResult<Option<Vec<PublishOutcome>>>`。

同一个 receiver 是单一 DB owner；方法只接受一个共同 lifecycle，所有请求须带相同 expected incarnation。输入至多四项，重复 task_id、lease_token 或 doc_key 及 mixed incarnation 均在 DB lock/BEGIN 之前返回 InvalidParams。空组明确返回 `Some(vec![])`，不访问 DB 或 lifecycle，即使 lifecycle 已关闭也是如此。

非空组依次执行 write mutex → BEGIN IMMEDIATE → 一次共同 lifecycle permit → 稳定输入顺序逐项原 CAS/ack → 每个 changed 的成功项原 bump 一次 → COMMIT → 返回同序 outcomes。permit 持续到 COMMIT 或错误回滚结束。关闭返回 None 且无写入；已经拿到 permit 的整组 commit 在 close 返回前完成。组内结果一次共同可见；不是旧单项的逐项可见语义。

Live incarnation / token+claimed / source version / input digest 与 non-null encoding / active space 仍由原 CAS 按原优先序处理。不增 lease 时钟门：expired 未 reclaim 的 token 可完成；reclaim 后旧 token 的 ack/retry 均不可影响 successor。拒绝仍是原 fenced retry outcome，可与其他合法项 commit；Q4 不 bump，k 个 changed 产生 epoch +k。调用方仅能在成功 COMMIT 后计 completed。

SQL、ack、epoch 或 COMMIT 错误都不返回任何部分 outcomes，尝试整组 ROLLBACK；COMMIT 错误文本明确提醒结果可能不确定，不能据此声称必定没生效。恢复须核对 current task/token/manifest，并在重试前重新准备/验证外部 artifact，不能盲送失败组旧描述符。

DB API 信任现有 PublishRequest 的 artifact_ref，和旧接口一样；它不构造 cache receipt，也不验证缓存。cache 准备/完整验证与 HTTP 全部在外部，此轮没有任何生产调用者使用新 facade。

## 验证与证据

官方已安装 direct `1.95.0-x86_64-unknown-linux-gnu/bin`；使用安装脚本既定 `CARGO_HOME=/workspace/.cargo`，原 Cargo.lock 的正常官方 registry 下载。首次 shell 未继承该配置，默认 `/home/agent/.cargo` 下载遇只读错误后停止；没有更改 HOME、调用 rustup、改权限或开展 EROFS 实验。

正常 fixture 都是各测试自有 tempfile DB，依据现有 IndexDb 测试惯例用正常 SQLite seed 与 facade。并发可见性/close 测试仅通过 private controlled seam 在第一项完成、COMMIT 前暂停；其他专项测试均直接调用公开 facade。SQL/ack/epoch 的失败来自 fixture SQL trigger；COMMIT 失败来自 fixture deferred foreign key，验证该具体确定失败的回滚，不把它泛化成所有 COMMIT 错误均未生效。

初红：[red.log](bounded-db-publication-evidence/red.log)，只有缺失新 API/private seam 的 E0599，exit 101。初次实现时一个 test 错把原 ack 保留 token 行为当作清 token，6/7 通过；已纠正 oracle、未更改单项行为。最终绿：[green.log](bounded-db-publication-evidence/green.log)，9/9 专项测试通过。

```sh
CARGO_HOME=/workspace/.cargo PATH=/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin:$PATH cargo test --locked -p cc-db --lib semantic_publish::group_tests
CARGO_HOME=/workspace/.cargo PATH=/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin:$PATH cargo test --locked -p cc-db --test semantic_publish --test semantic_lease
```

原单项 publish 11/11、lease 9/9 通过：[regression.log](bounded-db-publication-evidence/regression.log)。日志仅规范化尾部空行，原运行记录保留在 `/tmp/bounded-group-{red,green,regression}.log`；不更改测试输出内容。改变文件 direct rustfmt --check 与最终提交范围 git diff --check 通过。未执行宽泛 runtime/workspace tests、kill/crash、OS 权限/EROFS/GCWAL 实验、真实 provider 或 benchmark。

## 专项 TODO

- [x] <=4、共享 incarnation/lifecycle、重复 task/token/doc 与空组不访问 DB。
- [x] 四项 changed epoch +4；mix success/reject/Q4 duplicate 按输入顺序；其他 clocks 不动。
- [x] 第二项 SQL 错误整组 rollback；拒绝 retry 与更早 success/epoch 也全部回滚。
- [x] ack、epoch、受控 COMMIT SQL 错误不返回部分 outcomes。
- [x] closed 无写；第一项未 commit 对正常 reader 不可见；permit 让 close 等待整组 commit。
- [x] source/encoding/input/space/incarnation/lease fences、拒绝优先序、retry exhausted 计数；expired unreclaimed 可完成，reclaimed 旧 token 不能影响 successor。
- [x] 旧单项发布与 lease 专项回归；原单项实现静态 byte equality。
- [ ] queue ready-frontier/end-flush、custody/report/joins 与生产接线：后续独立 session，当前未做。
- [ ] cache warm 优化与 cache 验证：本轮未做。
- [ ] 性能/资源/实际吞吐：后续另行授权；当前未知，不能宣称提速。
- [ ] draft PR / CI 远端可用性：GitHub CLI REST 和 GraphQL 元数据读取均返回 Forbidden；connector 读取返回 Transport closed。branch push 和 draft 创建分别记录实际结果，不能把推送当成 PR/CI 成功。
