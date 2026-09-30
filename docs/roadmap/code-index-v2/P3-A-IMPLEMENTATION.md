# P3-A｜项目模型、TS 本地配置解析与最终验收

日期：2026-09-28。基线 HEAD：`4514630dcd26481cf6dbc2aff38824ed71ef06da`；保留既有未提交成果，HEAD 未移动。P3-001～005 在本文声明范围完成，累计 65 done / 127 todo，P3 完成 5/20；下一批 P3-B。**不是 G3、完整编译器或发行认证。**

## 1. 最终证据与中断恢复

本批上轮已落地主要实现，但连接中断前尚未完成最终验收，任务表因此保持 60/132。本轮重新核实实际源码，补成本测试、支持范围文档、CI 入口并检查旧回归。最初 `final` 验收在复核发现隐藏配置事件遗漏后主动停止；原命令记录和 `final/interruption.json` 保留，不能当作成功结果。

最终接受的是 `artifacts/benchmarks/p3a-20260928-KTGS/final-v2/validation.json`：**522 个覆盖文件、25 条最终命令 exit 0**。集合成员与文件内容均检查，摘要为：

`48750ff11621439baf933b11fa52ca6d94ae89e5abb2b8a551cc46cf65319b6e`

覆盖源码、测试、运行文档、CI、脚本和本批重放脚本；路线图状态与旧结果工件不在源码冻结范围内，另行做任务派生与证据路径检查。命令日志哈希及配对二进制哈希再次核验通过，详见 `scope-check.json`。`entry-source.tar.gz` 保留本批进入时的源码，包括之前已完成的未提交改动。

## 2. 实际实现与任务映射

| 任务 | 实现与边界 |
|---|---|
| P3-001 | cc-model 定义不可变 FileCatalog、ProjectModel、配置输入/来源、模式/条件与模块结果。prepare 捕获一次，parse 和 dirty reload 消费同一模型，resolve 不逐 import 读盘。package root 当前只是发现描述，非完整包解析。 |
| P3-002 | 复用 WalkManifest 和已对账源码集合；scoped 复用已记录根并消费配置/源码事件；配置即使不产出源码 chunk 仍驱动失效。实际 discover 位于 project_model/mod.rs，不另造平行 discovery 引擎。 |
| P3-003 | 内容摘要键控解析缓存，mtime/size 相同也重新确认内容；相对 extends/DAG、环/深度/大小限制、输入摘要校验与提交复核。共享选项、诊断去重及 effective 配置上限避免重复继承膨胀。 |
| P3-004 | 最近目录 tsconfig/jsconfig 归属、字符串安全 JSONC、有序 extends 数组、定义配置的相对路径来源。最近配置是产品归属策略，不是完整 tsc files/include/references 算法。 |
| P3-005 | 本地 paths/baseUrl、精确键/最长前缀、多目标顺序、扩展名替代和负向探测依赖。node10/bundler/local_compat 的本地子集明确标记；同优先级重叠模式、不支持的 Node/package 模式不猜赢家。 |

主要源码：`cc-model/src/project_model.rs`，`cc-index/src/project_model/{mod,config_cache,jsonc,typescript}.rs`，`cc-index/src/module_resolution/{mod,typescript}.rs`；build_plan、indexer、dirty、snapshot/write/config_link、ResolutionManifest 和 watcher 均已接线。实现仍属于现有七 crate 架构，不新增独立索引或恢复引擎。

模块路径不是符号身份：每文件 ResolutionManifest 增加 modules 证据、probes/config dependencies/mode/conditions，沿用现有事务与容量界限。modules_resolved/unresolved/unsupported 与符号/调用计数分开。14 表 oracle 继续保留目标 ID、UID、策略与置信度，模块证据在现有 manifest 表内比较；配置元数据确认通过单独的完整性/恢复用例验证，不能将 oracle 宣称为任意数据库全表快照。

## 3. 修复与红绿证据

**配置能力缺失。** 原实现不能处理最近配置、继承别名和 config-only 切换，`red-p3a-alias` 的三个测试均失败。现专项、全仓与真实 MCP 回归通过。

**被忽略配置反复删除/重建。** 旧 config linker 会把已从源码准入排除的配置重新写为启发式文件，下一次扫描再删除它，反复 rebase frontier，预算为一时总停在首个消费者。项目模型现在拥有这些配置的解释权，旧 writer 不再重新插入它们；`dev-p3a-2`、`red-ignored-config` 的失败保留，忽略源码配置、零源码重解析、重启续跑及 full/inc 一致性通过。

**隐藏继承配置事件遗漏。** 最终复核发现 loader 接受 `.config/base.json`，而 watcher 的隐藏目录过滤必定丢弃事件。新增负例 `red-hidden-config-watcher` 先失败；修复为 loader/watcher 共用配置路径策略，普通隐藏配置目录允许，受保护、生成、缓存与虚拟环境目录显式 Unsupported。`green-hidden-watcher`、`green-hidden-model`、`green-hidden-integration` 与最终双工具链均通过。这不是恶意并发 symlink 交换或所有 OS 事件丢失情形的认证。

**同优先级别名保护。** normalized map 不保留源对象顺序，重叠且同最长前缀的 pattern 返回 Unsupported，不按 map 顺序冒充确定语义；该最终补丁本轮已执行正负回归。

## 4. 最终验证结果

| 命令组 | stable 1.97.0 | Rust 1.95.0 |
|---|---:|---:|
| 全仓严格 Clippy，全部目标，含 HTTP 特性 | 通过 | 通过 |
| Workspace（含 doctest） | 1561 passed / 36 ignored | 1561 passed / 36 ignored |
| cc-eval HTTP 特性测试 | 164 passed / 30 ignored | 164 passed / 30 ignored |
| 显式真实 MCP 子进程回归 | 15 passed | 15 passed |
| Watcher 专项 | 13 passed | 13 passed |
| 查询期间 generation 改变负例 | 1 passed | 1 passed |
| 单连接池并发回归重复两次 | 每次 1 passed | 每次 1 passed |

不同命令覆盖重叠，不能相加；ignored 不算通过。独立项目模型集成测试 16 项、产品 SQLite/索引集成测试 10 项已包括在 workspace 中；额外 P3-A 真实 MCP 项在显式子进程命令运行，不冒充进程内 duplex 测试。

真实 MCP 验证配置目标 A→B、三个消费者预算一、重启后继续处理、归零前 incomplete。两个工具链均构建并记录精确二进制哈希。CI 已加回归入口，但没有运行远端 Actions。实际平台仅本地 macOS arm64。

## 5. 成本观测

最终 `p3a_cost` 两项 release 测试通过。146 条索引样本包含两次 full、24 次 no-op、120 次 config-resume；另 30 条纯解析样本共 111000 次查找。以下不是和旧版本配对的提速结论：

| 无关 TS 文件 | no-op 中位数（12样本） | 配置续跑单次中位数（60样本） | 每次配置发现读取 |
|---:|---:|---:|---:|
| 1000 | 20.391 ms | 18.198 ms | 2 |
| 5000 | 30.506 ms | 25.684 ms | 2 |

配置续跑有 25 个消费者、预算五，每次切换五轮完成，期间不重新 parse 源码。记录中 config_reads 只统计 discovery：提交前会再复核配置，不能把这个计数称作全部 IO。纯解析样本先移除磁盘源文件和配置，再用既有快照完成查找；这是“解析不读盘”的机制证据，不是最新磁盘语义的声明。

目录与祖先集合整理仍随文件数增长，相关 dependency/completed-prefix 和持久化 payload 也有成本。旧 P2-D release 成本与 SQL 机制测试单独重跑通过。原始样本和汇总：`observations/final-v2-p3a-release-cost/`、`cost-summary.json`。未测 RSS、p95/p99、100k 或真实多仓公开负载。

## 6. 固定题库与回放

`paired/summary.json` 已完成。P2-D 与本批使用同一个 evaluator、相同固定输入，4 套共 51 题、8 组、306 次请求；逐题 Top-1 和 nDCG 变化均为零，metrics/query-slices/costs 回放哈希一致。

原 Python/Rust signature mutation 由各自版本 evaluator 执行，四组 14 表 equal=true。14 工具输入契约一致，standalone mutation-case 正确重放成功。**S11 no-answer 仍 gate_failed/exit1，三份 compare 仍 inconclusive**；不把脚本成功误称为所有质量门通过。rolling 当前源码 R09 仍引用历史已删除 helper，gold 未为变绿修改，本批质量比较依赖历史 frozen 输入而非将 rolling 题库认证为有效 holdout。

## 7. 发布、回滚和未完成边界

当前 schema15 不兼容14及更早缓存，隔离全量重建用于升级/回滚；本轮没有打开/清空开发者日常索引。捕获配置在提交前复核，输入确认在事实与 frontier 发布后进行；崩溃可重复失效，不宣称所有元数据/后处理/文件系统原子一致。新出现但未观察到的配置根仍依赖事件/完整对账。

未支持：Node16/NodeNext/package exports/imports、完整 workspace/编译器、条件包分支、自定义 rootDirs/suffix、任意资产、OS 快照、多进程热降级、跨平台发行、embedding/live OCE。保持 Unknown、Unsupported、incomplete 与成功状态的区别。

下一批 **P3-B（006–010）**：TS package/workspace 与按模式扩展名/ESM，迁移已有 Rust workspace、Rust 模块/cfg、Python 包/src layout。先核对本报告、GATE 和实际源码，不重建第二套配置缓存或依赖引擎。

## 8. 证据索引

所有原件位于 `artifacts/benchmarks/p3a-20260928-KTGS/`：`final-v2/validation.json`、`paired/summary.json`、`cost-summary.json`、`scope-check.json`、`change-map.json`、`evidence-classification.json`、`checks/` 与 `observations/`。旧失败、首轮中断记录不覆盖。任务结构校验与派生 TODO 另存 PLAN-CHECK.json；它不是测试验收替代品。

无 commit/push/PR/merge。HEAD 与日常索引保持不变。
