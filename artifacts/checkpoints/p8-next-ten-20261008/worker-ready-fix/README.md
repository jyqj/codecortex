# P7 worker contention：等待静止顺序修复及单次定向验证

源码修复 commit 为 `83dae5a6e12ccf9900971666948bc33a3b99115a`，基线
`6149de92f6790b68eca38bb2b91e4c4de352a2e4`。只修改
`crates/cc-eval/tests/p7_worker_contention.rs` 的两个 wait predicate，
文件新 SHA256 为
`2e42e24fbafcce01144fa89a6ebb02e3460bd2e4d9263e00712df18d718abbc4`。

初始 ready 先在独立语句读取并释放 index guard，非零 query pin 继续原有
bounded wait。最终 drain 先确认旧 provider active 为 0，再同样确认 pin
为 0。只有满足这些静止前提后，才执行原 `semantic_coverage().unwrap()`
与 `published > 0 / uncovered == 0` 判断。没有吞掉 coverage 错误，未改
三次 generation fence、2 秒查询 watchdog、5 秒进度 watchdog、2 毫秒
轮询间隔、seed、32 样本、并发宽度或任何 quiet/held/发布测量断言。

`corpus_wave` 在提交前独立检查最小 diff，结论为
`accepted_scoped_test_readiness_order_fix`，无发现。远端源码和独立 review
的统一登记由主任务完成；本次日志提交不更新 source guard 或 registry。

## 实际结果

只运行原 `p7_worker_contention` test target **一次**，明确启用 `semantic`：

```text
test real_slow_backfill_preserves_local_progress_and_records_every_request ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.62s
```

命令为 Cargo exit 0；原始 stdout JSONL 在 raw 档案内按原字节保留，包括
末尾空行，stderr 也提供顶层副本。编译日志报告
dev profile 用时 1m 26s。三个 seed `[7, 19, 43]` 的 quiet/held 两阶段，
分别按并发 `[1, 4]` 采集原有每格 32 样本，总计 **384 个请求**。
逐格核对每个 ordinal 0–31 均存在，无丢弃样本或再次抽样。
每个 seed 的 protocol、held-before、全部 quiet/held 请求及 summary
都在档案内；原 stale publication、local source hit、pool/provider 等
断言随原测试通过。没有用这个局部验证声明完整 P7-015 或性能认证完成。

## 构建与旧证据隔离

实际命令、环境、compiler artifact、test binary 摘要和源码前后 manifest
见 `test-receipt.json`。本次为 **warm 依赖定向测试**，Cargo 共记录 229 个
compiler artifacts：172 个 `fresh=true`、57 个 `fresh=false`。固定 785 个
Cargo/crate 输入相对之前 cold source 仅上述一个测试文件不同，编译前后
源码身份一致。没有把本次测试解释为新的冷构建格。

复用的 target 是本代理先前独占创建的
`/dev/shm/codecortex-p8-cold-default-ebacee/run-01/linux-1.95-default/target`。
旧 cold source clone 与回执未修改；原 default 二进制前后仍为
`4d9fd5d537c8b49f33b0df88a633c1236296342ae91037f04a2eabb626710f77`。
测试后再次执行原 cold `--verify`，exit 0。
资源观测中 `oom` / `oom_kill` 为 0；没有删除其他会话的缓存或工件。

单文件 Rust 1.95 `rustfmt --check` exit 0；工作区 `cargo fmt --all -- --check`
在本稀疏 checkout 中因缺少另一历史测试引用的
`artifacts/checkpoints/capability-snapshot-optimization-20261003/current-status-under-test.rs`
而退出 1。本轮没有展开该历史快照或改动其他 Rust 文件，不将局部格式检查
记录为全工作区 gate 通过。

## 归档与原失败

`raw-evidence.tar.gz` 保存完整两条 Cargo 输出、实际 compiler artifact、
源码前后 manifest、15 个 measurement 文件、preflight、资源记录、版本
与验证日志。`evidence-index.json` 保存逐文件及 archive 摘要。大测试
二进制和 target 不提交；test receipt 记录原 live 路径及摘要。

原 CI run `37662135135` / job `112932058462` 在初始 coverage 读取处失败的
完整日志继续保留于 `ci/first-p7-job-112932058462.log`，SHA256 为
`f847fe6cbbd5c2741094e9c0a7d0228a71085868460d3024cafab3199ebd0f7d`。
本次本地通过不改写原失败，也不等同于修复后的远端 CI 已完成。

```sh
mkdir /tmp/p7-worker-ready-evidence
tar -xzf artifacts/checkpoints/p8-next-ten-20261008/worker-ready-fix/raw-evidence.tar.gz \
  -C /tmp/p7-worker-ready-evidence
```
