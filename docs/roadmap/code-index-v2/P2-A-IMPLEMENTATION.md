# P2-A 实施与验证报告

2026-09-28 收口；HEAD `4514630dcd26481cf6dbc2aff38824ed71ef06da`。**P2-001～005 完成，累计45项done、147项todo；下一批P2-B。整个P2/G2尚未完成。** 本批恢复了两轮中断留下的源码与证据，继续复核、修复并重新验证，没有覆盖继承工作或将历史测试冒充本次结果。未提交、推送、创建PR或调用embedding/模型。

## 1. 五项交付与实现落点

| 任务 | 交付 |
|---|---|
| P2-001 | `cc-model/src/public_surface.rs` 与 ParseOutcome：Known、KnownEmpty、Unknown，词法可见性域，声明、转发、条件、原因、版本；缺失旧字段默认Unknown |
| P2-002 | 一份规范编码与BLAKE3指纹；`cc-db/src/public_surface_store.rs` 同文件事务、外键删除、校验读取；复用已有dirty编排而不新增空壳模块 |
| P2-003 | `cc-parsers/src/exports/jsts.rs`：named/default/type、ES直接/两步/命名空间转发、局部类型依赖、const/let/var及有限静态CommonJS；动态和推断边界显式Unknown |
| P2-004 | `exports/rust.rs`：pub、pub(crate)、受限/模块可见性、签名、类型/trait/impl成员、pub use、cfg文本；宏、活动属性和const求值不冒充静态完备 |
| P2-005 | `exports/python.rs`：模块/类声明、参数/默认值/注解、静态列表/元组__all__、包具名转发和生成器形态；星号、动态与未建模实例形态显式Unknown |

提取直接消费各语言解析器已经取得的AST，不执行源码、不调用编译器/LSP，也不为接口提取重新解析全文件。Known仅指当前声明的文件局部语法子集，不表示完整语言语义、实际可访问性或运行时导出已被证明。Go等后续语言与Vue/Svelte组件接口仍按能力缺口保守表达。

## 2. 指纹与持久化合同

`KnownEmpty`有独立空接口指纹；Unknown可保存部分声明与原因，但没有可作为“未变”证据的指纹。Unknown与自身比较仍保守视为不能证明未变。接口和转发集合规范排序、去重；签名token/参数次序以及字面量内部空格保留。规范编码带格式标签、u32版本、u64长度分隔；DB和增量比较共用同一编码，空/非空均有独立手写字节golden。

PublicSurface格式版本为1、前缀`ps1:`；当前提取器分别为`jsts-declared-v3`、`rust-declared-v3`、`python-declared-v2`。数据库schema为**10**。P1的schema7及未验收P2草稿schema8/9均走已有rebuild-on-mismatch路径，防止未改源码沿用较旧提取器的错误Known状态。

`public_surfaces`随文件和符号同事务提交，删除级联；没有脱离文件写事务的独立surface写API。正常批写、增量、全量staging和实验direct writer均进入共用写助手。坏指纹、未来格式、文件身份错配会失败；无半批写入或错误epoch推进。600文件批量读取覆盖占位符分批边界，使用单读连接。DirtyResolveOnly保留内容未变的文件局部声明证据。

升级或回滚二进制应使用隔离缓存全量重建，不能混用schema10与旧缓存。本轮只在临时夹具中测试迁移，没有打开或重建用户日常项目索引；原有持久资产恢复路径没有被删除，也没有新增多进程热降级认证。

## 3. 为增量验收完成的必要接线

旧、新接口通过PublicSurface比较，缺失或Unknown不再当成“没有export变化”。依赖者由已解析import和已落库call/ref正向目标找到，再进入既有有界dirty闭包。具名/两步转发沿已有resolved imports传播，不凭语义相似性制造关系、不递归拼接上游hash。

重载时同时清理call/ref的旧target id、uid、path、resolution kind、confidence和strategy，避免清了UID却因旧ID存在跳过解析。仅保留已在同文件符号库存验证完整身份的`parser_exact`绑定。重复写入同一语义符号时保留其已有community归属；图输入变化时原后处理仍重算。没有从oracle删除社区字段、目标身份或策略以制造等价。

完整DirtyPlanner、正负依赖持久化、name-bucket、missing-path、一般重导出环固定点、耐久partial frontier与跨项目解析仍属于P2-B/C及P3。P2-010/011/013只增加已提前完成的接线说明，仍然todo。

## 4. 本次恢复后的三项新增缺口

**嵌套Rust活动属性。** impl/trait成员上的过程属性或cfg_attr原来会绕过顶层未知能力判定，错误保留Known。本轮让顶层与签名遍历共用属性判定；静态inline等对照仍Known。`red-nested-shapes`记录有效失败，修复后原断言通过。

**ES计算属性名。** 导出的对象或类使用计算键时，原实现保留语法token却错误声明无需求值。现在标记`jsts_computed_property_name_not_evaluated`，普通静态属性保持原行为。与前项合计两条有效红测试；完整边界测试9项转绿。

**定义移动留下旧定位ID。** 只加注释移动声明时，接口指纹和语义UID不变，但位置派生的symbol_id改变；增量路径的call_edges/symbol_refs保留旧ID，完整九表oracle真实失败。现在对接口未变的文件批量检查已经被跨文件引用消费的`(symbol_id, symbol_uid)`，旧地址不在新库存时触发同一有界重解析。行号不加入接口指纹，也不因此无条件重解析所有普通body编辑。Python、Rust、TypeScript移动声明回归，以及三语言普通body不提升消费者的回归均通过。

本次新增红/绿证据位于`artifacts/benchmarks/p2a-closeout-20260928/checks/`，失败原件没有删除。更早两轮P2的编译/开发/边界失败仍保留在`p2a-20260927`与`p2a-resume-20260927`，当前入口和差异映射将它们与本次源码区分。

## 5. 冻结源码的最终验证

最终源码覆盖摘要：`aeb2d29759d4d81b577c493630ef283d37874bf7be23b904b87c864a327acec5`，463个文件；36条最终验收命令退出0。原始命令、输出摘要、编译器和二进制绑定在`artifacts/benchmarks/p2a-closeout-20260928/final-v2/validation.json`。配对评测完成后再次核验源码未漂移。

| 验证 | stable1.97.0 | Rust1.95.0 |
|---|---:|---:|
| 全仓all-targets+eval-http严格Clippy，-D warnings | exit0 | exit0 |
| workspace全部测试，含doctest | 1457 passed / 0 failed / 29 ignored | 同左 |
| cc-eval+eval-http | 122 passed / 0 failed / 23 ignored | 同左 |
| workspace与eval二进制构建 | exit0 | exit0 |
| 新P2-A真实MCP接口覆盖/新目标UID | 1 passed | 同左 |
| P1-D成本/文档/并发真实MCP | 1 / 1 / 1 passed | 同左 |
| P1-C契约/解释真实MCP | 1 / 1 passed | 同左 |
| P1-B/P1-A/P0真实MCP | 2 / 2 / 1 passed | 同左 |
| watcher专项、核心并发额外复跑 | 10 passed；核心2次通过 | 同左 |

不同命令包含重叠测试，不相加；ignored不算通过。新P2-A stdio覆盖Python真实index/search/graph_query的UID更新；Rust/TS/声明移动等其余案例为实际解析器+SQLite+构建链路测试，不偷换成全部语言MCP黑盒证明。本机macOS arm64验证，SDK15.4只对子进程设置，未修改全局SDK或默认工具链。未运行Linux/Windows/远端Actions。

## 6. 增量oracle的前后结果

| 原始变更场景 | P1-D | P2-A |
|---|---|---|
| Python仅改provider签名 | equal=false，exit1 | equal=true，exit0 |
| Rust仅改provider签名 | equal=false，exit1 | equal=true，exit0 |

沿用原始题库和mutation，既有目标UID、物理ID、策略等字段保留。旧P1-D使用当时8表oracle；新版本增加public_surfaces、检查9表，报告明确列出版本范围。除两侧一致性外，测试独立断言消费者必须指向新provider、目标UID必须仍存在，不能让两侧共同丢边而通过。

原B03两个最小场景关闭并成为阻断回归；这不等于所有跨语言增量错误已修复。额外回归覆盖TS签名和局部类型、具名/type-only转发、Python包与星号Unknown、Rust workspace pub use、body/no-op、声明移动、direct writer、删除、重开和不支持语言的构建覆盖状态。

## 7. 固定检索benchmark

同一个新runner、相同冻结源码/题库/答案/配置，只替换P1-D与最终P2-A产品二进制。四套题库共8组、306次查询；逐题Top-1/nDCG delta全为0。metrics、query-slices和costs.jsonl八组均离线回放且摘要一致；14工具旧新输入schema/未知字段处理合同收据一致。

| 数据 | 每版独立题/请求 | P1-D Top1 / nDCG@10 | P2-A Top1 / nDCG@10 |
|---|---:|---:|---:|
| 原七文件源码集 | 14 / 42 | .8571428571 / .9379235538 | 同左 |
| 原三文件smoke | 11 / 33 | .7 / .7 | 同左 |
| 定位/范围回归集 | 8 / 24 | 1 / 1 | 同左 |
| 六文件双语意图集 | 18 / 54 | .625 / .65625 | 同左 |

smoke均值只计算10道正答案题；S11仍每版3次no-answer失败，raw gate继续exit1，未删题或换答案。双语集四道无标识符中文题的既有限制仍在。正式比较均为inconclusive，不作显著性、尾延迟、全仓或语义召回提升声明。当前源码CI清单digest的刷新独立于历史frozen-inputs，不用新题库替代配对输入。

## 8. 交接与限制

P2-A-GATE为`passed_local_batch`，不是G2或M1发布完成。权威任务状态只在tasks.json更新、05-TODO由脚本生成。下一批P2-B（P2-006～010）：Go package公共接口、其他语言保守能力表、ResolutionOutcome、warm/cold消歧，以及正式正/负解析依赖。

代码仍保留Rust核心、七crate、默认离线和14个MCP工具；没有embedding、ANN、LSP或新服务。没有真实OCE/模型、付费代码外传、holdout、多仓600题、100k或新增release性能认证。结果状态里的Unknown/partial不被重命名成全量成功。

远端Goal checkpoint被工具层拦截，未绕过或宣称远端Goal同步完成；代码、本地任务状态和证据独立验收。本地未提交、推送或创建PR。源码与证据包不是部署二进制，回滚和后续施工应保留所有既有P0/P1/P2记录。
