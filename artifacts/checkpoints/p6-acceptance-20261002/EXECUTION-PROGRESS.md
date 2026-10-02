# P6-020 执行进度（批次 5：P6 无网络语义底座验收 + G6）

日期：2026-10-02。HEAD：`0de7c890fcb2a152b4b21eafc4d8ad1a2c3885a6`（批次 1~4 交付的未提交工作树之上，零 crates/ 改动）。

## 1. 执行序列

1. 依据精读：`tasks.json` P6-020 逐字、`IMPLEMENTATION-ORDER.md` 批次 5 定义、round09~12 收口审计（接线轮 13 项待办表权威位置 round12）、`P5-GATE.json` 口径先例、`06-VALIDATION.md` V 定义与 G6 行。
2. 证据装配 E1（依赖图/默认包）：默认 `cargo tree -p cc-server -e normal` cc-semantic 边计数 0；`--features semantic` 计 1；cc-semantic 传递闭包无网络客户端 crate；默认/semantic 双口径 `cargo build --locked --offline` 均 exit 0（19.76s / 8.75s）。收据 `E1-dependency-default-package.json`。
3. 证据装配 E2（故障矩阵 + exact oracle 复放）：`cargo test -p cc-semantic --locked --offline` 两轮全绿 117 passed / 0 failed（lib 60 含 fake 六变体故障矩阵与 exact oracle 16 单测；9 集成套件 57 含 publish CAS/queue/recovery/GC/degrade/space_switch/manifest_exact）。收据 `E2-fault-matrix-exact-oracle-RECEIPT.json`。
4. 证据装配 V18 探针：默认构建二进制（不含 cc-semantic）stdio MCP 探针 —— 14 工具名与 schema 契约在册、`semantic_state=not_configured`、`dense_state=disabled`、`dense_reason=provider_and_vector_publication_not_implemented`、未知工具 -32602、search 正常；零网络零 key；`~/Library/Caches/codecortex/semantic` 不存在。收据 `v18-probe-default-binary.json`。
5. 全量回归：`SDKROOT=… cargo test --workspace --locked --offline`，结果见 `regression-workspace.log` 与 GATE `known_failures` 分类。
6. 验收裁决与 G6 定稿：`docs/roadmap/code-index-v2/P6-GATE.json`（本轮证据充分，draft→定稿一体）。
7. tasks.json P6-020 → done + evidence 回填 + execution_note 追加 + `python3 scripts/code_index_plan.py --write` 重派生 + round13 审计。

## 2. 边界声明（不推断、不越权）

- 库层完成、组合根未接线：接线轮 13 项待办表（权威位置 `artifacts/checkpoints/todolist-completion-audit/round12/p6-batch4-closure-audit.json`）不阻塞 P6 库层验收，但在 GATE 中显式列为边界；semantic 默认 disabled 是当前唯一真实生产行为。
- fake 完成不冒充真实 provider 效果：真实 provider、带可重放 seed 的全故障矩阵正式回归（P7-016）、真实子进程 SIGKILL 级故障正式化，均归属 P7 线，本轮只复放既有矩阵。
- 进程级故障：P6 现状为进程内 kill 模拟（round11 移交口径为"归验收轮正式化"），本轮如实记录为未做的进程级正式化并标注 P7-016 归属，不越权补建故障基础设施。
- 不推断 P7+/G7/G8/G9 任何门；G5/M2 维持 P5 既有本地口径。
- V 系列裁决为"库层/本地范围"口径（passed_local_scope），非发行认证。

## 3. 红线遵守

未 git commit；未放宽任何断言/阈值；全程 `--offline`（零网络成立）；GATE 显式声明"库层完成、组合根未接线、semantic 默认 disabled"。
