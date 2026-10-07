# P8 后续十项：实现、证据与 PR 管理

本批按原始任务 ID 推进 `P8-002/003/004/011/012/014/015/016/017/020`，分三轮交付。任务定义、验收条件及硬依赖不变。工程推进不等于完整任务验收；逐轮计数以 [进度表](../../../docs/roadmap/code-index-v2/P8-NEXT-PROGRESS.md) 和 `tasks.json` 为准。

## 第一轮

- `P8-002`：固定公开 DEV admission 的 Git 对象、查询身份、native/compat 投影、源文件与 gold span 审计；实际 183 个输入、301 native / 256 compat。正式多语言 600 条与独立 holdout 认证仍开放。
- `P8-004`：开发语料边界与生产路径签名扫描、原始发现及人工定位。保留 2 条原始 `review_required`，独立 holdout 正文读取为 0；不据此声明语义层面的无过拟合。
- `P8-012`：Linux/macOS × MSRV/stable × default/semantic 八格冷构建编排，编译器、输入、产物与失败证据绑定。本次八格均 `not_run`，不能声称平台认证通过。
- `P8-016`：既有固定 default/semantic 产物的有限离线回滚，四次真实 stdio 启动、未来 schema 注入和受控重建、恢复备份、配置切换。旧产物来源按原收据保留；未知 cache 格式与真实相邻版本包回滚未执行。

[语料/边界证据](corpus/) · [平台/回滚证据](platform/) · [本次源码独立审查](source-review.json)。

## 源码与验证范围

本地固定 Rust 源码 `3f6cca74c1c9da16cd5eca818c1bce57659d1db0` 与已发布 source commit `78ae91eeae6edae6bea29c27f24b251773341c00` 的全部 **785 项 crate/Cargo 输入**相同。独立审查记录固定在 `7f992a0e328aa1f088746dca360d7d5302e2f2e7`，涵盖 16 个相对 P8 候选变动的路径。

整合保留 #144 的 dense coverage、GC、worker 与 strategy 变更，保留 #145 的 P8 工具及证据。额外修复稳定 Rust 的 deprecated atomic API，采用保持限额和溢出语义的 CAS 循环；统计的 nearest-rank 算法归为一个所有者，并以独立整数 oracle 验证临界样本数与重复值。

v10 入口分别重建两套历史接受链，再应用固定审查的顺序差异；两套旧 guard/registry 字节、历史拒绝条件及 CI 原有步骤继续核验。源码准入只证明来源和字节，不继承完整质量、100k 或发布结论。

| 实际执行 | 结果 | 原始证据 |
|---|---|---|
| Rust fmt | 通过 | 同一固定 Rust 输入，无后续 Rust 修改 |
| Atomic 限额与并发测试 | 2 passed / 0 failed | [atomic-budget.log](validation/atomic-budget.log) |
| P8 CLI/load/measurements/route/scale 回归 | 34 passed / 0 failed / 0 ignored | [p8-regression.log](validation/p8-regression.log) |
| 全 workspace、all-targets 严格 Clippy | exit 0 | [clippy.log](validation/clippy.log) |
| 第一轮全部 P8 Python 控制 | 100 passed，包括 61 个本轮新控制 | [round1-python.log](validation/round1-python.log) |
| v10 新源码与 CI 拒绝控制 | 12 passed / 0 failed | [source-v10-tests.log](validation/source-v10-tests.log) |
| 实际 v10 源码准入 | passed，785 inputs | [source-v10-cli.json](validation/source-v10-cli.json) |
| 实际新版历史与任务定义核验 | passed，192 原定义、4 个视图 | [historical-v2.json](validation/historical-v2.json) |
| 本地全 workspace Rust 测试 | 编译阶段因临时空间上限主动停止，exit 130；测试未执行 | [原日志](validation/workspace-tests.log)、[停止回执](validation/workspace-test-stop.json) |

新评测器和服务器真实构建的 [cc-eval witness](validation/cc-eval-build-witness.json)、[codecortex witness](validation/codecortex-build-witness.json) 记录实际命令、二进制摘要、日志和完整输入清单。它们是构建后的源码/产物对应记录，**不是新冷构建收据或编译器密码学证明**。其中服务器命令复用前述测试 target 的依赖，未加 `--no-default-features`；实际 Cargo artifact 的 features 为 `[]`，原始 JSON 保留。

## PR 管理

#144 的固定 head `eb7cdc55aa94c8d6865bed14fa37fff08080af33` 已确认两套 GitHub workflow 成功：
[CI 37657882021](https://github.com/jyqj/codecortex/actions/runs/37657882021)、[P7 engineering 37657882000](https://github.com/jyqj/codecortex/actions/runs/37657882000)。以 expected-head 保护合并成功，main merge 为 `b951f27d3ed50b7755bc2456c6425355f753ec17`，主线剩余 40 项未验收任务。

#145 的原失败保留：stable Clippy 拒绝 deprecated `fetch_update`。本批已修复且严格 Clippy 通过，等待集成 PR 的完整远端 CI。原失败没有改标成功。

其余 43 个旧草稿仍有独立改动，不批量关闭或删除分支。它们的存在不影响本批按原始任务 ID 去重计数。最终 PR 状态与 CI 以实际 GitHub 结果另行追加。
