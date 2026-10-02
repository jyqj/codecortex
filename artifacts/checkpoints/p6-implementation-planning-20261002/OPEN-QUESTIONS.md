# P6 实施前开放问题（需用户/ADR 裁决）

> 只读规划发现，未擅自裁决。按对实施顺序的影响排序。

## Q1｜P5-020 未完成，P6 全链前置不成立（最高优先）

`tasks.json`（tasks.json:45）`current_phase=P5`、`next_task=P5-019`（in_progress）、
`P5-020 status=todo`；而 P6-002 的 `depends_on` 是 P6-001，P6-001 `depends_on
["P5-020"]`，P6-006/P6-011/P6-016/P6-020 也直接依赖 `P5-020`。ADR-0003 第 193-197 行
已如实记录该冲突。**需裁决**：P5-019 收口与 P5-020/G5 认证的排期；在此之前 P6 批次 1
是否允许任何形式的先行（本规划默认：不允许，见 `IMPLEMENTATION-ORDER.md` 批次 0）。

## Q2｜冻结闭包基线数字不一致：629 vs 623

委托方口径为"629 文件冻结闭包"；仓库内 `06-VALIDATION.md:7` 记载"冻结 623 文件、
6675050 字节，摘要 `44b30ae1…`"（证据 `artifacts/benchmarks/p5d-20260930-resume/final-v3`），
未找到 629 的记载。**需裁决**：批次 0 重认证以哪个口径为基线；若 629 是更新后的闭包，
其 digest/证据目录在哪。在裁决前，本规划按"批次 0 重新冻结、以届时实测为准"编写。

## Q3｜"默认包"验证口径与 workspace 全量编译的关系

P6-002 验收"默认编译/启动不拉网络模型实现"，但 cc-semantic 加入 workspace members
（Cargo.toml:3-11）后 `cargo test --workspace` 必然编译它。V21 的"默认/semantic 两包"
（`06-VALIDATION.md:41`）需明确操作定义：是指 `cargo build -p cc-server`（默认
feature）依赖图不含 cc-semantic（`cargo tree` 证明），还是指安装产物二进制？这决定
P6-020 的检查脚本与 P6-002 的测试写法。建议口径：**默认 feature 依赖图 + 产物二进制
双查**，需确认。

## Q4｜WriteEffect 组合与 semantic_epoch 的推进语义细节

两处需 ADR/owner 拍板：

1. P6-006 源码事务同时改 index 内容与语义可见集合，需组合效应
   `{Index, Semantic}` 同 commit 各 bump 恰好一次。ADR 第 94-95 行只按单类别表述
   （"Index/Evidence 必 bump；Semantic 按语义规则"），未写组合；本规划取"一笔提交
   可携带效应集合"（`TASK-BRIEFS.md` P6-004），需确认是否符合 ADR 意图。
2. "Semantic 按语义 epoch 规则推进"中，重复 ack（可见集合未变）是否绝对不 bump：
   本规划取"只在可见集合实际变化时 bump"，与 P6-012 steps"只可见集合变化 bump"
   一致，但意味着 publish 的 ack 路径要做变化检测（`TASK-BRIEFS.md` P6-011 风险节），
   复杂度换取缓存不无谓失效。需确认取舍。

## Q5｜artifact cache 的存放位置与 namespace 定义

ADR 只定"内容寻址、跨项目默认隔离"（第 100-103 行），未定：根目录（项目内隐藏目录
vs 用户级缓存目录）、namespace 的构成（项目绝对路径 digest？`index_incarnation`？
配置 project_id？）、是否随 `index.sqlite3` 删除而清理。这直接影响 P6-008 路径
布局、P6-014"重建不误删付费向量"（若 namespace 含 incarnation，重建即换 namespace，
复用逻辑要跨 namespace 查找）与 P6-017 回滚复用。**建议 namespace 绑定项目身份而非
incarnation**，需用户裁决（涉及用户磁盘偏好，不宜代行）。

## Q6｜是否放宽 P6-008 ← P6-007 依赖以增加批次 2 并行度

`tasks.json` 中 P6-008 `depends_on P6-007`，但 cache 的实现只消费 P6-003 的 spec
digest 与文件系统，不消费 claim/lease 状态。放宽该边可让 008 与 006/007 并行。
放宽属 `tasks.json` 变更（依赖语义是任务状态机的一部分），须 owner 明确批准，本规划
保持原边。

## Q7｜Schema 版本号与发布节点

P6-005"按发布节点合并 schema 版本"：本规划取**单次 21→22**（全部语义表一次进 v22，
`TASK-BRIEFS.md` P6-005）。若"发布节点"指 P6 内多个 gate（如 P6-B/P6-C 各一节点），
则可能拆 v22（manifest/outbox/spaces 表）与 v23（后续调整）两次 bump，每次都触发
rebuild-on-mismatch 全量重建（index_migrate.rs:49-56），成本与 V21 降级矩阵都不同。
需确认"发布节点"的定义。

## Q8｜`P6-001` 的 `DESIGN.md`/`STORAGE.md` 文本同步时点

ADR-0003 第 173-176 行把章程文本同步挂到"P6-019 文档轮或 owner 收口轮"。本规划默认
归 P6-019（与 STORAGE.md 全量更新同轮，避免两次触碰同一文档的冻结语义）。若 owner
希望提前（例如批次 1 前就消除"无限定单库"表述与 P6-005 实施的时间差），需另行排期，
且注意 docs/ 改动不触发 crates/ 冻结失效。
