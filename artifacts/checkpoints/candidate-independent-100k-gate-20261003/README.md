# Candidate 独立云 consumer：前置 build 门禁失败

**正式运行 0 次；真实 smoke 0 次；产品启动 0 次。** 本次唯一 prepare 因 baseline 原 Cargo.lock release build 无法解析 `index.crates.io` 失败，Cargo exit 101、prepare CLI exit 2，耗时 16.026534911s。原日志显示获取 `blake3` 所需 crates.io `config.json` 时 DNS 失败。Cargo 自带网络重试属于该次原命令；没有重新执行 prepare/run-single，没有换源或复用已有缓存。

固定 driver `800d32d50bfe158cf86366fb913aabef73f8d965`（PR128）；baseline `513a98c9a94b15ec77153df41af26fa3c8c0b5e8`；candidate `e3c04fed903c4d0e3c26b5d7cf0e055a6f7c5207`。candidate 不含 PR127 DB facade。两版精确 clean worktree 与 tracked input Git blob/hash 已检查，初始证据见 initial-gates.json.gz。Rust 为 official direct path 的 1.95.0 (59807616e 2026-04-14)，真实工具版本和原命令见 baseline-build-receipt.json；无 compiler artifact/binary hash，不把失败 receipt 当 verified receipt。

26 项无害 driver tests 全通过（1.577s），git diff --check 通过。/workspace/.agents 为空；三个固定 checkout 未发现 AGENTS.md/SKILL.md。读取 driver 文档及原 protocol-manifest 后，使用协议原参数，没有修改 driver。

真实 /proc/self/cgroup 为 0::/；mountinfo 的 cgroup2 挂载点 /sys/fs/cgroup、mount root /..，已有路径 inode 148。memory.current/cgroup.procs/memory.stat 可读；direct members 空，harness 未列出，PID/cgroup namespace 映射不能证明，relation 与 independence 都记 unknown。初始 memory.current 840896512 bytes，低于 12GiB；磁盘 free 约 31.9GB，满足 4GiB reserve；memory.max 为 17179869184，只读且未修改。当前 proc mount 在 prepare 前后均未观察到 rustc/cargo build，不声称全机器证明。

baseline build 已保存 build-before/build/build-complete 同步及周期 composition（含 memory.current/stat/events/max、group identity、角色 PID/start/RSS/membership 读取）。anon/file/slab 字段重叠不相加；这些 cgroup 数值包含 build/cache/runner，不能当 product RSS。生成/startup/drain/cleanup/product CPU/RSS/IO/HTTP 阶段未发生；100k exact counts、300s boundary、tail 和 readiness 均未知/未运行。没有 builds-ready、candidate receipt，故不得进入32 smoke或正式运行。query、C4 repeated/distinct、正常 EOF 与 reopen 全部 not_run。

协议仍为 100000/128dims/parse4/HTTP4/2/claim16，ready300/RPC300/EOF15/overall900/hold120、12GiB guard/4GiB reserve。执行用仅 PATH/PYTHONPATH/PYTHONDONTWRITEBYTECODE 的环境；未加载 provider secrets，未用真实 key。未清 OS cache、改 cgroup、提高限额、运行 heldout/GC/WAL/kill/crash 或已拒绝 runtime 路线。未开展因果或性能比较。

本目录只保存本任务新 manifest/日志/receipt/报告；压缩日志保留原字节 SHA256 于 task-manifest.json，压缩件和其余文件由 SHA256SUMS 覆盖。无旧42files、private localdiag、corpus/cache/DB/binary/target 或非本任务进程原始 cmdline。该报告记录明确前置失败，不是 readiness/100k 验收或 acceptance。
