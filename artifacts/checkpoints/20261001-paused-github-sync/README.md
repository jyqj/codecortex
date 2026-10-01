# 2026-10-01 用户暂停与 GitHub 全进度快照

**开发、验证、正式矩阵均已按用户指令暂停。当前没有活执行进程。G5/M2 未通过。**

- 项目：`/Users/jin/Desktop/codecortex-rust`；上传分支 `main`，远端 `jyqj/codecortex`。
- 唯一任务源：`docs/roadmap/code-index-v2/tasks.json`，192 项：**118 done / 1 in_progress / 73 todo**。本次没有勾选未完成任务，没有发行 tag。
- P0～P4 各20 done；P5 18 done、P5-019进行中、P5-020待办；P6/P7/P8各20待办，P9共12待办。完整目标保持，不把本地工程子集当全部交付。
- 当前工程冻结：629文件，6,783,535 bytes，源摘要 `3cd542941467c4d4855266dbd5658bf6a2928cd4ba16bb81f87246008225a461`。此摘要是明确的运行时/配置/测试/内部文档范围，不含本快照/可变路线图，不冒整个Git checkout哈希。

## 已保存的实际实现

全部当前源码/测试/配置改动直接进入仓库：项目租约/LRU/空闲回收、共享有界进程探针、Darwin CPU Mach ticks真实校准、compound identifier literal FTS整组预算、UID/byte/DocVersion图声明批映射、可信intent/source-support优先级与全JSON预算、canonical scoped exact-path域，以及仅known-equality的版本化domain说明压缩。

新增真实回归测试包括SQLite红绿、MCP no-answer、长函数/同一行/Unicode/嵌套/跨文件/版本图映射、真实Flask route/handler/return、低预算多facet原件、千文件PathDomain/SQLtripwire/语言/file scope/Markdown/编辑删除、numeric/nonce宽度与未知label保真。**不通过断言、gold、预算或SourceProof未被放宽。**

## 验证层次与失败原件

| 证据 | 实际结论 | 边界 |
| --- | --- | --- |
| P5-D resume/final-v3 | 独立38命令通过，P5-016～018已收口 | 不证明019/020/G5 |
| core-v1 | workspace1810通过/2失败 | 原Flask marker引用退化，失败保留 |
| core-v2 | 独立21工程命令通过 | 后续源变动不能借此拼新绿 |
| core-v3 | workspace1821通过/1失败 | 新说明文字挤掉731实际正文，保硬断言后通用修复 |
| core-v4 | 14/21均已完成绿，但producer stdout BrokenPipe失败 | 不用14+7拼全门 |
| **core-v5** | **同629源独立21命令通过，actual tool74031 exit0原对象持久**；两工具链workspace1823/0/60、HTTP293/0/53、focused105/0/4、协议1、stdio25、watcher17、release成本4 | **工程core_only_not_G5** |
| formal-v1 | 全29阶段采证，但共享target+mtime导致110/111真实exact关闭，48边可信0；mixed各C有100/300 path库存Partial | 所有raw/原strict红保真，候选质量证据不被错误二进制冒领 |
| formal-v2 | 正确8个独立fresh编译；stage1 alloff path空NoMatch触正文门，ownedwait2 | 测量前停，不给空body假证明 |
| formal-v3 | 全9actual开关/Full111等价真绿；baseline153 strict红；stage3 nativeablate因不同真实`CARGO_TARGET_DIR`整体options比较而foundation2，ownedwait2 | 候选8cells请求0/1224，后续facet/mixed未开始 |

新的目的单一控制fixture仅让literal path位于真实函数chunk，保731完整marker，为off态提供词法/grep正文证据；它不是生产特判，不改原51/facet gold，也没有降低门。

## 暂停时最后待做事项

外部 **versioned semantic evaluation ABI receipt projection** 仅获独立设计裁决：把真实输出目录位置与编译语义分开；完整原8producer receipt、独立targets、env/script、source、binary、Cargo实际fingerprint仍封存，实际witness继续读actualreceipt，native另读明确derived metric-plan。其它compiler/SDK/flags/profile/LTO/codegen/targettriple/jobs/锁或未知差异必须硬拒。

**该投影尚未实施、未验证、未新锁批准、未正式运行。** 不生成虚假PASS。恢复后先完成它与负例、新总合同批准，再完整51/110span/48edges/720facet/typed graph/fanout/allC mixed/raw replay/native resource/生命周期/独立审计；019/020只能在全部原义务验收后收口，然后继续P6～P9。

## 上传与保留范围

- `evidence/` 保存评测harness、实际脚本、全部重要版本输入、manifest、producer/owned/tool终态、独立审计、command日志、真实开关raw、必要原件/预算失败与回归raw、原51固定gold及source闭包。
- `evidence-index.json` 列每个上传原件路径、Git可审阅路径、bytes/SHA与本地未上传目录/文件。独立审计原文位于相邻 `todolist-completion-audit/`；worker暂停和P6/P8准备相邻保存。
- `target/`、二进制、SQLite、source.tar.gz、重复编译源码和大量完整混合/性能raw **保留在本机但不上传**。未上传不等不存在或无错误；manifest/SHA/结果摘要及必要raw公开，完整原件路径明确列出。
- 没有真实付费provider授权/调用；P7真实provider、P8 holdout/100k/Linux/发行、P9收益决策仍未完成。模板/合成1k corpus不冒真实仓、holdout或100k。

Git同步审阅/secret pattern扫描/远端验证只属于用户本次上传要求，不是重启产品验证。精确上传commit与remote ref由同步收据及最终回复给出。
