# P8 本地规模矩阵与增量 / fanout 驱动

本交付推进 **P8-005、P8-006 的本地 profile 准备**。它按 04-PHASES 允许的提前准备范围实现可执行驱动，未改变 tasks.json 完成态、P7 门槛、历史 100k 结果、任何生产 API、gold 或 oracle。

## 入口与执行边界

`crates/cc-eval/src/bin/p8-scale.rs` 仅属于开发评测工具。实现为 `benchmark::p8_scale`，复用 `synth::generate`、`CodeIndexBackend` 的真实 in-process MCP index dispatch、`mutation_case::evaluate` 和 `oracle::canonical`；没有第二套解析器、索引器或缩减版 oracle。

默认明确是 60 个生成文件、1 次重复的本地 smoke。它另加 1 个可见 `tsconfig.json`，并在 3 个既有 TypeScript 文件上增加配置解析见证；因此输入清单是 61 个可见输入，报告分别列出 requested/generated/auxiliary/indexed 文件计数。SQLite 中的文件数可能小于输入清单数，不能把 unsupported 文件默认为已索引。生成器计划函数数与实际 DB symbol/chunk 数也分栏。

```sh
cargo run -p cc-eval --bin p8-scale --offline --locked -- \
  --output /tmp/p8-scale-smoke-unique
```

明确选择任意 60–100000 文件数，或一次给出固定矩阵：

```sh
# 这里只示例选项；本次没有执行 1k–100k 的性能矩阵。
cargo run -p cc-eval --bin p8-scale --release --offline --locked -- \
  --profile release --files 1000,5000,10000,50000,100000 \
  --seed 12648430 --repetitions 30 \
  --batch-sizes 1,10,100,1000 --fanouts 1,4,16,64,128 \
  --dirty-budget 8 --max-resume-builds 128 \
  --deadline-ms 86400000 --max-output-bytes 536870912 \
  --output /tmp/p8-scale-release-measurement-unique
```

`--matrix` 与显式 `--files` 互斥，等价于选择 1k/5k/10k/50k/100k。`--profile release` 要求 release 编译、至少 30 次重复和规范矩阵中的规模；单独指定某一规模仍只生成该规模数据，不是完整矩阵认证。公开参数还限制重复最多 200、最多 5 个互不重复规模、最多 4 个 batch 大小、fanout 1–128、dirty budget 1–10000、resume budget 1–1024、24 小时全局 deadline、1–512 MiB 证据上限。

## 真实执行的操作

每一规模与 repetition 使用两份独立的同 seed 源码树。A 持续增量；B 在每个检查点执行真实 full build。runtime config 明确关闭 auto index 和 semantic，并把 parser 并发固定为 2。监督进程清除 6 个会影响索引行为或缓存位置的既有 CODECORTEX 环境覆盖项，包括 `CODECORTEX_CACHE_DIR`，确保数据库留在父进程拥有的 fixture 目录中；没有 provider、远端请求或 API key 使用。

| 阶段 | 实际操作 | 证据 |
|---|---|---|
| cold | 两份全新项目、空 index，分别 full build | 两份完整 IndexReport、各阶段时间、实际表计数、完整 oracle parity |
| no_op | 不写入源码，执行增量与 full control | 严格要求 added/updated/removed 为 0，保留全部扫描与阶段工作 |
| body | 在生成的 TypeScript needle 函数体加入真实局部声明 | 原文件与新内容 digest、完整写入操作、增量与 full raw |
| API | 给真实 TS hub 的公开签名增加可选参数 | 原有调用者语料保留，观察真实 dirty propagation，而非伪造影响数量 |
| config | 只改 `tsconfig.json` 的 `@p8/config` 目标 | 手写期望检查既有 consumer 的调用边从第一个目标文件切到第二个；A/B full parity 同时执行 |
| batch | 对恰好指定数量的既有代码文件追加带迭代编号的注释 | 保留每个操作及 before/after digest，不把不足文件数的批次缩小后宣称通过 |
| fanout | 独立 TS fixture，1 个 API 与 N 个显式 import/call consumer；真实修改 API 签名 | 原 mutation_case、逐 consumer 手写 call-edge 断言、每次原生增量报告、full canonical 事实与原失败签名 |

fanout 是独立 workload，报告实际 fixture 文件数为 N+1，不把它当成主合成矩阵的 1k–100k 工作量。超过 dirty budget 只是输入条件；`first_build_incomplete` 记录实际观察，不能据 requested fanout 虚构触发了 partial closure。每次 resume 均有单独 raw IndexReport；到达预算仍不完整则不得认证 parity。

主矩阵每次 `build_finished` 保存原封不动的 IndexReport，包含 `files_*`、`dirty_plan`（含实际 dependency SQL 工作）、`document_changes`、`project_model`、`resolution_freshness` 和 `phase_timing`。report 缺少的测量不会补 0。worker 单进程 RSS/CPU snapshot 有原 reader 的明确归属，未知可为 null；它既不是连续峰值，也不包含未知进程树。

SQLite 计数分列 files、symbols、chunks、call_edges、semantic_edges、test_edges、所有原 oracle 表以及 DB/WAL/SHM 字节数。不同 edge 表可能表达不同关系，不把它们相加并冒充去重总数。向量字段固定表达 `state: disabled, count: null`，不以 0 个向量表示完整语义工作量。

## Oracle 语义与容量

小规模直接调用既有 `oracle::canonical`：执行原 integrity/FK 检查，比较原 15 个表的完整 canonical rows，只排除原 oracle 已明确排除的物理列。比较先于摘要；证据输出各表两侧行数、完整 digest 和最多 3 个差异示例。`resolution_manifests` 的完整 JSON 排序会因 digest 变化而移动行，因此仅其诊断示例按真实 `file_path` 对齐，导出同文件的两侧完整原始行、差异文件总数与最多 32 条递归 payload 差异；单边缺失明确标记。其他表的示例仍按排序位置配对。所有示例限制和对齐方式只影响诊断输出，不影响完整原 oracle 判等范围。

既有内存 oracle 的每表 100000 行保护保持不变。驱动在调用前读取真实两侧表计数；超过该界限时使用 `oracle::compare_streaming` 的磁盘排序路径。两个路径共用唯一的字段投影和 SQLite 值到 canonical JSON 的转换，仍比较原 15 张表的全部行、保留重复行。每侧先建立只读事务快照并执行原 integrity/FK 检查；临时 SQLite 按完整 canonical JSON 文本的二进制顺序建立覆盖索引，两个有序流逐行比较，随后才计算摘要。数组摘要与旧 oracle 的完整 JSON 数组摘要一致。

磁盘路径固定排序页缓存为 2 MiB，默认每侧每表最多 500 万行、两侧所有表累计 canonical 行文本最多 4 GiB、临时 SQLite 文件最多 8 GiB、单行原始内容与编码后文本各最多 1 MiB。临时库禁用 journal、同步写与 mmap；所有文件仅属于本次诊断，成功或错误返回后清理。覆盖索引随写入维护，比较使用该索引，避免另外产生不受文件页数限额约束的外部排序文件。页缓存不是整个产品或 SQLite 进程峰值声明。行数、字节或磁盘限额耗尽、输入损坏、未知表与 I/O 错误都会失败，不能变成部分采样的 `equal=true`。

每个大表保留最多 3 个按 canonical 顺序对齐的差异示例；单侧示例超过 16 KiB 时仅输出其长度和摘要，并显式标注正文省略。这只限制诊断输出，完整逐行比较继续执行。判等保持原 `serde_json::Value` 语义，例如 `-0.0` 与 `+0.0` 相等；它们的序列化文本、排序位置和摘要仍保留原样，不用摘要相同与否替代事实判等。小表仍保留既有 `resolution_manifests` 按真实 `file_path` 对齐的详细诊断。报告另记录实际 canonical 字节数、临时文件峰值、限额与 `disk_backed_exact_v1` 模式。索引 closure 未完成时即使所有表相同，状态仍为 `incomplete_not_certified`、`equal=false`。

这消除了大规模验证的固定 10 万行阻塞；它自身不代表已经执行完 1k–100k 矩阵、30 次重复、heldout 或 G8 发行认证。

fanout 使用原 mutation_case 保留完整 canonical 事实和独立断言结果；原工具不返回 full rebuild 的阶段 timing，因此该字段明确 unavailable。fanout 的外部 wall time 是整个 fixture replay，包括两侧建库和 oracle；不能当成单次增量耗时。曲线汇总分别保留全部 repetition 的 fixture replay 微秒数、原 IndexReport `elapsed_ms` 跨 resume 总和及实际 build 数；缺失原字段则汇总为 null，不以 0 补齐。这些点与主矩阵外层增量时间分栏，不混用时间范围或单位。

## 不可变证据、超时与退出码

每个 output 目录必须不存在，目录和文件均以 create/create_new 建立。原 run 不覆盖，旧失败不重写。输出包含：

| 文件 | 内容 |
|---|---|
| `plan.json` | 显式版本、seed、规模、重复、mutation 与预算配置 |
| `raw.jsonl` | 开始事件、实际输入清单及 digest、逐 mutation 原操作/摘要、逐 build 原报告、完整比较后摘要、fanout 原评估结果 |
| `worker.stderr` | 有界原 stderr；超过 256 KiB 明确预算停止 |
| `worker-summary.json` | 子进程正常完成或捕获错误后的结果；被强制 deadline 中断时可能不存在 |
| `report.json` | 父监督结果、实际退出码、worker PID、前后 binary digest、deadline/清理结果与 certification 状态 |

父进程监督同一个实际 eval executable。deadline 涵盖子进程的生成、构建、parity、证据阶段以及 stderr drain；即使同步 SQLite/index 调用卡住也由父进程结束 worker。Unix worker 使用独立进程组；仅该组被结束，正常退出后也清理该组，避免后代持有 stderr 管道。只对已经结束的 drain thread 执行 join；到达原 deadline 仍未结束则标记失败并关闭证据写入端，保证不会在报告完成后继续改写 stderr。summary 也以 128 KiB+1 的有界读取检查大小。父进程拥有临时目录，子进程被结束后仍清理其 fixture。轮询周期 10 ms，清理耗时单列。

raw 在整条 JSONL 写入前检查剩余预算，拒绝的记录不会写半行；保留已经写好的 raw。全局证据预算为 control/summary/report 与有界 stderr 留出 768 KiB。stdout 不参与测量，也不包含另一份原始输出。开始事件可用于辨认 deadline 时尚未完成的 build；未完成 latency 不会从报告中伪装成成功样本。

退出码：0 表示本地选定测量全部完成且对应诊断断言通过；1 表示实测诊断失败或未满足比较条件；2 表示输入、协议、工具、binary 漂移或基础设施错误；3 表示 deadline / 证据预算终止。**任何退出码都不等于完整 P8 或发布认证通过。**

## 统计与未完成范围

每个 repetition 从新索引开始，所有样本保留，不取最快值。主矩阵 cold 指空 parse/index 缓存，OS page cache 未清理；后续增量在同一进程/数据库上连续执行。索引调用原始 wall clock 与 IndexReport 阶段时间分列，包含 raw/snapshot 开销的外层阶段 wall 也不伪装为纯索引 CPU 时间。

本次默认 smoke 的 N=1 只验证可执行流程，不生成可信 p95/p99、置信区间、因果提速或质量泛化声明。release 模式的至少 30 次重复也不自动满足 09-BENCHMARK 对可靠 tail profile 的要求。需要同环境、冻结二进制与源码收据、注册门槛和独立统计审阅。

`release_certification` 与 `full_100k_certification` 始终明确 **not_run**；P7-020/P8-001/P8-004 的原依赖未被本工具绕过。100k 完整认证、C=1/4/8/16、连续资源峰值、真实 provider、heldout、跨平台和发行锁仍按各自任务单独验收。

### 本地验证入口

```sh
cargo test -p cc-eval --test p8_scale --offline --locked -- --test-threads=1
```

测试实际创建 60 文件的 native parser/SQLite/MCP smoke、独立 fanout 反例、父监督 deadline、退出后仍持 stderr 的真实后代进程和证据预算停止，并确认旧 output 不能被覆盖。集成测试显式采用 300 秒 correctness 工作预算以容纳共享 CI；CLI 默认仍是 120 秒，两者均不属于性能通过阈值。可设置 `P8_SCALE_TEST_EVIDENCE=/absolute/new/path` 保留 smoke raw；不设置时测试使用临时目录。实际运行结果由本批交付记录提供，不把测试源码存在本身当作已执行证据。
