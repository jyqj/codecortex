# P7-016 独立崩溃恢复准备

本块为 prepared；P7-016 依赖 P7-015，后者依赖未完成 P7-014，因此任务保持 todo。P7-014 的 V19 质量、clean holdout custody 与 live 条件仍 blocked。

固定源码 ee46fa564714ef75b2d839e5e37364cd35fd7406，测试用 Unix 真实子进程调用未修改的 IndexDb/LeaseGuard/ArtifactCache/Publisher/recover_scan/EmbedHandler。三份 seed-specific 合成输入，各杀四种持久化边界，ready PID 必须与拥有的 Child handle 相符；kill/wait 验证 signal=9。Drop 仅兜底清理该拥有 child。保留临时 repo/DB/cache 原件，不修改其他数据。

12 次 SIGKILL 全过：未提交 SQL 事务六表均回滚；已 claim 的任务自然到期，reclaim 不消耗新 provider 调用；已落盘 artifact 恢复发布零重复 embedding；manifest 与 ack 已提交时重启不重发。每个 reopen 都 integrity_check=ok、foreign_key_check 无行。九个任务终态 done、manifest 唯一、semantic_epoch 恰好 +1；每场景三次后续恢复无操作、无费用调用/epoch/claim 增量。总 fake provider 调用每场景恰好 1。

attempt_count 是 claim 次数：claim-crash/recovery/cache-miss handback/worker 合计 3；artifact-crash/recovery 合计 2；published 为 1；不等同 provider 调用。恢复和 handback 没有主动消耗错误预算，但认领本身按既有协议计数。本块不是真实费用 ledger、未知 crash 后付费是否发生的证明。

一个 parent test passed，child entry 在常规 runner 中 ignored，但本轮显式执行并杀死 12 次。相关原测试 publish_cas、queue_worker、semantic_recovery 共 19 passed/0 failed，strict scoped clippy 与 workspace fmt 通过。原生产代码、Cargo、原测试未改。

仅验证完成边界的 process kill/reopen，不证明 put/CAS 内部任意断点或 power loss。staging rename、GC/publish、cache corrupt/delete、完整 retry/close/rebuild/delete 组合及 production stdio 启动恢复待后续。不是完整 V14/V17/P7-016 验收，不重开已接受门。无 live provider/网络/权限扩展/holdout 访问。

receipt.json 记录精确命令、toolchain、binary SHA256 与日志 hash。results.json 与 children/ 是执行原始机器证据；local-original-manifest.json 指向保留的临时 DB/cache 原件及 hashes。原始全编译/fmt日志只在 /tmp 保留，以免再次上传旧凭据样内容；不包含旧被拒日志。

复放请用全新空 output 目录（测试拒绝覆盖旧结果）：
```sh
P7_CRASH_EVIDENCE_DIR=/tmp/p7-crash-replay-new CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=1 cargo test --locked --offline -p cc-semantic --test p7_crash_preparation -- --nocapture
python3 artifacts/checkpoints/cloud-p7-crash-preparation-20261003/verify.py
```
