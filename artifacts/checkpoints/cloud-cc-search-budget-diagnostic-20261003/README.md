# cc-search 预算状态诊断（2026-10-03）

结论：本次未证实生产正确性 bug，不改生产算法、预算、gold、评分或历史状态。新四仓 public DEV 的质量失败保持；本 PR 只增加自编边界/缓存回归及可重放 aggregate 诊断。

固定 base：`88f2cf099c8b81f3acef485fd5ac9b01c63ce790`（PR131）；base 的 crates/Cargo 与 `e3c04fed903c4d0e3c26b5d7cf0e055a6f7c5207` 无差异。工作分支 `cloud/cc-search-budget-diagnostic`。仓库和 `/workspace/.agents` 无 AGENTS.md/SKILL.md；读了 CONTRIBUTING.md、docs/internals/SEARCH.md、docs/roadmap/code-index-v2/02-CONTRACTS.md C09–C13，以及实际 planning/lane/DAO/cache 源码。未访问被禁止的旧拒绝 source、private localdiag、runtime/GCWAL/kill/stagingtests 或 provider。

## 实际冻结 raw 的 aggregate

来源为 group-js `535ff1b13b841af8021346a660c83c57919525e2` 和 group-pygo `fff0931e5b470aa3b9111d6ce55444f9b40f98cf` 内的新 raw 压缩包。`audit_raw.py` 通过普通 git object 读取，跳过 JS pilot，只输出计数，不保存/发送 query、gold、答案或源码正文。archive SHA256 见 raw-aggregate.json；原 archive、原失败和原 aggregate 均未覆盖。

| 项目 | PyGo | JS full |
|---|---:|---:|
| 完整 raw 行数 | 888 | 783 |
| graph Partial / graph_expansion_limit | 888 | 783 |
| graph 查询 token 范围 | 7–29 | 6–24 |
| lexical candidate_limit | 78 | 195 |
| lexical candidate_limit_and_query_expansion_budget | 780 | 537 |
| lexical query_expansion_atom_budget | 30 | 39 |
| lexical Complete | 0 | 12 |
| path Partial / path_token_limit | 846 | 705 |
| path Complete | 42 | 78 |

全部实际 local、semantic disabled；全部 scope 预算 top_k=10、lexical/path/exact=24、grep=12、rerank_window=40、grep_scan_cap=20000。graph 实际输出预算 max(graph_top_k,top_k)=12，内部是5 token/每 token10种子/合计20种子/每种子每方向10邻居。

PyGo 30 个低水位 lexical Partial 的候选数为3–17；JS 39 个为0–18；原因均为查询扩展省略。候选截断行均返回24个候选，DAO读取第25项证明剩余候选存在。Partial 并非“候选恰好等于cap”直接推断。graph 所有行都有超过5个 token，足以独立触发 token省略；现有合并 reason 无法从 raw 再区分是否同时存在种子或邻居截断，不能声称它们没有发生。

JS 原 lane-audit 只读 compact `lane_receipts`，遗漏6行完整 `lanes`；本 aggregate 两者兼容，补齐这6行（graph Partial6、lexical query-budget Partial6、path Partial6）。不更改原汇总，差异明确披露。

## 自编边界与缓存验证

`crates/cc-search/src/lanes/budget_boundary_tests.rs` 是10个真正调用 SearchPlan/SQLite DAO/lane 的测试，全部自编，不来自 DEV gold。通过 cfg(test) 注册；生产编译代码不变。

- lexical 候选数2/3/4对应预算3，SQL分别读取2/3/4行，只有4项才 Partial。
- lexical 源token11/12/13、扩展atoms11/12/13；相等不误报，真实省略为Partial。第13个源词独占命中、被省略组件独占命中给出真正漏召回见证。
- graph token4/5/6、每token种子9/10/11、合计种子19/20/21、双方向邻居9/10/11、最终候选2/3/4；相等Complete，超限Partial。
- graph零命中长查询仍Partial；第六token的真实种子可命中却被预算省略，说明短结果/零结果不足以证明穷尽。
- graph_weight=0经run_lanes得到Disabled且无截断原因，未运行策略不计省略。
- 同一长空查询的图缓存确实Arc复用，fresh diagnostics给同一Partial；缩短为5词得到独立缓存Complete，未出现旧status污染。该合成验证不能证明历史raw每次都重算，但证明同一状态无需旧缓存即可产生。

测试没有把Partial转为NoMatch，也没有因“自然语言短词无用”假定它们不可能命中。

## 有限算法设计与资源边界（仅提案，不实施）

现有候选层lookahead已经正确：lexical取limit+1；种子每token取11；邻居每方向取11；集合层在保留cap前测len>cap。不要扩大常量，也不要只凭输出少于top_k改Complete。

查询层想降低保守Partial，必须有剩余工作的证明。例如保持原预算与确定性顺序，使用同一索引generation的可信posting/种子集合证明遗漏term/group无新候选或其集合已被包含；证明读取也必须计入既定预算。拿不到证明、证据过期、超过资源限额时保留Partial；空结果只有所有义务已证穷尽才可支持NoMatch。简单重复term去重也须评审BM25排序及policy指纹影响，不能当作纯状态修复。

另一路有限 exact-top-k 应明确它只证明声明的精确域/排序top-k，不代表全语义枚举；需要独立输出剩余候选/查询义务，不可把当前Complete合同悄悄重定义为“完成有界计划”。改变公共合同、诊断reason/schema、预算或排名policy都交回root，当前不实施。

现有Rust可见边界：lexical默认最多25个候选元数据；graph最多5次种子查询×11行、20种子、两方向最多20×11×2=440邻居探测行、留存最多400邻居+20种子（去重后不超过420个UID）用于映射。SQL FTS/LIKE、GROUP BY/window仍可能扫描/排序更多底层索引记录；这些是结果行/映射边界，不是总VM步骤、时间、I/O或内存认证。cap+1探测不是额外正文解压。索引generation guard仍适用，不能以分阶段读取冒充数据库快照。

## 验证与范围

Rust1.95.0 direct executables，正常平台环境，owned `/workspace/.cargo`/`.rustup`，原Cargo.lock不变。初次offline缺reqwest缓存，正常 `cargo fetch --locked` 成功后offline测试；所有尝试日志保留。

最终 `cargo test --locked --offline -p cc-search`：308 passed / 0 failed / 0 ignored；doc-tests0。`cargo clippy --locked --offline -p cc-search --all-targets -- -D warnings` 通过；涉及文件rustfmt与git diff --check通过。初始测试代码的3个编译错误及第一次doc-test因rustdoc不在PATH未执行均保留日志；最终通过显式RUSTDOC完成。未跑全workspace、真实provider、正式质量/性能矩阵，不推断其通过。没有生产改动，故不重新执行旧gold批次、不生成“变绿”结论。
