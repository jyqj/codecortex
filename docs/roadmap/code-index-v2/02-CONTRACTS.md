# 02｜代码索引核心契约

> 本文是拟实现规格。契约编号 C01–C16；实现任务、依赖和状态只在 tasks.json，验证编号在 06-VALIDATION.md。未特别说明的字段名是逻辑名称，实施时需经过实际类型/数据库映射评审。

## C01｜能力与可信度不混用

语言能力不是一个 supports=true。ParseCapabilities 至少表达 symbols、imports、exports、types、call_sites、chunk_boundaries 和各能力的 exact/heuristic/unsupported 状态。词法召回、静态解析、运行时观察、LSP 观察分别记录 provenance。相似度不是解析置信度；命中分数不能自动升级 ParserTier。现有 GraphExplain/BuildExplain/score_trace 继续使用，新诊断从这套词汇延伸。

当前 overlay_files 只是文件路径提示。没有传入内容、版本和失效协议时，不得声称读到了未保存缓冲区。本次主线不扩展编辑器协议；未来真实 overlay 用独立 SourceSnapshot，不能混入磁盘已提交事实。

## C02｜源码快照与跨度

SourceSnapshotId = hash(原始字节 + 编码标识)，至少保存文件规范路径、字节数、原始内容 digest、读取时元信息。byte span 使用半开区间 [start,end)，对外 line span 使用明确的 1-based inclusive 约定；统一转换函数负责 CRLF、UTF-8、多字节字符和末尾换行。源码正文必须来自同一快照的连续字节区间，不从 AST token 重建。展示换行归一化必须标明，原始 hash 仍按 bytes 计算。

一次 build 的文件集合不是操作系统原子快照。扫描后的文件变化要在读取/发布时检查，重读或标记 pending，不允许“hash 是旧的、解析正文是新的”。strict_hash 可作为审计模式；默认 stat 快路径保留，但 watcher 溢出、重命名、配置变化和 git 切换触发对账。

读取证据时按 snapshot_id 取得索引期文本，或验证磁盘 hash 后读取。磁盘已改变时返回 indexed/stale_with_disk_change 状态，不拿旧行号解释新文本。已删除文件不得被当成当前存在；旧快照证据只能显式标 historical，不能混入默认当前搜索。

## C03｜公共表面 PublicSurface

公共表面是“可能改变其他文件解析结果的接口”，不是简单 JS export 字段。条目包括 logical symbol identity、qualified name、kind、visibility domain、signature/type surface、reexports、module membership、条件编译/解析条件摘要。序列化固定排序、长度分隔和版本，不能用不明确的字符串拼接或 HashMap 顺序。

| 语言 | 主线能力范围 | 保守处理 |
|---|---|---|
| JS/TS | named/default/type export、ES reexport、两步 forwarding、CommonJS 静态可识别赋值 | 动态导出/运行时 require 标 heuristic，影响者保守重解析 |
| Rust | pub/pub(crate)/受限可见性、模块项、公开签名、pub use、workspace 依赖别名 | cfg/features 的所选条件进入项目模型；宏展开未覆盖时不承诺编译器精度 |
| Python | 模块顶层绑定、静态 __all__、显式/星号重导出、包 __init__ | 动态 __all__/monkey patch 标 unknown surface，不能当成无导出 |
| Go | package 对外可见标识符、导出类型/方法、包文件集合 | build tags/GOOS/GOARCH 固定；未支持条件保守声明 |
| Java/C/C++/generic | 先提取可证明的声明和依赖面，声明 capability | 不承诺完整 classpath、宏/模板、动态派发语义；未知导致保守失效而非 silent normal |

返回 EmptyKnown 与 Unknown 必须不同。指纹缺失不能视为“公共接口未变”。不直接将所有局部变量纳入 public surface。

## C04｜解析结果与依赖证明

ResolutionOutcome = Resolved(target, strategy, confidence, dependencies) | Ambiguous(candidates, reason) | Unresolved(reason, dependencies) | Unsupported(capability)。确定性候选按语义相关性排序，再以规范路径/qname/kind/stable identity 固定 tie-break。桶内插入顺序、线程数和目录缓存历史不能改变最终结果。

如语义本身不能区分，保留 Ambiguous；不要因确定性排序就把第一名伪装成精确目标。候选截断标记 limit/candidate_count_lower_bound，不能标成唯一。

解析依赖至少包括：目标公共表面；所查询模块入口/别名配置；同名候选桶；负向查找“这个名字/路径此前不存在”；相关文件集合；feature 条件。新增同名符号必须使依赖 global-unique 的结果重新判定，删除/新增文件必须使负向路径查找失效。只依赖 import graph 不足以覆盖这些场景。

## C05｜增量闭包与一致性状态

把变化分为 SourceBody、PublicSurface、ModuleConfig、FileInventory、ParserVersion、ChunkerVersion、FeatureConditions。优先只对需要的层更新：纯 body 变化通常不传播导入者；surface/config/inventory 变化按依赖图传播；parser 能力版本改变可以要求重建对应语言事实。没有充分依赖信息时保守扩大，不静默跳过。

复用当前 dirty closure 和 DirtyResolveOnly。跨语言 package 级关系先映射为 package contribution，再关联文件。重导出环以固定点处理，指纹采用环/组件稳定化策略，不把每轮拼接上游 hash 导致永不收敛。

预算不足时持久化 remainder/frontier、起始 basis、待处理原因；状态 incomplete，后续内部增量维护任务继续。该任务属于索引维护，不引入通用 Agent 工作流。所有根因事件都进入 durable frontier，不能一次返回 budget_exceeded 后永远遗忘。basis 改变时重新验证 frontier，必要时重新求闭包。用户要求完整关系时可触发有预算的对账或返回明确不足，不能默认把旧边当新边。

## C06｜ProjectModel 与模块解析

FileCatalog/WalkManifest 是允许读取的文件目录来源；ProjectModel discovery 一次读取配置，保存内容 digest 与继承/引用 DAG；resolve 阶段仅消费不可变快照。配置文件即使不产出源码 chunk 也必须参与 watcher 和失效。禁止对每条 import 重复遍历项目或读取 tsconfig。

TypeScript 路线：最近配置、JSONC、extends 循环保护、paths/baseUrl、workspace package、package imports/exports、条件分支、扩展名解析。固定的 export 条件优先级不是所有 Node/TS 模式真理；记录 module_resolution_mode 与 conditions，未知模式保守返回。通过规范夹具/可选编译器 oracle 证明声明的支持范围。

Rust 路线：保留 cargo_workspace 已通过能力，再补 crate/self/super、模块文件、路径别名、visibility、cfg。Python 路线：src layout、相对点、package/namespace 边界、__init__；不得执行用户 Python 文件来“发现”导出。Go 路线：go.mod/go.work/replace、包目录和文件集，平台条件显式。所有配置解析限制深度、大小、环和项目外访问。

## C07｜实体身份、文档版本、向量产物分层

EntityKey 表达源码实体；DocKey 表达一个检索单元；DocVersion 表达该单元的内容与投影版本。现有 symbol_uid 可以作为迁移依据，但路径参与的 UID 不承诺跨重命名稳定。重命名 lineage 是显式可选映射，不靠猜测把不同实体合并。

DocVersion 覆盖 source digest/span、chunker version、document kind、render template version。EmbeddingInputHash 必须由实际发送的 input bytes 和 DocumentEncodingSpec 决定，不能用空白归一化 hash 替代原始 Python/string 语义。query instruction 不进入文档输入时，其变化只失效查询编码/缓存；模型坐标空间变化则文档/查询一起切换。

DocKey 可以在结构边界稳定时复用；无法稳定匹配则生成新版本并撤销旧 manifest 映射。相同输入可共享向量产物，但不同文件来源不能因去重丢失 provenance。重复函数、相同模板、路径变动、顶部插入必须有显式测试。legacy chunk_id 只作为兼容 wire 字段，不参与异步 publish 判定。

## C08｜切块与渲染

复用每文件一次 AST 提取的紧凑边界，不保留全仓 AST，不引入第二解析器。小函数/方法整体；大类递归至成员；大函数沿 statement/block；注释与签名优先关联；相邻小碎片仅在同父域、同文档类型并满足预算时合并。gap、文件尾部、泛型回退和超长行必须走同一上限。

SourceText 与 EmbeddingText 分离：前者是可核验源片段，后者可加路径、语言、父符号、签名、文档类别；不无上限把依赖正文拼入每块。上下文元数据变化触发哪些文档重新编码由投影依赖明确记录。

预算至少包括 max_bytes/max_chars 和模型 token limit。无可靠 tokenizer 的 provider 使用保守预算和明确 token_estimate，不能称精确计费。超长单行采用 UTF-8 安全 byte spans 或明确 skipped；禁止把被截短正文标成完整行。覆盖不变式检查输出 span union 与应索引源码范围，允许有明确原因的跳过，不以空数组掩盖 parse 错误。

## C09｜范围语义

HardScope = repo/worktree namespace + caller file_paths/path_prefix/languages 的交集。None 表示不增加约束，Some(empty) 表示空集合，永不退化全仓。DSL 与参数冲突按交集或显式错误处理，规则必须统一。

SoftHints = working/recent/pinned/overlay/preselected/graph-neighbor，只影响优先级和 lane 预算。不得写回 HardScope。各 lane 输出和最终 hydrate 均做 hard scope 守卫。路径归一化不全局小写；大小写取决于文件系统/仓库协议，Windows 分隔符映射单独测试。

独立 exact-symbol/path/lexical/dense 召回不被软预选锁死。grep 可先 scoped 再 bounded global fallback，并输出 scanned/cap/reason。向量 scope 筛选应发生在 top-k 之前；ANN 做不到时 overfetch+refill 并报告欠召回，不在过滤后假装足额。

## C10｜候选与通道状态

CandidateRef 至少包含 doc_key/version、source span、lane id、lane rank、raw score、scoring spec。LaneOutcome 必须表达 disabled/not_configured/complete/partial/timeout/unavailable/error/cancelled，附耗时、候选数、覆盖度、截断理由。没有结果与没有执行不是同一个状态。

非可选基础数据读取错误遵循现有类型化错误；可选 dense/rerank 失败可降级，但不能吞错误后缓存成 complete。RRF 只融合 rank，不混加 BM25 与 cosine 原始数值。每通道权重/去重方法/候选数上限纳入 policy fingerprint。NaN/Inf/重复候选/无效版本拒收。固定 tie-break；结果顺序不是线程完成顺序。

## C11｜查询执行与锁

CodeIndex 短锁内取得 QueryHandle：Arc-owned DB/service/config + repository incarnation + generation。网络调用前释放 RwLock、build gate、读池连接和 SQL transaction。本地 CPU/SQLite 操作使用有界执行器；不能简单在 scoped-thread join 上等待网络，也不为引入网络而重写全部 DB API 为 async。

request deadline 为总预算；每 lane/rerank 有子预算，取消向下传播。对无法立刻中断的阻塞工作限制数量并禁止过期结果发布/缓存，不能误把 future drop 当成线程终止。QueryHandle 持有项目资源，活跃查询不因 LRU idle eviction 失效；关闭时有界等待并留下可恢复 outbox。

## C12｜generation 与缓存

Generation = persisted index_incarnation + index_epoch + evidence_epoch + semantic_epoch。incarnation 在重建换库时变化，阻止不同数据库重用相同 epoch。index/evidence 的既有意义保留；worker heartbeat、retry_at 等辅助写不应刷新所有搜索缓存。

| 缓存 | 必须含的依赖 |
|---|---|
| 文件/符号/目录派生 | 原有 seed token + parser/project-model/解析规则版本 |
| chunk/source text | doc version/source digest，而非仅序号 |
| 本地召回 | incarnation/index epoch + hard scope/query/policy |
| 图富化 | 上述 + evidence epoch/图限制 |
| dense | 上述 + semantic epoch/vector-space/query encoding spec |
| 最终上下文 | 所用所有 generation + selector/budget/rerank spec |

多连接读取无法自动构成单一 SQLite snapshot。第一版采用读前/读后 generation 验证+有限重试；需要严格快照时使用明确 snapshot/read-view 方案，不能持连接跨网络。持续写入导致重试耗尽时返回 retryable/partial，不把混代数据缓存。降级结果不进入普通完整结果缓存，可用单独短 TTL 的故障抑制缓存并保留状态。

## C13｜选择、预算与证据

排序与集合选择分开：selector 只从已验证候选中选择，不重造相关性分数。任务类型驱动配额：locate 可以集中一个文件；change/trace 可以分配实现/调用/接口/测试预算。path diversity 只是约束之一，不是目标本身。重叠以同源码版本 byte spans 计算，跨文档重复由实体和内容辅助判断。

BudgetPacker 对首项也执行限制；预算包括 metadata、分隔符、出处与 JSON envelope，不只计算正文。token 数未知时同时 enforce bytes 并标 estimate。过大节点选择更小合法 slice/outline/ref，而不是截断序列化 JSON。输出仍为可解析完整对象，含省略数量/原因/继续读取提示和实际 budget 使用。

## C14｜wire 与配置兼容

保留 14 个工具以及现有 search.mode=hybrid/symbol 的语义。新增 retrieval_strategy=local/auto/semantic 等字段需 schema、sanitize、dispatch、handler、status、文档和 stdio E2E 一次闭环。auto 在未配置语义能力时等价 local；显式 semantic 且未配置应返回明确 unavailable，不能伪装完成。

默认不发送网络请求、不创建语义缓存、不启动 LSP。配置字段采用结构化校验，feature 关闭仍能解释 config 状态而非崩溃。废弃字段给迁移诊断，不能长期维护多套相同配置名。

## C15｜性能可归因

每阶段报告 touched/parsed/resolved/written/reconciled 数量，catalog cache hit/compaction、FTS 扫描/解压、SQL calls、候选筛选、预算拒绝和向量复用统计。索引报告是用户触发的返回证据，不引入持续产品遥测数据库。benchmark 独立采集资源与原始结果，见 09-BENCHMARK.md。

## C16｜完成定义

一个能力完成必须同时具备：行为规格、实际模型/持久化/编排/MCP 接线、正负回归、旧功能回归、性能/质量对照、配置与文档、故障恢复/回滚、当前目标 SHA 的证据。写了 trait 或新增目录不算完成。任何为性能保留的近似都必须有 capability、precision/recall 及作用范围，不把“更快”兑换成未声明的错误。
