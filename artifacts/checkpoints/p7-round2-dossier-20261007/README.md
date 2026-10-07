# 多轮 subagent 推进记录与 G7 复核材料

本记录提交前的任务权威为 **192 项：152 done / 37 todo / 2 in_progress / 1 blocked，共 40 项未完成**。共 **10 个原始任务编号**取得代码实现或实际复验进展；其中 P7-011、P7-012 已按原验收条件完成。推进数量不等于完成数量，也不把重复测试、13 个接线子项或授权状态处理当作新 TODO。

`tasks.json` 是状态权威；`delivery-ledger-v1.json` 固定任务哈希、十个原始编号、实际源码、证据和限制。`evidence-index-v1.json` 是显式 P7 输入索引，`generated-v1/` 是实际运行 `scripts/p7_gate_report.py` 生成的复核报告，原命令、退出码和输出哈希见 `generation-receipt.json`。

**当前复核版本为 [generated-v2/README.md](generated-v2/README.md)**，其输入是 `evidence-index-v2.json`。独审进一步明确了原接线第 8 项的项目内 `ClaimFairness` → `WorkerLimits.claim_order` → drain → SQL ORDER BY 依据，不能只引用跨项目 ProviderGate 的公平机制。该项实现状态、所有原归属、任务哈希和计数均未改变；另补入原因仍未确定的 deadline 只读独审。原 v1 输入和生成结果完整保留，变更说明及实际命令见 `v2-clarification.json` 和 `generation-v2-receipt.json`。

## 本轮十项交付

| 原任务 | 本轮进展 | 当前边界 |
| --- | --- | --- |
| P7-011 | 14 条原 V05/V16 工程要求复核；13 个 HTTP 函数和相同输入的 #143 CI 证据 | 原范围已验收；新的完整组合 CI 另记 |
| P7-012 | 查询内 artifact coverage 缺口降为不可缓存 Partial；固定回归先失败后通过，59 个最终函数成功 | 原机制范围已验收，完整语义质量不由此认证 |
| P7-013 | 58 个原 deadline/cache/public 函数复验；新 CI 纳入原矩阵 | 原正常并发 57 成功/1 失败保留；单独诊断不覆盖失败 |
| P7-014 | GC retention checked conversion 修复；多配置 63 次成功执行/46 个函数及 13 个生命周期检查点 | 原四点调度与条件 GC 计数/切换日志责任仍有缺项 |
| P7-015 | worker 竞争测量夹具；384 个固定请求、9 个行为函数通过、旧输入零发布 | 完整有界化、性能和资源门槛开放；长尾和未知资源归属保留 |
| P7-016 | GC unlink 错误传播与实际删除计数修复；5 个新控制和 8 个原控制通过 | 完整故障/恢复、mark-unlink 竞争和上游依赖开放 |
| P7-017 | 固定源码/编译器/feature 的私有构建收据；2 产品与 2 原 runner 构建成功 | 2 个原隔离 gate 均在 `/proc` 映射处失败，14 工具矩阵尚未验收 |
| P7-019 | `ablate-strategies` 公共策略实现和实际三策略回执 | 7 协议函数、default smoke 和三策略成功；dense-only/heldout/live/全 V19-V20 开放 |
| P7-020 | G7 显式索引与总账生成器、9 个最终合成控制及本次实际生成报告 | 工程 `not_accepted`，live `blocked`；生成成功不授予任务或发行批准 |
| P8-001 | 输入冻结工具、15 个最终控制、具体 26 输入/116 关系锁定和报告新增负控 | `preparation_only`；无新 benchmark、无 release candidate，上游 P7-020 未完成 |

P7-018 的 D1+D2 授权阻塞另列，不计入上述十项。原决策文件逐字保留，没有生成 live manifest 或调用真实 provider。

## 验收边界与原失败

原 192 个任务编号、验收条件、硬依赖和条件依赖保持不变。P7-001～010 历史 `done` 不因缺少本轮条目而重开；同时，`06-VALIDATION.md` 要求的 G7 当前 run-id 证据仍需要独立评估，历史 `done` 不自动充当当前 G7 的验证。生成报告保留全部缺少本轮提交的任务清单，同时单列已验收任务和未完成任务。

13 项接线责任逐项按原 round12/TASK-BRIEFS/IMPLEMENTATION-ORDER 对账，P7-005、P7-010、P7-011/012、P7-015 和 P7-016 的原归属保留。调度点、条件 GC counters、机会式回收、完整 desired projection 和条件 switch-log cap 的未完项不会因表格行齐全而关闭。

不同任务的实际执行源码保留各自身份。新的 v8 source guard 检查 776 个完整输入、6 个独立审查差异及精确 CI 变更；源码准入与 14 项完整性控制通过只说明输入一致，不证明这些独立来源的行为已在整个组合上重新执行。下一次 PR CI 应核验最终组合，结果另行记录。

017 的两次隔离门禁在原 `/proc/<child PID>/status` 读取处失败。独立探针显示内外 PID namespace 映射不同；没有编辑旧门禁、预算或隔离策略，也没有用非隔离通过替换这些失败。seccomp 探针的阻断成功不等于观测到零 socket 尝试。具体原始日志、构建/执行身份和独立存储审查均已保留。

019 的早期共享缓存观察和首次 canonical Clippy 失败也保留；目标包清理后的固定源码 Clippy 成功单独记录。fake HTTP 三策略使用公开的 `local/auto/semantic` 策略，公开 semantic 仍包含 local lanes，因此不把这三个策略称为原要求的 dense-only 消融，不据此宣称真实语义收益或实际账单。

P8 的输入锁绑定的是已经执行的 019 原始输入，后续复核需要保留的 binary/source/raw bundle；Git 中的锁和收据不包含可执行文件字节。原模型、硬件和费用未知项保持未知。

原失败、独立审查的首次拒绝、后续修复和后续成功都按各自固定源码保存。`shared-cache-cleanup.json` 仅记录清理当次观察；先前发生的缓存目录重新出现现象及原因未知不被改写，清理结果不代表持续不存在。
