# Cypher 变长路径用惰性 BFS fast path，不下沉内存邻接缓存

- Status: accepted
- Date: 2026-06-10

## Context and Problem Statement

Cypher `graph_query` 的变长路径（`-[:CALLS*min..max]->`）通过 SQLite
`WITH RECURSIVE` CTE 遍历 `call_edges`，而 cc-server 的 GraphReadModel 已为
trace/impact 等工具维护 generation-keyed 内存邻接缓存。同一份调用图存在两套
遍历且零缓存共享。架构评审（候选 6）提出：是否应把邻接缓存下沉到 cc-search
之下，让 Cypher 复用？

## Decision Drivers

- 50k symbols / 200k call_edges 合成基准（release）：
  - LIMIT-50 变长查询：递归 CTE p50 7–71 ms；warm 内存 BFS 63 µs（120–1140x）；
    **零预热的惰性逐点 BFS（`call_edges_from_uid_lite` 点查 + 查询内 memo）
    206–280 µs（30–250x）**。
  - 全量展开（万行级）：惰性 BFS 与 CTE 打平（~100 ms vs ~99 ms），整表缓存
    仅多赚 ~3x，但单次构建成本 263 ms，需 4+ 次查询回本。
- 缓存下沉需要调整 crate 依赖方向（GraphReadModel 在 cc-server，cc-search
  在其下），并引入跨 crate 的失效协调。

## Considered Options

1. 下沉整表邻接缓存到 cc-search 之下，Cypher 复用
2. Cypher executor 内置惰性逐点 BFS fast path，仅依赖 cc-db 现有 API
3. 维持 SQL CTE 不变

## Decision Outcome

选择方案 2。惰性 BFS 拿到 30–250x 中绝大部分收益，无预热、无跨 crate 缓存
失效耦合（索引重建天然失效）；整表缓存的边际收益只在重复全量展开场景出现，
而该场景已由资格门留在 SQL 路径。

实现：`crates/cc-search/src/cypher/fast_path.rs`。保守资格门（单 MATCH、
CALLS 单变长段、正向、种子侧 name/symbol_uid 等值、简单属性 RETURN、
LIMIT ≤ 1000），其余形态回落原 CTE；语义与 CTE 逐行等价（(root, uid, depth)
元组多重性、环回根、`SELECT DISTINCT` 投影去重均有等价性测试锁定）。
`CODECORTEX_CYPHER_FAST_PATH=0` 可禁用。落地后 LIMIT-50 场景 p50
12.7–52.6 ms → 0.18–0.77 ms。

### Consequences

- 未来若出现"重复全量展开"的真实负载，再评估整表缓存；届时本 ADR 的
  基准方法（`crates/cc-server/tests/graph_traversal_bench.rs`，`#[ignore]`）
  可直接复跑。
- SQL 路径对变长段忽略方向（`<-`/`--` 均按正向）是既有怪癖；fast path 不
  复制该行为，直接回落。修复该怪癖时两条路径需同步。（"直接回落"与"两条
  路径需同步"已被下方 2026-06-10 更新取代。）
- 2026-06-10 更新（C7）：上述"两条路径需同步"的义务已由共享语义声明收敛到
  一处——`crates/cc-search/src/cypher/traversal_semantics.rs` 以单变体枚举
  声明方向处理（`DirectionHandling::IgnoreDirection`，注明为兼容性怪癖）、
  元组多重性（`(root, uid, depth)` 去重）、环终止（仅 max_hops 深度上限）
  与投影去重（`SELECT DISTINCT`）；CTE 翻译与 fast path 均通过穷尽 `match`
  消费该声明。据此 fast path 资格门不再拒绝 `<-`/`--` 变长形态（与 CTE 同样
  按正向遍历，等价性测试逐行锁定），其余资格门不变。编译期约束：多重性/环/
  去重三条规则新增变体会直接打断两条引擎的消费点；方向规则是间接约束——
  新增 `DirectionHandling` 变体直接打断的是 `orient()` 与 fast path 资格门，
  两条引擎经由 `orient()` 返回的 `WalkOrientation` 感知方向，行走方式不同时
  需为 `WalkOrientation` 新增变体（届时才直接打断两端的行走映射）。
- 2026-06-11 更新（R2-D）：资格门可见化——`build_plan` 的拒绝原因从
  `&'static str` 收敛为 `FastPathIneligibility` 枚举（Display 输出为稳定
  token，测试快照锁定），随 `graph_query` 响应新增的 `fast_path` 元数据字段
  透出（`used: true` / `used: false + reason` / 环境变量禁用 / 非变长查询时
  省略字段）；门常量（LIMIT 上限、合格边类型、种子列、可投影列）收敛为
  `FastPathConfig::DEFAULT` 单一声明。资格门的判定语义逐位不变。
