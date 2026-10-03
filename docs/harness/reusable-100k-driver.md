# 固定 PR126 可复用 100k driver（本轮仅准备）

入口：`PYTHONPATH=scripts python3 -m resource_harness.driver`。
精确合同在 `scripts/resource_harness/protocol-manifest.json`；参数不可改规模、
seed、并发、timeout 或 guard。import/help/manifest 不启动进程。
`prepare` 显式构建两版，`run-single` 显式执行前置 smoke 加一个 variant 的一次正式测量。
没有自动运行、CI 接入、双版循环、自动 accept、merge 或部署。

源与历史固定：

| 角色 | head |
| --- | --- |
| 原协议/scripts/证据 PR126 | `da8b618cbf8b67683c6b16880419089f7159c15e` |
| 已审阅 Observer/RPC | `759306e0d0c335e37cd8d78c3163fc4808065817` |
| baseline | `513a98c9a94b15ec77153df41af26fa3c8c0b5e8` |
| candidate | `e3c04fed903c4d0e3c26b5d7cf0e055a6f7c5207` |

baseline 与 candidate 的 `p7_release_resource_preparation.py` SHA256 相同：
`a76865a466252f5863c8e0ad278c4c6814093d4a4a629a9409acf2e6b916901b`。
manifest 固定七份原 run/build/preflight/execute 脚本的 SHA256。
新 protocol.py 的 source/mock/snapshot/db_report/wait_file/digest/write 函数
保留原函数字节；fixture 按原函数 SHA256 检查。tree_snapshot 来自 paired run 的
每线程 children 递归修正，保留非原子、共享页重复计算及 coverage error 声明。

历史两版 ready gate 都失败；candidate 原 `memory_above_12GiB` 仍是失败。
原件只读，本次不执行历史 run/build/analyze/execute/package 脚本，不重包装旧失败。
不上传旧 42files、私有 localdiag、源 corpus、DB、cache、binary 或 target。

## 执行合同

100000 个确定性 Rust 字符串，i=0..99999、value/seed=0，无 RNG；128 dims、
parse4、HTTP global4/per-project2、claim batch16。ready300s、RPC300s、
status gap0.2s、EOF15s，受控 terminate 后再等15s、overall900s、HTTP hold120s；
12GiB memory guard、4GiB disk reserve。配置完整值在 manifest，包括 local 查询、
top_k5、禁 query network、retry1/120000ms 与固定 fake 模型/loopback。

`prepare` 在任何产品之前完成两份 locked semantic-http release build，隔离 owned
CARGO_HOME/TMPDIR/target、jobs4，使用原 Rust1.95.0。build receipt 记录实际
compiler artifact/features/profile、二进制 SHA256、源 HEAD/tree 与 Cargo/crates/scripts
文件 SHA256（逐项核对HEAD Git blob，不依赖仅clean状态）、tool versions、命令、环境、日志 SHA256。checkout 必须精确且 clean；
历史 artifacts 不加入源文件 hash 读取清单。缺失/失败 receipt 不触发自动 build。

`run-single` 同时验证两份 receipt、源文件、实际 binary 与同一 driver 文件 hash，
扫描可见 proc cmdline，任何 rustc/cargo build 或权限可见性错误都阻断。
记录“当前 proc mount 未观察到 compiler”，不冒称全机器证明。
选 baseline 时先跑 baseline32、再 candidate32；选 candidate 时跑 candidate32。
每次 smoke 使用该版自己的新 repo/cache，真实执行合同要求 held POST=2、returned=0、
backfilling，然后 release、完整 ready/manifests/integrity/FK、query、并发、EOF、reopen。
candidate 的 smoke 不借 baseline cache。任一前置条件失败，formal_runs=0。
smoke 后再次校验两版 build/source/binary 与 compiler scan，再进入唯一正式 100k。
本轮没有执行这些真实 build/smoke/100k 合同，仅验证无害 fixture。

正式阶段保留原判定：cold scanned=parsed=added=COUNT、skipped0、parse errors空；
held DB filesCOUNT、integrity ok/FK0、co_change/test edges0；release 后300s 内接受
coherent v2 ready，边界前后 deadline 与 terminal 检查；final files/document/semantic
manifest 都COUNT。四条串行参考查询后，C4 repeated 与 distinct 两个 burst 的完整 hits
逐条精确比较。正常 EOF 必须 exit0 且 forced=False；重开 ready、DB counts完全相等、
四条 hits完全相等、再次正常 EOF。cleanup tail 不充当 deadline counts 或 ready 验收。
原300s只读 DB boundary observer保留。原失败/缺阶段会记录 failed/not_run。

## Observer、guard 与真实权限边界

生成前后、build前/中/完成、productstartup前/后、cold、HTTP release/drain、串行/
并发、normal EOF、reopen、cleanup前后同步记录。产品时20ms periodic composition 与
原 root/tree RSS/CPU/I/O日志并存。memory.current/stat/events/max、group path/inode/
direct members、每角色 PID/start_time/state/cgroup membership 有来源与读取起止时间。
anon/file/slab不相加；缺字段 null+error，root RSS不充当完整树/PSS或cgroup使用量。

`--group` 只能指向已有实际 measured cgroup。调用者在独立云 session 里检查
`/proc/self/cgroup`、`/proc/self/mountinfo` 与该路径的 cgroup.procs，检查产品/model角色
的实际 membership。成员列表可能受 PID namespace/proc mount 限制为空；relation 与
独立性此时 unknown，不能把空列表写成“无其他进程”。支持 scope 只有
`shared_build_cache_runner` 或 `unknown`。没有迁移进程、创建 cgroup/namespace、
修改 limit、改系统配置、dropcache 或绕过权限。saved session 独立也不证明cgroup独立。

guard先写 summary failed 和原 reason，再发布 terminal，立即唤醒所有 pending，
最后执行原 owned root/model terminate。persist报错仍通知pending，报错本身向外传播。
只存在 StdioRPC 的一个 stdout reader；initialized通知、tools14、structuredContent
解包保留。EOF/exit/坏JSON/write/timeout均独立日志，response第一份投递获胜，迟到
wire/response保留；接受ready前还查terminal，收到ready不能清除guard。
退出watcher不依赖pipe EOF。正常关闭前查终态；关闭不会产生ready。
EOF/terminate等待不完成，或者后代保留pipe，明确cleanup失败；不升级kill或杀后代。
transport join有界，未完成reader/log保持到CLI退出，daemon reader避免无限挂起；
此时日志可能不完整，并记录cleanup_incomplete，不能宣称完整证据。

必需 memory.current/cgroup.procs 不可读、初始>=12GiB、free<4GiB、输出不可写、
源码/receipt/hash错误、compiler可见性拒绝：在产品前失败。memory.stat等缺字段记录
unknown；原 environment所需文件缺失会保留真实异常并阻断，不填0。
loopback bind/文件/执行权限拒绝保留原异常；无 sudo/chmod/安全策略替代路径。
输出目录拒绝覆盖，运行异常及新日志hash记录到该session。prepare失败不会产生
builds-ready，run失败不会变pass。单独readiness结果不宣称严格paired因果提速。

## 后续两独立 consumer 的复制用法（本轮未执行）

父级先固定本PR最终HEAD，再分别下发 baseline/candidate 两个 saved 云session。
每个session使用同一HEAD的 `scripts/resource_harness/` 与本说明；如复制文件而非git
checkout，保留整个目录及manifest，不复制任何历史artifact。两版产品源码仍要独立
精确clean checkout。测试中的原断言比对需要原固定git对象，故推荐完整checkout。

```sh
# 将 <FROZEN_DRIVER_HEAD> 替换为父级固定的交付SHA，以下仅为后续consumer指令。
git fetch origin <FROZEN_DRIVER_HEAD> 513a98c9a94b15ec77153df41af26fa3c8c0b5e8 e3c04fed903c4d0e3c26b5d7cf0e055a6f7c5207
git switch --detach <FROZEN_DRIVER_HEAD>
git worktree add --detach /workspace/source-baseline 513a98c9a94b15ec77153df41af26fa3c8c0b5e8
git worktree add --detach /workspace/source-candidate e3c04fed903c4d0e3c26b5d7cf0e055a6f7c5207
export PYTHONDONTWRITEBYTECODE=1
export PYTHONPATH="$PWD/scripts"
python3 -m resource_harness.driver manifest
cat /proc/self/cgroup
cat /proc/self/mountinfo
cat /sys/fs/cgroup/cgroup.procs
# 此处先核实实际group、Rust1.95.0、磁盘、权限、无其它compiler；不改变任何限制。
mkdir -p /workspace/measurements
python3 -m resource_harness.driver prepare \
  --baseline-root /workspace/source-baseline --candidate-root /workspace/source-candidate \
  --toolchain-bin /workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin \
  --output /workspace/measurements/builds \
  --group /sys/fs/cgroup --scope unknown
# baseline消费者用baseline，candidate消费者用candidate；每个独立session只调用一次。
python3 -m resource_harness.driver run-single \
  --builds-ready /workspace/measurements/builds/builds-ready.json \
  --variant baseline --output /workspace/measurements/baseline-once \
  --group /sys/fs/cgroup --scope unknown
```

`/sys/fs/cgroup`只是常见mount示例，必须与该session实际观测对上；不允许因示例
不适用而新建系统隔离。也可在各session完成prepare后先暂停，由父级核验builds-ready
再下发run。无需同机baseline→candidate固定顺序；两个独立环境参数不能推导同机因果。
后续保存 `builds-ready`/receipts/build logs 与该新session目录的session、identity、
composition、RPC/HTTP、source manifest、config、DB只读报告、summary、logs-sha256；
不上传产品源码、cache、DB、binary、target或旧诊断文件。

## 所核对的行为差异

| 原脚本 | 新行为与理由 |
| --- | --- |
| run/preflight顶层import即执行、硬编码OUT/ROOT/SHA | 函数与显式CLI；路径参数化，两个SHA固定。无import-time产品启动 |
| execute先检查两build、固定baseline→candidate各正式一次 | prepare发布hash-bound bundle；两个消费者各只正式一次。无自动pair/统计比较 |
| execute两版32-file前置、candidate完整cache smoke | baseline消费者仍两版前置；candidate消费者只candidate自身32；均不复用cache |
| preflight目录误名live/n100000及pass名100k | 精确命名n32/passed_preflight_32，避免把smoke包装为100k |
| 每版读取ROOT历史preparation并替换tree hook | 同函数字节抽取为protocol，tree修正直接保留；不运行历史driver入口 |
| 原 source-files检查725项 | HEAD/tree/clean照旧，再hash所有Cargo/crates/scripts/.cargo tracked inputs；排除历史artifacts/docs读取 |
| build固定绝对toolchain、isolated target/cargo/tmp | 绝对路径可参数化，实际rustc仍固定1.95.0；命令/features/profile/jobs/guard不变 |
| build简单current/disk样本 | Observer同步build组成，receipt增加源文件/driver/log身份；本轮不build |
| product原reader+响应queue，guard仅terminate | 用单StdioRPC reader，先失败再唤醒pending；无需Empty300s后才看到guard |
| rpc request/response日志、共享last_timing | 增wire/send/notification/terminal日志；timing为thread-local，原延迟字段及received-response口径保留 |
| phase仅赋字符串、startup遗漏生成/build组成 | phase setter同步Observer，外部边界同步；root/tree旧日志保留，group组成额外记录 |
| release后赋drain phase/start | release之前同步drain composition并设deadline，然后release；记录边界本身的少量观测开销，不声称零扰动 |
| ready前后只查deadline | 同deadline，额外Gate/terminal检查，已收到ready但guard/exit/EOF终态仍拒绝 |
| normal_close未区分先退出与EOF意图 | 正常关闭前查terminal，意外先退出不能冒充正常；失败cleanup使用单独路径 |
| raw在sampler join后立刻close，无reader join | EOF/exit后有界join再close；不完整pipe明确失败、无kill升级 |
| pass可能先写、cleanup失败追加但不降级 | cleanup/guard失败最终强制failed；不把cleanup exit0计为ready通过 |
| run.jsonl原gzip归档 | 该session产品/RPC/HTTP仍gzip；composition另保留；仅新目录log-hash，无旧package/report重打包 |
| analyze/compare/report/supplement/timeline/package | 不调用；新driver输出原测量检查结果，不回写历史或自动性能验收 |

fixture使用本次自写Python fake product、临时普通group文件、toy32 SQLite与owned loopback
mock；无Rust编译/真实产品/100k/GC/WAL/crash。已有runtime14项保留；新driver12项：
协议/函数hash、原postready assertion AST、无启动入口、build屏障/顺序、missing receipt、
失败smoke无formal、真实读取错误、单reader/MCP解包/phase、guard持久化顺序、toy32完整组合、fake build同步阶段、Git隐藏dirty时仍拒绝变更blob。
最终运行命令：`PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests/resource_harness -v`。
26 passed；`git diff --check`通过。一次fixture拦截器未放行git init导致测试失败，随后
只放行fixture新repo初始化及read-only git命令后通过；未因此运行任何真实产品或修改协议。
