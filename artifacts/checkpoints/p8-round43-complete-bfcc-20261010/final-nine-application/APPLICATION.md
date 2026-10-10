# 九项任务的待应用草案

当前 `apply_now=false`。原任务、依赖、标准、失败和旧研究均不修改。本目录的准备材料不产生任务完成信用。

`nine-task-application.bound-G2.applyfalse.json` 是从不可变原模板 `b4af966dd0c82e48f10d26ca314b4c5a65bc7868` 复制后绑定实际 G2/source/build 的候选。它的八项下游 updates 与 source_component_index 和原模板逐结构完全相同，九项定义哈希与原 tasks blob `adb44ee70a4c129bfb443d4ecf678fb934c8ea6b` 全等。18/36 是该准备时的实际快照；后来的27/54在 `readiness-and-real-repository.addendum.json` 追加，不能覆盖旧时间记录。

## 需要填入的最终事实

1. 同 source `f97c5068d056969705e8387ba1f37d83847f5bee`、run `38026411200`、attempt 1 的完整45格/85原记录和不可变custody目录。每个原收据都必须真正通过；当前只有27格/54记录，18格/31记录未知。
2. Root 唯一接收原 Actions `p8-task-matrix` 与完整 aggregate job log，经独立审核后的原聚合回执。原workflow等待全部measure，再执行原 `matrix.aggregate`；不用本草案的JSON、combine调用或重复本地aggregate替代它。
3. 从原输出填写各scale/profile/fanout的phase计数、独立事实、完整初始/最终15表parity和真实超预算 incomplete→closure证据。85条是40 setup＋40 mutation＋5 fanout；5个cold点只是预先选定的no_op setup子集，不再加计5条。
4. 实际当前源码回归/原依赖和最终适用性决定。已有G2同树主CI通过；目前保留的普通CI快照是25/26，原一小时soak待实际终态。这是已有merge条件，不是给全部旧组件新增统一重跑门。
5. 正式 P8-006 非作者验收决定、真实日期和最终应用基线。当前原aggregate、完整catalog、正式006决定、最终应用base/date仍为null。09-D4真实repo证据按附录限定引用：旧release cc-switch/Flask已经运行，但不冒称G2真实repo增量/phase/fullparity已执行；最终任务与完整V20/D4发布声明须分开判断。

## 应用顺序与原字段

只在实际决定通过以后，在独立候选检出中按精确ID更新以下原顺序。每项均先检查原harddeps已done；不是按数组位置写入。

| ID | 原harddeps | 此项闭合后done / remaining |
|---|---|---|
| P8-006 | P7-020、P8-001、P8-005 | 165 / 27 |
| P8-007 | P8-006 | 166 / 26 |
| P8-008 | P8-007 | 167 / 25 |
| P8-009 | P8-008 | 168 / 24 |
| P8-010 | P8-009 | 169 / 23 |
| P8-011 | P7-020、P8-007、P8-010 | 170 / 22 |
| P8-012 | P8-011 | 171 / 21 |
| P8-013 | P8-012 | 172 / 20 |
| P8-016 | P7-020、P8-012、P8-013 | 173 / 19 |

每项只使用候选 `conditional_patch.append_evidence`、`append_implementation_notes` 和实际通过后的 `status=done`。证据定位用完整不可变 commit＋repo path（或已经实际创建的blob与后续commit桥）。原notes和evidence只追加，不重写历史not_run/失败。把当前G2适用性/CI附在旧source-scoped证据旁，不能把旧运行改成G2运行。除明确允许的三个任务进度字段外，原definitions、acceptance/subgates、deps与其余183任务保持原样。

新45是预先注册的N=1描述性研究。分host/environment列原值，不算同机因果加速、pooled CI或稳定p99。P8-008自己的N/分布/CI来自它已接受的原生命周期研究，绝不以新45替代。旧150/1500/N30、PR1921350/N30及所有失败仍保持原身份和结论。

九项真实闭合后，若fresh ledger没有其它变化，导航可转 `next_task=P8-017`，`current_phase=P8`，`last_implementation_date` 填实际日期；不能使用本准备时日期冒充验收日期。此时173/192 done、19 remaining、session新增10。P8-017–019是后续独立链，本文不修改它们。

## 原生成器与验证

在已经审准进度编辑的候选目录中，运行原脚本两次，各保留实际argv/exit/stdout/stderr：

```sh
python3 scripts/code_index_plan.py --write
python3 scripts/code_index_plan.py
```

本次没有运行这些命令。最终允许产生差异的五文件为：

- `docs/roadmap/code-index-v2/tasks.json`
- `docs/roadmap/code-index-v2/05-TODO.md`
- `README.md`
- `docs/roadmap/code-index-v2/README.md`
- `docs/roadmap/code-index-v2/08-HANDOFF.md`

`06-VALIDATION.md` 是生成器只读输入。不得改guard/registry、旧定义或原研究门槛来让校验通过。实际publication、普通CI与merge由Root处理；此草案不更新任何ref，也不提前记完成。
