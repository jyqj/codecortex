# 可恢复 retry 有限 drain 进度修复（独立候选）

## 固定来源与真实反例

实验生产源码冻结在 PR119 `5cce6eb3af90b79c786d30f349ca6a674dfcd4a1`，PR119 evidence head 为 `8c7c764ac17c5725c08ab2c22a4c2aea87ca35a2`。独立 serial base 为 PR114 `52a50730b58e2b59351449fc80f65771b24c26a8`。本候选不代表独立 review 接受，不并入 canonical PR117，不处理另一 owner 的 shared gate blocker。

只添加自有真实 CodeIndex / SemanticRuntime synthetic fixture 后先运行两源。正常空项目 schedule 耗尽实际 cursor（None），随后写入 4 个独立 Rust 文档，通过正常 handlers::core::build_index 生成并调度任务。provider 首次正常返回预期 Timeout，其余 FakeProvider 正常有限成功（50ms latency）。没有手写模拟 drain 循环、裸 idle SQL enqueue 或修改 cursor；没有真实 provider、heldout 或系统故障。

| 来源 | provider calls | done | pending/backoff | idle ready stranded | cursor/requested/running/pins |
| --- | ---: | ---: | ---: | ---: | --- |
| 冻结 parallel | 2 | 1 | 1 | 2 | None/false/false/0 |
| serial base | 4 | 3 | 1 | 0 | None/false/false/0 |
| queue 修后 parallel | 4 | 3 | 1 | 0 | None/false/false/0 |

完整任务 id/state/attempt/available_at/token/last_error 在 `parallel-before-confirmed.log`、`serial-before.log`、`runtime-after.log`。正常 done 保留历史 token；pending 解除 token、无 claimed 遗留。坏项 attempt=1 且 available_at 在未来；正常项 attempt=1。旧 parallel 后续普通 request 能完成剩余 ready 项（总 calls=4）；修后再次普通 request 为0新增 calls，未提前重试坏项。修后每次真实 provider entry 断言 running=true、physical pin=1，idle 时 pin=0。

`runtime-fixture-before.patch` 是两源实际复现的同一 test-only patch。旧反例仍保留为 ignored test（仅在冻结源应用 fixture 后执行），原作者旧 evidence 未改。serial worktree 的生产源码未改，parallel 在证实差异后才修改 queue。

## 最小生产修复

唯一生产改动为 queue.rs 的 NeedsRetry 分支：正常 fenced retry / lease-lost 处理后继续本有限 drain；生命周期已关闭仍 stopped=true、started hand-back 后 break。真正 handler/db Err、cancel、active-space change 的停止/物理 join/error 返回保留。没有第二次 attempt 落账、提前 claim 未到 available_at 的任务或改变 publish fence。

runtime production prefix 与冻结源字节相同（`identity.json`）。runtime 仅新增限定 tests；其它生产文件、wiring/service_factory/admission/providerfactory、cache、DDL、版本、DEV/CI、中央 TODO 及 Cargo.lock 均未改。

## 有限验证

- `queue-final.log`：10 tests passed，包括 mixed retry widths 0/1/2/4、共享16预算及4条续调、20独立token/20独立task、pending backoff、0 ready stranded、关闭时 NeedsRetry 仍停止 claim、真正双 handler error 返回、cancel、spacechange、doc edit、normal enqueue。
- `serial-regression.log`：queue_worker 9 + retry_worker_layering 5 passed。
- `runtime-after.log`：初始1真实 CodeIndex/runtime regression passed；`runtime-final-widths.log`：width0、width2 两个真实 CodeIndex/runtime regression passed，保留的旧源反例 ignored。
- `runtime-lifecycle-regression.log`：3 tests passed；物理 join 前 close 后 pin=1/Running=true，join 后 pin=0；factory销毁/neighbor；局部真实 gate 测试只证明局部 injected gate，shared first-wins 反例 ignored 未运行，也不宣布解决。
- `fmt.log`：官方真实 rustfmt 对3个修改 Rust 文件 --check 成功；源码 diff whitespace check 成功。
- `clippy.log`、追加 width0 后 `clippy-final.log`：cc-semantic/cc-server all-targets semantic-http `-D warnings` 成功，未降低规则；`fmt-final.log` 最终格式检查成功。

没有 AB/100k、GC/WAL kill/crash/fault、全workspace tests 或真实模型质量声明。本修复只处理已经请求的有限 drain 内因新 stopped-on-NeedsRetry 造成的 ready 遗留。无请求裸DB idle enqueue 与既有 future-backoff 唤醒边界仍需原 composition-root 请求；没有常驻 timer 或广改调度。

## 环境与保留失败

直接官方 toolchain `/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin`，cargo/rustc 均1.95.0，未调用 rustup 初始化、改 credential/权限或系统 HOME。CARGO_HOME=/workspace/.cargo，CARGO_TARGET_DIR=/workspace/codecortex/target。首次 offline 缺锁定 reqwest，`parallel-before.log` 保留；按授权从官方 registry 获取，锁未变。

首次 fixture 未覆写默认 cache root，`parallel-before-online.log` 记录 EROFS（成功项也 NeedsRetry，因此不算目标反例）。后改用已有 CODECORTEX_SEMANTIC_CACHE_ROOT seam 的自有 `/tmp/cc-retry-progress-owned-cache`；`parallel-before-owned-cache.log` 记录 fixture 误以为 done 必须清空历史 token 的断言失败，修正为没有 live claimed 且 pending 无 token；没有改变 production token 行为。

一次额外 raw-binary 调用漏传 cache-root env，误选拒写默认目标，日志 `parallel-regression-missing-cache-env.log` 保留，明确这是执行失误，不算有效证据或获准目标重试。另一次 raw-binary 调用可能遇到共享 target 被 serial 构建覆盖，`raw-binary-identity-uncertain.log` 不计入任何源身份结论。所有正式结论只取明确 cargo checkout +日志。

## 实际命令

通用环境：PATH=/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin:$PATH CARGO_HOME=/workspace/.cargo CARGO_TARGET_DIR=/workspace/codecortex/target CODECORTEX_SEMANTIC_CACHE_ROOT=/tmp/cc-retry-progress-owned-cache。

冻结 parallel checkout 应用 before fixture 后：`cargo test --locked --offline -p cc-server --features semantic-http recoverable_retry_frozen_parallel_counterexample -- --ignored --nocapture`。
serial checkout 应用同一 fixture：`cargo test --locked --offline -p cc-server --lib --features semantic-http recoverable_retry_drains_other_ready_documents -- --nocapture`。
修后同一 regression；queue：`cargo test --locked --offline -p cc-semantic --test bounded_parallel -- --nocapture`；旧serial兼容：`cargo test --locked --offline -p cc-semantic --test queue_worker --test retry_worker_layering -- --nocapture`；runtime生命周期：`cargo test --locked --offline -p cc-server --lib --features semantic-http bounded_parallel_ -- --nocapture`。
Clippy：直接官方 `cargo-clippy clippy --locked --offline -p cc-semantic -p cc-server --all-targets --features semantic-http -- -D warnings`，官方 binary 路径同上。

## 父补充 review

只读核对 PR120 `c6e0d13811ad2d315dfc3c797f170ab61c68567f` 元数据：独立 reviewer 对冻结源 REJECT/HOLD，真实有效4/2局部门控+AdmittedProvider观察12项中10 ready遗留，width0亦停；11项独立tests为其证据，不计作本块 tests。此处修复不代其宣布接受，需父按新源码重审。
