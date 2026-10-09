# P8 第 20 轮证据 checkpoint

已知数据截止：**2026-10-09 08:49:00 UTC**。本目录仅保存 19 份原文本证据；记录整理时间可以晚于截止时间，不代表新增执行。

**新增完成 0 项，仍余 29 项。** 原台账为 163 done、16 in_progress、12 todo、1 blocked。本次归档不修改任务、产品、工作流或原研究。

## 来源与范围

| 范围 | 固定身份 | 此截止的实际状态 |
|---|---|---|
| 原正式规模研究 | source `275e8799d4947d297329073eaa3ca675d3fd0777`，run `37896198208`，attempt 1 | build 与 1k/5k/10k 的 repetition 0 已按原谓词接受：3/150 shards、32 samples、5 capacity；147 shards 尚未接受。 |
| 50k 新交付观察 | artifact `11603614090`，job `113710393563` | 原 API 显示 job success 并已上传；截至本记录未接收、未独立验证，不加计接受数。 |
| PR175 | head `8da1f956f6b0dd3413a37c639a79e1e0d939de81` | 新 P/R/G 整合与发布原件已保存；七个新 CI run 的记录是初期排队观察，不是通过证明。 |
| PR182 文档回填 | head `496854bca73548906215a893d52010d90fa91d0f`，tree `d3292affce1d711d2f2c6d94c21510a8361de6fc` | 十条 evidence-only 追加及四派生视图已发布；原 Python/CI 仍待该 PR 实际运行。V8 复现不是 Python 执行。 |
| 历史 G2 工程研究 | source `0272a1fb152fd76a7cfb22386a580629d4038a64`，run `37877604259` | 原 100k 工程步骤 failure、CLI exit 1、“native shard failed”；真实 native 失败原因尚未建立。原 ZIP 尚未完整接收，不推断 deadline、OOM 或 runner loss。 |
| 完整 150 片 consumer | 固定 `fbd6ca6c27adc7af855fc4e4ece25c95d6195213`、`ca9f49761b80d1cc0cf95b9720667763f2a4eb0e`、`161db232dce15211761551861a7a13248801c2b0` | 非作者静审接受范围已封存。AST＋11 synthetic 的两次 native 请求均为传输 404，无 job、无实际执行结果；完整接收尚未执行。 |

目录中的各身份不能互换。PR175、PR182、历史 G2 的结果没有改标为原 `275e` 正式研究；已完成的工程或文档工作不折算原 TODO 完成数。

## 文件

- `consumer/`：原消费计划、固定代码、合成控制、作者准备、两次设备传输失败、非作者静审，共 8 件。
- `docs-backfill/`：v1 有界证明、v2 仅十处文案修订 manifest、非作者审查，共 3 件。五份完整文档正文已经在 PR182 固定树中，本目录不重复。
- `publication/`：R19 首次发布、PR175 实际 P/R/G 桥、R20 初期 PR 事实、PR175/PR182 的 root 发布回执，共 5 件。
- `formal275e/`：原研究完整 API 快照及有限接受范围，共 1 件。
- `historical-G2/`：历史工程终态 API 回执及原完整 job log，共 2 件。

全部 19 件的原 Git blob、UTF-8 字节数和 SHA256 见 [manifest.json](manifest.json)。作者已逐件完整 Git GET 校验；归档保留原字节，没有改写失败或补造通过。

本 checkpoint 以 R19 `cd92ce4b931e770994770ba3dba6fd0a47a505ea` / tree `1b6f119dcc8f846ce8691c25dfa9b8ff129f300a` 为唯一父基线，只新增本目录。R19 大型原件、此前 canonical 审查叶、五份文档全文不重复归档；PR179 后续 canonical 审查、08:56 发布与执行证明，以及未来 hosted-controls 分支和执行结果不在本次截止范围；指定的 f84c 初期 PR 事实原件中已有的 PR179/180 历史条目保持原字节。
