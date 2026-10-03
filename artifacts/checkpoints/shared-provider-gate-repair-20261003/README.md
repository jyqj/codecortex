# Shared provider gate repair — 2026-10-03

仅修 provider concurrency gate。基线为已获父接受的 PR117 `098ebd9c08031e0b652e7b021d8abbc9e8b19c3d`（输入中的 `PR117098…` 是 PR 号与 SHA 连写）。分支 `fix/shared-provider-gate-20261003`；不含未接受的 FIFO/parallel 分支。没有更改 cache/DDL/GC/WAL/version/DEV/中央 TODO、queue 或 runtime 调度。`semantic_runtime.rs` 生产改动仅 doc/query 的必要 gate 装配和错误传播，另挂载限定测试文件。

## 关键正确性结果

- **旧实现真实不合格**：固定 parallel production `5cce6eb3af90b79c786d30f349ca6a674dfcd4a1` 中的原反例及其三个辅助 fixture 原样抽取到 `original-counterexample.rs`，在 PR117 基线临时挂入同名 `semantic_runtime::tests` 后运行。真实 assemble + AdmittedProvider 跨 B/C 项目、doc/query 五个调用观察到请求 4/2、实际 5/3。`base-counterexample-workspace-cache.log` 的 `1 passed` 意味着**反例断言成立**，绝非旧 gate 合格。没有改原反例或导入 parallel runtime/queue。`replay-base.sh` 使用独立 /tmp 源树重放；本轮实际运行的基线源码身份见 `identity.json`。未捕获初次基线 binary hash，不能外推为固定二进制证据。
- **修后上限通过**：`repaired-production.log` 的生产工厂测试通过 `assemble_with`、`from_config`、实际 doc factory 与 query context factory 创建旧 unlimited wrappers，然后原地采用 4/2。同一 outer Arc 持续共享；真实 OpenAI-compatible → reqwest → owned loopback HTTP 的 doc/query 跨两个 namespace 同时持有 4 个、各项目 2 个。项目已满而 global 尚有空间时额外 doc 被拒绝；邻项目 query 仍进入；global 已满时另一个 default-after-explicit wrapper 在 HTTP 前被拒绝。四个返回后 in-flight/per-project 归零，新调用成功。不是 snapshot-only 或 fake semaphore 证明。
- **配置语义**：`GateState` 同一个 admission mutex 保护 limits + 一次性 explicit 标记。只有 in-flight=0、project counts 空、waiter queue 空、granted tickets 空时从默认采用 caps；保留 core/Arc、计数规则、cooldown、tickets 和 fair queue，不撤销 permit。已显式配置后 same caps 幂等（busy 时也可），不同 caps 返回 `CcError::Config`；零/unlimited 请求继承已有限制，不能放宽 global cap。只读 getter/snapshot 不提交显式政策。
- **生产绕过闭合**：生产 doc/query 不再因 `max_concurrent=0` 捕获 `gate=None`。两类 wrapper 始终持有 process gate，RetryingProvider 也得到同一 gate 以共享 cooldown。raw `build_embedding_provider[_with]` 仍是文档声明的未装饰 provider seam；实际生产两个消费者均在 `from_config` 装饰。注入 `SemanticRuntime::new` 是原有测试/安装 seam，不宣称它自动装饰任意外部 provider。
- **明确错误**：shared init 返回 Result，`configured_semantic_provider_gate` 统一默认/显式配置，assemble/from_config 用 `?`。新独立进程测试实际检查 `CodeIndex::set_project` busy/conflict 返回 Config 且不发布 subsystem。其既有 `CodeIndex::new`、ProjectServices/ProjectSession 初始化继续通过 `?` 传播；不改错误为默认 provider 或延迟声称已生效。

## 限定验证与保留失败

| 检查 | 结果 / 日志 |
|---|---|
| 原反例 on PR117 | 1 反例断言成立，实际 5/3；`base-counterexample-workspace-cache.log` |
| admission 全限定模块 | 44 passed（含新增 5 tests）；`admission-tests.log` |
| 新真实 global registry / production 工厂 | 4 独立子进程各 1 passed，parent harness 1 passed；`repaired-production.log` |
| 原生产 query + fairness/lifecycle | 6 + 1 passed；`production-regressions.log` |
| 原 raw provider factory | 8 passed；`provider-factory-tests.log` |
| service gate/breaker identity | 2 passed；`service-factory-tests.log` |
| 原 runtime 组 | 6 passed / **1 failed** / 4 ignored；`runtime-tests.log` |
| rustfmt 指定五文件、git diff --check | exit 0；`fmt.log` / receipts |
| cc-semantic + cc-server all-targets semantic-http strict Clippy | exit 0；`clippy-final.log` |

新全局测试由普通 parent harness 自动启动同一 test binary 的独立 `--exact --ignored` 子进程，绝不清空/替换 OnceLock。其他有限测试使用独立真实 ProviderGate：64 次 init/acquire race；并发 same caps、conflict；busy 原 permit 正确释放后采用；Arc core identity/cooldown 保留；残留 waiter/grant/project-count 的三个 guard 为**local white-box 状态检查**，不是实际排队时间线证明。原 admission 的有限公平队列、neighbor、release/cooldown 测试仍通过。

保留的失败：

1. 初次 Cargo 默认缓存位于只读 `/home/agent/.cargo`，exit101（`base-counterexample.log`）；之后使用已有 `/workspace/.cargo`，不重试原拒写目标。
2. 首次修后编译发现 Fn closure 消费捕获 gate，exit101（`repaired-production-compile-failure.log`）；改为 clone 同一 Arc。
3. 原 runtime `post_index_worker_crosses_pages_and_reopen_reuses_artifacts` 使用默认 cache 目录，cache put EROFS，eligible1100/published0，exit101；**未重试该拒写路径、未改 fixture 或断言**。其余成功不能抹掉这项失败，不称完整 runtime/all-tests green。
4. Cargo Clippy 子命令经 PATH 调用了 rustup 代理，初始化只读 `/home/agent/.rustup` 失败 exit1（`clippy-proxy-failure.log`）；直接调用已有真实 `cargo-clippy`，PATH 指向真实 kernel 后通过。未改 HOME/RUSTUP_HOME、权限或凭据。

工具链固定 `/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin`；所有网络请求是 owned synthetic loopback，credential 为 /tmp 下 synthetic-only 文件。没有真实 provider、heldout、100k、GC/WAL kill/crash/fault、拒写目标重试、merge/forcepush/deploy。未做吞吐收益或 parallel 接受结论。完整命令及退出码见 `receipts.json`，输入/最终源码及日志校验见 `identity.json` / `SHA256SUMS`。

## 本任务 TODO

- [x] 精确 PR117 基线、固定 5cce 原反例复现留证。
- [x] 同一 gate/core/mutex 原地采用、busy/refusal、same/conflict/default 规则。
- [x] 生产旧 default wrapper 后续受限、真实 query/doc/global/project/neighbor 验证。
- [x] idle/busy/identity/release/race/ticket safety/default disabled 零调用有限测试。
- [x] 限定 fmt/Clippy/正常 API 回归、保留原失败。
- [x] 源码与证据单独 commit/push、connector 建自有 draft 并核对 head/base。
- [ ] **另列未解：circuit breaker 的 first-wins。** 本修复不改变它、不声称全部 policy 重配已解决。
- [ ] 既有 runtime 默认 cache EROFS 测试仍失败；不在本次权限/源码范围修复。
- [ ] 新 head 全远端 CI / 最终接受由父继续审查；本交付不 merge。
