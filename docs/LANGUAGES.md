# 语言与框架支持

CodeCortex 自动识别 30 种语言标识符（外加 `Unknown` 兜底），分三个提取
层级，并内置 16 个语义框架 resolver。

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
