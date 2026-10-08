# P7-017 离线产品与 P8-001 候选执行：固定 CI 原始证据验收

本目录记录 `9ebe1f619a298b9955d576d9d8250368259924d2` 的真实完成结果。两套离线产品矩阵和默认产品的 P8 候选执行均通过本次原始附件复核；原任务依赖仍由主任务台账统一闭环。本提交只增加证据，不修改任务状态、产品、评分器或验收要求。

## 固定来源与实际 CI

- 完整 source commit：`9ebe1f619a298b9955d576d9d8250368259924d2`。
- 完整 tree：`bd14c262bc8d029a64f28e05c29e611ec645f929`。
- 实际构建输入：794 项 crate/Cargo 文件；规范输入清单 SHA256 为 `23ea8ef6558b4054d13d12dfdad138fef0a8ac6bf0d91d3499094db1c115eb2a`。
- GitHub run：[`37724421493`](https://github.com/jyqj/codecortex/actions/runs/37724421493)。这是显式检出的上述 head，不是另一份 PR merge commit。
- default job：[`113139222927`](https://github.com/jyqj/codecortex/actions/runs/37724421493/job/113139222927)，原 14 工具和 P8 build/run 步均实际执行成功。
- semantic job：[`113139222987`](https://github.com/jyqj/codecortex/actions/runs/37724421493/job/113139222987)，原 14 工具实际执行成功；P8 按既定 default-only 条件跳过。

完整 job log、固定审查源码 hash、重新解析 trace 的脚本及结构化回执见 `ci-9ebe/`。原 `ci2313` 与更早失败材料保持原字节和失败结论，新结果不覆盖旧结果。

`ci9ebe-offline-log-review.json` 保留只读日志阶段的“等待附件”状态；完成附件复核后的权威结果是 `offline-review.json` 和 `p8/review.json`。原始日志保留了 GitHub 时间戳后的空格与工具输出末尾空行，因此全目录 `git diff --check` 会报告这些原始格式；未为消除格式提示而修改原证据。

| 身份 | 实际 SHA256 |
| --- | --- |
| default `codecortex` | `2fc3b7733a599073e0d93eab139b6c24d4e392652f6c4c4a124d7abfc3aacff7` |
| semantic `codecortex` | `4c6a086b67d5c9e8b13aca79e2546b73ad2ff71a738f29bfbf753f72568fe237` |
| 原工具 adapter test runner | `f0b7689d0cb5dc03a225e02be15b0273d34fff67b8efb8a652bb642d9692b367` |
| P8 原 `cc-eval` scorer | `e735402876bd985b7adcd9c15b348711a624063ab3dbd6932505aa503d873aea` |
| 网络隔离 launcher | `cea9c968db91b1f7d7354e9f7b873199e5f444305aa5cb70165d4a22f3a73433` |
| 完整进程树 trace verifier | `66f17363721e1186d3087ef3259919e2c9c7782cfba18cda9662fd0706ca17d1` |

两个产品和 P8 scorer 都有实际 Cargo 成功输出、所选 binary artifact、工具链、构建前后源码身份及产物 hash。原工具 test runner 的完整 Cargo JSONL、构建前后 794 项源码和 CI 对实际 executable 的 hash 也保留；GitHub artifact 没有单独携带这个 test runner 二进制，其身份依据是 CI 的原始构建与执行回执。所有实际产品二进制和 P8 scorer 二进制都已在完整 ZIP 内逐字节流式核验。

## P7-017：四个配置案例、八个真实产品进程

default 包的 Cargo feature 为 `[]`，semantic 包为 `["semantic"]`。每个包各运行两个配置：未配置 semantic，以及显式 `semantic.enabled=false`、无可用 key 的配置。子进程使用清空的环境、夹具 HOME/XDG 路径和 PATH，保留原禁网和原产品功能。

每个配置案例都列出并调用原 14 种工具：`status`、`index`、`search`、`context`、`node`、`explore`、`trace`、`relations`、`impact`、`architecture`、`files`、`graph_query`、`ingest_traces`、`adr`。本地源码、错误契约、无 semantic cache 的原断言均成立。每个案例另起真实产品进程重开数据库，检查索引状态、search、本地持久 ADR 的列出和删除。这里是 4 次完整工具种类覆盖与 4 次重开，不把 8 个产品根进程误写成 8 次全工具矩阵。

| 包 | 完整 matrix trace PID 数 | 产品根进程 | 产品树内进程／线程成员 | 外部网络或未知调用违规 | 单列匿名 Unix IPC |
| --- | ---: | ---: | ---: | ---: | ---: |
| default | 130 | 4 | 122 | 0 | 4 |
| semantic | 128 | 4 | 120 | 0 | 4 |
| 合计 | 258 | 8 | 242 | 0 | 8 |

PID 数按两个独立 CI job 分别计数。完整 matrix 还包含 runner 和故意的正探针，所以它的总数与产品树内成员数不同。每个产品树成员有完整退出记录，原始 trace 经固定 verifier 重算，与 CI `verification.json` 逐字段一致。

8 条产品 IPC 都有真实 `AF_UNIX` 新建匿名 pair 的内核 inode/peer 证据，并单独报告。父 runner 所需的本机 spawn 确认通信不改变产品的外部网络口径。实际 IPv4/IPv6 `EPERM` 预检、两个独立的精确地址族 `SIGSYS` 探针、父进程正常退出但子进程联网被杀的反例，以及真实 8-byte 匿名 pair 接收控制均完成；其完整原始 trace 也重新解析通过。没有以父进程 exit 0 推导整个产品树零尝试。

详细结果见 `ci-9ebe/offline-review.json`、各包的 `review.json`、完整 `raw/trace-controls/` 与 `raw/trace-matrix/`。

## P8-001：冻结后实际执行与原 scorer 复算

候选 SHA256：`0b2765bbf6994b708e729a0e48e02c03d5272c87271d5de3b49ad24c59a7076d`。

归档 SHA256：`a34d7b1d7f4546940b02e57a2c9f21fefdeebe2d4873e0289044f7c24f1aa37a`。

独立复核核对了候选的 884 个源码快照：每个文件的 SHA256、长度、可执行属性、HEAD/index mode 与 Git blob OID 均对应固定的 `9ebe1f61`。候选总计 894 个文件；archive 的 953 个 payload 文件、完整 checksum 清单，以及 candidate/evidence 两套复制品均与原件完全一致。

本次实际执行沿用原 `p0-rust-api` DEV 夹具：2 个源码文件、1 题、1 次 repetition、seed 27、timeout 30,000 ms、top-k 10，原题库和原 gold 字节保持不变。实际 MCP adapter 使用冻结的产品文件和独立冻结的 `cc-eval` scorer；原 manifest 的 HEAD 是 `9ebe1f61`，dirty 为空，infrastructure failure 为 null。原始 measured row 为 1，status 为 success。

原 scorer 的 replay 在独立结果副本执行，`metrics.json`、`gate.json`、`report.md` 与原 measurement 逐字节一致。原 measurement 全清单在 replay 和漂移检查后保持不变。binary、config、scoring、model、source、query+embedded gold、corpus source 这 7 种副本漂移都有不同的 before/after hash，并被拒绝。

实际 profile 为 `dev`，候选规格中的对应标签为 `debug`；两者都有原构建回执支持。P8 scorer 如实记录复用了同一固定源码的 product target，没有称为冷构建。模型为 disabled。

原 gate 仍是 `baseline_recorded_not_quality_certified`，exit 0；`release_certified=false`，`latest_updated=false`。本次一题结果的 Top1/nDCG 都为 1，只用于原夹具执行和复算关系，不支撑多仓、holdout、100k、稳定性能或发行质量认证。P8-001 的 P7-020 前置仍由主任务闭环；其他 P8 原任务没有因此结项。

详细证据见 `ci-9ebe/p8/review.json` 及 `raw/p8-candidate-execution/` 中的原始 candidate manifest、build receipts、execution witness、measurement、replay、7 种 drift receipts 和 archive 清单。

## 完整附件与复核方式

| GitHub artifact | ZIP bytes | ZIP SHA256 | 全成员流式核验 |
| --- | ---: | --- | --- |
| `11526689419`（default + P8） | 141,743,515 | `7fbacfab8b2300302bd7042947dfcf51a94c5d362c63374751702d328320bd05` | 2,092 项；603,019,046 解压字节 |
| `11526858595`（semantic） | 18,300,427 | `f538b8755549d57cbf5333c4ea0fb6be55833dfd44683655571820538fcdbde9` | 164 项；77,156,679 解压字节 |

两份原 ZIP 均通过 GitHub 正式下载和工作区放置获得，长度和 SHA256 与 API 元数据一致。复核对所有成员实际读取至 EOF，校验 ZIP CRC、长度、SHA256 和 Unix mode；没有只信任 archive 自报 hash，也没有解出数份大二进制占用磁盘。本目录保存必要的小型原始成员和两份完整 ZIP 成员清单。大二进制及全部源码复制品保留在原 GitHub artifact 中。

`ci-9ebe/audit/` 保存本次实际执行的只读复核脚本原字节与工具退出结果。脚本固定本次来源和 artifact 身份；输出采用新目录、拒绝覆盖。若在另一台机器复核，应在脚本副本中重绑定本地 checkout、ZIP 和新的输出路径，并单独记录该副本 hash，不改本目录原始脚本或原输出。脚本仅重新解析已有证据，不再启动产品、联网或改台账。

网络 guard/verifier 的作者为 `pr_audit`，其固定源码在 CI 前已由 `todo_acceptance` 和 `p7_wiring` 独立审查。本次 `pr_audit` 做的是与 CI 原执行分离的附件核验；P8 driver 的作者为 `todo_acceptance`，其源码及本次执行材料均由 `pr_audit` 独立审查。
