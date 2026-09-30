# P4-A｜原始源码快照与层级切块

状态：P4-A（P4-001～005）已通过final-v2本地声明范围验收，85 done/107 todo，P4完成5/20；下一批P4-B，G4尚未完成。

基线 main@4514630dcd26481cf6dbc2aff38824ed71ef06da。入口核实P3-D final-v3全部覆盖源码无漂移，既有未提交源码和文档归档于本批 entry.json、entry-source.tar.gz。运行目录 artifacts/benchmarks/p4a-20260928-VyxH/。无提交、推送、PR、合并或日常索引清理。

## 任务实际落点

| 任务 | 实现 | 验证范围 |
|---|---|---|
| P4-001 | cc-model/source.rs；chunk.source；chunks.source_json；解析前scan hash检查 | 原始UTF-8/opaque身份、CRLF/BOM/末尾换行、半开区间、字节列、scalar/batch plain/zstd写入、重启与十四表对照 |
| P4-002 | ParseOutcome.source_structure；chunker/boundaries.rs；六个tree-backed生产解析器 | 复用既有Tree游标遍历、拥有型坐标、无Node/Tree逃逸、有界部分结果；语言parse计数测试不冒充整个索引只有一次AST |
| P4-003 | chunker/split.rs与merge.rs；小类/大类独立用例 | 小叶符号整体、容器成员可检索、父域breadcrumb；父标题受限真实成员元数据不写入源码正文、不复制父函数体 |
| P4-004 | 语句/块边界递归及精确有界fallback | 长函数控制语句不被随意行窗切断、UTF-8/CRLF分界、零行预算进度；16KiB兜底，组合配置预算留给P4-B |
| P4-005 | 邻接文档注释、装饰器/Python literal docstring、箭头函数签名 | 不跨空行绑独立配置注释，长函数文档与签名前缀在预算内保持一起，签名坐标不包括箭头函数实现 |

## 实际失败与修复

`red-source`复现三个旧缺陷：换行重构丢失CRLF及末尾换行、类方法文档分离、SFC只输出拼接脚本而不是组件原文。新正文统一来自原始snapshot切片；SFC改为保留字节位置的单次script mask解析，最终切块再次使用原组件，模板结构仍未建模。

`dev-source-3`暴露导出包装节点遮住真实定义身份，修复为声明拥有export/decorator跨度；没有删除独立注释负例。`red-doc-signature`再复现Python长函数docstring分离和箭头签名包含整段实现，两者都在实际AST边界路径修复。

`dev-workspace-1`与`dev-golden-2`暴露方法/类检索退化：容器整体切块使log/withdraw失去独立标签，拆出成员后父标题又失去直接成员词。现在小叶节点仍完整、容器成员独立，父标题breadcrumb包含最多16个真实直接成员且整体限1024UTF-8字节。未修改94个既有语料案例或gold，开发复核恢复94/94；最终仍须逐题固定语料比较，不能用均值抵消退化。

P4-A额外添加扫描摘要与解析输入核验，针对同长度变化测试拒绝新正文/旧hash组合；保留的原始文本本身仍是有效旧快照，不宣称磁盘已最新或自动调度重试。

## 证据层级与边界

原始源码gold由手写UTF-8/CRLF字节与具体方法/语句提供；数据库对照保留新source_json，不从oracle删除新增字段。压缩测试显式要求实际zstd记录，重载与重建继续逐切片核验。真实MCP案例由产品子进程执行index/graph_query/search，实际搜索metadata.source_evidence已携带索引快照的字节坐标；benchmark v3从公开响应独立验证完整原件摘要、切片、行号及slice digest。另有只读SQLite字节核验，两个层级分开。不是执行查询时自动证明磁盘已最新。

SourceStructure是暂存的拥有型坐标，不是持久化整文件AST；每chunk来源证据独立落库。旧chunk:path:index wire身份保留，但不是DocKey/Version；最小索引快照字节证据投影已因本批原文正确性要求提前贯通；完整freshness-aware hydrate、磁盘陈旧性和异步文档身份仍属于P4-C，source/embed展示分离的后续预算规则属于P4-B。常规小碎片合并/重复区间优化与组合预算属于P4-B。当前project chunk_line_budget字段尚未贯穿Registry，生产仍用默认80；直接Chunker API可配置。这一既有缺口明确记录而非声称已接线。

schema19要求从18及更早隔离重建，ProjectModel布局仍3。默认离线、无新provider、无新MCP工具输入。G4、公开holdout、100k/RSS/尾延迟、完整语言语义、跨平台和发行认证不由本批通过代替。

## 首次冻结配对的硬失败与修复

首次final的29条功能命令通过，但paired-final大量invalid evidence：产品返回新原始字节切片，public hydration却丢弃数据库中的坐标，旧normalizer按整行归一化核验。该次不是本批完成证据。现在DB到公开search metadata的最小证明投影贯通，adapter明确升级为public-v3，非法证明不能退回legacy分支；旧产品仅在缺少新证据时继续原来的严格行核验，gold和评分公式未变。

字节证明修复后，R14仍存在真正的排序退化，R12提升恰好抵消Top1均值。原因是连续文件级说明被拆为多个单行片段。red-file-documentation独立负例失败后，修复为同父域连续注释的预算内分组，不跨空行/定义；开发子集复测14题Top1和nDCG均1，只有R12相比P3-D提升，其他三套在字节证明修复后逐题一致。最终必须在final-v2冻结与paired-final-v2重新确认，不将开发值当验收。

## 最终验收

只接受 `final-v2` 与 `paired-final-v2`，首个final功能通过但公开源码证据门禁失败，原记录保留、不计最终验收。

565文件冻结、29条Cargo命令，两工具链各workspace1647passed/46ignored、HTTP180passed/39ignored、显式真实MCP19passed、watcher17passed；固定51题306请求无逐题负差分且源码证据全部有效。冻结摘要 `12d564788bcb59f6e841504d5720172ae3d0be7b2e2aef0ec16d94e00e2999bd`，集合、内容、命令日志、二进制、题库和历史输入均已复核。

| 检查 | stable1.97.0 | Rust1.95.0 |
|---|---|---|
| workspace | 1647 passed / 46 ignored | 1647 passed / 46 ignored |
| http | 180 passed / 39 ignored | 180 passed / 39 ignored |
| real-mcp | 19 passed / 0 ignored | 19 passed / 0 ignored |
| watcher | 17 passed / 0 ignored | 17 passed / 0 ignored |

命令覆盖有重叠，不相加，ignored不当通过。真实MCP从公开search收到source_evidence并独立核验；SQLite原文检查为另一个证据层级。

源码子集Top1 0.928571 → 1.000000，nDCG@10 0.973638 → 1.000000。逐题变化：`[{"dataset": "frozen-source", "case_id": "R12", "top1_delta": 1.0, "ndcg10_delta": 0.36907024642854247}]`。S11失败仍保留；3份正式性能compare仍inconclusive。开发题参与修复，不是独立holdout。14工具输入合同、原两语言十四表签名对照、独立CLI和回放均通过。

15条release观测对应10/100/500方法的3种合成规模，每种重复5次。

| 方法数 | 源码字节 | 切块数 | parse+chunk中位ms | 边界JSON字节 | chunk来源JSON字节 |
|---:|---:|---:|---:|---:|---:|
| 10 | 4361 | [13] | 1.929 | [24687] | [5411] |
| 100 | 43511 | [103] | 11.730 | [245901] | [43603] |
| 500 | 218311 | [503] | 48.601 | [1243905] | [215733] |

这是解析/边界/切块总时间及元数据开销观察，不是节省token、整体加速、RSS或p95认证。相同四个Rust/Python模块外部案例在两工具链运行通过；不是切块编译器oracle，Go/TS缺项保持not_run。

下一批P4-B（006～010）处理小碎片合并、gap/长行、总预算与配置接线、source/embed投影；不把已经有的字节上限和坐标基础重复实现。P4-A本地声明范围，G4、DocKey/Version、完整freshness-aware hydrate、项目chunk_line_budget接线、通用小块合并、公开holdout、100k/RSS/尾延迟、跨平台及发行认证未完成；S11仍失败，性能compare inconclusive；外部Go/TS编译器未运行。
