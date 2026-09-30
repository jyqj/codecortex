# 2026-09-30｜Code Index V2 开发进度快照

本次目标是保存累计源码、文档、任务清单和必要验证证据到Git，不继续功能开发，不把开发快照当成正式发行。父基线为`4514630dcd26481cf6dbc2aff38824ed71ef06da`，目标仓库为`jyqj/codecortex`的`main`；实际提交号与是否同步以Git历史和远端ref为准。

## 任务状态

唯一状态源为[tasks.json](tasks.json)，[05-TODO.md](05-TODO.md)由`scripts/code_index_plan.py --write`生成，不手工改勾选。当前为**192项：115 done / 3 in_progress / 74 todo**。

| 范围 | 当前状态 |
|---|---|
| P0～P4 | 每阶段20/20 done，共100项；保留各批原始验收范围，不扩展为发行认证。 |
| P5 | 15/20 done；P5-016～018为in_progress，P5-019/020为todo。 |
| P6～P8 | 每阶段0/20 done，共60项todo。 |
| P9 | 0/12 done，12项todo。 |

本次修正了首页、交接、任务根摘要和PLAN-CHECK中的滞后状态，未新增任何done任务。

## 当前阻塞：P5-D final-v3未通过

原证据目录为`artifacts/benchmarks/p5d-20260930-runtime/final-v3/`。`validation.json`的总体状态是`failed`，共14条已记录命令，独立`audit.json`尚未生成。即使收据中字段名为`accepted_task_scope`，它也不能覆盖总体失败结果。

| 历史验收命令组 | 已记录结果 |
|---|---|
| stable workspace | 1782 passed / 0 failed / 59 ignored；退出0。 |
| stable HTTP | 267 passed / 0 failed / 52 ignored；退出0。 |
| stable focused | 82 passed / 0 failed / 3 ignored；退出0。 |
| stable真实MCP / 协议 / watcher | 分别25 / 1 / 17 passed；退出0。 |
| Rust 1.95 strict / build | 均退出0。 |
| Rust 1.95 workspace | 1781 passed / 1 failed / 59 ignored；退出101。 |

测试组存在重叠，不相加成唯一测试总量，ignored不算通过。上述是既有冻结验证收据，不是本次重新执行的完整测试。

失败项为`project_session::tests::close_idle_instances_closes_cached_non_active_projects`，日志位置`crates/cc-server/src/project_session.rs:777`。断言要求活动B与缓存非活动A均被关闭，预期2，实际1。目前仅记录现象，未判定是环境/调度因素还是实现缺陷，不删除失败或放宽断言。

下一步先调查并复验此失败，完成P5-016～018所需冻结验证与独立审计后再勾选；之后进入P5-019质量/成本/并发消融与P5-020整体验收。原Partial/S11及真实预算Partial保留，G5/M2、真实provider、100k、跨平台和发行认证均未完成。

## 源码与证据边界

保存前已逐文件核验原冻结清单：622文件、6,655,806字节，原摘要`ef63da5b557224f04bc1f6a79b4666221fdf96c17fbd625f8fdc6f24f41a4f5b`；无文件hash偏差。14条命令的日志SHA-256也全部一致。此后本次只进行任务文档、派生清单、忽略规则与快照证据管理，不修改运行时实现；最终轻量检查与源码范围复核记录见快照证据目录。

轻量原始证据随Git保存到`artifacts/checkpoints/20260930-git-sync/p5d-final-v3/`：validation、source-manifest、source-review、stable-contract及14份命令日志。`evidence-index.json`记录复制前的原始路径、大小与SHA-256；副本不改写旧HEAD、原始绝对路径或摘要，因此其中引用的大型原始工作区路径在干净clone中未必存在。这是可审阅失败收据包，不是包含所有二进制与历史实验数据的完整复现包。

原有约10.2 GB未跟踪内容主要来自历史benchmark二进制、实验工作区、raw观察和重复导出。它们全部保留本地，不删除、不全部推入Git；根`.gitignore`排除`artifacts`下的原始产物，只允许`artifacts/checkpoints/`中的整理证据入库。既有任务引用的完整历史产物仍属本地证据，不因文档入Git就声称这些大型文件已经上传。

## 维护入口

完成任务时先更新tasks.json，再生成TODO，重跑计划检查并更新PLAN-CHECK；禁止只勾Markdown或复制历史绿灯。当前源码保存为开发checkpoint，不创建发行标签，不修改历史验收的target_sha，不将旧收据包装成新提交的完整测试认证。
