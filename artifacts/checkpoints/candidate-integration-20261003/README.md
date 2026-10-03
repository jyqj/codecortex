# 独立候选：FIFO / parallel / retry / shared gate 限定集成

**仅候选局部验证，最终验收 BLOCKED；不能称完整通过。** 分支 `candidate/integrate-gate-parallel-retry-20261003`。基于已接受正常路径 PR117，并保留 PR121 为真实祖先。父负责最终验收。本轮不做 100k，不改 session/core startup，不 merge / force-push / deploy。

父在本轮追加的独立 gate review 结论：同 Arc 核心限流 bounded pass，但完整验收 **REJECT**。未关闭 **P1**：`ProjectSession::new` 吞 gate `Config` 后回退空 index，并缓存到被拒项目路径；后续指定路径仍 `Ok`。这是父转达的独立结论，本轮未自行复现或修复；精确链路由父向 reviewer 索取，另立修复任务。另一独立复审仍待父收敛。局部测试或 CI 绿色不关闭此 P1。

## 固定来源矩阵

| 来源 | 固定 tip | 生产来源 / 采用方法 |
|---|---|---|
| PR117 已接受正常基线 | `098ebd9c08031e0b652e7b021d8abbc9e8b19c3d` | 真实祖先；完整 DirectWriter canonical SQLite DDL/index restoration、PR113 status v2/schema24/manifest3/HTTP17 CI 协议继承 |
| PR121 shared gate | `0a8169a2c097cf20d90b01f4d7367ebe5857a1a0` | 真实祖先；同 Arc gate adoption、doc/query 工厂、新测试模块完整保留 |
| PR114 FIFO | `52a50730b58e2b59351449fc80f65771b24c26a8` | `c73128b500c2a19daa8a293caa52afb711b03516` + `29095f9` 的 crates 差异三方应用；必要 DEV lock 协议与 tests 继承 |
| PR119 parallel | `8c7c764ac17c5725c08ab2c22a4c2aea87ca35a2` | `5cce6eb3af90b79c786d30f349ca6a674dfcd4a1` 的四文件生产/tests 差异三方应用 |
| PR122 retry | `cec4ad8ef47c6c3fe86cae6a35e357935b94a0ec` | `7bc4935aeb4ff650618e0341e5f42b0e665418c6` 的三文件生产/tests 差异三方应用 |

真实祖先：117→121、114→119→122；117/121 均不是 122 祖先。两线 merge-base = `5ffbadcf48e26523b2eb46beda0d187a2e2e29cd`。所有 fetched tips 与固定 SHA 相符。机器记录见 `sources.json`。

## 最小集成及冲突处理

最终生产集成使用 `git diff <source>^ <source> -- crates` + `git apply --3way --index`，自动合入，无生产冲突；每个候选 commit 消息记录固定原 source，避免把旧分支文件整件恢复到 accepted baseline。FIFO、parallel、retry 生产 delta 分别落在 `277f249`、`9a8be6b`、`2ecb192`，新增组合测试为 `79193f0`。

最初试完整 cherry-pick FIFO 会引入 145 文件/大量历史 artifacts，后续仅 evidence README/SHA256SUMS 冲突；该未推送序列已 abort，候选恢复到精确 PR121，再采用生产/tests-only 三方补丁。最终未复制 FIFO/parallel 的巨量无关测量目录。保留少量历史 README/反例/独立 review 日志用于追溯，完整 artifacts 仍在固定源 PR。

没有整文件覆盖 `semantic_runtime.rs`。`from_config` doc/query 捕获相同 Arc 与 shared test module、parallel `run_round`、retry tests 共存。queue.rs/bounded_parallel.rs 与 PR122 字节相同；admission.rs/service_factory.rs 与 PR121 字节相同；DirectWriter、其 accepted tests、capability_status、CI、Cargo.lock 与 PR117 字节相同。SQL 只加物理 FIFO index，schema/manifest/status 版本未改。DirectWriter 继续引用完整 canonical schema，限定真实重建测试包含新 index restoration，见 `schema-restoration.log` 与最终精确重放。

## 限定组合证据

最终权威证据为 `verified-final/receipts.json` + `identity.json`：真实 official rustc/cargo 1.95.0、argv/exit/log、全部 Rust source SHA256、每个 test binary 的 SHA256/features/package ID 均记录。代码 source commit 为 `79193f0`；其后仅 evidence/中央账目补注，不改生产或 tests。不同历史来源的 binary identity 留在 `history/*identity.json`，不混称当前 binary。首次较早日志缺 binary hash，不用于最终同源身份结论。

| 检查 | 结果 / 有限范围 |
|---|---|
| shared-provider parent harness | 精确 parent 1 passed；7 个隔离子进程各真实 1 passed：原 4 个 production gate cases + 新 3 个组合 case。子测试 ignored 是隔离执行机制，parent 检查真实非零 pass；不是旧反例 exit0 |
| 实际 doc/query + queue/runtime，width0/2 | 各 20 个真实文档，经正常 CodeIndex/build_index、from_config、loopback HTTP、共享 gate、原 cache/publish/CAS；default wrappers 先创建，再 adopt 4/2。width0 实际1、width2 实际2；held 时跨项目 doc/query 共4，单项目≤2；拒绝调用不进入HTTP |
| 普通 503 + claim16 / backoff | 第1轮 claimed16、done15、backoff1、尚ready4；第2轮 done19、ready0、backoff1；下一轮不提前调用。future-backoff wakeup/timer不在此次修复范围 |
| 实际 HTTP close/cancel/join | 两个 started HTTP 时 running=1/pin=1/gate=2；close使取消并拒绝新schedule，物理join后 running/pin/gate=0、claimed/done/pending-token=0、published0 |
| 原 runtime exact tests | 3 passed：普通 retry width2、width0 schedule 继续其他ready，及 held join/factory/pin 生命周期。自有fixture显式cache根；不运行被拒测试或宽泛runtime组 |
| bounded_parallel / queue_worker | 10 + 9 passed；claim16/token独立、wrong/stale lease fence、close、未started refund、ordinary doc/space fence、errors-after-join、partial enqueue、retry继续有限budget |
| FIFO / canonical rebuild | semantic_outbox 20 passed；两个 DirectWriter 精确 schema-restoration/error-isolation tests passed。未运行 GC/WAL fault 测试 |
| strict Clippy / fmt / diff | 直接官方 cargo-clippy，cc-semantic+cc-server all-target semantic-http `-D warnings`；直接官方 rustfmt；diff check，均通过 |

所有 HTTP 为自有 loopback fixture；cache/DB为授权 `/tmp` 或 checkpoint 自有 cache。不使用外部/付费 provider、heldout、原应用执行或 GC/WAL kill/crash/fault，不改 cache fsync、持久化和 schema 版本。

## 必须保留的历史拒绝和失败

- PR119 原 shared-gate 缺陷：default 先初始化后请求4/2被忽略，真实 global5/project3。原反例源码仍在继承 PR121 evidence 的 `original-counterexample.rs` 和 runtime ignored case；原输出保存在 `history/pr119-shared-gate-*.log`。退出0只表示观察到缺陷，**不是修复合格**。此次不在修后源码上运行旧反例。
- PR120 固定 `5cce6eb` 独立 **REJECT/HOLD**：普通 NeedsRetry 后12 rows中10ready滞留。`history/pr120-REVIEW.md`、TODO、identity、final-run/retry-run 均为固定源原字节，结论不擦除、不追 moving head。
- PR122 修复作者的 before/README/identity 保留在 `history/`，不替代独立验收。当前有限正常503组合与schedule回归是新的局部证据。
- 首次 offline build：cache缺reqwest，exit101、not executed；联网补依赖成功。未安装/切换compiler。
- `queue.log`：本轮误写不存在的 target `queue_consumer`，exit101；更正为 `queue_worker`，原日志保留。
- `clippy.log`：一次 cargo clippy误走子命令rustup代理，只读 `/home/agent/.rustup` exit1；没有重试该路径/更改HOME/RUSTUP_HOME/权限。现成真实 cargo-clippy 直接调用通过。
- `verified/`：首版证据解析器将 `10 passed` 中的子串 `0 passed` 误判为未执行，bounded tests实际上 exit0/10passed；修为完整数字解析后在 `verified-final/`重放。原解析失败receipt/log保留，不称产品失败或隐藏工具错误。
- 首次最终全delta diff-check捕获raw transcript末尾空行，见 `diff-raw-log-eof.log`；为保留原日志字节，本目录 `.gitattributes` 只对 `.log` 关闭 blank-at-eof 检查，其余 whitespace/code检查不变。随后候选相对PR121新增delta diff-check通过；相对PR117仍有继承PR121原日志EOF空行告警，历史原字节未改，不称全delta绿色。

复现：仓库根运行 `python3 artifacts/checkpoints/candidate-integration-20261003/replay.py /tmp/cc-candidate-replay`。脚本只编译指定 package/target，并按全名执行runtime相关case，不选宽泛runtime或被拒用例；忽略旧反例不会计作pass。

## 待父验收与交付阻塞

1. 新 P1 startup/session Config吞错+缓存被拒路径：独立gate review完整REJECT，未修，父另立任务；核心gate局部pass不能关闭。
2. 另一独立复审与父组合验收待完成；旧PR120拒绝对旧源仍有效。
3. 性能正式验收/AB/100k、真实live效果、heldout、GC/WAL故障、完整HTTP17协议重跑均 **not_run**；本轮仅继承正常路径已接受协议，未扩大执行。
4. GitHub `gh pr view 120` 返回 `Forbidden`：动作停止，不换路线/身份。候选commit/push、draft create和CI的具体结果以 `delivery.json` 为准，未读CI就不称绿色。
5. 中央任务只追加局部证据和P1限制，P7-020/P8-005保持todo，性能验收不关闭。派生中央TODO由正常plan脚本生成。
