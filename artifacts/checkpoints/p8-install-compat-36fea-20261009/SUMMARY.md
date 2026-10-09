# 本轮 PR 管理与 P8-017 / P8-018 推进

## 结果与原任务计数

本线程完成了三轮子代理实施、独立复审和整合，按仓库正常合并接口依次合并了 [#182](https://github.com/jyqj/codecortex/pull/182)、[#175](https://github.com/jyqj/codecortex/pull/175) 和 [#179](https://github.com/jyqj/codecortex/pull/179)。本次产品修复基于合并后的 main `f7a003636b4902f569404f4e493e1fff7630093e`。后续 main 的 #183 只修改两个文档，已正常合并纳入；产品和验证输入未变化。

原任务仍为 **192 项：163 done、16 in_progress、12 todo、1 blocked，剩余 29 项**。三轮新增完全关闭的原 TODO 均为 **0**。用户要求的至少 10 项原 TODO 完全完成尚未达到；测试数、已合 PR、修复案例和实施子项均未计作原 TODO 完成。

| 轮次 | 已完成的工作 | 剩余原 TODO |
| --- | --- | ---: |
| 1 | 复核 PR 与任务权威数据；合并 #182 的组件证据；审查并清理 engine 重复测试 | 29 |
| 2 | 修复 Codex 安装契约和 CLI 错误退出；纠正排障事实；复现并修复独审发现的注释保留问题 | 29 |
| 3 | 合并 #175 → #179；整合后实际回归；固定独立来源审查与证据，更新原任务实施记录 | 29 |

## 产品与文档变更

- `engine.rs` 删除与已注册 `engine_lane_tests.rs` 重复的 18 个测试及重复 helper，净减 870 行。保留执行体、17 个其余测试及生产代码段，公开 API 不变。独立审查记录见 [engine review](validation/review/engine-independent-review.json)。
- Codex 安装/卸载使用现有锁定版本 `toml_edit 0.22.27` 按 TOML 结构定位服务。重复安装更新 `command` / `args`；带引号、点号、内联、嵌套表和多行值均有回归。路径正确转义，环境、超时、其他服务、用户选项及键前/键后注释得以保留。
- 无效 UTF-8、语法或表结构、读取错误及已存在的 URL transport 在写入前报错；目标不存在时卸载保持原字节。普通文件写入方式保留，没有新增原子写入或并发编辑保证。
- 任一安装/卸载目标失败时，CLI 汇报各目标结果并返回非零退出。已成功的其他目标保持其实际结果。
- README / TROUBLESHOOTING 更新安装契约及七项已陈旧事实；TEST_PLAN 将旧数字明确为历史基线，并记录本次实际范围。随后纳入 main 的 #183 文档提交，并将其中旧的“失败仍可能返回退出码 0”说明改为本次已验证的非零退出契约。

独审先复现了键前注释丢失，再对固定新 binary 的标准表和点号键各执行两次安装，确认注释、键格式、值和幂等性。旧报告生成时的摘要读取竞态已在 [修订审查](validation/review/installer-independent-review.json) 中明确更正，原报告与失败样本按原字节保留。

## 固定身份和验证

以下为实际 Rust 执行和首次来源校验所用的本地身份。产品 P：`b069c73a0d76736e1dfb3eb64e79cad6980b773f`。绑定提交 G：`2295e53c07117331faf4adbcfd967cfa8104401b`。独立审查提交 R：`733b70584fd9cf5581096fb2edfc7006f9a7dd80`，审查文件 SHA-256 为 `187d9b22ab7f09ed0bf52c25a16f514c1f3e93e13827e221c96d2dc3a7e571d5`。

[完整独立审查](independent-source-review.json) 对 1,090 个产品输入、相对原 BASE 的 49 项差异和 139 个验证输入进行核对。当前源码守卫的算法、BASE、VERSION、验证域、排除项与 CI 保持原文，仅更新原有四个绑定常量及其 registry。

| 本轮实际执行 | 结果 | 原记录 |
| --- | --- | --- |
| 全仓格式检查与严格 Clippy `--workspace --all-targets -- -D warnings` | 均 exit 0 | [独审保全的截断前观察](independent-source-review.json)，[Clippy 原日志](validation/integrated-gates-restored/clippy.log) |
| `cc-index` / `cc-search` library tests | 427 passed + 1 原 ignored；301 passed | [日志](validation/bounded-gates/index-search-libs.log) |
| 全部 installer 单测 / 真实 CLI 回归 | 49 passed；2 passed | [单测](validation/bounded-gates/installer-unit.log)，[CLI](validation/bounded-gates/installer-cli.log) |
| 规定 `integration_fixtures_and_corpus` 筛选 | 1 个 Rust test passed；88 个筛选后零测试的 target 未计作通过案例 | [日志](validation/bounded-gates/corpus.log) |
| Rust latency statistics library / binary | 2 passed；5 passed | [library](validation/bounded-gates/statistics-lib.log)，[binary](validation/bounded-gates/statistics-bin.log) |
| Python runtime 正常 discovery 入口 | 32 passed | [收据](validation/python-discovery/receipt.json) |
| 计划与文档事实检查 | 均 exit 0；原 29 项仍开放 | [原命令收据](validation/bounded-gates/commands.json) |
| 原 v15 完整源码校验 | exit 0，执行了原历史 proof | [原收据](validation/guard/01-v15-receipt.json)、[日志](validation/guard/01-v15.log) |
| 原当前 v15 防篡改用例 | 7 passed，exit 0 | [收据](validation/guard/03-v15-tests-receipt.json)、[日志](validation/guard/03-v15-tests.log) |

整合后 Rust 回归的 1,090 项产品输入 before / after 完全一致。索引模块的 ignored 是原有 benchmark，未计作性能测量。以上为默认本地构建的指定范围，未声称新的跨平台冷构建、live provider 或完整规模认证。

## GitHub 发布身份与复核

完整交付树通过 GitHub Git data API 发布：远端产品 P′ 为 `49a03e1f9fa33b7b85cbc9680b47f521afbd8abd`，独立审查 R′ 为 `bd9f7979ee76bf1ca3f0d8be284fd1a8f39fea22`，最终绑定 G′ 为 `dad1098d89ec7ab7771ec69e7cc61c50ab4a2c18`。P′ 的完整 tree 与本地交付 E `e172d24715fb33c0b59ce2fcff0dd1da57c98191` 相同；P′ 的 1,090 个产品输入和 139 个验证输入与原本地 P 相同。这两种相等关系没有混用。

[新的独立来源审查](published-independent-source-review.json) 对实际远端对象确认上述关系；[绑定后的独立复核](validation/review/post-publication-guard-review.json) 确认 R′→G′ 仅修改原四个常量与对应 registry 身份。原审查文件和所有旧命令仍保留各自本地提交身份、时间、返回结果及失败边界。

最终 G′ 下已重新实际执行原 v15 CLI（exit 0，包含原历史 proof）和当前 v15 的 7 项防篡改测试（7 passed，exit 0）。[执行身份与前后输入核对](validation/published-guard/identity.json)、[v15 收据](validation/published-guard/01-v15-receipt.json) 及 [7 项测试收据](validation/published-guard/02-current-v15-tests-receipt.json) 是这次新执行的记录；旧 Rust 测试没有被重标为新执行。原 1,090 / 139 项输入及 guard / registry 在执行前后保持相同。

原本地 P/R/G/E 提交对象保存在 [Git bundle](local-validation-history.bundle)，[摘要与前置提交](local-validation-history.json) 记录其完整身份。已有仓库包含 main 历史时，可导入这些对象以复核旧执行来源：

```sh
git fetch artifacts/checkpoints/p8-install-compat-36fea-20261009/local-validation-history.bundle refs/heads/work/20261009-todo-rounds-36fea-local-evidence
```

[10:13 UTC 的规模状态复查](publication-scale-status.json) 仍显示原两条 study 只有 3 / 150 和 4 / 150 个已完成分片，没有新增完整规模认证或原 TODO 结项。

## 保留的失败与范围

本地全历史 source-integrity suite 在观察到 35 个已完成用例通过后受控中断，记录为 [interrupted_not_passed](validation/guard/02-source-integrity-receipt.json)。本轮采用已实际通过的原 v15 完整历史 proof 与当前 v15 文件的 7 项原测试，覆盖四个绑定常量及 registry 的实际风险。仓库全部历史测试及 CI 完整入口保持原样；本地中断不代表整套通过。

完整 `cargo test --workspace` 尝试在编译/链接阶段遇到 ENOSPC，没有完整成功结果。原日志在 8,192 bytes 处截断；写收据时也遇到 ENOSPC，原 `commands.json` 为零字节。Cargo 数字退出码未知，wrapper 的退出 1 来自其收据写入失败。[环境复核](validation/review/environment-baseline-review.json) 保留该边界，没有将它改记为 sampler 失败或整套通过。

之后只清理本线程可重建的编译缓存，分批执行上表中的语料和受影响模块；这些通过记录保留各自范围。旧 sparse 缺样本、错误 Python 模块入口、原始红测以及注释反例亦分别保留。对原实现新增的 Codex 回归曾得到 5 passed / 9 failed，两个真实 CLI 回归均失败；修复后的通过属于各自的新源码记录。

## 尚待完成

P8-017 仍依赖 P8-016 及其原验收链；本轮只证明指定清理与合入组件。[只读所有权清单](validation/review/cleanup-owner-inventory.json) 记录了 14 类边界，并定位 `default_lanes` 与 `QueryPolicy::resolve` 两处默认 lane catalogue；当前五项 ID 和顺序相同，但维护所有权尚待统一。该清单不声称穷尽，也没有将保留的公开 API、独立测试参照或不同后端误作重复删除。P8-018 仍依赖 P8-017，完整 V18/V21 继续按原范围评定。

两条现有规模 study 的 [本轮 GitHub 状态观察](scale-status.json) 仍分别只有 3 / 150 和 4 / 150 个已完成分片。前者 50k / 100k 尚在运行，后者 100k 尚在运行；完整五规模、每规模 N=30 的原认证尚未取得。本线程未新建 study。P7-018 的真实 provider 认证还需要其原先规定的明确授权和预算。

新证据仅追加到原 P8-017 / P8-018 的实施记录；原任务状态、定义、依赖、样本规模和验收门槛保持原样。
