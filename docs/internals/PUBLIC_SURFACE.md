# 公共接口表面（P2-A / P2-B）

## 定义与边界

`cc-model::public_surface::PublicSurface` 是文件局部的声明接口证据，服务于“何时必须重做依赖方解析”，不是语言编译器、运行时执行结果或跨项目可见性裁决器。它不等同于 JS `export_name`，也不把每个函数体内的局部变量加入接口。

`Known` 表示该文件在当前提取器所声明的静态语法子集内有接口条目；`KnownEmpty` 表示该子集内确实没有条目；`Unknown` 表示语法错误、能力缺失、动态构造或预算不足，可能同时保留部分已识别条目。Unknown 没有可用于“未变化”判定的指纹。同一个 Unknown 与自身比较也不能证明接口未变。旧 `ParseOutcome` 缺少该字段时反序列化为 Unknown。

可见性分为 Exported、Module、Crate、Restricted(raw)、Global，记录词法声明而不是是否能从外部 crate/包实际访问。私有顶层声明仍可作为项目内部引用的依赖；普通函数体编辑不因此无条件提升导入者。

## 编码、版本与存储

P2-D 删除旧 export_name SQL/cc-index 双指纹公式；兼容读取方法 get_export_fingerprint(s) 也使用以下单一编码。KnownEmpty 可比较，Unknown/缺失仍不可比较。

格式版本为 `PUBLIC_SURFACE_VERSION=1`，指纹前缀 `ps1:`。提取器行为版本为 `jsts-declared-v3`、`rust-declared-v3`、`python-declared-v2`、`go-declared-v1` 和保守能力表 `conservative-v1`，与数据库 schema 版本独立。

规范编码以 `codecortex.public-surface\0` 开头，格式版本用 u32 小端；字符串及序列使用 u64 小端长度分隔。条目、转发记录、条件与原因规范排序、去重；参数与 token 顺序保留，字符串字面量内部空白和转义不被空白归一化抹掉。BLAKE3 摘要由这一处编码产生，DB 和 dirty 比较不重复实现公式。空与非空编码均有独立手写字节金样。

`public_surfaces(file_path, format_version, payload, fingerprint, package_key)` 与文件、符号同事务写入，外键关联 `files` 并级联删除；没有独立的 surface 写 API。正常批写、增量写、全量重建和实验 direct writer 均进入共享插入助手。DirtyResolveOnly 不重写未重新解析文件的声明证据。坏版本、内容/指纹不一致或显式文件身份不符报错；批写失败不推进 epoch，不留下半批文件。读取按占位符上限分批，复用一个读连接。

当前数据库 schema 为 **21**（当前文档清单与渲染规格）（逐文件切块规则与原始字节坐标）：P4-A 的 19、P3-D 的 18、P3-C 的 17、P3-B 的 16、P3-A 的 15、P2-D 的 14、P2-C 的 13、P2-B 的 12、P1 的 7、P2-A 的 10 及未验收草稿 8/9/11 均走原有 rebuild-on-mismatch。P2-C 增加持久 frontier 并移除 Python 正则伪调用，P2-D 同时移除 JS/TS 正则伪调用/引用并修正实际调用位置；旧事实必须重建。新增解析依赖表、包索引，以及修正后的 Rust 调用源码位置不能与旧缓存混用。升级/回滚都应使用隔离缓存重新构建；本轮测试没有打开或重建开发者的实际项目索引。原有持久资产导出/恢复路径继续保留，不能把损坏数据库恢复或多进程热降级视为本批新认证能力。

## 各语言能力

| 语言 | 本批提取 | 不作确定性承诺的情况 |
|---|---|---|
| JS/TS/JSX/TSX | named/default/type exports、ES 直接与两步 forwarding、命名空间转发、局部类型依赖、const/let/var、显式函数/方法类型、类和接口成员、静态 CommonJS 对象/属性绑定 | 推断返回类型、动态默认值/声明形态、运行时导出修改、重复绑定、环境/全局扩展、计算键、未识别转义、被遮蔽/脱离原对象的 CommonJS exports 别名等为 Unknown |
| Rust | pub/pub(crate)/受限/模块可见性、顶层与内联模块声明、类型/字段/trait/impl 签名、pub use 别名和原始路由、cfg 条件文本 | 不执行宏、过程属性、const fn 求值或 cfg_attr；保留 cfg 文本不代表已实现 feature 选择和 workspace 语义解析 |
| Python | 模块与类的静态绑定、参数/默认值/注解、静态列表/元组 __all__、显式私有绑定、包具名转发、生成器形态 | 动态 __all__、缺失的 __all__ 名称、星号展开、装饰器、运行时绑定/monkey patch、未建模实例形态等为 Unknown |

CommonJS 仅支持可静态识别的子集，不执行 require。`module.exports = {}; exports.x = 1` 不得被合成一个看似完整的对象接口。Python 中 `__all__` 控制星号可见性，不删除显式可导入的私有绑定；内部嵌套生成器不改变外层普通函数的接口形态。Rust `const fn` 位于 impl 中也不能因为忽略函数体而被认证为接口未变。

Rust 方法/关联项上的过程属性和 `cfg_attr` 也应用同一未知能力判定，不因它们位于 impl/trait 内而跳过。ES 对象/类的计算属性名需要求值，标为 Unknown；普通静态成员不受此规则影响。两类边界均有修复前失败与修复后通过的回归。

Go 使用已有 AST 提取 package、包级声明、泛型类型、别名、方法接收者与签名；常量组保留原始顺序，函数体局部变量不进入表面。首字母 Unicode Lu 的名称标记 Exported，其余包级声明标记 Module，包内部引用仍可依赖它们。`PackageKey(directory,name,test_files)` 隔离不同目录、不同包名和测试贡献；内部测试视图包含对应生产包，生产视图不包含测试文件。聚合 `pkg1:` 指纹对成员路径与各成员表面排序，不递归拼接上游包指纹。任一成员 Unknown 使聚合为 Unknown；平台文件、build tag、cgo、dot import 和未建模推断不被认证为已求值。索引的是准入文件的静态贡献，不是当前 GOOS/GOARCH 下的编译器选件结果。

Vue/Svelte 的脚本接口不等于组件接口，保持 Unknown。Java 的 classpath、C/C++ 的预处理器/模板、spec-driven/generic 的完整接口没有实现；能力表为这些语言分别保留语言身份与原因，而非返回无语言信息的 KnownEmpty。符号/调用提取能力与接口完整性是两个独立维度。

## 增量执行

现有 `indexer_phases/dirty.rs` 直接消费 `changed_from`，不另建增量引擎。新/旧证据 Unknown、缺失、版本不同或已知指纹变化，均进入现有有界脏闭包。依赖来源包括既有 import/call/ref 正向关系，以及 P2-B 同事务保存的目标表面、名称桶、路径探测、包贡献和配置条件；已知转发与 Unknown facade 继续按当前闭包规则传播。

公共指纹不含行号，但现有 `symbol_id` 是行列定位身份、`symbol_uid` 是语义身份。对已知接口未变化的文件，批量读取被跨文件 call/ref 实际消费的 `(symbol_id, symbol_uid)`；若新符号清单不再包含旧地址，将目标文件加入同一有界脏闭包。插入注释使声明移动时仍需刷新引用地址；普通函数体编辑未改变被引用地址时不额外提升消费者。读取复用一个连接并按占位符批量，不能把它称为完整的负向依赖或全仓成本上限。

此前失败的名称/路径查找与新增同名候选现在参与重解析；已索引且可识别的配置路径变化也触发依赖者失效。P2-C 已实现持久化剩余闭包、有限集合重导出传播及恢复，见 [INCREMENTAL_RECOVERY.md](INCREMENTAL_RECOVERY.md)。完整 ProjectModel、feature 条件求值、忽略文件的配置监听仍归 P3。现有 Cargo reader 识别 workspace 成员 package 名，不支持 `[dependencies]` 重命名；重解析发生不等于未知模块已能解析。`normal` 只表示当前依赖闭包在预算内收敛，不表示整门语言语义已证明。详见 [RESOLUTION_DEPENDENCIES.md](RESOLUTION_DEPENDENCIES.md)。

重新加载时清除 resolver 产生的目标 id/uid/path/策略/置信度；只有 parser_exact、同文件且完整目标身份仍存在的调用/引用可以保留。目标集合只构建一次，用哈希集合避免每条引用线性遍历全部符号。符号替换时，同一 file/uid/name/kind 的派生社区标签暂存并恢复，避免“图输入签名未变、后处理跳过”却丢失标签；新身份不会继承旧标签。

## 可观测性与验证

`IndexReport.public_surface_coverage` 是**本次实际解析文件**的 known/known_empty/unknown 计数与去重原因统计，不是全仓覆盖率，也不包含仅从 DB 重载的文件。无操作构建 parsed_files=0。旧工具名和输入参数不变，只追加输出字段。

验证分层：模型字节金样；实际 tree-sitter 提取和反例；真实 SQLite 事务/epoch/600 文件批读；原始 Python/Rust mutation 的完整事实和当前目标 UID；TS、Rust、Python 重导出链；三语言 body-only 不额外解析消费者；真实 MCP 的 index→symbol search→graph_query 消费者目标检查。

当前 P2-C oracle 比较十四表：P2-B 的十一表追加 resolution_frontier、semantic_edges、dispatch_sites，保留目标 ID/UID、解析策略、歧义与符号属性。旧 runner/报告分别保留原八表/P2-A 九表协议，不能用新表缺失来伪造旧版产品失败。真实 MCP 证明公开执行路径；白盒 SQL 对照证明所列事实等价，两者不互相冒充。Rust/部分转发目标仍可能采用原有 global_unique 启发式策略，本批不改称编译器精确解析。

测试 watchdog、预算内夹具、debug 回归，不证明 100k 性能、完整跨语言固定点或平台发布认证。G2 须等待 P2-D 与完整阶段验收，不能由 P2-A/P2-B/P2-C 通过提前放行。
