# P8-006 与下游硬依赖的原文范围复核

本次固定主线 `b9b089bb4eae072affe9326681d4980eae15fd84`，官方 commit 的完整 tree 为 `6461938788665701cfa4fa095a064a3a404ec492`，与已保全 G9cc6 完全相同。原 task ledger Git blob 为 `adb44ee70a4c129bfb443d4ecf678fb934c8ea6b`。原根目录和 docs 完整目录对象没有 `AGENTS.md`；没有凭空引入 AGENTS 审批或附加验收条款。

**P8-006 的通用定义没有硬编码 150 片；当前登记的完整矩阵确有 150 片承诺。下游 depends_on 则明确是硬集成依赖，不能解释为仅工作排序。** 本复核不修改定义、状态、协议或任何既有研究。

| 原文位置 | 明确内容 | 可支持的范围 |
|---|---|---|
| `tasks.json:12161–12175`，P8-006 `steps/acceptance/validations` | no-op/body/API/config/batch、超预算闭包、各 phase 计数；时间可归因、闭包完成后 full parity、未完成显式 status；相关旧功能回归 | 原任务范围是真实增量规模与 fanout 证据。没有 150、1500、N30 字段 |
| `06-VALIDATION.md:29` V07 | 增删改、链环、大 fanout、负向查找、新同名、warm/cold 多轮固定点和 full parity | 语义场景最低要求，不规定 150 样本布局 |
| `06-VALIDATION.md:42` V20 | 1k–100k、cold/no-op/body/API/config/batch、C1/4/8/16、资源归属、no best-of | 最低场景清单，没有规定所有维度的笛卡尔积 |
| `09-BENCHMARK.md:163–167,171` | A 连续增量/B 每检查点 full；reconcile 后一致；四档 batch 1/10/100/1000 | 不能用只有 cold 的 005 或零散 smoke 自动代替完整 006 内容 |
| `09-BENCHMARK.md:173` | 每种 query profile 全样本、起始建议每层至少 30、正式尾延迟至少 200；数值是设计下限，可经 P0 资源评估锁定 | 不能删除其采样义务；也不能无证据把其 query-profile 量词扩大为每个 indexing-update cell 永久强制 N30 |
| `P8-SCALE.md:3,29` | 本地 profile 准备；具体 release 模式要求至少 30 次；单规模不是完整矩阵认证 | 后续实现的具体 producer/protocol 规则，不是原任务 JSON 中新增的通用常数 |
| `P8-SCALE.md:149–156,217–218,267–275` | 每注册记录全 raw/closure/15 表；5×30=150 片、1500 样本；缺片/失败不能聚合通过 | 当前完整矩阵的明确约定，不能把已登记研究事后改成只要 rep0 或 N1 |

“必须完整 150 正结果”应准确表述为：**若以当前注册的完整矩阵作为 006 的验收研究，需要它原定义的完整 150 槽覆盖和聚合通过。** 不意味着每个中间 build 都必须 complete：超预算后的真实 incomplete 状态及后续闭包恢复本就是必需观察，必须保留。更不能把 150 泛化为所有未来证据组织形式的永久任务定义。

现有 `future-nine-task-application.applyfalse.json` 的 `future_006_receipt_required.registered_population` 和 `round33-full-wide-cohort-bound-registration/006-acceptance-to-expected-cohort-evidence.applyfalse.json` 的完整 150 槽字段，准确适用于**所选注册总体**。其中“原验收/template”的措辞不能独立证明通用任务天然要求这个数值。若将来另作真实、事先明确的证据组合，其是否覆盖原 V07/V20 与任务全部要求需要实际审查，不能仅凭本次文字澄清将当前不完整研究或诊断前缀标成完成。这里没有提出替代研究、降低 N、重写旧总体或追加新门。

硬依赖的依据不含糊：`tasks.json:13865` 定义 `depends_on` 为硬集成依赖；`scripts/code_index_plan.py:47–52` 对 done 行逐一要求其 depends_on 状态 done；`04-PHASES.md:22` 允许提前准备和运行本地 profile，但不豁免集成条件，`:169` 还要求证据、验收、评审及回滚齐备。以下是当前原字段，九项 apply-false 输入与之逐项一致：

| 任务 | 原硬依赖 | 当前所需依赖状态 |
|---|---|---|
| P8-006 | P7-020、P8-001、P8-005 | 三项均 done；006 自身仍 in_progress |
| P8-007 | P8-006 | in_progress |
| P8-008 | P8-007 | in_progress |
| P8-009 | P8-008 | in_progress |
| P8-010 | P8-009 | in_progress |
| P8-011 | P7-020、P8-007、P8-010 | done、in_progress、in_progress |
| P8-012 | P8-011 | in_progress |
| P8-013 | P8-012 | in_progress |
| P8-016 | P7-020、P8-012、P8-013 | done、in_progress、in_progress |

`depends_on` 不禁止先完成独立组件，因此已接受的 007–013/016 自身证据不必为等待依赖而重测；但在不改原定义的范围内，不能跳过这些依赖将整项标 done。`verify_historical_integrations_v2.py:28,192–202` 只允许任务 status/evidence/implementation_notes 作为进度变化，保护原 depends_on 和 acceptance 等定义；本次没有运行或修改该 helper。

当前继续保持 006 及下游未完成，理由分别是自身证据范围尚未闭合、真实硬依赖仍开放；不把“所有研究永远都须 150”当新规则，也不把显式 incomplete 等同任务完成。原 ledger 仍 164/192 done、28 remaining。

验证范围：只读本地固定 Git 对象及原候选；缺少的四份 roadmap 正文从固定 b9 主线只读获取并按已知 tree blob 重算。没有读取任何研究 ZIP、执行原测试/校验器/测量或修改 refs/PR/labels。`review.json` 保留精确文件 Git/SHA、字段、行号与候选对应关系。
