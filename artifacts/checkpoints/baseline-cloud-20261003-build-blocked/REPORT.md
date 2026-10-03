# Baseline 独立云 consumer：prepare 阻断，formal_runs=0

本次唯一任务在准备阶段明确失败后停止。没有调用 run-single，没有启动真实产品、32-file smoke 或 100k；没有 readiness/性能结果，不构成 baseline 产品失败或严格 paired 因果提速证据。

固定 driver `800d32d50bfe158cf86366fb913aabef73f8d965`；baseline `513a98c9a94b15ec77153df41af26fa3c8c0b5e8`；candidate `e3c04fed903c4d0e3c26b5d7cf0e055a6f7c5207`。读取 reusable-100k-driver.md/protocol-manifest.json，.agents/.codex 为空，三个 checkout 未发现 AGENTS.md/SKILL.md。26 个无害 driver tests 全部通过（1.574s）。两版精确 clean HEAD、Git blob/source hashes、原 Cargo.lock 与指定 official direct Rust 1.95.0 在准备前核对成功。没有 PR127 DB facade。

## 硬门禁和失败

`/proc/self/cgroup` 为 `0::/`；mountinfo 中已有 cgroup2 mount `/sys/fs/cgroup`、root `/..`，只读。memory.current/stat/events/max、cpu.max、cgroup.procs 可读；cgroup.procs 返回空列表，因此实际直接成员关联和独立性 unknown，不能据此证明无其他进程。初始 memory.current 840040448 bytes；磁盘可用约 31.86 GB，满足 12 GiB guard / 4 GiB reserve。当前 proc mount 中准备前与失败后均未观察到 compiler，仅限该可见范围。

原 driver prepare 调用一次，isolated CARGO_HOME/TMPDIR/target，jobs4、release、semantic-http、--locked 未改。baseline Cargo 在读取 crates.io config.json 时 DNS 解析失败（下载 blake3 依赖前），build exit 101、wall 16.029905441s、guard_stop null，prepare CLI exit 2。candidate build 未开始，未生成 builds-ready，失败 receipt 无 binary/compiler-artifact 身份。没有通过缓存或改路由恢复，也没有修门禁后硬跑。

**执行者环境处理缺陷：**为避免继承 provider secrets，prepare 外层使用 env -i，只提供工具链 PATH、HOME=/workspace、LANG 和 Python 配置；这同时删除了实例已有 HTTP_PROXY/HTTPS_PROXY/NO_PROXY 及小写同名变量。仅检查这些变量名，不读取/发布其值。代理配置被删除可能解释 DNS 失败，但本任务未重试验证这一推断。失败不是已证实的权限拒绝或 driver bug；driver 原文件未修改。应由父级决定是否另立任务保留安全网络配置后执行。

## 观测与证据范围

35 个 build composition 样本包含 build-before/build/build-complete。cgroup 峰值样本 853614592 bytes，是整个已挂载 group 用量，**不是 product RSS**。首样本 anon/file/slab 为 60538880/679841792/101598056 bytes，末样本为 60637184/679972864/101655904 bytes；这些字段存在重叠，不相加。角色 PID/start_time/state/cgroup/root RSS 在原 composition 保留；进程树 CPU/I/O 与 HTTP 产品观测未运行。非原子采样不能证明真实峰值，空成员列表与 namespace 可见性保留 unknown。

没有真实生成、启动、drain、deadline exact counts、cleanup tail、ready、query、C4、normal EOF 或 reopen 数据；这些均 not_run，不能填零或用 build 数据替代。dummy fixture 仅自生成无效值；外层空继承环境，无真实 provider secrets。未运行 heldout/GC/WAL/kill/crash/旧 runtime EROFS；未清 OS cache、改变 cgroup/limits、merge/deploy/accept。

仅收录本次新生成的测试、初始 gate、失败 prepare/build receipt/stderr/composition/cargo 日志。原日志 gzip(mtime=0)，manifest 同时保留原始与压缩 bytes/SHA256。无历史42files、private localdiag、corpus、cache、DB、binary、target；composition 无原始 cmdline。build stderr 是本任务 Cargo 的公开依赖/DNS错误。发布差异仅本目录。
