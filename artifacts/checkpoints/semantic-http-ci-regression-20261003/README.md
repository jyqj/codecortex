# 单一 semantic-http CI 回归覆盖

固定基线：PR107 `350aaacf2f24f533321c495cd4a72ea38029f7df`。仅修改 `.github/workflows/ci.yml` 与本目录；生产、原测试、版本、DEV locks、其它 worker 文件均不变。已查找工作区/仓库 AGENTS.md、.agents、.codex 与本地 skill 文件：仓库无 AGENTS.md/SKILL.md，工作区两个指导目录为空；已读 CONTRIBUTING.md。此任务没有适用的专属 CI skill，也没有派子 Agent。

## 新增自动检查

在现有 check job 默认 workspace tests 后插入一个必需步骤，30 分钟上限，调用 `check.py`；每个 Cargo 子命令上限 600 秒。官方 toolchain/cache、已有步骤及其失败行为逐字节保留，无 continue-on-error，无权限变更。失败日志打印到 CI，函数保留真实退出码；测试名称漂移、空集合、缺项、ignored 或非完整成功摘要均失败。

实际命令（每个目标先以相同参数追加 `-- --list` 检查精确名称集合，然后运行）：

```sh
cargo test -p cc-db --lib capability_read::tests:: --locked -- --show-output --test-threads=1
cargo test -p cc-server --features semantic-http --lib capability_status::tests:: --locked -- --show-output --test-threads=1
cargo test -p cc-server --features semantic-http --test p7_v11_ready_epoch_independent_review --locked -- --show-output --test-threads=1
```

DB 无 feature 声明，使用其默认配置；两个 server 目标均显式启用 semantic-http（包含 semantic 与 reqwest）。不能用 default cfg 下的零测试证明 feature 覆盖。完整名称与命令在 `evidence/commands.json`，静态控制在 `evidence/static-control.json`。列表阶段只是发现，不增加通过计数。

## 源码确认的范围

- DB 6 项：同一事务下全部字段的旧快照/提交后新快照；unwired 跳过 semantic counts 且不写；unsupported/error/undefined HAS_MOVED fail-closed；DELETE-journal 普通 rename 与打开文件身份；incarnation 更新拒绝旧身份但普通 epoch churn 可接受；缺 count table 的只读错误不会变零值/部分 ready（新建 minimal fixture，query_only 后不写）。没有 WAL kill/fault。
- Status 10 项：stale/unattached wiring；active space 与 pending/failed/eligible/published 状态；degradation 及 healthy/unattached；query network authority 与 live encoder 生命周期；configured model 与旧 active space；真实正常 FakeProvider worker publication 交错后返回完整旧观察、稳定后 ready；普通 index churn 的一次观察；incarnation 改变的重试与三次身份变动后的保守失败。不是全量 semantic-runtime 测试。
- Stdio 1 项：`ready_snapshot_bounds_query_generation_without_claiming_quiescence`，内部分别覆盖 search/context。Cargo 构建当前 cc-server 产品，通过 `env!("CARGO_BIN_EXE_codecortex")` 启动真实 mcp 子进程；不依赖之前 CI 的 default binary 或外部 binary override。loopback synthetic HTTP 正常查询挂起期间，正常增量 index/status 推进，释放后严格返回 RPC -32603/retryable 的 generation conflict；不内部重试 provider IO；稳定 semantic/local 查询校验当前 source bytes 与 generation，ready 只作为历史观察。无真实 credentials/provider/heldout。

PR109 `0c1e0b3a1b21620aa8f10abf4f48342b8f0458d8` README 已读。其自编 harness 的 7 unique/9 executions 与 3 compiled mutants 没有复制进本 CI，也没有计为自动 CI 通过。

## 明确限制

此步骤覆盖上述既有生产 inline tests 和 stdio oracle，不自动化 PR109 独立 harness 的完整 tuple/priority matrix 或三个 mutants。未证明所有 worker failure/assembly/degradation 组合、任意调度、跨平台/自定义 VFS、serialization 后替库、全量 V11/P7、性能、GC/WAL fault、真实 provider 或 heldout。当前 CI P0 DEV 锁问题属于其它任务，未修改、未重复运行该任务。全量 workspace/runtime/fault 回归未执行。

## 本地结果

最终运行 **17 passed / 0 failed / 0 ignored**（DB 6、semantic-http status 10、semantic-http stdio 1）；发现阶段不计执行信用。`evidence/result-identity.json` 记录官方 Rust/Cargo 1.95.0、当前源码等于基线的证明、实际 test/产品 binary SHA256 与 Cargo fingerprint features（server library/integration/product 均为 semantic + semantic-http，DB 为 []），所有构建沿用 -D warnings。stdio raw JSON 两份分别是 search/context，`evidence/stdio-audit.json` 独立核对两次真实 -32603/retryable conflict 和一次 provider attempt。cargo fmt --all -- --check、git diff --check 通过；没有扩大测试范围。

两次脚本解析失败已保留：第一次 --nocapture 使成功测试诊断插入 progress 行；第二次 --show-output 下真实子进程/tracing FD 输出仍能插入 progress 行。底层测试均成功，但脚本正确保持失败状态直到修正。最终从 libtest 的 canonical successes 名称列表读取实际通过集合，并同时校验精确 summary；不依赖易被诊断打断的 progress 行。不修改生产测试/断言，不 suppress 失败或 warning。两次 preliminary runs 不计入最终 17。

完整 evidence 与远端交付见任务 TODO。完成后停止，不 merge/forcepush/deploy。

单目标日志只规范化 EOF 空行以通过 git whitespace 检查；完整 local-run.log 保留命令与全部诊断内容。
