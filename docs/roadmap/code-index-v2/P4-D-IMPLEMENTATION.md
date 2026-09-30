# P4-D｜身份扰动、切块消融、资源成本与 G4 收口

状态：**P4-016～020及G4在本页声明的本地范围内完成**。累计100 done、92 todo；P4完成20/20，下一批P5-A，M2仍需P5。接受 `artifacts/benchmarks/p4d-20260929-g4/final/validation.json` 与 `paired-final/summary.json`；冻结586个覆盖文件，源码摘要 `78a88e6ad2955e488900670fe92ff74d0b02d4d83eaa481f9785cbd4e3007773`，验证前后无漂移。本轮没有提交、推送、创建PR、合并或清空开发者日常索引。

## 1. P4-016：文档身份扰动与可复用输入边界

新增版本化计划 `benchmarks/mutations/p4d-document-identity.json` 与真实 parser/SQLite/增量构建测试 `document_identity.rs`。五组操作覆盖：

- 在已有后继片段之后追加函数，量化稳定中间片段的合法输入复用；
- 文件顶部插入，验证位置进入版本与渲染输入身份；
- 复制相同函数，验证不同 occurrence 的 DocKey/encoding key 不发生别名；
- LF 与 CRLF 互换，验证原始字节、版本和编码输入变化；
- rename 后产生新的索引内身份，旧引用不再 current；撤销后精确恢复原身份。

另固定渲染文本不变、只改变 renderer/encoding spec，证明 DocKey仍对应同一源码实体，但 DocVersion 与 encoding key 必须变化，不能因 input hash相同而复用错误向量输入。所有案例都校验 stale撤销、恢复后的 `is_current`、manifest中的源码切片和encoding key→spec/input单射。开发红测还确认：单函数文件尾部追加会改变原切片，不能被错误标作稳定片段；最终夹具改为有既存后继块的真实稳定区域，没有通过放宽断言伪造复用。

## 2. P4-017：固定检索器的切块质量消融

在现有 `cc-eval::benchmark::ablation` 中增加P4专用入口。两侧使用同一当前 parser、SQLite、公开 hybrid search、预算、源码和查询，只改变 `ChunkPolicy.merge_min_bytes`：baseline=0（隔离对照），candidate=160（生产合并）。每个命中都核验当前磁盘原文、半开span、slice digest和 `current_verified`；每题分别检查路径Top1、符号Top1、required facets、reciprocal rank及源码重叠率。

固定4题结果：逐题Top1和reciprocal rank无负差分，符号断言全部通过，source evidence failure=0，duplication delta=0；`billing_exact_symbol` 的facet覆盖由0.5升至1.0。文档数25→20，模型输入字节7417→6156。块数与字节减少只在质量/源码门通过后报告，不被当作质量改善本身。该语料是确定性的开发集，不是公开holdout或一般语义质量认证。

## 3. P4-018：构建、文档存储与RSS机制成本

新增release-only `p4d_cost`。32/256文件各执行3次全建，每次再执行5次no-op和4次单文件更新：共6次全建、30次no-op、24次single-file update；分别断言 `files_parsed=N/0/1`，并记录phase timing、chunks、最大块、source/model-input/manifest字节和SQLite文件大小。

| 文件数 | 全建中位数 | no-op中位数 | 单文件更新中位数 | documents | manifest JSON | DB文件 |
|---:|---:|---:|---:|---:|---:|---:|
| 32 | 67.352 ms | 21.172 ms | 23.428 ms | 224 | 428,780 B | 2,142,208 B |
| 256 | 174.293 ms | 23.275 ms | 28.384 ms | 1,792 | 3,437,924 B | 13,029,376 B |

1000/5000条语句的大函数各3次实际解析：1000条为11.710–12.011 ms，5000条为87.823–91.894 ms；每次parse_count=1、原文字节完整分区、最大块55 B，边界序列化代理分别190,303 B和966,290 B。数据库中没有持久AST表。

`sampler::Resources` 增加与索引内存控制器同源的native current-RSS可选读数，同时保留 `ps` 进程快照；不可用状态为 `None`，不再冒充0 B。采样只发生在阶段边界，可能错过瞬时峰值，因此不能外推为peak RSS、100k规模或p95/p99结论。

## 4. P4-019：单一生产切块核心

删除旧的 `Chunker::new(line_budget)` 和静默 `max(1)/bounded` 兼容面。独立调用者统一传入完整 `ChunkPolicy` 并通过 `Chunker::from_policy` 显式校验；`ParserRegistry`保留构建期捕获的原始策略，在公开parse边界返回无效配置错误。

AST、启发式符号和纯行fallback仍是不同的**边界来源**，但都只生成 `SourceStructure`，随后进入唯一生产核心：

```text
SourceStructure -> Chunker::from_structure -> Partition -> coalesce
```

merge-disabled只存在于隔离benchmark配置，不是第二套生产chunker。架构守卫新增：旧line-only构造器必须不存在、核心路径必须存在、`cc-model/cc-db/cc-index`不得持久化 `tree_sitter::Tree`。schema保持21，ProjectModel3、file-state aggregate5不变。

## 5. G4冻结验收

最终范围固定为586个实现/配置/测试/fixture/internal docs/CI/script文件，源清单总计6,281,555 B；接受归档 `accepted-source.tar.gz`，SHA-256为 `18493266ad852d2067f02ffd421fd9c12a71ec6d1f3845fc0907878c56ed2e14`。roadmap状态与原始大工件不纳入被测源码摘要，允许验收后填写任务证据，但不得改动冻结代码。

| 检查 | stable 1.97.0 | Rust 1.95.0 |
|---|---:|---:|
| 严格Clippy，workspace/all-targets/eval-http | 通过 | 通过 |
| Workspace + doctest | 1678 passed / 51 ignored | 1678 passed / 51 ignored |
| HTTP特性 | 200 passed / 44 ignored | 200 passed / 44 ignored |
| P4-D身份/质量专项 | 2 passed | 2 passed |
| 真实MCP stdio | 21 passed | 21 passed |
| watcher专项 | 17 passed | 17 passed |

另有2个架构检查、两工具链外部oracle和4组release成本命令通过；Rust/Python可用oracle均通过，Go工具不可用的两项保持 `not_run`。19条Cargo命令每条均保存argv、环境、退出码、测试计数、日志hash及命令后的源码漂移检查。

P4-C精确保存二进制与P4-D冻结二进制在相同4套固定开发题上完成51题、两版本、每题3次共306请求。逐题Top1/nDCG负差分为0，invalid hit=0，全部回放一致；Python/Rust的15表mutation oracle在两版本均equal；14个MCP工具输入契约完全一致。S11无答案失败在前后版本原样保留，没有删除gold或把失败改名为成功。

## 6. G4范围、回滚与后续

G4只放行本地P4能力：原始bytes与span、层级/预算切块、文档身份/版本/当前manifest、实际磁盘freshness、身份扰动安全、固定检索器消融、可归因的局部资源成本及单一生产chunk核心。它不放行异步publish/incarnation/provider、vector cache、全查询/文件系统原子快照、公开holdout、100k、瞬时峰值RSS、尾延迟、长时soak、Linux/Windows/远端Actions或发行包。

回滚保持schema21隔离重建路径；P4-C精确二进制和本批完整源码归档均保留，开发者日常索引未触碰。下一批P5-A（P5-001～005）从统一 `Candidate/LaneOutcome` 开始，迁移已有lexical/grep/graph而不重写已验证算法，再增加独立exact-symbol/path lane与可回放RRF trace。P5必须继续保留P4的DocVersion/source freshness，不把S11或预算/选择问题塞回chunker解决。M2要到P5/G5完成后才成立。
