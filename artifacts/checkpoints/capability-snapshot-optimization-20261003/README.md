# Capability snapshot 优化：v2 单一 point-in-time 诊断

已完成本单优化块，生产冻结 `11af963c33cfa68cc9497e355464c1d6d058adac`（base final `6db4d396e5d994388ada1e97c3d28947ffdb9c81`，基线生产 `9ebdb155c64e094b3d774be41dbe7ab8c4e222c1`）。有限 release FakeProvider AB 表明 status 读成本和错误率下降；1k ready 观察时间没有一致改善，5k 两组有小幅改善。100k 未运行、不套旧 PR101 结果。完成即停，独立 review 另开。

## 最终行为与删掉的重复工作

`retrieval-capabilities-v2` 明示 `consistency=point_in_time`、`generation_scope=observed_database_snapshot`，保留完整 generation 对象；身份字段 `identity_validation` 为观察边界已检查或 not_observed，`semantic_active_space` 与数值同事务，运行态标 `service_state_scope=process_observed_separately`。ready 只表示该观察 generation 的覆盖，不承诺响应时 latest 或后续 query 成功。

cc-db 一个 typed snapshot、一个短 query-only SQLite transaction，读 generation、files/symbols、freshness、active space、pending/failed/eligible/published；SQLite 第一笔 metadata SELECT 固定全部数值的时点。普通发布返回完整旧或新观察均合法，不再因为 epoch 前进重扫。旧 root+新 coverage 反例仍被新 oracle 拒绝。配置 space 从同事务字段投影，worker failed/degraded 保守优先级不变；query 严格 fence 完全不改。

Capability 路径不再调用通用八表 stats，因此移除该路径的 chunks/symbol_refs/call_edges/test_edges/routes/literal_index 六个无用计数；移除 coverage 的 stale 关联 COUNT、零 eligible reason 的额外 document COUNT、coverage 内层重试和普通 generation 外层重试。通用 stats、独立 semantic_coverage 及其原契约仍供其他消费者使用，未改它们。wired 且 eligible>0 的稳定 source-level COUNT 从13项减为6项，unwired从8项减为2项；这是静态语句/子查询库存，**不是实测 VM/page/fullscan steps**。普通发布不重扫由真实 worker churn oracle 的每次一次观察另行证明；只有检测到身份变化时最多三次重取。

`HAS_MOVED` 私有只读 FFI 在事务前后验证 SQLite 实际打开的 main 文件；事务/lease释放后 typed incarnation控制读取再次检查文件身份。没有嵌套 pool checkout、rawconn上泄、query错误归零或读侧schema修复。SQLITE_NOTFOUND/错误/非定义布尔结果返回 identity-unverified 的保守不可用，不假设未移动。normal rename/changed incarnation 的可检测身份问题不会把旧 lease报告为新库 ready。不改 rebuild/pool替换/GC/WAL；不承诺协调住响应发出之后的替库窗口。Linux Unix VFS实测通过，其他平台/VFS未测；不支持该filecontrol的诊断受限，不据此宣称普通query不可用。

生产差异仅 `capability_status.rs`、cc-db 新 `capability_read.rs`、`lib.rs` 最小导出及 `freshness_store.rs` 同连接helper提取；schema/migration/parser/queue/cache/publish/rebuild/gc无修改。

## 当前测试入口与消费者迁移

原 `p7_status_snapshot_independent_review` 入口已迁到当前 v2 生产源码的精确前缀编译（仅省略内部私有 fixture 测试区块；测试检查除文件末尾空行外逐字节一致），不再 include 冻结 v1 候选作为当前CI绿灯。生产 lib tests 另行直接测试实际 QueryServices/当前实现。私有 query-auth shim只用于该隔离entry中明确关闭网络的fixture；真实公共参数套件另验证实际产品gate。

删除旧冻结 `support/p7_status_review/candidate.rs` 及被迁移的 checks fixture；v1 原文件/oracle完整保留在 base `6db4d39` 和方案1安全检查点 `c3382c6` 的 Git 中。legacy.rs 只在实质反例测试中作明确 red counterfactual；AB里的 exact v1 status只作baseline，生产没有v1/v2兼容双路径。方案1临时重复入口 `capability_snapshot_latest_contract.rs` 已删除，没有新增 ignore/skip。

`engine.capabilities_info`、diagnostics retrieval及 status capabilities/schema/all 共用单一投影，无spec解析或缓存compat分支。公共 V18 status helper增加 v2 spec/consistency/generation_scope/service_scope断言；原参数、权限、生命周期、generation shape判据保留。公共V11 query fence oracle源码未改且通过。MCP_TOOLS.md、CONFIGURATION.md、STORAGE.md及 [ADR-0004](../../../docs/adr/0004-capability-status-point-in-time.md) 明确迁移含义；根 ledger/TODO 未改。consumer-inventory.json 是发现清单，包含历史条目/宽泛匹配，不是所有条目都作为新生产消费者改写。

## 当前验证

命令、退出码、时长与日志见 checks.json：

| 当前检查 | 结果 |
|---|---:|
| typed snapshot/身份/readonly错误传播 | 6/0 |
| 原独立 semantic coverage contract | 3/0 |
| 默认 actual production status lib | 7/0 |
| semantic-http actual production status lib | 10/0 |
| 迁移后的当前 source 一致性/反例/配置/降级 oracle | 6/0 |
| 原实际 worker failure/retired state优先级 | 1/0 |
| 实际 stdio MCP V18参数契约 | 12/0 |
| 实际 stdio MCP严格query generation fence | 1/0 |
| fmt、cc-db/cc-server定向 `-D warnings` clippy | exit0 |

共46次test executions（default/HTTP重叠，不是46个独立问题）；未执行全仓/heldout/完整gate矩阵。所有正常发布/identity测试沿用原5s/3s边界，不提高阈值。rename是独立 synthetic DELETE-journal fixture，不运行rebuild、kill、GC/WAL故障或曾遭RO拒绝动作。

保留最初源码前缀验证失败：5个行为测试通过，唯一失败为摘取文件EOF空行与源文件前缀不一致；修正仅忽略EOF空白，代码身份判据保留，原失败log与checks-first-pass.json在册。初始driver release构建调用private attach失败，随后用已有public setters修正；旧failed build log保留，v2 build exit0。更早方案1的日志只属历史安全检查点，不计v2认证。

## 有限 1k/5k AB

同一个 release binary（SHA256 `dfb5b92fd7b32dcf87ea6dc19195d8342fcf991ec6f34c5e8b6bad1cb47db1bf`），Rust1.95，opt-level3/thinLTO/codegen-units1；两臂共享其余产品代码，baseline是 exact final6db4d39 capability源码，freshness提取保留旧SQL/事务。每规模两组，顺序baseline→candidate及candidate→baseline，独立cold cache；相同固定Rust generator、dimension2、batch16、read pool1、poll后sleep200ms、期限120s。实际start-to-start间隔包括status自身时间，未降低poll或提高超时。测量为in-process实际production status+真实SemanticRuntime/Gated FakeProvider，**不是MCP RPC latency**，没有真实provider/HTTP。

| 规模 | 旧→新 status错误/总poll | poll mean ms（旧→新） | 稳态mean ms（旧→新，各256次） | ready观察mean ms（旧→新） |
|---|---|---:|---:|---:|
| 1k | 3/6 → 0/7 | 2.471 → 0.382 | 0.140 → 0.111 | 411.2 → 505.9 |
| 5k | 14/28 → 0/28 | 12.030 → 1.083 | 0.341 → 0.254 | 2786.2 → 2631.2 |

1k poll平均成本减约84.5%，5k减约91.0%；steady成本减约20.8%/25.6%。旧errors都是generation changed三轮耗尽，新8次全部ready、观察错误0。steady128次/运行是在回填测量结束后采样，不计入ready时间。

必须保留的不利项：1k pair1 旧409.761ms、新607.509ms，候选慢197.747ms且多一次poll；pair2旧412.709ms、新404.280ms。candidate pair1在405.81ms观察965/1000已发布，下一轮606.59ms才观察ready，不能声称1k回填加速，也不能把它仅归因于事务语义或某个未测等待。5k pair1旧2764.012ms、新2630.318ms，pair2旧2808.355ms、新2632.113ms；ready观察mean减5.56%，只有两组且受200ms cadence/调度影响，不是worker-throughput因果分解或100k承诺。原始cases、全部poll状态/时间/steady样本和退出码均保存，未重跑筛好数。

AB-PROTOCOL.json/AB-RUNS.json/AB-SUMMARY.json给出身份、配置、全部派生数字。analyze_ab.py验证全部case counts、source/binary身份、候选scope、published/pending约束、最终ready及错误归因。binary留在本环境target/release，未上传；复建不承诺bitwise一致。

## 复现与历史边界

```sh
# Rust1.95和依赖准备好后；本环境用/workspace/.cargo与/workspace/.rustup
python3 artifacts/checkpoints/capability-snapshot-optimization-20261003/run_checks.py
CARGO_TARGET_DIR="$PWD/target" cargo build --release --locked --offline --manifest-path artifacts/checkpoints/capability-snapshot-optimization-20261003/driver/Cargo.toml
python3 artifacts/checkpoints/capability-snapshot-optimization-20261003/run_ab.py
python3 artifacts/checkpoints/capability-snapshot-optimization-20261003/analyze_ab.py
```

复测会产生新的source SHA/时延，不覆盖本次唯一数字的来源归属。prepare_design.py、PROPOSED-DB-ONLY.patch、POINT-IN-TIME-SERVER.patch、原source-evidence/交付JSON为此前设计阶段历史材料，应在对应历史commit解释或重放，不能拿未应用草案替代本次生产实现。源码prefix若未来改变，应先同步current-status-under-test再执行身份guard，不跳过guard。

已读仓库CONTRIBUTING与相关ADR/存储/源码；AGENTS/SKILL文件未发现，没有适用的专项个人skill。所有新artifact写入本专属目录。真实provider0、heldout/账号/私源未读未外传；没有merge/forcepush/deploy。旧PRcreate已被GitHubForbidden拒绝，本次不重试、不换权限/通道。交付状态见delivery.json，独立review与100k随后另开。
