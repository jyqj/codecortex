# P7-013 原条件验收对账（条件判断，未更改状态）

审查对象：PR #144，head `149aa04f24ddcfd02c3aa5626a59343e88d74866`。
原 CI run `37653732411`；新 P7 workflow run `37653732468`。

**结论：如果新 P7 原始 58 个函数的正常执行、源码归属和相关旧 CI 均经终态原始证据确认通过，可按原条件接受 P7-013 的工程范围。未发现需要另外完成其他任务整门才能接受本项的原义务。** 此文不是整项接受收据，不修改 `tasks.json`。本次旧 CI 已在历史任务状态比较处失败，后续相关回归被跳过，因此 P7-013 仍保持 `in_progress`，总未完成数保持 40。

已读取 `pr_audit` 的独立报告：新 P7 矩阵逐 identity 核对为 58/0/0，实际 merge `1d73d87a602c214ca227ed896fa9abc9ba83c5b3` 的 tree 与 PR head 的 `c19cf91f6455472806b0d08a5408016e262ab5ab` tree 相同，source-before/after 全部 776 输入对应 `baf2bd238706a0fd88b91c80357d1780415afd5b917ef56d4bf283f3932e8ee6`。这一执行核验由该审查者提供，不能归为本文作者重新执行。最终收据应链接其固定报告和实际原日志。

`build_environment` 已明确旧 CI 的失败链：`verify_fixed_e3_integration.py:23` 调用 `verify_packing_integration.py:62`，历史 `{task_id: status}` 与 `manifest['preserved_task_states']` 比较失败。fmt、Clippy、default compile/default regression 已成功；后续固定 17 项 semantic-http、显式 default/semantic stdio 和 P5-B/C/D 步骤被跳过。本次不能把这些未执行部分计为通过。修复与新终态审查仍由 root 和 legacy 审查者负责。

## 原义务与范围依据

`tasks.json` 的 P7-013 原文要求：fake/HTTP 慢请求取消；auto 回本地、explicit semantic 明确不足；网络不占读写锁；故障结果不缓存成完整成功；相关旧功能回归通过，无证据项不得标完成。唯一硬依赖为 P7-012，PR head 上该依赖已 `done`。本次逐字段比较了原 base `886f90a542a6174a037c79eebbb4f74848fb1f53` 与 PR head，任务的 scope、steps、acceptance、validations 和 dependencies 未改变。

以下文件的原字节亦与 base 一致，完整 SHA 和定位保存在配套 JSON：

- `artifacts/checkpoints/p7-implementation-planning-20261002/TASK-BRIEFS.md:458–503`：P7-013 的专门说明、策略语义、锁与故障缓存验收、查询内联编码裁决。
- 同目录 `IMPLEMENTATION-ORDER.md:168–173`：G3-P7 逐主题分配 V05/V16、V11/V19、V11/V15、V18、V14/V17/V20；没有把批内所有整门重复分配给每一项。
- `docs/roadmap/code-index-v2/02-CONTRACTS.md:87–107`：C11、C12 的锁、预算、取消、物理容量、缓存和有限重试义务。
- `docs/roadmap/code-index-v2/06-VALIDATION.md:33,37,59`：V11、V15 最低场景及工程/fake 与 live 的分开结论。该文开头明确 Vxx 是验证包，不代表所有未来场景已认证。

## 适用条件与这次应确认的证据

| 原义务 | 当前原测试及源码依据 | 接受时应确认 |
| --- | --- | --- |
| 不可变总 deadline；子预算不超父；取消与过期发布拒绝 | `execution::tests` 五项；`semantic_query_encoding::tests` 十三项；`p7_query_deadline_public` | 原正常执行全部通过。公共 fixture 保留 150 ms child / 8000 ms parent，以及取消情形的 2000 ms child；不得改成串行诊断或延长原界限。 |
| fake/stub 慢请求和真实 HTTP 取消；物理工作未退出前保留容量 | `p7_acceptance_matrix::transport::slow_offline_transport_cancellation_keeps_worker_capacity_until_physical_exit`；编码 Drop/cancel/publication tests；生产 fairness 六项 | future drop 不计作线程退出，迟到结果不入缓存；query pin、foreground/background permit、共享 provider gate 在原断言范围内保持。 |
| auto 保留 local；explicit semantic 明确不足 | `same_request_fault_recovery_and_repeated_failure_never_reuses_complete_success` 同时测试 Auto 和 Semantic；原真实 stdio deadline/cancel 两项；旧 P5-B/P5-D 实际 stdio | 失败 lane 的 status/reason/coverage 明确，local hits 不被伪造为 semantic complete。原 `semantic` 是附加可选 lane，不是 dense-only；保留 local hits 且明确不足符合该原约定。 |
| 锁外网络、单连接读池与有界 CPU/线程 | 原 fairness 在 held HTTP 阶段测试 local/单读池进展、CPU 空闲和独立 SQLite `BEGIN IMMEDIATE; ROLLBACK`；stub 场景测试 `try_write`；生产短锁 capture | 对应行为通过，并保留当前源码短锁取得 Arc-owned handle 后释放再做网络的结构。这里证明受控执行器/许可边界，不把它扩称全 OS 线程或 RSS 上限证明。 |
| 故障不缓存成普通完整成功 | fault → complete → fault 序列每次产生新 lane receipt；真实 HTTP 超时后的调用次数与恢复/暖缓存断言；编码缓存拒绝错误批次 | 不能只看到最终有 local hits 就判成功。需原 status、coverage、再次调用计数和空 query cache 断言均通过。 |
| V15 故障分类及返回合法性 | 原 transport fixture 覆盖 429、500、auth、timeout、cancelled、缺/重复 index、错误维度、zero、非数值；编码单测另有 NaN、脱敏与类型化错误 | 当前相关函数与既有 provider library 回归按各自执行归属通过，不将 fake 结果解释为真实模型效果。 |
| V11 混代拒绝、有限重试、完整当前缓存依赖 | `p7_v05_v11_independent_review`、generation、ready epoch、key matrix、cache consumption、independent consumption/status 等原目标 | 原 3 次和有界嵌套重试、config/generation/cache-domain 断言继续执行，不能为了收口只选择 deadline 两个函数。 |
| 查询内联编码必须明确裁决 | `docs/CONFIGURATION.md:245`；`semantic_runtime.rs:447–592`；`config.rs` 独立默认 false；编码与状态授权单测 | 新收据明示已有决定：默认不发 query；仅 `semantic-http` + enabled + network opt-in + 独立 `allow_query_network` 才安装编码器；一次尝试、绝对剩余预算、有界阻塞工作、独立前台容量和原费用帽继续生效。 |
| 相关旧功能回归 | 旧 CI 的编译、fmt、Clippy、default regression、semantic lib/normal targets、固定 17 项 HTTP/status/ready，以及 P5-B/C/D 和实际 MCP 产品回归 | 读取真实终态和 raw 结果、分列 ignored；不得把 no-run、skipped、其他 SHA 的通过或未完整执行的命令计入。 |

### 58 项的精确范围

HTTP integration 的 11 个 target 共 40 项：

| Target | 原函数数 |
| --- | ---: |
| `cache_key_review_behavior` | 2 |
| `p7_acceptance_matrix` | 7 |
| `p7_production_fairness_model_review` | 6 |
| `p7_query_deadline_public` | 2 |
| `p7_status_snapshot_independent_review` | 6 |
| `p7_v05_v11_independent_review` | 6 |
| `p7_v11_cache_consumption` | 3 |
| `p7_v11_cache_key_matrix` | 3 |
| `p7_v11_generation_public` | 2 |
| `p7_v11_ready_epoch_independent_review` | 1 |
| `v11_consumption_independent_review` | 2 |

再加 `semantic_query_encoding::tests::` 13 项与 `execution::tests::` 5 项，合计 58 个不同函数。它们混合 L1/L2/L3：`p7_query_deadline_public.rs:150–171` 和 ready target 的 `:137–158` 确实使用 `TokioChildProcess` 启动 `env!("CARGO_BIN_EXE_codecortex")`，所以包含真实构建产品 stdio；其他进程内服务、SQLite 和单测应按自身层级记录。

## 没有额外转移到 P7-013 的整门

查询内联策略的规划问题已由当前配置文档和生产实现明确解决，不应继续把原 `OPEN-QUESTIONS.md` 的历史提问当作今天仍无决定；也不能把关闭 query 网络误述为关闭已另行 opt-in 的后台文档回填网络。相关生产 gate、默认配置和编码代码在 PR head 上与此次本地固定源 `5af7ac…` 完全相同。历史 P7-014 参数矩阵可按原 SHA 引用；它不是新 head 的重复执行。

`P7-REMAINING-GATES.json` 已有明确的 `current_vs_future_V11` 决定：所有当前 V11 义务仍适用；尚不存在的 parser/rule/metric/tokenizer/spec successor 不能被虚构为当前必须跑的迁移版本。未来真实支持升级时仍须执行原跨构建/旧句柄/reopen 义务，现状继续保留 `not_run_cross_build`，没有被本文改为 passed。

P7-014 的完整五态接线与调度、P7-015 的资源/性能及剩余 reconcile 投影、P7-016 的完整 seeded crash/GC-WAL 矩阵、P7-017 的完整离线包验收仍属原任务。它们的整个未完成状态不是 P7-013 的新增反向依赖；任何本次实际相关旧回归失败仍必须据实处理。

D1+D2、`06-VALIDATION.md:59` 和实施顺序第 3 节明确允许工程/fake 接受，live 另记 blocked。P7-018 的授权、P7-019 的质量/holdout/消融、完整 G7 和 release 不能随 P7-013 收口而变为通过，也不需要为了 P7-013 去执行真实付费调用。

## 正式收据必须保留的边界

原 `5af7ac…` 正常执行 57/1 和唯一一次 `--test-threads=1` 的 2/0 诊断均原样保留。新的正常强制 CI 成功是新观察，不抹去旧失败，不证明旧失败的原因。原失败的完整 recovery response 和阶段/E2E 时间未被原 fixture 的断言后 trace 写出，不能从其他场景补造。

正式收据应同时固定 PR head、实际 checkout/merge SHA、workflow run/job/attempt、source-before/after 完整映射、构建 target/toolchain/命令归属、原日志 hash、逐 target 计数和两位 CI 审查者的报告。GitHub pull-request 的 merge SHA 可以不同于 head SHA；必须记录双方并核对实际 source tree，不能凭名称混同。没有收集的 binary digest 不应填造。

条件全部满足后，root 可另出当前工程接受收据、更新 P7-013 的状态和 evidence/notes，保持原验收与硬依赖不变。若仅此一项结项，未完成数从 40 变为 39；本审查未执行该更新，也不判定 P7-014、G7、live 或 release 完成。
