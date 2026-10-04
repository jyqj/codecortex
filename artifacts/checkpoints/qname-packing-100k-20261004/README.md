# qname + Python/C++ + packing 新产品独立 100k 资格验证

本次结论：**passed_declared_100k_local_scope**。正式仅 1 次，前置自身 32 smoke 1 次，locked release build 1 次；无失败、无缺阶段、无 guard/deadline 触发、无重跑或 best-of。结论只绑定本次 source/二进制和原自有 loopback 方法，整体 release 仍需其它 gate，父级 V19/P7 不关。

## 固定身份与方法

- 集成基点：`37dd042eaa1209a86e0cafdcd92ae77e036e76f5`。
- 实构建产品：`90858afae647a513537bf118932a7ba5020ee98b`；tree `cdeb6c61f0c9d3a93bcd4f19eccb171671c68d68`。Cargo/lock 和八 crate 的生产输入与集成基点逐项 Git blob 比对相同；两提交差异仅测试文件。
- binary SHA256：`dbfe122aa507dd708cd141b264178ef84aae25fe6d0ceb8aacb8e140e79595ca`。
- receipt SHA256：`cb3fe484f1f71cbfc1f7cf9a3ae5d1c4407ec0eacbc109285029fdcd353cd04f`。753 个源码输入 SHA256/HEAD Git blob、artifact features/profile、driver/current-source/bundle、实际 binary 均核对。
- 官方工具链实测：`rustc 1.95.0 (59807616e 2026-04-14)`；原 Cargo.lock、semantic-http、release opt3、无 debug assertions、jobs4。独立 source checkout、CARGO_HOME/TMPDIR/target/index/cache；正常环境 proxy 继承，未绕路或改系统设置。构建 155.868149s，exit0。
- `harness/resource_harness/current-source.json` 是具名新身份配置。旧 `scripts/resource_harness/` 和历史 manifest 均未修改。owned harness 的 build/measurement/product/protocol/runtime/manifest 字节与原入口完全相同；只改 SOURCES 为当前 candidate、增加 current-source hash、去掉旧 baseline 身份参数。
- 从正常 origin 取固定对象；原 manifest 七份 run/build/preflight/execute 脚本 SHA256 全核对。PR129 `1be640ec…` 的 harness 与当前原 harness 相同，仅用作方法/固定 fixture 来源。旧 e3/88f2 或不同 cloud 的任何通过均未继承，不作 paired 性能因果结论。

## 实测结果

| gate | 本次结果 |
| --- | --- |
| safe units | 原 26 + 新身份 4 全通过；仅自写 Python fake/owned toy fixture |
| 原 32 smoke | held POST=2、returned=0、batch sizes=[1,1]、backfilling；完整 ready/query/C4/EOF/reopen passed |
| fixture | 100000 真实文件，i=0..99999、value/seed0、无 RNG；4088890 逻辑字节 |
| fixture ordered SHA256 | `881ec28155e0cfe0ca1b73830276d1861f06169ada71a4e52fd2374cdae754de` |
| cold | 28.436435412s；scanned=parsed=added=100000、skipped0、parse errors空 |
| held DB | files/symbols/chunks/document_manifest=100000、semantic_manifest0；integrity ok/FK0；co_change/test edges0 |
| ready drain | **176.764548183s < 原300s**，coherent retrieval-capabilities-v2 identity checked |
| final DB | files/symbols/chunks/document_manifest/semantic_manifest=100000；done100000、attempt_count100000；integrity ok/FK0 |
| 原串行 query | resource_00000 / resource_00001 / resource_50000 / resource_99999；local/top_k5、完整非空 hits |
| repeated C4 | offered4、observed in-flight4；0.037431965s；每条完整 hits 精确等于串行 |
| distinct C4 | offered4、observed in-flight4；0.023180691s；每条完整 hits 精确等于串行 |
| 正常 EOF | exit0 / forced=False，0.063899673s |
| reopen | ready、全部 DB counts 和四条完整 hits 精确相等 |
| reopen 正常 EOF | exit0 / forced=False，0.007343351s |
| formal wall | 227.986213760s（不是 ready deadline 数字） |
| 补充 schema 只读观察 | 正式结束后 user_version25、chunk_symbol_identity100000；model3 源常量；不作 deadline 观察 |

原300s boundary observer 在提前 ready 后按原方法停止，故没有 deadline-300s-db 文件。cleanup-tail 单列，保留原时间标签；不拿 tail counts 或 ready_window_end 替代 ready drain 或 deadline counts。guard/deadline 若触发仍判失败；本次均未触发。

## 配置、资源与观察边界

原配置完整保存：parse4、dimensions128、HTTP global4/project2、claim max_batch16、call retry1/120000ms、查询 local 且 allow_query_network=False、top_k5、原 timeout/guard。配置的 per-project2 使 production queue 的本地 attempt width=4；默认未显式配置时 width=1。HTTP gate 是另一层 4/2 约束，claim16 也不是 HTTP 并发16。32 smoke 的实际 held HTTP=2 支持观察口径；status 不暴露 runtime gate caps，保留 unknown，不能把配置值充当独立测量。

- cgroup 原上限 17179869184 bytes；**guard 保持 12884901888 bytes (12 GiB)**，未提高。20ms composition/root/tree 样本和同步边界原样保留，build 0.5s、guard0.1s、disk reserve4GiB。
- 本次正式 cgroup sampled max：**8506327040 bytes (7.922134 GiB)**。该样本 anon=1945288704、file=5102616576、slab=1447197240 bytes；这些计数不相加。memory.stat/events/max、path/inode/direct members、角色 PID/start_time/state/实际 membership、CPU/I/O 原样保留，见 `resource-analysis.json` 与压缩 raw。
- 产品 root sampled RSS max：**2219831296 bytes (2.067379 GiB)**；观测 VmHWM max=2220392448 bytes。reopen root sampled max=49786880 bytes。RSS 与 cgroup 分开，不冒称 PSS/完整树/精确 lifetime peak。
- **进程树覆盖不完整**：原 recursive every-thread children 方法的全部正式 root samples 都记录本 proc mount 缺失 `/proc/PID/task/TID/children`；因此树 RSS 合计只能称所观察到的 root。错误保留，不填0。
- cgroup.procs 可读但返回空；角色 `/proc/PID/cgroup` 为 `0::/`，relation/independence 均 **unknown**。云 session、独立 target/cache 不证明独占 cgroup。无 cgroup/namespace/limit 修改、dropcache、sudo 或系统替代路径。

## 证据与范围

`evidence/` 只保留本次 72 份安全 raw/receipt/config/summary/query/HTTP/RPC/resource/source-hash 文件；compression 前后 SHA256 和字节数见 `publication-manifest.json`，归档独立校验通过。100000 行 fixture hash manifest、完整 C4/reopen hits 和两次 EOF 已做独立只读复核。`result.json` 是机器摘要，`evidence-verification.json` 为复核结果。

不提交 corpus/repo/cache/DB/WAL/SHM/binary/target/Cargo cache、旧私有诊断或42files。未读 public DEV gold，未改 product/qname/budget/ranking；未跑旧 post_index_worker_crosses_pages_and_reopen_reuses_artifacts 或包含它的 broad，未做旧 GC/WAL/kill/staging/EROFS/private42 export，未调用 public provider/真实 external account。只原自有 loopback。无 baseline 新 formal，无 merge/deploy。

证据提交 `484edeb6acfabaf9fa02aa7b67234552bdfe254f` 已经正常 origin push 成功。唯一 draft PR 尝试返回 `Post "https://api.github.com/graphql": Forbidden`，已停止、无重试/绕路。实际 receipt 见 `publication-status.json` / `push-first.log` / `draft-attempt-once.log`，可审阅正文见 `PR-BODY.md`。
