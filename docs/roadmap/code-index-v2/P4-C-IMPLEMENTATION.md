# P4-C｜文档身份、当前清单与源码读取一致性

状态：**P4-011～015按本地声明范围完成**；95/192 done，97 todo；P4 15/20。仅接受final-v2与paired-final-v2。584覆盖文件冻结、31条Cargo命令、双工具链各workspace1676passed/50ignored、HTTP198passed/43ignored、真实MCP21passed、watcher17passed；固定51题306请求逐题无负差分/源证据无错。G4未完成。

## 范围与实现

P4-011～015复用P4-B原文字节、ChunkPolicy和纯render API。新增cc-model/identity.rs的索引内DocKey/DocVersion、快照限定EntityKey及按实际模型输入/encoding spec计算的复用key。旧chunk ID继续用于wire/FTS定位，不作为版本发布权威。路径改名产生新key；相同切片在部分位置变化下可保持key，但新快照/span/version改变。重复内容保留不同来源及出现次数，不承诺语义重命名跟踪或跨仓库全局ID。

cc-index/documents/delta.rs在现有prepare解析任务中构建DocumentBatch，包含upsert/remove/unchanged与复用候选。结果随FileWriteUnit进入既有staging和scalar/table-major事务写入；没有第二套解析器、索引引擎、异步队列或provider。新增当前document_manifest及files.document_spec，并将spec纳入file-state聚合/缓存，独立于ChunkPolicy。规范改变可触发未改源码重处理；失败文件不会被错误确认。DocChanges仅统计此次投影文件，不是全仓总数或整文件删除的审计日志。

原始FTS正文与rowid对齐不变；文档表通过chunk外键与显式删除trigger清理。新的manifest完整性/版本/原文验证贯穿标量和批量写入、全量staging与查询hydration。损坏或缺失的必需manifest报错，不能降级成无版本结果。原文有效但模型输入超预算时，保留源码块并保存render_error/no encoding key，不伪造模型输入。

## 已复现的产品问题与修复

初始red-documents-source保留三项失败：未建立当前manifest、旧符号行号切到新磁盘的无关文本、删除文件仍由热搜索缓存返回。修复后公开hybrid搜索在每次调用（含cache hit）核验磁盘和当前索引版本，省略变更/删除/受限证据并标partial；底层SearchEngine仍明确返回indexed_snapshot。符号源码、explore、代码区域与trace/flow片段使用共享读取核验；显示标准化文本另有标注。路径大小写/分隔符/内部symlink既有兼容继续由共享guard测试，不复制保留旧算法。

开发workspace发现context_flask_routes和一个旧排序夹具失败。前者由新增元数据推挤旧JSON前缀预览导致，补上有界源码渲染、文件级引用/原因及诊断预览；继续标_truncated，未抬高输出上限、改排序或修改gold。预览过程中发现graph_enrichment及小预算重复诊断问题，分别保留失败并修复。排序夹具此前无磁盘原件、哈希为hash-<path>，现补独立原件/准确坐标/摘要；原有可手算分数及排序断言均保留。

第一次冻结final在真实MCP并发旧回归失败：磁盘写入尚未被索引时，P4-C按设计省略不一致源码，而P1-D旧测试无条件要求非空。没有改产品恢复旧的不安全行为；测试将非空/最新正文要求保留并扩大到全部8个静止后的scope，并增加确定性磁盘先变更→明确stale/partial负例。并发期非空必须有current_verified和doc_version，空结果必须有同scope的明确变化/版本原因且不能因预算耗尽蒙混，范围断言不变。第一次final原始失败留存；之后重新冻结完整验收，不以该失败的局部成功代替最终结果。

## 验收方法

p4c_identity与p4c_documents使用手写正负例：位置变化、不同来源/重复出现、不同encoding spec、损坏输入/manifest、空文件、render超预算、rename/delete/no-op/reopen、spec失效、写入回滚、direct/standard重建、温缓存磁盘变化与路径限制。真实MCP覆盖index/search版本、磁盘变更抑制、重建更新、重启及删除清单。差分oracle在本版增加document_manifest，比较15表及完整新字段；旧P4-B二进制仍按自身14表验证，不删列换成功。

固定4套51题/306请求的开发集配对保留原答案、原失败与每题差分；S11及样本不足的性能结论不得掩盖。新的public-v4只增加source partial解释，原字节proof及legacy严格核验不放宽；旧回放须使用对应适配器二进制。

p4c_cost以实际release索引/无变化构建/公开搜索观测20/200文件，每规模5次（10条机制样本）；首次全量只有单次记录，不能宣称RSS、100k、p95或生产提速。持久化完整model input及每次磁盘核验有额外成本，纯索引缓存命中不等于零IO。

## 兼容与后续

schema21、ProjectModel3、file-state aggregate5；旧20及更早隔离重建。14个工具输入不新增破坏项，输出增加可选document与source_freshness。身份默认以所属IndexDb为namespace，不能脱离索引句柄用裸key授权。API is_current只检查当前索引，不是原子异步worker发布；incarnation/provider/vector任务仍P6/P7。读取后文件仍可能变化，不承诺OS/全查询原子快照。文档历史日志、公开holdout、峰值内存、100k、跨平台和发行未认证。

P4-015原计划列symbol_extract.rs，但该文件只是用户任务名称抽取；实际读取位于engine_query、handlers/context、graph_trace/flow及cc-search/evidence。本轮修正任务scope以反映真实入口，不重造无关名称抽取逻辑。下一批P4-D仍须完成全阶段切块/文档质量和成本验收。

证据根：artifacts/benchmarks/p4c-20260928-pHaf/。本轮未提交/push/PR/合并，未清空开发者日常索引。

## 最终证据

584覆盖文件冻结、31条Cargo命令、双工具链各workspace1676passed/50ignored、HTTP198passed/43ignored、真实MCP21passed、watcher17passed；固定51题306请求逐题无负差分/源证据无错。

{
  "stable": {
    "workspace": {
      "passed": 1676,
      "failed": 0,
      "ignored": 50
    },
    "http": {
      "passed": 198,
      "failed": 0,
      "ignored": 43
    },
    "real-mcp": {
      "passed": 21,
      "failed": 0,
      "ignored": 0
    },
    "watcher": {
      "passed": 17,
      "failed": 0,
      "ignored": 0
    }
  },
  "1.95.0": {
    "workspace": {
      "passed": 1676,
      "failed": 0,
      "ignored": 50
    },
    "http": {
      "passed": 198,
      "failed": 0,
      "ignored": 43
    },
    "real-mcp": {
      "passed": 21,
      "failed": 0,
      "ignored": 0
    },
    "watcher": {
      "passed": 17,
      "failed": 0,
      "ignored": 0
    }
  }
}

全部原始记录见artifacts/benchmarks/p4c-20260928-pHaf；scope-check核验源码、日志、旧输入、二进制、15表oracle及完整源码归档，不以旧HEAD或开发期零散测试代替最终证据。
