# PR #147 与 P8 主线进度整合

## 结果与固定来源

本次解决 #145 合并后 #147 的六个共享进度文件冲突。保留原始 **192** 个任务 ID、顺序、验收条件、步骤、依赖、scope 和条件字段；按 ID 保留两边全部 evidence 和 implementation notes。

| 来源 | 固定提交 |
|---|---|
| 第一父提交：P7-013 验收候选 | `753b66846a96cdb3cc541fe50f94b7ccdb05fe07` |
| 第二父提交：已合并 P8 的 main | `d53a4972af92fd10a5cddb9f15ffdf06414b3d54` |
| 共同基线 | `b951f27d3ed50b7755bc2456c6425355f753ec17` |
| 原始任务定义基线 | `6d02d77f018a5965a6f289b0b43558ed4b9f8322` |

整合后为 **153 done / 27 todo / 11 in_progress / 1 blocked，剩余 39 项**，next 为 **P7-014**。P7-013 保持已验收的 done；P7-020 保留 v3 证据且仍为 todo。main 的十项 P8 全部保持 in_progress，其新增证据和解释原文完整保留；没有新增 P8 完成声明。

产品、scripts、tests、CI 与 Cargo 输入逐 Git 对象等于固定新 main。当前静态来源集合是 **785** 个输入，manifest SHA-256 为 `264e2e7eade762c9ee1f698db1935b0876c0cf62663e957b8839bc3e16eaa2e1`，source-version 为 `p7-p8-engineering-20261007-v10`。完整来源边界见 [new-main-boundary-review.json](new-main-boundary-review.json)，该审查原文按字节复制。

## 六文件合并与历史保留

两边任务修改不重叠：候选只改 P7-013、P7-020，main 只改十项 P8。保留 main 的任务集合，仅用候选的两个完整任务对象替换对应 ID。execution_note 保留共同历史及双方各自追加段，再追加本次新计数。08-HANDOFF 保留 main 新增的非生成内容。

四个进度视图由原 `scripts/code_index_plan.py --write` 重新生成，`PLAN-CHECK.json` 保存成功命令的原 stdout。原 #147 的 **55** 个证据文件连同 mode/blob 均保持候选原值；旧 v3 与独立审查内容不改写。完整模式/blob 表及校验范围见 [integration-receipt.json](integration-receipt.json)。

## 实际轻量校验

| 原命令 | 实际结果 |
|---|---|
| `python3 scripts/code_index_plan.py --write` | exit 0；192 项、4 个视图；153/27/11/1 |
| `python3 scripts/code_index_plan.py` | exit 0；派生视图与任务源一致 |
| `python3 scripts/p8_facts.py --check` | exit 0；声明事实检查通过，`runtime_certified=false` |

命令时间、stdout/stderr 文件与摘要见 [validation-results.json](validation-results.json)。P8 facts 生成器不读取任务进度，本次没有重写 `P8-FACTS.md`。

磁盘满时使用 tmpfs 中的独立 Git 仓库和只读 object alternates：索引完整保留全部文件，只物化生成器输入与小收据。未物化路径仍在提交树中；所有暂存均指定精确路径。此环境未运行 Rust、完整 workspace 测试、历史 CLI 或 source-integrity CLI。新合并提交后续由完整 CI checkout 执行原工作流。

## 旧 CI 收据的边界

旧 #147 的首次 legacy 失败继续保留：`index` warm p95 501.93ms 违反原严格 `<500ms` 门槛。唯一一次原条件复验的 check job `112947172558` 成功；这是旧 head `753b66846a96cdb3cc541fe50f94b7ccdb05fe07` / checkout `248f81a19048a36daa06cec804beaab35ec02577` 的结果。没有把失败根因认证为噪声或 flaky，也没有把旧 776 输入的通过结果扩展为新 785 输入的实测。G7、真实 provider 与发行认证不因这次合并升级。

本目录按原字节保留旧 P7 终态、legacy 首败、唯一复验的小 review 与观察文本。**复制的 [pr147-legacy-attempt2-observation.md](pr147-legacy-attempt2-observation.md) 保留原目录语境**：文中的 `attempt2-review.json`、`carried-forward-jobs.json`、`check-job.log` 等相对名属于原 `/dev/shm/pr147-legacy-ci-attempt2-20261007` 目录。本 checkpoint 仅保存重命名的小 review [pr147-legacy-attempt2-review.json](pr147-legacy-attempt2-review.json) 与观察原文，没有声称收齐原始运行包；实际完整运行可查 [Actions attempt 2](https://github.com/jyqj/codecortex/actions/runs/37665494198/attempts/2)。其他复制收据内的原始路径同样保持其历史上下文。

本次整合提交的全量 CI 在创建本收据时尚未运行。提交身份由 Git 的两个父节点和提交树记录，避免在自身文件中制造循环哈希引用。
