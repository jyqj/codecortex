# P7-014 三个冻结反例的修复后独立复核

精确生产基线 `7481731429118f45b6144518e8e9cfad3e02d7b4`（PR27）；修复 commit `1b8a3d54bc28581fd1d908ba5a9c9d155eff1e87`。独立 worktree `/tmp/codecortex-p7-014-recheck`、独立 build target `/tmp/p7-014-recheck-build`，没有混入 CI receipt 分支改动。

复现测试与 PR25 冻结 `dfc4000b53043faaa810254fd831fd98f0d7f070` **字节相同**，SHA256 `7a8548f0717e525d29edfbd143b560049c1f51abba91b13e979658c496d0ae1b`。模型、fixture、交错条件、三个正确断言均未修改；修复树已包含相同测试，本分支仅增本证据目录。

实际独立运行：

```sh
cargo test -p cc-server --features semantic --test p7_runtime_independent_review --locked --offline -- --nocapture
cargo test -p cc-server --features semantic-http --test p7_runtime_independent_review --locked --offline -- --nocapture
```

两 feature 均 **3 passed / 0 failed / 0 ignored**，只合成 in-process provider，不发 HTTP。日志/JSON 分别在 `semantic*`。

结果：发布窗口的 close 后 manifest 保持 0；新 reopened ledger 的 degraded 投影不再被旧 worker 清除；130 文档取消反例为 **1 provider call、1 charged task、0 unstarted input-unavailable retry、0 published、最终 pin=0**。

## 原 race 边界审查

- `cc-db semantic_publish::LifecycleFence` 持有 Mutex 的 permit。claim/publish 都先取得 SQLite IMMEDIATE transaction，再取得 permit；permit 覆盖 claim/publish 写入和 commit。close 将同一 fence 退休。因此 SQLite writer contention 不阻止 close 获取 fence；release writer 后提交检查能拒绝已退休 runtime。这是原反例要求的可线性化提交边界，不是重复非原子 closed if。
- queue 在 cancellation 时显式 `TaskExit::Cancelled`，token-fenced handback；未开始工作的 claim 退回次数，在途工作保留已计的一次，并停止后续 drain claim。原 pending/claimed/failed 和 doc/lease CAS fence 不因取消被重置或绕过。
- `set_semantic_degradation_for` 持有当前 semantic port 的读锁，检查 Arc instance identity，并将该读锁保持至 degradation 更新。teardown/replacement 必须取得同一 port 写锁，所以旧写先于 teardown/new attachment，或被实例检查拒绝；没有 check-then-unlocked-write 的原覆盖窗口。
- provider I/O 发生在 claim transaction/permit释放后；publication permit只保护短 DB transaction，不跨网络。原测试里的实际 DB/CodeIndex 可用及物理 worker pin 释放断言均继续通过。

独立结论仅覆盖这三个已报告反例的修复。没有引用主 owner 的 workspace/HTTP stdio 通过数代替本次测试，也没有继续发散新生命周期条件。未重复整个旧矩阵、全仓 lint 或完整 V18；P7-014 可登记这三项独立 review passed，但整项继续 in_progress，直至其余正式验收完成。
