# 第32轮：8个开放PR处置

**原TODO：163已完成／29剩余；本轮正式新增0。** 当前main固定 `4efbc97`，下列为固定head的只读决策，不表示已合并、关闭或通过尚未结束的CI。发布绑定：本提交的 `artifacts/checkpoints/p8-round32-a217-20261009/round32-public-evidence.tar.gz`（原选集见成员 `pr-audit/pr-disposition/PR-DISPOSITION.md`）。

| PR / 固定head | 当前决定 | 具体理由 | 已有独审 |
|---|---|---|---|
| [#180](https://github.com/jyqj/codecortex/pull/180) · `832f79b8702c` | Ready，正常评审；等待实际合并门 | 原v15/组合树已核；本轮源码CI尚未全完，原C研究与G2观察仍分源。后续M5候选尚非当前head。 | [M4-independent-source-review.json](/workspace/scratch/a217aaae3bde/p8-db-lock-observation-integration/M4-independent-source-review.json) |
| [#184](https://github.com/jyqj/codecortex/pull/184) · `b356043c2c0c` | Ready，保留待当前checks | 复用canonical schema中既有两条索引DDL，无新增索引/写放大；精确Mac21项控与fmt/clippy/v15已验，但不替代当前Actions完整CI。 | [independent-scoped-review.json](/workspace/scratch/a217aaae3bde/original-C3ff-platform-gates/PR184-canonical-index-review/independent-scoped-review.json) |
| [#178](https://github.com/jyqj/codecortex/pull/178) · `c8be5afaac56` | Draft保留；原study终态后可作功能后继关闭 | 不是G4 Git祖先。225项原增量=17同/5演进/203原archive缺；功能hunk已保留/演进，必须明确固定历史源码、独有档案与study结果承接，不称全字节合入。 | [independent-supersession-review.json](/workspace/scratch/a217aaae3bde/original-C3ff-platform-gates/PR178-supersession-review/independent-supersession-review.json) |
| [#137](https://github.com/jyqj/codecortex/pull/137) · `bc4e5602e108` | Draft保留，选择性迁移诊断 | 原CI两新增test先因缺固定input env失败；旧packing guard又固定731inputs/ci.yml，而head有732，接线修补不能独立令旧分支合规。测试/原零质量证据未被当前覆盖，不混入M5。 | [scoped-triage-and-repair-boundary.json](/workspace/scratch/a217aaae3bde/pr137-qname-ci-triage/scoped-triage-and-repair-boundary.json) |
| [#127](https://github.com/jyqj/codecortex/pull/127) · `222da6476197` | Draft保留，未来按需提取 | ≤4批次semantic CAS发布API仍独有，未接production queue；主线/G4为0同/1演进/7缺。旧base历史CI绿不构成当前生命周期兼容/吞吐证明。 | [independent-triage-review.json](/workspace/scratch/a217aaae3bde/original-C3ff-platform-gates/PR127-65-triage/independent-triage-review.json) |
| [#65](https://github.com/jyqj/codecortex/pull/65) · `3babaa5aa0b1` | Draft保留预注册/承接决策 | 10000次按repo-membership分层paired family方案未被现2000非分层报告实现替代；保留comment6075374015仍有效。custody bookkeeping覆盖不等统计设计已吸收，不新增holdout实验。 | [independent-triage-review.json](/workspace/scratch/a217aaae3bde/original-C3ff-platform-gates/PR127-65-triage/independent-triage-review.json) |
| [#4](https://github.com/jyqj/codecortex/pull/4) · `0466d490a169` | 保留历史意图；仅逐项提取 | 叠在PR3旧树；AST/typed依赖/缓存等有新实现，但旧gold fixture与symbol→chunk语义尚需明确取舍，不能整枝覆盖schema25或视全重复。 | [legacy-pr3-pr4-disposition-review.json](/workspace/scratch/a217aaae3bde/scale-pr180-C-review/legacy-pr3-pr4-triage/legacy-pr3-pr4-disposition-review.json) |
| [#3](https://github.com/jyqj/codecortex/pull/3) · `9619641903f0` | 保留至残余迁移明确 | 唯一确定遗漏的final graph-neighbor filepath tie已作单文件候选并静审，尚未编译/执行；top20另点不改。旧fixture/projection意图仍需记录，不因一个hunk拟吸收就关闭全PR。 | [legacy-pr3-pr4-disposition-review.json](/workspace/scratch/a217aaae3bde/scale-pr180-C-review/legacy-pr3-pr4-triage/legacy-pr3-pr4-disposition-review.json) |

13:19 UTC官方完整分页快照：#180 gates成功、engineering执行中，其CI三job排队；#184 gates/engineering及MSRV成功，check/security执行中。其余平台/生命周期/runtime/P7仍按实际各自状态继续，均不能称全绿。完整head、各报告SHA及当前分页路径见同目录 `PR-DISPOSITION.json` 与 `monitor-1791551949215/`。

**执行边界：** #178的原study由runtime代理唯一监控，不取消、不重复，也不将其样本改标G4。G2的原v15、fmt、1+8+1 Rust及77 Python控制已独立接受；四档真实混合尚待。#3候选仅源码静审通过，不能用其两个新增test数量关闭原TODO。后续正常操作由根代理执行，保留原分支/历史负例/原件。
