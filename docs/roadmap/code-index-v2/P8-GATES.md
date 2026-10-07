# P8 本地失败门与收据

本轮推进原任务 P8-013，执行现有 benchmark CLI 的失败注入测试。完整 V03/V04/V20 及上游硬依赖仍待验收；以下行为只描述已实现的本地门。

## 修复的两个错误通过路径

`gate::evaluate` 在计划样本数为 0 时返回 `invalid_measurement` / 2。取消仍优先返回 `cancelled` / 3；空的基线不能凭没有失败行而得到可用测量结论。

比较流程继续从锁定 raw 重算报告。若基线 p95 为 0 或缺失，延迟比值为 unavailable，结果必须为 `inconclusive` / 1；样本行数达到策略下限不会使缺失分母成为通过证据。硬件来源缺失、样本不足、质量区间无法证明策略边界也显式写入 `inconclusive_reasons`。

| 条件 | 比较状态 | 退出码 |
|---|---|---:|
| 有效可比输入且所有现有门通过 | passed | 0 |
| 质量或延迟回归、基础可用性门失败 | failed | 1 |
| 样本、硬件或可测延迟不足 | inconclusive | 1 |
| 策略损坏、raw/输入锁漂移或不可比 | invalid_measurement | 2 |

这些门保留既有 family bootstrap 质量统计和所有 attempt 的 p95 点比值。它们没有生成冷热分层的比值 CI，不构成完整尾延迟或发行质量认证。

## 失败收据与不可覆盖输出

`cc-eval compare` 在解析策略与重放工件之前独占创建输出文件。策略或输入锁失败也会保存带 `status`、`exit_code`、`reasons` 及输入路径的 JSON，原始运行目录不被删除。指定输出已存在时拒绝覆盖，包括空文件和符号链接。

```sh
cargo run -p cc-eval --bin cc-eval -- compare \
  --baseline /absolute/runs/base \
  --candidate /absolute/runs/candidate \
  --gate /absolute/policy.json \
  --output /absolute/comparisons/new-result.json
```

原始运行报告重放仍按现有 `report::replay` 更新派生报告。应先在运行目录中完成比较，再用 `p8_release_evidence.py archive` 建立不可覆盖的快照；候选绑定和发行批准是另外的步骤。

## 可复现验证

```sh
cargo test -p cc-eval --test benchmark_cli --test benchmark_reports --locked
```

新增 6 个 CLI/门测试验证：零样本、零基线时延、质量与延迟回归、样本不足、raw 锁漂移，以及坏策略与输出复用。它们创建明确标识的合成 benchmark 行与真实锁定工件，调用实际 CLI，并核对非零退出、失败 JSON 与原始字节保留。夹具数字不是产品性能观测。

修复前 6 项中 4 项失败；修复后 6 项及既有 report 11 项通过。原始执行日志和独立静态审查见 `artifacts/checkpoints/p8-local-waves-20261007/round2/`。

## 增量对账发现并修复的产品反例

P8-006 的 60 文件真实索引测量首先发现 `resolution_manifests` 与全量不同。两文件最小复现确认：数据库中同时存在 `route:*` 原始边和 `route_node:*` 派生节点；旧 dirty reload 将两者都当作解析输入，造成增量 manifest 多一条 route lookup。

修复仅让 `load_file_edges_for_reresolve` 排除派生节点命名空间；原始路由和派生路由仍写入数据库，公开图读接口保持原有数据。回归测试同时要求全部 15 个现有 oracle 表相等、两条路由仍存在及真实 API 调用目标正确。旧代码失败，修复后通过，既有 DB route 7 项也通过。未修改 oracle 投影或降低验收标准。

这个窄修复不新增数据库迁移。已经持久化的旧污染 manifest 不会被纯 no-op 自动改写；现存索引需要后续真实重解析或一次全量重建来更新相关证据。
