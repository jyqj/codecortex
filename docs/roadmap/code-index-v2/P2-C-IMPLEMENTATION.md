# P2-C 实施与验证报告

2026-09-28；基线 `main@4514630dcd26481cf6dbc2aff38824ed71ef06da`。**P2-011～015 五项本地验收完成；累计55 done、137 todo，P2完成15/20。下一批P2-D，P2/G2和M1尚未完成。** 本轮同时修复上轮审计发现的Python伪调用，以及目标字段审查发现的dispatch handler残留。未提交、推送、创建PR、合并、调用模型或改动开发者日常索引。

## 1. 基线与变更归属

开始时P2-B已验收的477个覆盖文件均未漂移，另有尚不能编译的P2-C测试。先保存522文件的工作区基线和源码归档，保留既有未提交成果。当前实现复用七crate、SQLite、现有dirty closure及类型化写入口，没有另造索引/依赖引擎，没有引入embedding或新MCP工具。

本批源码/测试/运行文档/CI及验证脚本共12新增、38修改，映射见 `artifacts/benchmarks/p2c-20260928-OttX/change-map.json`。任务状态文档另行收口。历史HEAD并不包含这批未提交源码；不能只检出HEAD就声称复现了它。

## 2. 五项交付

| 任务 | 实际完成范围 |
|---|---|
| P2-011 | body、public_surface、bound_address、configuration、inventory、candidate_set、resumed/rebased原因；实际解析/依赖处理数量与预算；无实际候选消费者的私有名字变化不提升全部导入者 |
| P2-012 | 有限文件贡献集合的链/环传播；传播根与当前basis已解析集合分离；预算前缀保存后续转发贡献；TS20层含环、Python/Rust链环与实际目标检查 |
| P2-013 | call/ref/route/semantic/dispatch/import/symbol字段矩阵；清理真实持久化的dispatch handler UID；保留本地源码身份、提取置信度与Unsupported词法证据；十四表差分 |
| P2-014 | 同事务resolution_frontier、epoch围栏、basis重建、原始失效原因保留；无变化、重启、空事件、watcher自动续跑；删除根、disabled再启用、跨lookup窗口与rollback/损坏负例 |
| P2-015 | index/status及公开MCP对象型查询的resolution_freshness；混代查询明确changed_during_query；旧数组型关系欠账时报错；截断外保留状态；输入schema不变 |

具体合同及字段矩阵见 `docs/internals/INCREMENTAL_RECOVERY.md`。规划中的incremental/change_kind.rs、planner.rs、reconcile.rs按实际职责映射到现有indexer_phases和cc-model，没有为了匹配规划文件树制造冗余目录。

## 3. 剩余工作如何不再丢失

持久化的是原始失效根、事件和当前basis下的完成集合，不是某次limit+1查询得到的截断列表。发生新相关变化时保留旧原因、清空旧完成证明并重新定基；此前的provider根自身也可以成为新provider的消费者。删除provider不会通过外键把其未完成消费者的解释一并删除。

文件事实、关系、frontier替换/确认及index epoch在同一IMMEDIATE事务提交。旧prepare epoch、不合法payload或后续写入失败不会确认剩余任务。上限是200,000个组合状态条目、16MiB payload；超限明确失败并要求恢复，不以丢失事件换取成功。

独立MCP实测：3消费者、每次预算1，首次更新后绑定1个且incomplete；进程重启后再增量绑定2个且incomplete；第三次绑定3个后ready。无新文件事件的watcher也能通过原有build gate消费两轮余量。纯body变化不额外重解析导入者的原回归保持通过。

`resolution_freshness.completed_files` 是当前basis已处理的文件集合计数，包含本次解析根和已处理/删除文件，不是“已完成消费者数”或全仓完成率。`ready` 仅表示observed_resolution_invalidations范围无持久欠账，不证明未观察到的磁盘变化、语言编译器语义或整个查询的MVCC快照。

## 4. 真实缺陷、开发失败与修复分开

保留三组产品红测试：恢复2项失败、Python调用真值4项失败、dispatch残留1项失败。现均有阻断正向回归。原新增P2-C测试的Vec.as_array编译错误首先修正，再证明其实际恢复断言确实为红。

Python由旧正则遍历改为已有AST上的真实调用/引用节点。声明、注释、字符串正文不产生调用；真实递归、跨行调用、f-string表达式、唯一嵌套所属函数、默认表达式和动态调用链位置均有独立断言。参数/赋值遮蔽、装饰绑定及不支持的动态调用保持Unsupported，不被全局名字或type fallback改写成parser_exact。公开MCP中原伪调用文件的CALLS查询为空。

另保留新代码集成失败：SQLite参数类型、私有写接口测试接线、epoch审计入口和初版候选事件过度提升。尤其原body-only测试没有被削弱；通过限制无实际消费者的候选事件修正过度失效。分类见 `evidence-classification.json`，不把开发编译失败包装为旧产品收益。

## 5. 冻结源码最终验收

覆盖源码摘要：`c392dc347bcd0a11cb3d7addb6eaf81c5656ec4c513aa9f446038530f2df8a8a`。**504个覆盖文件、22条最终命令全部exit0**。核验文件新增/删除集合及内容，而非只检查旧manifest中的成员；命令日志哈希均匹配。代码集合覆盖crates、scripts、CI、运行文档、root Markdown/Cargo与本批四个验证脚本；roadmap状态和历史结果不计入代码摘要。根.gitignore另外核验未改动。

| 检查 | stable 1.97.0 | Rust 1.95.0 |
|---|---|---|
| 全仓all-targets+eval-http严格Clippy，-D warnings | exit0 | exit0 |
| Workspace测试，含doctest | 1516 passed / 0 failed / 31 ignored | 相同 |
| cc-eval + eval-http | 146 passed / 0 failed / 25 ignored | 相同 |
| Workspace及eval二进制构建 | exit0 | exit0 |
| P0/P1/P2-A/P2-B/P2-C真实MCP | 13 passed | 13 passed |
| Watcher专项 | 11 passed | 11 passed |
| 查询generation变化专项 | 1 passed | 1 passed |
| 原核心并发额外重复 | 2次通过 | 2次通过 |

测试命令有重叠，不能相加。ignored不算成功；真实stdio由显式命令执行。仅macOS arm64本地；SDK15.4参数仅作用于子进程，没有改变全局工具链。CI配置增加P2-C步骤，不等于远端Actions运行通过。

P2-A/B/C的34项增量回归在十四表oracle下通过；新增的resolution_frontier、semantic_edges、dispatch_sites没有通过删除ID/UID/策略/置信度规避差异。旧P2-B runner保持十一表口径。Python/Rust原signature mutation两版均equal=true/exit0。独立真实目标/无调用断言仍必需，因为full与incremental可以共同出错。

## 6. 固定输入配对检索

同一当前evaluator、同一源码/问题/答案/配置，只替换P2-B与P2-C精确产品binary：四套51道独立题，各重复3次，两版合计8组306次MCP请求。四套逐题Top1/nDCG delta全部为0；metrics、query-slices、costs八组均可离线回放且哈希一致。新旧14个MCP工具输入合同一致。

| 题库 | 独立题数 / 每版请求 | 两版Top1 | 两版nDCG@10 |
|---|---:|---:|---:|
| 冻结源码 | 14 / 42 | 0.8571428571 | 0.9379235538 |
| 冻结smoke | 11 / 33 | 0.7 | 0.7 |
| 精确定位/范围 | 8 / 24 | 1.0 | 1.0 |
| 双语意图 | 18 / 54 | 0.625 | 0.65625 |

smoke质量均值只包含10道正答案题；S11两版各3次仍失败，原gate保留exit1。三份正式compare均inconclusive，不据此宣称速度提升。四道无标识符中文语义问题仍未解决。当前源码manifest只迁移source摘要，问题/答案及历史frozen-inputs哈希未变。

## 7. 交接与剩余边界

下一批P2-D（P2-016～020）：扩大多语言mutation及独立真值/缩减；测name bucket、tombstone、依赖行和frontier完成集合增长的成本，制定压实/清理策略；清理旧JS假设与过时文字；完成全阶段G2，而不是重复建设已存在的frontier/解析依赖引擎。

Java classpath、C/C++预处理器、Go build条件、Cargo依赖重命名及完整ProjectModel仍归P3。没有embedding/live OCE、LSP、付费provider、holdout、多仓600题、100k、release尾延迟/峰值内存或跨平台认证。

schema13不兼容schema12；升级/回滚使用隔离缓存全量重建及既有持久资产恢复流程，原P2-B binary和入口源码归档保留。不要为验证清空日常索引。任务只在最终证据齐全后改为done；派生TODO、README、PLAN-CHECK与交接同步为55/137，不复制历史绿灯冒充新源码证明。

收口补充：WebCodex通用流水账仍保留历史红测试/集成失败，且对自定义多命令脚本显示attempt_boundary_unavailable；不能把工具卡片的混合历史等同于本次冻结源码结果。验收依据是逐条argv/退出码/日志哈希和前后源码集合一致的final/validation.json，以及paired/summary.json。只读hygiene的23个未跟踪测试路径均为应保留的回归交付物，不按临时文件删除；无冲突或secret-like路径。独立收口核验见artifacts/benchmarks/p2c-20260928-OttX/closeout.json。
