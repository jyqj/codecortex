# 架构决策记录（ADR）

记录"为什么这样做"的决策文档。索引内容是缓存、可重建；决策依据不是——
量化的取舍一旦散佚，后人只能重新踩坑。

## 索引

| 编号 | 标题 | 状态 | 日期 |
|------|------|------|------|
| [0001](0001-cypher-traversal-lazy-bfs-fast-path.md) | Cypher 变长路径用惰性 BFS fast path，不下沉内存邻接缓存 | accepted | 2026-06-10 |
| [0002](0002-staged-commit-postprocess-out-of-write-lock.md) | Commit 三段化：postprocess/analysis 计算移出写锁，接受最终一致窗口 | accepted | 2026-06-12 |

## 撰写约定

- **何时写**：跨 crate 的结构性决策、有量化依据的取舍（基准数据、锁
  窗口测量）、或推翻过往做法的方向变更。局部实现细节写在
  `docs/internals/` 即可，不立 ADR。
- **编号**：四位递增（`0003-...`），文件名为
  `<编号>-<kebab-case-标题>.md`，新增后在上表登记一行。
- **语言**：简体中文；代码标识符、命令、日志保留原文。
- **结构**：沿用 0001/0002 的 MADR 风格——
  `Status / Date`、`Context and Problem Statement`、`Decision Drivers`
  （能量化就量化）、`Considered Options`、`Decision Outcome`（含实现
  位置与守住决策的测试）。
- **状态流转**：`proposed` → `accepted` → （被替代时）`superseded by
  ADR-XXXX`，被替代的 ADR 不删除、只改状态。
- 仓库内的 `adr` MCP 工具操作的是**索引库里的 ADR 表**（面向被索引的
  目标项目）；本目录是 CodeCortex 自身的决策记录，两者互不相关。
