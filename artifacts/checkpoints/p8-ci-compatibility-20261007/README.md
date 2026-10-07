# PR #145：稳定 Rust 工具链兼容失败原始证据

本目录封存原 PR head `ae906513886bef4501d0a1d2ecffd68ef76c3f5d` 的既有 [CI run 37656213598](https://github.com/jyqj/codecortex/actions/runs/37656213598)。check job 实际检出的是 PR 合并提交 `dc608fedae5bbb0b5869cf0acb0c88e160503559`；该 SHA 来自日志中的 `git log -1 --format=%H`，没有把 PR head 当作实际检出提交。

三个 job 均已结束：check 失败，msrv 和 security 成功。check 的格式检查成功，Clippy 在 `p8_load.rs:392` 拒绝新代码的 `Atomic<u64>::fetch_update` 弃用警告。实际稳定工具链为 rustc 1.99.0，命令为 `cargo clippy --workspace --all-targets -- -D warnings`，退出 101。此问题归为本批新增代码的工具链兼容回归；之前本地 1.95 的通过不能替代此工具链的验证。

`check-job.log` 是 GitHub 专用 job-log 接口返回的完整解码日志，保留 ANSI、时间戳和末尾换行；未裁剪、重排或修正。接口返回的最终 run/jobs 快照保存为 JSON，结构化结论见 `receipt.json`。文件大小和 SHA-256 见 `manifest.json`，包括 manifest 本身的总清单见 `SHA256SUMS`。

本收据只证明旧 head 的失败和其具体原因。独立子代理只读了既有 Actions 结果，没有触发云 Codex，没有重跑 Actions，没有改变代码或 CI，也没有执行修复后的本地测试；修复和后续验证应使用独立收据。
