# 多轮 TODO 推进：P7-013 验收与 39 项剩余

本轮按原始任务编号计数，共 **10 个 TODO 有代码实现或实际复验进展**。累计新增完成 **P7-011、P7-012、P7-013**，未完成数由轮初 **42** 降为 **39**。本次最终状态变更只有 P7-013 从 `in_progress` 到 `done`；P7-020 补充总账证据，整项状态仍为 `todo`。

任务权威为 `docs/roadmap/code-index-v2/tasks.json`，四个进度入口由 `scripts/code_index_plan.py --write` 同步。`task-transition.json` 绑定更新前后 SHA；`delivery-ledger-v2.json` 保存十个原编号、来源、证据和限制。原编号、步骤、验收条件、硬依赖、条件依赖和子门定义保持不变。

| 状态 | 数量 |
| --- | ---: |
| done | 153 |
| todo | 37 |
| in_progress | 1 |
| blocked | 1 |
| **未完成合计** | **39** |

P7 尚有 7 项未完成，P8 有 20 项，P9 有 12 项。下一项为 **P7-014：配置/status/MCP 全链贯通**。P7-018 继续按原 D1+D2 决策记录为 `blocked`；该授权状态处置不计入十项实现推进。

## PR 与当前执行证据

[PR #144](https://github.com/jyqj/codecortex/pull/144) 已合并，固定产品来源为 main `b951f27d3ed50b7755bc2456c6425355f753ec17`，完整 tree 为 `25558c46010e085d13f134f6ff623e7ce285cbd3`。其 PR head `eb7cdc55aa94c8d6865bed14fa37fff08080af33`、实际 PR checkout `389bcf601b6c25dd3a299a94a87f1c71c2481351` 与 main 的完整树相同。

| 实际运行 | 结果与范围 |
| --- | --- |
| [PR P7 工程回归 37657882000](https://github.com/jyqj/codecortex/actions/runs/37657882000) | 原 58 函数每例一次，58/0/0；整组 95 Rust + 15 Python 通过、1 项原样忽略 |
| [main P7 工程回归 37661087236](https://github.com/jyqj/codecortex/actions/runs/37661087236) | 独立正常执行，同样 58/0/0；整组 110 通过、1 项忽略 |
| [PR 原 CI 37657882021](https://github.com/jyqj/codecortex/actions/runs/37657882021) | check/MSRV/security 均成功；原 28 段命令完整执行 |
| [main 原 CI 37661087144](https://github.com/jyqj/codecortex/actions/runs/37661087144) | 三 job 均成功；默认回归、固定 HTTP、P5-B/C/D 和最后 semantic 产品 stdio 均实际通过 |

两次 P7 job 的全部 776 个 crate/Cargo 输入在运行前后逐字一致，并逐项匹配固定 Git blobs。共同映射 SHA256 为 `baf2bd238706a0fd88b91c80357d1780415afd5b917ef56d4bf283f3932e8ee6`。13 个原测试来源文件与历史源 `5af7ac…` 相同；没有修改原预算、断言或改用串行调度，实际数值线程宽度未单独测量。

两次 legacy check 各有 **3,039 次 Rust 通过、0 失败、126 次忽略**和 **144 次 Python 通过**。其中默认回归为 2,574/0/65，固定 HTTP 为 17/0/0。总数包含重复执行，不是唯一函数数；编译与 `--list` 不计作行为测试。legacy check 实际编译器为 1.99.0，MSRV 与 P7 job 为 1.95.0，各自独立归属。

原始 API、完整日志、逐函数索引、两个 GitHub 原始 ZIP、源码映射和独立审查分别保存于：

- [最终 PR/main P7 执行](../pr144-final-p7-ci-20261007/README.md)
- [成功版 PR legacy CI](../pr144-legacy-ci-passed-20261007/README.md)
- [main legacy CI 与原条件验收](../main-legacy-ci-37661087144-20261007/README.md)

## 十个原任务的实际推进

| 原任务 | 本轮交付与证据 | 最终任务状态 |
| --- | --- | --- |
| P7-011 | 按原 V05/V16 范围核对 14 条要求，13 个 HTTP 函数和 #143 同输入 CI 通过 | done |
| P7-012 | 缺失/损坏 eligible artifact 返回不可缓存的 Partial；固定回归和相关机制共 59 个函数通过 | done |
| P7-013 | 当前原 58 函数、完整相关旧回归、实际 HTTP/stdio 和独立原条件对账通过 | done |
| P7-014 | GC retention 有符号范围检查；63 次成功执行对应 46 个函数，13 个生命周期检查点 | in_progress；调度及条件计数/日志责任尚未齐全 |
| P7-015 | worker 竞争夹具，384 个实际请求观察、9 个行为函数通过，旧输入零发布 | todo；完整有界化、资源归属与性能门仍开放 |
| P7-016 | unlink 错误传播和实际删除计数修复；5 个新控制及 8 个原 GC 控制通过 | todo；完整 crash/恢复和回收竞争矩阵开放 |
| P7-017 | 私有构建/执行身份工具；11 个控制通过，2 个产品和 2 个原 runner 构建成功 | todo；两次原隔离 gate 在 `/proc` 映射处失败 |
| P7-019 | 公共策略消融入口、7 个协议函数、实际 default smoke 和三策略执行；27 原返回值、18 测量、12 loopback POST | todo；完整质量、dense-only 对照和实际成本未验收 |
| P7-020 | G7 总账生成器、9 个最终控制，当前显式 v3 输入和实际生成报告 | todo；工程 `not_accepted`、live `blocked` |
| P8-001 | 输入锁工具、15 个控制，26 个既有输入/116 个绑定关系和未登记报告负控 | todo；准备阶段，上游 P7-020 未完成 |

精确作者来源和早期失败详见 [原多轮交付总账](../p7-round2-dossier-20261007/README.md)。旧 v1/v2 输入、生成结果和审查文件保持原字节；本目录的新 `evidence-index-v3.json` 与 [generated-v3/README.md](generated-v3/README.md) 反映 P7-013 的当前验收。

## P7-013 为什么可以结项

[根验收收据](p7-013-acceptance.json) 依据当前支持的 V11/V15 工程要求，接受总 deadline、子预算/取消、物理容量归属、auto/local 回退与 explicit semantic 不足提示、锁外网络、故障缓存拒绝及当前缓存代际域。原始 58 项包含单测、真实服务和真实构建产品 stdio，各自按实际层级记录。

查询内联网络已有独立的默认关闭/显式开启决定：仅满足 enabled、semantic-http、network opt-in 和 allow_query_network 条件时安装查询编码器，保留一次尝试、绝对剩余预算、前台容量和费用上限。原条件中的未来真实支持版本升级责任保持有效，没有虚构不存在的后继版本，也没有把其他任务的整门转移成 P7-013 的反向依赖。

## 原失败和未完成范围

首次 legacy run `37653732411` 仍保存为失败，后续步骤在该次运行中仍为 skipped。失败来自旧历史检查把当前任务状态与固定历史状态比较。v9 显式改用新版历史入口，把历史状态绑定到其真实固定提交，并继续校验当前全部 192 项原定义；九个旧 helpers 和历史 manifests 保持原字节。新版本的通过是另一组实际执行记录。

原本地 deadline 57/1 与唯一 2/0 串行诊断完整保留，旧 recovery 失败原因仍未证明。当前两次正常 CI 成功支持本次工程验收，不能补造原失败未写出的 response 或时间信息。

legacy 日志中的两个产品 SHA256 是实际 stdout 摘要；该工作流没有上传完整 build receipts、产品字节或逐例 JSON。其 stdio 执行没有启用 P7-017 隔离标志，所以仍为 `not_isolated_not_claimed`。017 的原隔离失败与未完成十四工具检查保持开放。

P7-014 的完整调度和条件责任、P7-015 资源/长尾、P7-016 故障矩阵、P7-019 heldout/质量/成本，以及 G7 和发行验收均保持原状态。公开 semantic 策略包含 local lanes，现有 fake 消融不能称为 dense-only 或真实模型收益。P8 输入锁记录既有执行，没有新增发行候选。

## 后续工作与 PR 队列

下一轮从 P7-014 的 13 项原接线责任继续推进，优先处理尚未完成的调度点及条件计数/日志决定，然后完成 P7-015/016/017 的原验收矩阵。每轮继续报告权威未完成数量。

原 PR 清理记录保留：本次已记录关闭 93 个旧 PR，并合并 #142、#143、#144。后续独立观察中 #10/#11/#111 也已关闭，操作者不归因于本记录。当前队列观察见 `pr-management-observation.json`；另出现的 P8 PR #145/#146 单独审查，其成果不重复计入上述十项。
