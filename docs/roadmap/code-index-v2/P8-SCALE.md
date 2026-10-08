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
| batch | 对恰好指定数量的既有生成文件追加带迭代编号的注释，优先原普通代码顺序，必要时补同树既有 route TS 与 YAML | 保留每个操作及 before/after digest；TS/Rust 使用 `//`，Python/YAML 使用 `#`；不增加文件或缩小批次，辅助 `tsconfig.json` 不进入 batch |
| fanout | 独立 TS fixture，1 个 API 与 N 个显式 import/call consumer；真实修改 API 签名 | 原 mutation_case、逐 consumer 手写 call-edge 断言、每次原生增量报告、full canonical 事实与原失败签名 |

fanout 是独立 workload，报告实际 fixture 文件数为 N+1，不把它当成主合成矩阵的 1k–100k 工作量。超过 dirty budget 只是输入条件；`first_build_incomplete` 记录实际观察，不能据 requested fanout 虚构触发了 partial closure。每次 resume 均有单独 raw IndexReport；到达预算仍不完整则不得认证 parity。

主矩阵每次 `build_finished` 保存原封不动的 IndexReport，包含 `files_*`、`dirty_plan`（含实际 dependency SQL 工作）、`document_changes`、`project_model`、`resolution_freshness` 和 `phase_timing`。report 缺少的测量不会补 0。worker 单进程 RSS/CPU snapshot 有原 reader 的明确归属，未知可为 null；它既不是连续峰值，也不包含未知进程树。

SQLite 计数分列 files、symbols、chunks、call_edges、semantic_edges、test_edges、所有原 oracle 表以及 DB/WAL/SHM 字节数。不同 edge 表可能表达不同关系，不把它们相加并冒充去重总数。向量字段固定表达 `state: disabled, count: null`，不以 0 个向量表示完整语义工作量。

## Oracle 语义与容量

小规模直接调用既有 `oracle::canonical`：执行原 integrity/FK 检查，比较原 15 个表的完整 canonical rows，只排除原 oracle 已明确排除的物理列。比较先于摘要；证据输出各表两侧行数、完整 digest 和最多 3 个差异示例。`resolution_manifests` 的完整 JSON 排序会因 digest 变化而移动行，因此仅其诊断示例按真实 `file_path` 对齐，导出同文件的两侧完整原始行、差异文件总数与最多 32 条递归 payload 差异；单边缺失明确标记。其他表的示例仍按排序位置配对。所有示例限制和对齐方式只影响诊断输出，不影响完整原 oracle 判等范围。

既有内存 oracle 的每表 100000 行保护保持不变。驱动在调用前读取真实两侧表计数；超过该界限时使用 `oracle::compare_streaming` 的磁盘排序路径。两个路径共用唯一的字段投影和 SQLite 值到 canonical JSON 的转换，仍比较原 15 张表的全部行、保留重复行。每侧先建立只读事务快照并执行原 integrity/FK 检查；临时 SQLite 按完整 canonical JSON 文本的二进制顺序建立覆盖索引，两个有序流逐行比较，随后才计算摘要。数组摘要与旧 oracle 的完整 JSON 数组摘要一致。

磁盘路径固定排序页缓存为 2 MiB，默认每侧每表最多 500 万行、两侧所有表累计 canonical 行文本最多 4 GiB、临时 SQLite 文件最多 8 GiB、单行原始内容与编码后文本各最多 1 MiB。临时库禁用 journal、同步写与 mmap；所有文件仅属于本次诊断，成功或错误返回后清理。当前物理布局为单个 `WITHOUT ROWID` 主 B-tree，主键 `(side, value COLLATE BINARY, ordinal)`；每侧每表的 ordinal 唯一，因此重复行完整保留，canonical 正文无需再同时存于 rowid 表与第二份覆盖索引。每表完成后 DELETE 并复用同一个有页数上限的临时库。比较直接按主键顺序读取，真实 SQLite 查询计划回归要求使用 PRIMARY KEY 且无 TEMP B-TREE，避免另建不受页数限额约束的外部排序文件。页缓存不是整个产品或 SQLite 进程峰值声明。行数、字节或磁盘限额耗尽、输入损坏、未知表与 I/O 错误都会失败，不能变成部分采样的 `equal=true`。

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

## 完整矩阵的确定分片与原始记录重放

单个 `--matrix --repetitions 30` 输出在输入清单阶段就可能超过原 512 MiB 上限。
2026-10-09 增加可选 `--shard-index I --shard-count C`：`repetitions` 仍表示预先登记的
**全部**重复数，release 的至少 30 次要求不变。第 I 片执行半开区间
`[floor(N*I/C), floor(N*(I+1)/C))`，原始 `repetition` 身份不从零重编。
每片仍有独立的 deadline、512 MiB 最大输出预算、临时树、不可覆盖输出、原始两侧
build 和完整 15 表 oracle。`shard_only=true` 明确说明该输出只覆盖局部样本；单片
成功不会将矩阵、P8 原任务或 G8 改为通过。

`scripts/p8_scale_matrix.py` 固定 1k/5k/10k/50k/100k、batch 1/10/100/1000、
fanout 1/4/16/64/128、dirty budget 8、最多 128 次 resume，默认 N=30、每规模 6 片。
普通 1k 生成树包含 960 普通代码文件、20 route TS 和 20 YAML；新的 batch policy
在需要时使用这些已经存在的额外输入，因此实际 1000 文件变更不再被代码专用列表的
960 上限阻塞。其他小批次仍采用原普通代码顺序，原操作正文与 before/after digest
继续完整记录。该变更不声称所有被修改文件都是普通代码文件。

fanout 是独立的小夹具工作量，预先指定由 **1k 规模的 6 片**执行，共每个 fanout
30 次。其他规模的片使用显式 `--skip-fanout`，该选项只允许和 shard 一起使用；
原 fanout 参数仍保留在计划中。聚合器拒绝把每个规模重复执行的同一 fanout population
合并成 150 个独立重复。未使用 shard 时，旧默认 smoke、全部 fanout 和原参数边界保持。

```sh
# 构建收据包括完整固定 crate/Cargo 清单、实际 Cargo compiler-artifact、
# release profile、编译前后源码、工具链，以及保留的同一可执行文件。
python3 scripts/p8_scale_matrix.py build \
  --root "$PWD" --output /tmp/p8-scale-fixed-build-new \
  --target-directory /tmp/p8-scale-fixed-target-new

# 示例只执行 30 个必要片中的 1 片。每个 output 均须是新的目录。
python3 scripts/p8_scale_matrix.py run \
  --root "$PWD" --build /tmp/p8-scale-fixed-build-new \
  --scale 1000 --shard-index 0 --shard-count 6 --repetitions 30 \
  --output /tmp/p8-scale-shard-1000-0-new

# 只有收齐所有 5×6 片后才能通过本次测量覆盖检查。
python3 scripts/p8_scale_matrix.py aggregate \
  --build /tmp/p8-scale-fixed-build-new \
  --inputs /tmp/p8-scale-shard-*-new \
  --output /tmp/p8-scale-matrix-review-new
```

构建和每片执行均以 SHA-256 封存实际文件清单。聚合时再使用保留的同一二进制的
只读 `--hash-file` 入口，流式核验 native plan/raw 中原有的 BLAKE3 digest，
不会因为 Python 环境缺少 BLAKE3 包而跳过核验。编译收据锁定实际优化 profile，
单纯路径、Git HEAD 或传入 `--release` 字样不足以证明二进制来源。
构建收据还固定当前 `p8_scale_matrix.py` 与 `p7_build_identity.py` 的完整 SHA-256，
并逐字节核对固定提交的 Git blob；build/run/replay 都必须在同一提交、同一版本的
driver/helper 上执行。运行后的脚本漂移或在另一个 checkout 加载的 helper 均被拒绝。

聚合器逐条消费完整 raw，要求每个注册样本的输入、两侧 cold、8 个 mutation 阶段、
每次原始 build、closure 完成态、全部 15 表行数/判等/摘要、配置独立见证齐全。
它重验 no-op 的实际零改动、批次数量与操作正文、每个 fanout 的完整 canonical
事实、逐 caller 调用边和每次 resume；同名重复记录、缺片、截断 JSONL、预算停止、
失败事件、不同源码/二进制/配置/输入/实际硬件环境均失败。输入与文件清单的摘要
只用于完整性绑定，不替代原 oracle 的 Value 判等，包括 signed zero 的既有语义。

全部默认片对应 **1500 个样本**：5 规模 × 9 主阶段 × 30 次，加 5 fanout × 30 次。
输出保留每组所有计时，分开原 engine 时间、MCP 调用 wall、包含证据处理的增量 wall、
full control、oracle wall 和整个 fanout replay；不混用这些范围。内存 canonical
排序只缓存每行原 JSON 排序键一次，不改投影、重复行、排序后 Value 判等或旧行数保护。
完整排序键只在当前表排序期间保留，旧数据和已有失败不会被重写。

`IndexReport.build_timing` 增加完整函数阶段计时，原 `phase_timing` 的 6 个毫秒字段
及 `elapsed_ms` 定义不变。新字段使用微秒：

| 字段 | 实际测量边界 |
|---|---|
| `prepare_us` | prepare 入口到其输出准备完成，包含扫描、解析、解析后快照和 full staging |
| `commit_write_us` | commit_write 入口到其输出准备完成，包含版本/配置检查、提交、catalog cache 与报告准备 |
| `postprocess_compute_us` | postprocess/analysis 计算函数的完整阶段 |
| `postprocess_apply_us` | apply 入口到最终报告准备完成，包含版本检查、写入与 freshness 读取 |
| `between_stages_us` | 上述函数边界之间的实际间隔，包括交接、外层 Indexer 创建和锁等待；不把它全部称为 queue time |
| `total_us` | 同一 prepare 起点到最终报告准备完成的连续时钟 |
| `prepare_snapshot_us` | prepare 内的输出快照和 chunk 压缩，属于 `prepare_us` 的子区间 |
| `full_staging_us` | 实际 `write_full_snapshot_build_staging` 调用，属于 `prepare_us` 的子区间；增量时明确为 null |

前四阶段与 `between_stages_us` 完整分割 `total_us`；由于 7 个时间区间各自取整，
允许的差仅为 0–6 微秒。聚合器逐个原 IndexReport 核验这个守恒关系、子区间包含关系、
build mode 对应的 staging 执行状态，以及内层时间不大于外层 MCP/增量/replay 时钟。
`prepare_snapshot_us` 与 `full_staging_us` 不再次加到父阶段上。原来处于 resolve 计时
之后、write 计时之前的 full staging 成本因此具有直接计时证据，不会被推定为 0。
新增实际 split-build 控制在三个交接处各等待 5 毫秒，验证该时间确实进入交接字段；
这种人为等待只用于计时正确性，不作为性能样本。

新的 `.github/workflows/p8-scale.yml` 仅由明确 dispatch 或 PR 的 `p8-scale-run`
标签启动；普通 PR 不会自动触发整个大矩阵。它先构建一次固定二进制，再分片执行，
最后下载每片原件独立重放，失败片也上传。默认旧协议遇到实际 runner 硬件不一致时
聚合明确失败；显式容量协议采用下面登记的按 host/CPU/kernel 分层覆盖规则。
一个 workflow 或测试文件的存在不构成已执行证据；
最终是否完成原 P8-005/006 仍由实际记录和独立任务验收决定。

```sh
python3 -B -m unittest discover -s scripts/tests -p test_p8_scale_matrix.py -v
```

这些 Python 控制使用明确标注的合成协议输入；它们验证缺失、漂移、假相等与重复
会被拒绝，不能充作产品规模执行。完整测量报告仍写
`release_certification: not_run`、`G8: not_evaluated`，不自动改动 `tasks.json`。

## 显式大规模容量协议 `scale_capacity_v1`

2026-10-09 的旧 8/128 工程预检先在 1k 暴露 Python direct export 的无关转发链加载缺陷；
按原 resolver 终止语义修复后，同一 1k 计划完成所有阶段。随后原 10k 计划保留了全部
9 组原件：cold/no-op 完成，body/API/config 与全部四种 batch 各在 129 次真实构建后
仍明确 `incomplete_not_certified`，不会因有 full control 而假装闭包完成。旧记录的来源为
`803773e4`，其失败没有被重标成新协议成功。这些工程预检也不代替正式 N=30 矩阵。

大范围失效有现存语义依据：函数体插入使位置派生的 symbol_id 移动；接收者调用的
SymbolInventory 依赖、ModuleConfig 依赖和未知/转发表面的保守传播还可能扩大 dirty
闭包。取消这些依赖而不证明完整读取依赖会漏掉过时目标。本协议显式登记更大的每次
工作预算，继续处理完整闭包；原深度/文件/转发边硬预算与 incomplete 语义保持。

| 参数 | 新协议固定值与范围 |
|---|---|
| 主规模 dirty work budget | 200，与当前产品默认值一致 |
| 主规模最大 resume | 1024，在既有 CLI 合法范围内 |
| 独立 fanout dirty/resume | 8/128，保留原专门超预算压力 |
| 主规模/阶段/重复 | 原五规模与全部九主阶段，N=30；每规模 30 片，每片恰好 1 rep |
| fanout population | 只由 1k 的 30 片执行，五个 fanout 各 N=30 |
| canonical 累计容量 | 显式专用入口 16 GiB，仍累计两侧全部十五表；通用默认 4 GiB 与通用 validator 上界 8 GiB 不变 |
| 同时占用 scratch | 8 GiB，保留原 max_page_count、2 MiB cache 和每表复用 |
| 每侧每表/每行保护 | 500 万行/1 MiB，保持原值 |
| native deadline/证据 | 18,000,000ms/512 MiB，每片独立，耗尽仍失败 |
| 可用磁盘预检 | `256 KiB × files + 4 GiB`；100k 约需 28.4 GiB；这是登记估计，不是峰值或足够空间的证明 |

`ScalePlan.capacity_profile` 必须明确为 `scale_capacity_v1`，同时指定 release、单一规范规模、
每片一个全局 repetition 与固定 200/1024。仅输入这个名字不会悄悄覆盖原 CLI 默认值。
旧 smoke/release 计划、默认 4 GiB oracle、通用非法预算拒绝和默认严格同环境重放继续可用。
新入口与旧入口复用同一投影、只读事务、integrity/FK 检查、全部重复行和逐行 Value 比较；
容量变化不改变输入、实际批次数量、全表事实、独立配置见证或原性能阈值。

矩阵记录完整固定 `capacity_contract`，大表的每份原 parity 必须报告精确匹配的容量、
scratch、cache 与布局。每片启动前记录实际 statvfs；其 `TMPDIR`/`TMP`/`TEMP` 显式绑定
同一受检临时根，native 父进程在其中管理全部 fixture 与 oracle。空间不足封存为
`not_run` 并返回非零，不启动测量、不自动改预算或清理宿主内容；结束再记录空间观察。
持续峰值仍需独立资源证据，前后 statvfs 不能冒充峰值。

```sh
python3 scripts/p8_scale_matrix.py run \
  --root "$PWD" --build /tmp/p8-scale-fixed-build-new \
  --scale 1000 --shard-index 0 --shard-count 30 --repetitions 30 \
  --capacity-profile scale_capacity_v1 --output /tmp/p8-capacity-1000-0-new

# 收齐全部 5×30 片及原五个 fanout 的每组 30 次后，才可能覆盖完成。
python3 scripts/p8_scale_matrix.py aggregate \
  --build /tmp/p8-scale-fixed-build-new --inputs /tmp/p8-capacity-*-new \
  --shard-count 30 --repetitions 30 --capacity-profile scale_capacity_v1 \
  --output /tmp/p8-capacity-matrix-review-new
```

新工作流明确选择该容量协议。它采用 `heterogeneous_stratified_coverage_v1`：每个样本
绑定实际 host、CPU 型号/数量、kernel 与 native CPU 并发观察；每组列出所有样本到
environment ID 的映射，并只在各环境层内计算分布。不同 host 不拼成同环境样本，
完整组的 N=30 仅表示全局覆盖；各层可能只有 N=1，层数与样本数全部保留。不输出
跨环境混合均值、因果提速或稳定尾部分位数，也不挑最快 host。源码、实际二进制、
工具链/构建 profile、features、seed、输入和真实运行配置仍须一致；原缺片、重复、
失败、未闭合、逐阶段 full parity 和 fanout 手写真值检查没有减少。
