# 09｜Benchmark 实现设计：以 oce-benchmark 为参考，扩展为代码索引验收系统

> 下文保留目标设计；本轮仅接受运行时与接口子集。

118 done / 1 in_progress / 73 todo，P5 为 18/20；P5-016～018 已验收，P5-019 正在实施通用查询质量修复与独立消融。P5-D 整批与 G5/M2 尚未完成。最新见 [P5-D-RUNTIME-IMPLEMENTATION.md](P5-D-RUNTIME-IMPLEMENTATION.md) 与 [P5-D-RUNTIME-GATE.json](P5-D-RUNTIME-GATE.json)。

冻结 623 文件、6675050 字节，摘要 `44b30ae15be8c0ab1cb1fe71c8cb3c5027d4d0085af17678565af89c9483c5a0`；38 条命令收据、源码归档、日志和不可变二进制一致。证据目录 `artifacts/benchmarks/p5d-20260930-resume/final-v3`。固定 51 题/306 请求无排序负差分、无效源码或新增完整性失败；原 source/intent Partial 和 S11 仍失败，完整检索 gate 保持 not_passed。两个可选 retrieval_strategy 字段以外，14 工具的旧输入属性和必填项保持一致。

新增 12 组 release 项目清理/租约观测，复跑 90 次旧查询成本和 192 次准入请求；可选 ps 进程树采样在本机停滞后显式关闭，对应 RSS 为 null、原生自身 RSS 单列。核心验证未跳过；不是 P5-019 的完整消融、100k、尾延迟或发行认证。

## 1. 冻结参考和审阅范围

参考仓库：oce-ai/oce-benchmark，master commit `d4f10554a18e31599d1e46d5d56da6588d4aa86c`，commit 时间 2026-09-04T09:43:58Z；2026-09-27 通过 GitHub connector 读取。已阅读 run_retrieval_eval.py 的完整返回范围、verify_benchmark_lock.py、evaluation-guide、出题 skill 主体和两份 metadata。不是运行过该 benchmark，也不是已经复核了 200 道题的全部标准答案。

固定源码：
- https://github.com/oce-ai/oce-benchmark/blob/d4f10554a18e31599d1e46d5d56da6588d4aa86c/scripts/run_retrieval_eval.py
- https://github.com/oce-ai/oce-benchmark/blob/d4f10554a18e31599d1e46d5d56da6588d4aa86c/scripts/verify_benchmark_lock.py
- https://github.com/oce-ai/oce-benchmark/blob/d4f10554a18e31599d1e46d5d56da6588d4aa86c/docs/evaluation-guide.md
- https://github.com/oce-ai/oce-benchmark/blob/d4f10554a18e31599d1e46d5d56da6588d4aa86c/.claude/skills/build-eval-benchmark/SKILL.md

两份 metadata 均标 questions=100，文档说明各 10 类×10 题：cc-switch 固定 `farion1231/cc-switch@40cac1a68edf8c9e7b3a89125cf40bb93a348404`；Flask 固定 `pallets/flask@22d924701a6ae2e4cd01e9a15bbaf3946094af65`。导入时仍须实际计数、校验文件 hash、逐题核验；本轮没有下载目标项目进行评分。

读取到的 GitHub license 元数据为 null，列出的 tree 无 LICENSE。它只说明本次未发现可直接依赖的许可证声明，不作法律结论。实施前登记使用方式和权限；默认独立实现方法，外部 benchmark 路径作为用户提供输入，不把整份第三方代码/题库直接复制进本仓库。上游运行器只作为固定外部对照工具。

## 2. 上游的实际行为与本项目映射

| 编号 | 上游实际实现 | 采用方式 / 必须补齐 |
|---|---|---|
| OB01 | 公共 HTTP 接口上传、等待、检索；reuse 时还使用 /find-missing | 保留黑盒原则；CodeCortex 走真实 MCP stdio，OCE 走 HTTP，不为评测强加生产 HTTP 服务 |
| OB02 | JSONL 字段 id/query_id、category、difficulty、query、expected_files | 增 importer；native schema 单独版本化，未知字段/重复 ID 严格校验 |
| OB03 | Top-1 命中任一 expected_files 即 1；不只首项 | 原样保留 compat_any_expected_top1，另加 native_primary_top1 |
| OB04 | 首 expected grade=2，其他=1；DCG 使用 linear gain/log2(rank+1) | compat 必须保持线性 gain，不能偷换常见 2^rel-1 后仍叫同口径 |
| OB05 | 从 formatted_retrieval 的 Path: 行取路径，保留首次出现并去重 | 兼容 parser 有版本/金样；MCP native 从结构化结果提取，空解析与空命中区分 |
| OB06 | glob 匹配，每个 expected 项最多消耗一次 | 兼容重现；native 用显式 alternative groups，歧义/重叠 glob 在导入时报告 |
| OB07 | suite 按 metadata 查 HEAD，可关闭；单文件 run 不自动执行 lock checker | 两种模式都强制 lock；不仅 HEAD，还验 dirty/untracked/submodule/LFS/实际输入 manifest |
| OB08 | HTTP query 错误按 0 分继续；suite 错误记报告，主入口未以失败总数显式非零退出 | 保留失败证据，但引入 result status 和 CI exit code；不能失败成绿色 |
| OB09 | resource.getrusage(RUSAGE_SELF).ru_maxrss，测评测进程，非服务进程 | runner/client/server/process-tree/外部服务分开；macOS/Linux 单位转换和采样峰值单独验证 |
| OB10 | 每题单次 latency，没有完整冷/热/并发分布 | 原始逐请求 samples、重复/随机顺序、分状态 latency，不能称为可靠 p95 |
| OB11 | 文件上传基于固定 ignore/大小/UTF-8 检查 | 公平对照用共同输入 manifest，避免 CodeCortex gitignore 与 uploader 输入不一致 |
| OB12 | must_contain 在出题 skill 中是人工注记，实际 scorer 不消费 | 导入时标 annotation_only；native evidence assertions 才是真正执行的约束 |
| OB13 | reuse_index 检查已有 blobs；没有源码 mutation/full-vs-inc oracle | 复用索引≠增量正确性，新增独立 mutation suite |
| OB14 | 题型叫 call_chain/cross_language，但评分仍然是文件命中 | 文件召回只证明找到了文件；有序路径和边正确性由 native graph suite 验证 |
| OB15 | readiness 循环读取 nonindexed/unknown；无独立逐 error 状态消费 | 自己的状态机区分 pending/failed/unknown/partial/ready，超时和分母透明 |
| OB16 | 目录分组报告与单变量比较 | 保留；另加 repo/category/language/difficulty/intent 宏平均与置信区间 |

不复制上游出题 skill 中“两次平均就稳定”等经验结论作为统计保证。样本量、重复次数、模型噪声和尾部估计按本项目基线校准。

## 3. 两个 score profiles，不混成一张伪排行榜

### oce-compat-v1

同一题库、同一目标 commit、同一输入文件 manifest，按上游行为计算：

```text
Top1Any = 1[第一个唯一返回路径匹配任一期望]
rel(expected[0])=2，rel(expected[i>0])=1
DCG@10 = Σ gain_i / log2(i+1)       # rank i 从 1 开始
nDCG@10 = DCG@10 / ideal_DCG@10
CompatScore = mean(Top1Any+nDCG@10)/2
```

每 expected 最多匹配一次，路径先保留首次出现去重。无期望项属于 schema 错误，不按 0 掩盖；上游存在的异常案例用兼容测试显式记录。glob 语义与 Python fnmatch 的平台差异要固定平台/路径规则；对重叠 glob 先做歧义分析，不能把不确定匹配顺序称跨平台完全一致。报告必须注明 adapter、parser version、source budget、effective top-k；仅截断最终输出不能假装上游内部预算已相同。

### codecortex-native-v1

支持多主答案/替代组、符号身份、源码跨度、关系和否定场景。指标独立报告，不用一个总分抵消数据错误：

| 指标 | 定义 |
|---|---|
| PrimaryTop1 | 排名第一的证据满足任一 primary answer group，而非任意 supporting 文件 |
| Recall@5/10/20 | 命中的答案组数/可适用答案组数；group 内 alternatives 命中一个即可 |
| MRR@10 | 第一个 primary group 命中的 reciprocal rank，截断定义固定 |
| Native nDCG@10 | 显式 grade+固定 linear gain；version 固定，不能和兼容分数混用 |
| SymbolAccuracy | repo+path+qname+kind/签名对应是否正确，不只比名字 |
| SpanPrecision/Recall | 返回源码 byte-span 与人工 gold evidence union 的交/并覆盖；不把整文件返回当满分 |
| EvidenceValidity | 路径存在、hash/版本/跨度/正文一致的证据比例；假源码/错位行号为硬失败 |
| FacetCoverage | 任务所需实现/接口/调用者/测试等 required facets 的覆盖比例 |
| DuplicationRate | 同版本源码重复字节/返回源码字节；metadata 与正文分别统计 |
| Empty/NoAnswer accuracy | 已验证无答案时正确表达 no_match；timeout 不算正确拒答 |
| Graph correctness | 有序 node/edge/方向/关系种类满足 gold constraints；文件命中不能替代 |
| Freshness | mutation 后已撤销旧事实/新事实可见、stale 显式程度与收敛时间 |

span 评测不依赖实现自己的 chunk_id，避免系统换切块后改变答案。gold 固定在源码字节/符号语义身份上。精确范围只是对人工标注区域的证据覆盖，不声称能自动判定所有语义相关性。

## 4. 实现落点与无重复引擎原则

继续使用 cc-eval，不另起 Python 服务/benchmark 仓库。新增开发用途 binary `cc-eval`，不改变产品 CLI 的 MCP-first 边界；仅测试工具允许输出原始 JSONL/统计文件。

```text
crates/cc-eval/
  src/bin/cc-eval.rs
  src/benchmark/
    mod.rs
    manifest.rs             # corpus/file/model/config/version lock
    schema.rs               # query + gold + result statuses
    importer_oce.rs         # external JSONL/metadata -> internal rows
    adapters/mod.rs
    adapters/mcp_stdio.rs   # spawn exact built artifact, real protocol
    adapters/oce_http.rs    # public endpoints only, optional eval-http feature
    adapters/rg.rs          # basic lexical baseline, literal args
    readiness.rs            # pending/failed/partial/ready + deadline
    normalizer.rs           # public results -> evidence rows
    metrics.rs              # pure scorer with golden tests
    oracle.rs               # full/inc canonical comparison, diagnostic tier
    mutations.rs            # deterministic edits in isolated fixture copies
    sampler.rs              # clock + resource/process-tree attribution
    runner.rs               # repetitions, randomized paired order, fail states
    statistics.rs           # bootstrap, paired deltas, macro/micro
    report.rs               # JSON/JSONL/Markdown, same source results
    gate.rs                 # pre-registered policy; exit code mapping
    ablation.rs             # only one variable/group changed per run
  benchmarks/
    schema/
    manifests/
    native/                 # newly authored corpora, held-out data separated
    mutations/
    goldens/
  tests/
    benchmark_scoring.rs
    benchmark_lock.rs
    benchmark_adapters.rs
    incremental_equivalence.rs
    benchmark_cli.rs
```

现有 runner.rs/lib.rs/types.rs/bench.rs 的 legacy corpus/断言保留，逐步适配同一 Results/metrics primitive；不能保留两个不同 nDCG 实现。现有进程内 rmcp dispatch seam 用于快速回归，正式 end-to-end score 使用独立 MCP 子进程，二者标签不同。白盒 DB dump 只用于 canonical oracle/阶段成本诊断，不给 retrieval system 提供 gold 或跳过真实公开接口。

## 5. 输入锁与工件 schema

SuiteManifest 至少锁：engine SHA+binary digest+build profile/features、目标 repo commit、dirty/submodules 状态、corpus/query/gold digest、实际文件 manifest digest、scoring version、adapter/parser version、配置 digest、model revision+encoding space、OS/CPU/RAM/toolchain、随机种子、冷/热定义、timeout/budget、runner version。

每题输入至少：id、repo_id、split、category、language、difficulty、query、query_family、intent、hard_scope、gold answer groups、required evidence/facets、optional graph constraints、mutation profile。用 query_family 把改写/中英文配对一起分配 split，避免泄漏。

每次查询原始结果至少：run_id/case_id/repetition、status、start/end monotonic、latency、raw artifact path/hash、normalized hits/spans、lane status、effective budget、generation/readiness、cost usage/estimated flag、reason。secret 永不落盘，私有源码只在用户授权的本地 evidence dir 保存。

正式 exit code 设计：0=完整且 gates passed；1=有效测量但 gate failed；2=输入锁/配置/协议/基础设施使测量无效；3=用户取消/预算终止（partial artifacts 保留）。schema/parser 失败不能变成“0 命中成功”。超时同时记录产品可用性失败和 deadline-censored latency，不从样本集中删掉。

## 6. 公平比较与 corpus 准入

suite 先生成两边都使用的 SourceManifest（规范相对路径+bytes hash+大小+纳入原因）。在独立临时 checkout 中应用相同 ignore policy；既不默认上传 private repo，也不把 docs/roadmap、答案文件、benchmark 本身混进被测源码。unsupported 文件/语言需显式列入 capability coverage；公共子集和全能力集分开，不选择性过滤难题。

OCE 的 source uploader 与 CodeCortex scanner 默认规则不相同。报告必须输出 input_diff；若不能对齐，不得直接声称系统 A 比 B 好。关闭/开启 embedding、使用不同模型，都是独立配置档案；在成本和网络条件不等价时保留多轴结果，不用综合排名掩盖条件差异。

不能为了分数写题目关键词词典、gold path boost、仓库特判；测试 corpus 不允许进入生产包。数据集变更与引擎调参分开评审，评分器变更必须重算两边，不能拿新评分与旧报告比较。

## 7. 数据集路线

| 层 | 规划覆盖（目标，不是已完成数量） | 阶段 |
|---|---|---|
| D0 | 40 个以内的确定性关键缺陷/契约样例，足够快且可定位 | P0/P1 |
| D1 | 外部 cc-switch+Flask 兼容集，共 metadata 声明的 200 题；导入前核验 | P0 importer，P1 起连续运行 |
| D2 | 6 个以上真实仓库，覆盖 Rust/JS/TS/Python/Go 与至少一个 mixed monorepo；初始约 600 个审阅问题 | P0 方法，P2–P5 扩充 |
| D3 | 100 个以上 mutation sequences，含单操作与多步组合；小型公开夹具可复现 | P0 oracle，P2–P7 |
| D4 | 1k/5k/10k/50k/100k 文件合成规模，外加真实仓库；同时报告 symbols/chunks/edges/vector count | P0 seed，P8 完整认证 |
| D5 | held-out repo/module/query-family；中英改写与自然语言零词面重合；不得被开发集反复调参污染 | P0 分割规则，P8 认证 |

保留上游十类：file_exact_match、configuration_lookup、component_location、api_usage、semantic_feature、error_handling、architecture_understanding、cross_language、symbol_location、call_chain。额外 tags 表达 ambiguous-name、reexport、config-only-change、no-answer、stale-index、filter、budget、generated/noisy 文件，避免新增分类互斥冲突。

每题先阅读固定源码、标主/支持答案，再独立复核；必须包含错误但字面更相近的 hard negatives。LLM 可辅助起草、找遗漏和二次审阅，不能同时由同一个系统生成答案又给自己评分。gold 争议进入 quarantine，版本化记录，不为了回归变绿直接删题。若使用 LLM judge，冻结 prompt/model、保存依据和重复一致性，作为旁证而非核心确定性 gate。

## 8. 增量 oracle

每条 mutation 从同一 pristine snapshot 克隆 A/B：A 连续执行事件域/普通增量，B 在每个检查点全量重建。canonical compare 排除 rowid/timestamps/物理插入顺序，但保留符号身份、目标关系、策略、歧义、公共表面、doc/span、FTS 实际查询和 semantic manifest。对允许部分闭包的阶段比较已声明 freshness，并在 reconcile 后要求一致。

操作矩阵：body/comment/signature/visibility/import/export/reexport/config/新增/删除/rename/case-only rename/新同名符号/负向查找补全/目录移动/feature 条件/大于 dirty budget 的 fanout/重导出环/同 mtime-size 内容变化。再叠加 worker 慢响应、取消、重启、DB 换库和 model switch。

不要让两条路径调用同一个错误 normalization 后“证明等价”。保留手写 gold 小夹具、DB integrity/FK、公开 MCP 查询和差异最小化样例；生成器使用固定 seed，失败序列自动 shrink 并落证据。真实源码可接受启发式 precision 限制，但不能接受同输入随 warm/cold 历史改变结果。

## 9. 性能采样与资源

分离：cold build（空 index/空 parse cache）、reopen query（新进程，OS page cache 未清）、warm uncached query（不同 query 防 result-cache）、warm cache-hit query、no-op update、single body update、public API fanout、config-only update、batch 1/10/100/1000 文件、watcher backlog、semantic backfill/partial query/model switch。

冷 OS cache 仅在明确且可复现的受控 runner 上测；没有清理就不要标 cold disk。常规 profile 不用 sudo 清缓存。release build 测性能，debug/test build 的结果单独标签。每种 query profile 预热后采集全部样本，禁止 best-of；起始建议每层至少 30 个重复，正式尾延迟 profile 至少 200 个同类样本并报告 N/CI，不能因样本小仍声称稳定 p99。各数值是设计下限，可在 P0 资源评估后锁定。

并发 C=1/4/8/16，混合读取与构建，采集 queue+service+end-to-end 时延、吞吐、error/timeout、DB lock wait、worker contention。负载发生器记录 offered load 和排队，不能用闭环吞吐隐藏尾部延迟。

资源分 client runner、CodeCortex 子进程树、LSP/模型服务、OCE 外部容器。缺权限时 reported unavailable，不填 0。macOS RSS 单位与 Linux 不同；ru_maxrss 是高水位，不是瞬时 RSS，采样函数必须有平台测试。记录 peak sampling interval、index/FTS/cache/向量占用、读写字节、model requests/tokens/cache hits/重复费用不确定次数。

## 10. 消融与统计

固定同 corpus、输入、预算和 seed，配对比较：现状→BM25 修复→软范围修复→模块/增量修复→新 chunker→coverage selector→dense→rerank。每个实验仅改变一组解释得清的因素；chunker 与 embedding 必须先单独 baseline，再合并。保留 lexical-only、graph-off、dense-only、local+dense、selector-off 和 rg baseline。

按 repository/query-family 分层 bootstrap 置信区间，报告 paired per-query delta、macro per repo/category、micro mean、失败样本。不能把同题 100 次重复当 100 个独立检索问题。latency 用独立时间分布，quality 的 stochastic rerank 重复与 query sampling 分开。

门槛在 P0 基线冻结后预注册。建议起始 guardrails：关键确定性正确性 100%；scope/source/版本泄漏 0；goldens scorer 完全一致；质量类平均退化预算 0.01（绝对 nDCG）与 PrimaryTop1 1 个百分点作为评审触发值，而非任意容忍错误；性能退化超过 10% 进入解释/审批，超过 20% 阻断目标 profile，需满足采样/同环境条件。语义能力要求 held-out 语义类收益 CI 下界>0 且 exact 类无实质退化；不预承诺具体加速倍数/分数。小样本 CI 不确定时结论 inconclusive，而非强行 pass。

## 11. CI 档位与输出

PR fast：schema/scorer/locks、关键契约、固定小 mutation、真实 stdio smoke，不联网模型。Nightly：公开真实仓库、10k/50k、随机 mutation seeds、并发、fake 故障与恢复。Release：100k、cold build/MSRV/macOS/Linux、长时 soak、完整 holdout、显式批准的真实 provider 对照。付费/私有源数据 profile 始终人工启用，不因普通 PR 自动发送代码。

每轮输出目录 `artifacts/benchmarks/<run-id>/`（未来实现）：manifest.json、queries.jsonl、raw/、normalized.jsonl、metrics.json、latency.jsonl、resources.jsonl、failures.jsonl、comparison.json、report.md、gate.json。summary 只由这些产物生成；latest 只是指针，不覆盖旧原始 run。run-id 含 engine/corpus/scoring digest 便于复核，不含密钥。

## 12. 原设计命令示意（P0实际命令以USAGE为准）

```sh
cargo run -p cc-eval --bin cc-eval -- validate --suite <suite-manifest>
cargo run -p cc-eval --bin cc-eval -- run --backend mcp-stdio --binary <exact-binary> --suite <suite> --profile pr
cargo run -p cc-eval --features eval-http --bin cc-eval -- run --backend oce-http --suite <suite> --profile quality
cargo run -p cc-eval --bin cc-eval -- compare --baseline <run-dir> --candidate <run-dir> --gate <gate-policy>
cargo run -p cc-eval --bin cc-eval -- mutate --suite <mutation-suite> --seed <seed>
```

实际 CLI 名称/参数在 P0 contract tests 冻结。此文不声称已经实现，也不在本轮运行模型、创建外部服务或修改基准目标仓库。
