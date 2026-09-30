# 解析结果、包贡献与增量依赖（P2-B）

## 结果不是一个可空目标

`cc-model::resolution` 的版本 1 合同区分 `Resolved {target,strategy,confidence}`、`Ambiguous {candidates,reason,candidate_count_lower_bound,truncated}`、`Unresolved {reason}` 和 `Unsupported {capability}`。持久化目标是规范文件路径、物理 ID、可选 UID、qname 与 kind，不保存目录槽位。图边只获得已选定的唯一目标；语义证据相同的候选保持歧义，稳定路径排序只决定展示顺序，不证明第一名正确。

声明解析仍是静态分析，不是完整编译器。`Resolved` 的 strategy/confidence 区分 scope/import 与 global/fuzzy 启发式。没有匹配、候选预算不足、未支持能力不能混成一个正常空数组。名称桶超出 resolver max-pool 时不扫描整个桶，返回 truncated 歧义；没有枚举就不声称精确候选总数。

同名候选按语义证据选择；同分保持 Ambiguous。类型/别名多文件贡献不再最后写入者获胜；冲突不能支撑一个已知层级。重复 import 别名既不能选第一次，也不能因其中一个来源缺失而把另一个候选包装为唯一。

## 包视图与缓存

Go `PackageKey` 区分目录、包名、生产/测试文件。每个包视图只构建一份名称索引，再由消费文件共享 Arc，避免逐文件复制整包路径和逐调用扫描全部成员。接收者方法不能作为裸包函数解析；不同包名和外部测试包不混合。内部测试可以使用生产声明，反方向不成立。build tags/平台条件没有被实际求值，公共接口证据保持 Unknown；这不是 Go compiler oracle。

跨构建目录仍由已持久化符号 seed token 验证，保留按文件增删、tombstone 和重建阈值。解析 LRU 在目录/包视图更新后失效。新增声明、删除声明、目标地址移动和签名/类型变化均可影响候选桶；普通函数体不直接进入桶投影。规范化结果不能随冷加载、插入顺序或已有缓存历史改变。

## 同文件事务

`resolution_manifests(file_path,version,payload,digest)` 保存按 site-kind/site-id 排序的结果；`resolution_dependencies(file_path,kind,key)` 是去重反向查询索引。两表外键关联 files、删除级联，与解析后的文件、符号和边在同一事务更新。dirty-only 重解析替换结果/依赖，但保留未经重新解析的 PublicSurface；全量 staging、常规写、实验 direct writer 使用相同插入助手。坏版本、身份/指纹损坏、冲突 site、重复候选、非法置信度失败，不推进半批事务的 epoch。

结果上限每文件 4096 条、依赖正常上限 4096 条、每项歧义最多 32 候选。结果和依赖的序列化内容共用 4 MiB 生成预算，规范化/反序列化后继续追加不会重置额度。超预算明确 incomplete/omitted，并保留最多三个全局保守依赖哨兵；持久化另有 6 MiB 总 JSON 安全上限。回填最终结果时不能把已撤销的旧 Resolved 重新放回来；原先不完整的状态不能因第二次投影变成 complete。

`IndexReport.resolution_coverage` 统计本次处理的文件（包括 dirty-only）的 resolved/ambiguous/unresolved/unsupported/incomplete/omitted；不是全仓覆盖率。它与仅统计实际 parse 文件的 public_surface_coverage 分开。完整记录通过 ReadOps 的类型化读取取得；没有增加 MCP 工具或修改原输入 schema。

## 依赖与失效

| 依赖类型 | 消费含义 | 典型变化 |
|---|---|---|
| target_surface | 已绑定或歧义候选的目标文件 | 接口变化、已消费地址移动、目标删除 |
| name_bucket | 实际查询的名字及限定名末段 | 新增同名、补上缺失名、候选类型/签名变化 |
| missing_path | 同一 import 解析原语产生的所有候选路径探测 | 文件新增/删除、补上优先级更高的入口 |
| module_config | 当前模块规则/条件的配置依赖 | 已索引 Cargo/Go/JS/Python 配置路径改变 |
| package_files | 生产/测试包贡献键 | 同包文件增删或接口变化 |
| symbol_inventory / file_inventory | 无充分精确证据或预算溢出的保守哨兵 | 相关全局候选或文件集合变化 |

`missing_path` 覆盖所有探测候选，不仅最终选中的路径；否则原来的 .js 结果不会因为新建更优先的 .ts 被重判。路径候选与实际 import resolver 共用一个函数，不复制第二套优先级。该函数不读取磁盘；实际选择验证普通文件及项目内归属。它仍只有既有基础 JS/Python 模块规则，完整条件解析留给 P3。

反向依赖查询按 kind/key 索引分批，排除已重解析/删除文件，再为既有 dirty closure 提供限额加一的溢出见证；不另造一个无界全仓 planner。新增同名会撤销旧 global-unique，删除竞争者可恢复唯一；不存在名称与路径的旧查询也会被刷新。事件范围 prepare/commit 与常规增量使用同一机制。

超出 dirty 文件预算必须返回 budget_exceeded，不能因部分文件已刷新就宣称闭包完整。P2-C 已增加同事务 frontier/basis、无变化/重启/watcher 续跑及有限集合重导出传播，详见 [INCREMENTAL_RECOVERY.md](INCREMENTAL_RECOVERY.md)；显式全量重建仍可恢复。一般语言模块语义与规模成本认证仍未完成。配置路径必须进入扫描事件才能被消费；该模块不是配置监视器或 ProjectModel。

## 源码位置与重复检测

P2-B 严格结果校验暴露了 Rust 链式调用共用接收者起点的旧 ID 碰撞。现在调用/引用以实际 callee token 锚定，保留链中不同调用；不得用 SQL 的最后一行覆盖掩盖冲突。Express 正则富化仅补 AST 尚未占用的注册位置，不把命名函数替换成 function 关键字；同一行多处注册保留真实列。全量/增量 route node 写入也复用同一个非空主键助手。

当前数据库 schema 21 与 P4-B 的 20、P4-A 的 19、P3-D 的 18、P3-C 的 17、P3-B 的 16、P3-A 的 15、P2-D 的 14、P2-C 的 13、P2-B 的 12、P2-A 的 10、未验收的 11 不兼容。升级/回滚使用隔离缓存全量重建，保留原始观测，不打开开发者日常索引来测试迁移。

P2-D 的反向窗口与工作量边界、独立真值、连续变更和失败缩减见 [INCREMENTAL_VERIFICATION.md](INCREMENTAL_VERIFICATION.md)。窗口不是剩余集合的完整枚举；不因返回 limit+1 路径就丢弃原始根因。

P3-A 的 TS 模块决策/路径探测/配置依赖进入现有 ResolutionManifest；配置发现只在 prepare 运行，提交前复核。配置-only 变化不要求生成源码 chunks。支持范围和成本见 [PROJECT_MODEL.md](PROJECT_MODEL.md)，不能将 local_compat 当作编译器模式。

P3-B导入语法、模块选择和真实重导出路径也属于同一证据链；显式导入缺失成员不得退回全局同名猜测。支持范围与有界工作量见 [MODULE_RESOLUTION.md](MODULE_RESOLUTION.md)。

## 验证范围

P2-B 的历史证据保持十一表口径；P2-C 当前 oracle 追加 frontier、semantic_edges、dispatch_sites，共十四表，不删目标身份/策略字段。

模型金样与坏数据、SQLite 事务/损坏/删除/600 文件批读、插入顺序与缓存历史、实际解析/十一表全量增量对照、Go 多文件/测试隔离、Cargo 已支持 package 名配置变化、事件范围调用、超限状态及真实 MCP 各自留证。测试源码和人工目标断言与检索答案分离，不通过删除 UID/策略/歧义表获得等价。具体已运行的工具链、命令、结果及未运行项见当批实施报告；此文不自行认证性能、跨平台、完整编译器或 G2。
