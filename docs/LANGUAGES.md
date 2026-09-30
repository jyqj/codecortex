# 语言与框架支持

CodeCortex 自动识别 30 种语言标识符（外加 `Unknown` 兜底），分三个提取
层级，并内置 16 个语义框架 resolver。

## 源码切块（P4-A/B）

源码块直接来自原始 UTF-8 字节，保留 CRLF 和末尾换行；现有 tree-sitter 任务同时输出有界 AST 坐标，类成员、长函数、注释与签名使用同一坐标体系。Vue/Svelte 脚本按原组件位置投影，不把脚本拼接文本当原文。无 AST 的语言明确使用回退。P4-B将同一行/字节/Unicode标量/估算token预算接入全部Registry解析器与SFC，具体范围见[CHUNK_POLICY.md](internals/CHUNK_POLICY.md)。详见 [SOURCE_CHUNKS.md](internals/SOURCE_CHUNKS.md)。

P4-C将上述块接入当前文档清单与独立DocVersion，并在公开源码读取前核验磁盘摘要；不存在的源码不会因为缓存命中而返回。此处不提供完整语言语义或全查询原子快照。详见[DOCUMENTS.md](internals/DOCUMENTS.md)。

## 项目模型与模块解析（P3）

最近 ts/jsconfig、JSONC、继承、paths/baseUrl 继续保留。P3-B新增捕获的workspace/package exports/imports、按导入语法及Node格式选条件、Rust声明模块/cfg和Python roots/相对包。P3-C新增Go本地module/workspace/replace包集合和显式External/Ambiguous/Unknown状态。配置发现与无磁盘模块解析分离，schema21保留导入上下文及包证据，并增加原始切块字节坐标、清除旧跨语言路径猜测。仅声明静态子集，不是完整编译器或运行时；Unsupported仍显式。见 [模块解析范围](internals/MODULE_RESOLUTION.md)。

| 模块解析语言／模式 | 已声明范围 | 未认证范围 |
|---|---|---|
| JS/TS：local_compat、node10、bundler、node16、nodenext | 捕获配置、条件有序的本地/workspace入口与路径替换 | 完整编译器默认值、安装依赖、所有构建产物映射 |
| Rust | 声明模块、path、crate/self/super、默认 feature 子集 | 宏、完整 target/profile 与 feature 合并 |
| Python | 静态 root/src、相对导入、普通包和 namespace 顺序 | 初始化器、import hooks、运行时路径变化 |
| Go | 无条件可移植、本地 module/workspace/replace 的非测试文件集合 | MVS、host target、cgo、完整类型与遮蔽语义 |
| Vue/Svelte | 已提取 script 的导入复用 JS/TS 规则 | 任意组件目标扩展和框架转换 |
| 其他语言模块规则 | 显式 Unsupported；提取/名称启发式仍是独立能力 | 不再将 Java 等导入猜成 JavaScript 文件 |

机器可检验矩阵见 `internals/MODULE_CAPABILITIES.json`；输入预算、符号链接、平台和并发边界见 [模块输入防护](internals/MODULE_INPUT_SAFETY.md)。模块结果与符号置信度、已观察到的增量新鲜度分别报告，不能互相代替。

## JS/TS 调用事实修正（P2-D）

移除按文本正则补充调用/引用的旧入口；真实 AST 调用只归属一次，声明、注释、字符串正文不是调用。保留 await 参数、模板插值、嵌套函数和链式接收者中的真实调用；动态可调用表达式保持 Unsupported。这里的静态解析阶梯不是完整 JS/TS 类型检查器，项目配置及模块规则继续由 P3 完善。当前 schema21 需隔离重建旧索引。见 [增量验证边界](internals/INCREMENTAL_VERIFICATION.md)。

## Python 调用事实修正（P2-C）

调用/引用改为遍历真实 AST 节点，不把声明、注释、字符串正文当作调用。嵌套函数有唯一所属作用域，真实递归、跨行调用和 f-string 表达式保留；参数/赋值遮蔽与动态绑定保持 Unsupported，不以全局同名猜测冒充 parser_exact。这不是完整 Python 执行或编译器推断。独立真值测试与增量对全量对照分别留证，详见 [增量恢复与事实边界](internals/INCREMENTAL_RECOVERY.md)。

## 公共接口能力（P2-A / P2-B）

公共接口指纹与下方符号提取层级分开。JS/TS/JSX/TSX、Rust、Python、Go 提供版本化的声明接口子集；动态、条件选择或未建模构造为 Unknown，不能当成无导出。Go 按目录、包名和生产/测试贡献组合，不把方法当作可直接调用的包函数。Java、C/C++、spec-driven 和 generic 由显式保守能力表标记 Unknown；现有符号/边提取仍保留，但不等于已证明 classpath、预处理器或运行时接口。Vue/Svelte 组件接口也不能由脚本导出替代。具体语法、可见性和条件边界见 [internals/PUBLIC_SURFACE.md](internals/PUBLIC_SURFACE.md)，解析结果与依赖见 [internals/RESOLUTION_DEPENDENCIES.md](internals/RESOLUTION_DEPENDENCIES.md)。

## 提取层级

### Semantic（置信度 0.85）

Python、JavaScript、TypeScript、TSX、JSX、Rust

完整 tree-sitter 解析，外加更深的文件内语义提取（限定名、作用域、
receiver/参数类型、dispatch sites、type refs）。层级只描述**解析期**的
提取深度——跨文件解析在 cc-index 中后置进行，单独提升
`resolution_confidence`。

### TreeSitter（置信度 0.7）

Java、Go、C、C++

完整 tree-sitter 解析，做标准的符号 / 调用 / 导入 / 语义边提取，但没有
上面那层更深的文件内语义富化。

全部 10 种 Semantic 与 TreeSitter 语言都提取符号、调用边、导入、数据流边
（env 访问 + 参数/返回流）和语义边。其余边类型按语言而异：

- **路由边**：Python、JS/TS、Go 在解析器层提取；Java（Spring）与 Rust
  （Actix / Axum）经框架 resolver。
- **出站 HTTP 调用边**：Python 与 JS/TS 在 AST 层；Go（`net/http`）、
  Java（RestTemplate / WebClient）、Rust（reqwest）经 URL 形态校验守护的
  保守模式匹配。
- **test edges、dispatch sites、类型赋值**：Python 与 JS/TS 最完整；
  其余语言部分支持。

### Heuristic 兜底（置信度 0.5）

C#、PHP、Ruby、Swift、Kotlin、Dart、Scala、Lua、Vue、Svelte

spec-driven 启发式（`SpecDrivenParser`）与 SFC 解析器（`sfc.rs`）：带语言
感知的模式匹配，捕获符号、导入与尽力而为的文件内调用边；不解析跨文件
调用或类型层级。

### Generic 兜底（置信度 0.3）

Markdown、SQL、YAML、TOML、HCL、Dockerfile、Bash、Protobuf、GraphQL、CMake

正则行级分块（`generic.rs`）：仅做基本的符号/结构识别，无调用边或类型
信息。这些语言主要为检索（FTS5/grep）与文件预选服务。

### 置信度分层

| 层 | 默认 | 来源 |
|------|------|------|
| Generic | 0.3 | 正则提取 |
| Heuristic | 0.5 | 带语言感知的模式匹配 |
| TreeSitter | 0.7 | 完整 AST 解析 |
| Semantic | 0.85 | 完整 AST + 更深的文件内语义提取 |
| Verified | 0.95 | 运行时验证（经 `ingest_traces`） |

注：`ingest_traces` 的证据 boost 只做数值置信度提升（每次匹配 +0.15、
封顶 1.0），不会把边迁移到 Verified 层；当前唯一写入 Verified 层的是
目录包含边（`cc-index/src/hierarchy.rs` 的 `ContainsFile`）。

解析器按元素 kind 赋的提取置信度单源化在
`ParserTier::element_confidence`（`crates/cc-model/src/lib.rs`）；未列出
的 kind 回落到上面的层默认值：

| 元素 kind | Semantic | TreeSitter |
|-----------|----------|------------|
| 符号 | 0.85 | 0.7 |
| 调用边 / 调用引用 | 0.7 | 0.7 |
| 标识符引用 | 0.6 | 0.6 |
| 语义边（声明式） | 0.95 | 0.95 |
| type ref（数据流） | 0.85 | — |
| 路由 | 0.85 | 0.8 |
| HTTP 调用（AST 检测） | — | 0.8 |
| dispatch site | 0.85 | — |

HTTP 调用边携带其**检测机制**的层级：AST 检测的记为 TreeSitter（0.8），
经 `http_call_helpers.rs` 正则检测的记为 Heuristic（0.7）。env 访问数据流
边总是正则检测，记为 Heuristic（0.8）。有意的偏离以具名常量留在调用点——
如按框架的路由校准（Next.js 0.92、Express 0.90、NestJS 0.88、中间件
0.80、DRF 0.75、Django urls 0.8）、JS/TS AST 调用边（0.85）、从
`raise`/`throw` 推断的 throws 边（0.9）。cc-index 解析器赋的解析期置信度
是另一个概念，不在本矩阵内。

## 提取能力备注

边提取的时机在解析器层（cc-parsers）与框架 resolver 层（cc-index）之间
是刻意不对称的：

- **路由边**在解析期提取的语言：Go、Python、JS/TS
  （`crates/cc-parsers/src/{go.rs, python/mod.rs, jsts/mod.rs}`）。Java
  没有解析期路由提取：Spring 路由完全由框架 resolver 合成
  （`crates/cc-index/src/framework_resolvers/spring.rs`）。Go 路由另由
  `go_router.rs` 富化（group/mount 前缀、跨文件 handler UID）。
- **dispatch sites** 只由 Python、JS/TS 与 Vue SFC 解析器产出
  （`python/mod.rs`、`jsts/mod.rs`、`sfc.rs`）。
- **出站 HTTP 调用边**来自 Python 与 JS/TS 的 AST 提取，以及 Go、Java、
  Rust 的共享保守模式匹配器
  （`crates/cc-parsers/src/http_call_helpers.rs`）。

## 语义框架 resolver（16）

resolver 把路由与 handler 挂到代码图上；**full** 层级还做跨文件 handler
引用解析。

### Full（15）—— 路由 + handler + 跨文件解析

| 语言 | 框架 |
|------|------|
| JavaScript / TypeScript | Express、NestJS、Hono、React、Vue、Svelte / SvelteKit |
| Python | Django、Flask、FastAPI |
| Go | Gin / Echo / Fiber / Chi / Gorilla（统一实现） |
| Java | Spring / Spring Boot |
| Rust | Actix-web、Axum |
| PHP | Laravel |
| Ruby | Rails |

### Partial（1）—— 仅 handler UID 解析

| 语言 | 框架 |
|------|------|
| C# | ASP.NET |

### 新增框架 resolver

创建 `crates/cc-index/src/framework_resolvers/<framework>.rs`，实现
`FrameworkResolver` trait，在 `default_registry()` 加一行
`registry.register(...)`
（[`framework_resolvers/mod.rs`](../crates/cc-index/src/framework_resolvers/mod.rs)）。
[`fastapi.rs`](../crates/cc-index/src/framework_resolvers/fastapi.rs)
是紧凑的 full 层参考实现。对有 mount/前缀语义的 HTTP 框架（router、
blueprint、URL include），声明一个 `MountSpec` 并把 `resolve_cross_file`
委托给共享的
[`mount_resolution.rs`](../crates/cc-index/src/framework_resolvers/mount_resolution.rs)
核心，不要手写 collect → 前缀 → 绑 UID 三步。其他缝隙见
[ARCHITECTURE.md](ARCHITECTURE.md#扩展点) 的扩展点目录。

## 仅检测的框架信号

通过 manifest 文件与导入模式识别、但没有专属 resolver（只检测，无语义
富化）：

Koa、Fastify、Next.js、Nuxt、Angular、Rocket、Remix、Vue Router、net/http
