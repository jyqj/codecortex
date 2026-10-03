# backfill 已定位路径成本拆分：单次诊断，未修改产品

下一生产优化优先级：**先减少 status 全量统计及 generation 变化后的重复扫描成本，再验证 claim 的候选排序访问路径。** 单 runtime 串行消费是确定的机制，但假 HTTP 服务段只占 2.48%；先调 provider 并发或 max_batch_items，无法解决本次占主导的本地工作。保留原 100k 失败：cold 21.638297s；300s 期限未 ready；失败 cleanup 后 done=29696，不是规模通过。

## 身份、范围与复现

- 固定产品源码 `574f7598662334c63e020da136c87f4f7281554d`，tree `7f9ea467c1705822aa403de7d273419ddfb45864`；唯一原始证据提交 `a8452be903e1a88c935e267f5d76879ce7ee2716` 的最终隔离轮。未使用 attempt01/02。
- 原产品 binary SHA256 `1dc3dbff22f9fda053c7e7eac6d11d27f0a7271cec28b902057e632be21b9b27`；Rust 1.95.0、release、semantic+semantic-http、opt-level=3、无 debug assertions。完整 build receipt/source identity 保存在 `analysis.json`；本环境没有原二进制，本次不重建、不声称验证了本地二进制内容。
- `analyze.py` 直接只读 git blob，无需切换产品版本或启动产品。原始 gzip 日志未改，逐 blob SHA256 和长度见 `inputs-manifest.json`。
- `sql_probe.py` 是单次有限 1k/5k 内存 SQL 诊断：同固定 `source(i,value=0)` generator，固定 outbox 原 DDL、候选 SELECT，无替代索引。没有产品运行、HTTP 请求、任务丢弃、并发/超时变更。keys 使用生成字节摘要作为替代键，不能冒称真实 DocKey/编码输入；SQLite/Python 身份、128 次暖查询、VM opcode/step、配置差异全部保存在 `sql-probe.json`。
- 所有新写入限定本目录。生产、版本、tasks、TODO 无内容编辑；不读取 heldout/私源；真实 provider=0；不执行 kill/fault injection/GC/WAL 测试、不复用旧长 session。没有产品级 1k/5k 重跑，因为现有日志已经提供足够的下一步优先级证据；不额外开启构建与完整评测。
- 固定源码及证据 tree 内无 AGENTS.md/SKILL.md；`/AGENTS.md`、`/workspace/AGENTS.md`、仓库根 AGENTS.md 均不存在。已读 CONTRIBUTING.md；应用 agent-architecture 的观测/行动/验证证据边界。用户限制优先，全仓测试（可能读 heldout）不运行。

复现（在有这两个 git object 的仓库中）：

```sh
python3 artifacts/checkpoints/backfill-cost-breakdown-20261003/analyze.py
python3 artifacts/checkpoints/backfill-cost-breakdown-20261003/sql_probe.py
python3 artifacts/checkpoints/backfill-cost-breakdown-20261003/verify.py
```

`analysis.json` 是精确数值；`analyze.log` 为人可读投影；`polls.json` 保留每次 RPC 起止、时延及状态。无生产 hook。

## 直接观测

| 项目 | 结果 | 意义/边界 |
|---|---:|---|
| HTTP requests / inputs/request / peak active | 29696 / 1 / 1 | 配置 max_batch_items=16、per_project=2 没有成为实际 batch/并发 |
| 首请求 gated 服务段 | 2634.081ms | 原 fixture 等待 release；单独排除，不归因 provider |
| ungated entered→returned | mean 0.259ms；p50 0.243；p99 0.518 | 服务端读完 body 后到 response write 后日志；不是完整客户端 RTT |
| returned→下一个 entered | mean 10.178ms；p50 8.413；p99 66.403 | 本地串行链、客户端 decode/发送与轮询干扰混合 |
| postgate span（含 cleanup） | 309.916751s；95.816条/s | HTTP 服务段累计 7.693807s（2.48%）；请求之间累计 302.222944s（97.52%） |
| status measurement window | 301.987382s；28846次 HTTP 返回 | 起止取首 poll sent/末 poll response；300s 超时在同步 poll 返回后判定 |
| cleanup 尾段 | 7.934609s；850次 HTTP 返回 | 这 850 条包含在最终 29696 内；不能算到 300s 期限内；末 poll 时待处理 proxy=71154 |
| status RPC | 293次；mean 831.065ms；p50 689.159；p99 2561.462 | 合计 243.501982s，窗口占比 80.633%；实际 sleep 间隙 mean 200.292ms |
| status 结果 | 292次 generation changed 错误；1次 backfilling | 292次 semantic_state=port_attached_unverified；不能视为任务失败或跳过 |
| poll 内 / 间隙吞吐 | 80.722 / 157.133条/s | 相关性，缺少无 poll 对照，不能宣称关闭 poll 得到 1.95倍加速 |
| 产品根进程 CPU | 441.81 CPU-s / 301.964 sampled wall-s | 约 1.463 cores；不是单核完全吃满，也不是4核达到上限 |
| 产品 IO 增量 | rchar=395906912360；syscr=96767814；write_bytes=3690246144；read_bytes=0 | rchar 是含页缓存的逻辑读取，不是设备流量；不能用 read_bytes=0 排除写入 fsync 等待 |

资源样本 delta 只在整个相邻采样间隔同属 poll 或 gap 时入桶，混合的 11.991s 另列、不摊派。poll 桶 237.498s：rchar=380771082842（全部窗口逻辑读的96.18%），CPU=394.35s，约1.660 cores。gap 桶 52.476s：rchar=3590227620，CPU=34.82s，约0.664 cores。资源采样约20ms，归属仍有延迟/边界误差，无法将 poll 桶的全部工作认定为 status 自身。

| 测量窗口位置 | pending proxy 起点 | 总吞吐条/s | poll 内条/s | 间隙条/s | poll 墙时占比 |
|---|---:|---:|---:|---:|---:|
| 0–30s | 100000 | 216.40 | 170.63 | 247.96 | 40.8% |
| 30–60s | 93508 | 138.87 | 126.64 | 166.74 | 69.5% |
| 60–90s | 89342 | 98.60 | 92.74 | 120.44 | 78.9% |
| 150–180s | 81363 | 67.83 | 64.30 | 94.24 | 88.2% |
| 210–240s | 77358 | 58.60 | 55.49 | 84.56 | 89.3% |
| 270–300s | 73495 | 72.73 | 70.05 | 96.79 | 90.0% |

完整11个窗口见 JSON。每个完整30s窗口的 poll 内吞吐都比间隙低，差异仍受队列长度/已发布集合/轮边界影响。随着 pending 降低，总吞吐反而下降；不能简单归因为“pending 越多 claim 越慢”。pending proxy 为100000减累计 HTTP返回，不是严格事务时点的 outbox pending；HTTP返回还未等价于已publish。

## 成本排序及归因限度

1. **状态统计/一致性重试：最强的首优化证据。** `capability_status.rs:90` 每轮先 stats/freshness，再比较 generation；最多3轮。`apply_semantic_wired:187` 调 pending/coverage，coverage 又有自身最多3轮 generation 验证（不是所有调用都一定执行9遍）。源码 `index_db.rs:830` 有多表 COUNT；coverage 有 document/manifest/outbox 计数及 stale 关联查询。观测到 RPC 几乎全失败、80.6%暴露、逻辑读取几乎全集中 poll 桶。建议下一生产优化做便宜的严格一致 status 读取/有界统计路径，保留 generation 与 ready 语义；不能绕过一致性验证或只改诊断 poll 频率来宣称产品通过。无 SQL trace，无法判定 COUNT、coverage、freshness 各自具体占比。
2. **串行本地每任务链：确定存在，具体 SQL/cache 分摊未知。** runtime `running` 原子标记限单 runtime，一轮16 claims，handler 一个 input；HTTP确证峰值1。队列每任务 reclaim/claim/renew、cache miss检查、HTTP、cache put+verify、publication CAS。97.52%的 postgate 墙时在 server 返回到下请求进入之间，而不是假服务段；这包含客户端 RTT剩余段，不能把97.52%都叫DB耗时。仅去掉假服务段的理论墙时上限约1.025倍（所有其他成本固定），不是完整 provider 优化上限。
3. **claim 候选 SQL：可行动的规模风险，但未证明它主导100k。** 原 ready 索引 `(state,available_at,space_id)` 与 FIFO `ORDER BY task_id` 不一致。隔离候选 SELECT 在 SQLite3.53.1 上选择 covering ready index + `USE TEMP B-TREE FOR ORDER BY`。1k pending 时9017 VM steps、mean0.0627ms；5k时45017 steps、mean0.2957ms（约4.71倍墙时，5倍规模）。done90%后分别917/4517 steps。**只证明这套隔离查询线性枚举当前可用 pending；没有100k产品SQL plan/真实available_at分布，不能把0.296ms线性外推或认定原产品就是此plan。** 后续应在产品实际库做只读 plan/profile，再决定与FIFO相符的索引/候选查询；本次没有添加索引。
4. **逐轮64输入 reconcile：明显的16周期附加成本。** within16 gap mean8.461ms，16边界mean35.939ms。差值27.478ms乘1855边界约50.97s（16.45%的postgate span），是按非边界均值估计的相关性增量，包含status、调度、job构造等，**不是精确64-input SQL成本**。每轮 source 会keyset读64项，再做input/publication/cache检查，随后claim16；100k扫描应在约1563轮、约25008次claims附近耗尽。日志后段>25024 gap mean12.427ms仍高于前段9.756ms，支持64项扫描并非唯一瓶颈，但与poll增长混杂。每1024边界mean36.177ms（28个），与普通round边界接近，不支持job重建是明显额外首因；30s提前结束也可能改变实际job边界。
5. **cache持久化/磁盘sync：有机制，无等待时长证据。** `cache.rs:410` 每对象.bin和.meta分别atomic_write；`:509` 每文件sync_all、rename、父目录best-effort sync。成功publish前cache.get验证。29696对象对应源码推算59392次文件sync、至多59392次目录sync尝试，未做syscall trace，不是实测syscall次数。原cache和DB提交的sync延迟没有日志；3.69GB write_bytes无法归因哪类写，也不能据此宣称“磁盘sync主导”。cache+verify、claim/renew/CAS单项都只能落在混合10.178ms gap预算内，重叠轮询不能相加。

## 误差与没有归因的部分

单次旧100k失败日志，没有独立重复或因果A/B；quantile为描述性经验分位，不给伪置信区间。HTTP日志entered在body读取之后，returned在写响应之后且需要log锁，客户端HTTP总往返没有计时。round边界按sequence%16推定，原最终计数与单input符合正常全批消费；若存在未发HTTP的claim，会破坏对应，不能称精确hook。资源样本是根PID/non-atomic，原proc children不可读，不称完整树PSS。CPU tick精度10ms、采样间隔约20ms，少量间隙未分桶；IO按sample delta并且pagecache/线程并发存在，不能归到具体函数。失败cleanup仍是原run脚本行为，本任务没有执行进程终止或故障测试。

不作100k完成时间线性外推；不提高原超时、不把no-poll/SQL诊断当规模通过。生产优化需要随后单独实现与同一固定100k规范验收；本次完成即停。
