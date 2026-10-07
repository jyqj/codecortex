# main 原 CI 与 P7-013 原条件独立复核

审查者：`/root/ci_history_review`。实际 GitHub Actions 运行：[37661087144](https://github.com/jyqj/codecortex/actions/runs/37661087144)，push / attempt 1。main 固定提交 `b951f27d3ed50b7755bc2456c6425355f753ec17`，整树 `25558c46010e085d13f134f6ff623e7ce285cbd3`，与已合并 PR #144 head `eb7cdc55aa94c8d6865bed14fa37fff08080af33` 相同。

## 实际结果

check、msrv、security 三个 job 及各自所有步骤均 completed/success。审查者取回三份原始日志、完整终态 run/jobs API，核对三处实际 checkout 和完整 28 段 workflow 命令。没有运行或重试产品、Rust 测试或 workflow。

| 范围 | 实际结果 |
|---|---|
| 原 CI Rust | 371 个 target 执行，3,039 次 passed、0 failed、126 次 ignored |
| Python | 46 个资源控件、75 个 source 控件、9 个 source architecture 控件、14 个历史 corpus 控件全部通过，共 144 |
| 固定 HTTP/status/ready stdio | 17 passed / 0 failed / 0 ignored，3 个 `--list` 只列为发现 |
| P5-B | 17 passed，包含显式产品 stdio 1 项；普通轮同项 ignored 独立保留 |
| P5-C | 32 passed，包含显式 stdio 1 项；普通轮 ignored 独立保留 |
| P5-D | 12 passed，包含显式 stdio 1 项；普通轮 ignored 独立保留 |
| 最终 semantic 产品 adapter | 4 passed，包括原完整 default/disabled semantic stdio 合约函数 |
| MSRV | 固定 1.95.0 的 workspace 与 optional HTTP 编译检查通过；没有将编译计为行为测试 |
| RustSec | 实际 JSON 为 vulnerabilities found=false/count=0、warnings={} |

这些是**执行次数**，包含重复运行和重复 ignored 观察，不是唯一函数总数。完整逐 target、逐函数索引保存在 archive。Cargo 的 stderr `Running` 行可先于上一 target 的 stdout 测试结果出现，解析器按同一步骤的 target FIFO 与完整 libtest stdout 块绑定，逐块核对函数数量及 summary；未使用“最近一条 Running”或同秒 API 时间戳猜测归属。

legacy check 实际使用原 stable selector 对应 **rustc 1.99.0**。新 P7 两个 job 固定 **1.95.0**，MSRV 也固定 1.95.0；它们的执行归属分开记录。legacy 私有构建摘要实际打印 default 产品 SHA256 `866ac758cfbaa999384f8634aa4c07697fb6e95a800402fcc0bfd8b3a67dcc80`、semantic 产品 `b7717ddf7af596cfc6cc11dd4a20eecf57d2994dda8d4c880a66bb6269110e33`，两者均声明 b951 源码、776 输入、dev profile 及各自 feature 集。

本 workflow 没有上传 artifacts。完整 build-receipt、二进制、stdio case JSON 未取回；这里保留实际 stdout 摘要，不把未上传文件说成已归档。也没有设置 `P7_017_NETWORK_POLICY=process_seccomp_deny_network`，因此其网络隔离状态仍为 `not_isolated_not_claimed`，不能替代原 P7-017 本地隔离失败。

## P7-013 接受边界

`p7-013-acceptance-review.json` 对照未改动的原条件作出 **accept_current_engineering_scope_under_original_conditions**。P7-012 依赖已 done。独立 Git 重建的全部 776 输入得到原审定 map `baf2bd238706a0fd88b91c80357d1780415afd5b917ef56d4bf283f3932e8ee6`，13 个原 58 项测试来源文件与 5af 原源码字节相同。

另读取 `ci_evidence` 的新 PR P7 run 37657882000 和 main P7 run 37661087236 全部必要原始日志，逐一交叉核对原 58 个身份的实际 case 和 summary 行，以及 before/after 的 776 条 source map。每次都是 58/0/0、每身份一次，原正常调度，没有改为串行诊断或 best-of。该交叉复核记录在 `p7-cross-binding-review.json`，完整 P7 原件由 `../pr144-final-p7-ci-20261007` 保存。

原 source 5af 的 57/1 失败与唯一串行 2/0 诊断原样保留；新成功是另一轮观察，未证明旧失败原因。原 legacy run 37653732411 的历史状态冻结失败与后续 skipped 也原样保留。本次已修复入口后的实际原 CI 完成，不能反写旧运行成功。

审查时 tasks 仍为 192 项：152 done / 37 todo / 2 in_progress / 1 blocked，**40 项未完成**。本提交仅增加证据，未改 tasks。root 可记录接受并将 P7-013 改为 done；仅此一项结项时剩 **39 项**，next_task 为 P7-014。完整 P7-014 接线、P7-015 资源/尾延迟、P7-016 crash 矩阵、P7-017 完整离线包、P7-018 live、P7-019 质量、G7 及 release 仍按原任务处理。

## 存储

`raw-observations.tar.gz` 保留 25 份原始 decoded 日志、API、完整索引、Git map、原 P7-013 对账文件与本次只读分析程序。API 原 JSON 仅补终末换行；日志保留原 BOM、ANSI、时间戳与末尾换行。四份 direct JSON 与 archive 对应原件逐字相同。

运行 `python3 verify_storage.py` 可在不解压、不执行任何 payload 的情况下检查 archive、全部 25 个成员及 4 份 direct mirrors。`checkpoint-files.json` 固定其长度和 SHA256。只读分析程序保留其实际工作目录参数，属于审计过程快照；存储核验器可直接在当前 checkpoint 使用。
