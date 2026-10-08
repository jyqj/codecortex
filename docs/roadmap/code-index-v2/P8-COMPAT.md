# P8-003：固定输入的兼容运行与对照入口

本轮为原始 `P8-003` 提供可执行的锁检查、真实 `cc-eval` 运行、raw 回放和逐 profile 对照入口。任务要求的外部范围仍是 `cc-switch` 与 Flask；两者的原始输入锁尚未取得，本轮均为 `not_run`。公开 Express dev 控制证明了入口可执行，不能计入外部 200 题的验收或发布认证。

原始范围与接受条件见 [09-BENCHMARK.md](09-BENCHMARK.md) 和 `tasks.json` 中的 `P8-003`。本文件不改变任务验收、评分器、gold、生产排名或统计阈值。

## 1. 工程入口

`scripts/p8_compat.py` 使用 Python 标准库，调用现有 Rust `cc-eval validate/run/replay/compare`。Python 层不实现第二套评分器。

```sh
python scripts/p8_compat.py run --lock /absolute/lock.json --output /absolute/new-run
python scripts/p8_compat.py run --lock /absolute/lock.json --output /absolute/new-validation --validate-only
python scripts/p8_compat.py compare --left /absolute/run-a --right /absolute/run-b \
  --evaluator /absolute/cc-eval --output /absolute/new-comparison
```

每次输出目录必须不存在，且不能与输入、二进制或 scorer 目录重叠。已有结果不会被覆盖。执行前后都会检查锁；每个子进程有独立进程组、超时与日志字节预算。错误、超时、信号退出以及“进程 exit 0 但无结果”的情况均不能成为成功回执。

`run` 将 compat 与可选 native 分别放入 `compat/`、`native/`。每个目录保留 Rust 产生的完整 manifest、query、raw response、normalized、metrics、gate 和 report。脚本复制结果到临时目录调用 Rust replay，逐字节保留原始输出，要求回放 metrics/gate 一致。回执记录原始文件 SHA256、字节数、命令、退出码、日志 hash 和脚本 hash。

`compare` 先验证两次保留结果的完整性和全部身份字段；不同 source/query/scorer/budget/config/binary/backend/platform/glob policy 不可比较，返回非零。实际 Rust comparator 也在结果副本上运行，避免其 replay 改写原始输入。compat 和 native 分开生成对照结果，沿用 Rust 的 failed/inconclusive/invalid 状态，不合并为一个“总分通过”。

`--validate-only` 只验证输入；其回执不能进入 compare。普通 run 的 exit 0 仅表示基线完整记录并能回放，所有回执的 `release_certified` 仍为 `false`。

## 2. 输入锁与外部接入

完整可用锁位于下述证据归档中的 `p8-compat-control-ebacee/lock.json`。锁采用 schema version 1，字段如下：

| 字段 | 绑定内容 |
| --- | --- |
| `dataset`, `repository`, `commit`, `source_mode` | 外部目标身份或显式 `public-dev-control`；完整 40 位 commit |
| `input_root` | 本地输入根目录；suite 的相对 source/query 指针必须保持在其内部 |
| `evaluator` | 可执行路径、SHA256、source SHA、build witness 路径与 SHA256 |
| `backend` | 本地 `rg` 或 `mcp-stdio` 的可执行路径与 SHA256；rg 实际 PATH 解析须一致 |
| `scorer` | 固定 source 中 10 个现有 Rust schema/manifest/import/normalize/validate/score/report/gate/compare/statistics 文件的 SHA256 |
| `platform` | OS、CPU architecture 与 `case-sensitive-slash-normalized-no-overlap-v1` |
| `suites` | compat 必选、native 可选；分别绑定 suite/query SHA256、source 文件全集与各自 SHA256 |
| `comparison_policy` | 现有 Rust comparator policy 文件及其 SHA256 |
| `timeout_seconds` | 单命令 1–600 秒；不改变 suite 中逐查询的 timeout |

Suite 自身的 query/source BLAKE3 digest 由实际 `cc-eval validate` 校验；wrapper 额外绑定原始文件 SHA256。source 清单必须精确一致，不能漏列或增列。重复 query ID、混用 scorer、非 dev split、指向 holdout 的路径、路径越界、符号链接和不同平台声明均被拒绝。expected-path glob 的具体合法性与重叠检查继续交给固定 Rust validator，wrapper 没有另建 glob 匹配器。

默认的 `local-default` 模式只接受显式本地默认配置 `{"auto_index":{"enabled":false}}`。run 使用真实 CLI 支持的 `smoke` 测量 profile，写入比较身份；不运行收费 provider。

外部输入接入仍须满足以下两个固定目标，并准备得到授权的原题库输入和来源锁：

| 原任务目标 | 固定 source commit | 本轮状态 |
| --- | --- | --- |
| `farion1231/cc-switch` | `40cac1a68edf8c9e7b3a89125cf40bb93a348404` | `not_run`：原始 source/query/import 锁未就绪 |
| `pallets/flask` | `22d924701a6ae2e4cd01e9a15bbaf3946094af65` | `not_run`：原始 source/query/import 锁未就绪 |

外部目标使用 `source_mode: git_checkout`。suite 的 `source.commit` 必须等于目标 commit；实际 source 根必须是对应 Git checkout 顶层，HEAD 一致，工作区无跟踪改动及未跟踪文件。本版不自动展开子模块，存在子模块时要求补齐独立来源锁后另行支持。输入与 suite 可放在同一 `input_root` 下的不同目录，suite/gold 不放入干净 source checkout。不能将 Express 等 snapshot 的名称改为 Flask 来通过检查。

build witness 作为操作方声明的来源证据按字节锁定；它与 scorer/source SHA 的绑定不是密码学编译证明，也不证明自定义二进制没有额外行为。可重复比较要求同一个 evaluator 和 backend 二进制；更换任一二进制会明确不可比。

## 3. 本轮真实执行

实际执行时间为 `2026-10-07T17:43–17:45Z`，材料目录沿用本轮 `p8-next-ten-20261008` 名称。输入直接来自已有公开 dev 资料，保持 suite、query、gold 与 source 字节不变，没有重新 freeze 或修改题目。

| 身份 | 固定值 |
| --- | --- |
| 公开语料 | Express，7 个 source 文件；上游 `7ef98448f8b38099ab1ded55e458538ad47a51e7` |
| 公开 dev author snapshot | `465e9e0bd435e2e30c08de8702f78a0d10c49c8e` |
| admission SHA256 | `b4388ca60612f6734719b348bf30e05dd0752b7b754f47cc078b590e35c1e425` |
| evaluator source | `78ae91eeae6edae6bea29c27f24b251773341c00` |
| evaluator SHA256 | `1217b6249fc5527e05d9d6ae2abd1c7db37a7ea60d8e0024a33f4dce7b2865e2` |
| 本地 rg SHA256 | `e62198eb19b136b88c330af83647b5a962cb99b6b1f066758568f12de1974849` |
| build witness SHA256 | `650cb67c0c4f7ebaf8fa1be3a4d2a1411514013b3d3f939b7d6f8a8b5854ef78` |
| 预算 | top-k 10；repetitions 3；warmup 0；逐查询 timeout 30,000 ms；seed 20261003 |
| 环境 | Linux / x86_64；本地 rg；固定 `smoke` profile；process probe 关闭 |

这份新二进制由本轮主代理实际 Cargo 构建产生。witness 说明构建后核对了 785 项 Rust/Cargo 输入与固定来源的一致性；未宣称冷构建或编译器 attestation。原件位于同轮 `validation/cc-eval-build-witness.json`，完整源清单为 `validation/current-product-source-inputs.json`。控制归档保留 witness 的相同字节与 10 个固定 scorer 源文件，不携带 77 MB 的可执行文件。

第一次 `run-a` 使用误读旧文档的 `--profile baseline`，真实 validator 成功后，当前 CLI 返回 `unknown measurement profile`、exit 2。此失败的命令、日志与回执完整保留；随后查验固定 CLI 源码，将 wrapper 改为 `smoke`。成功的两次执行使用相同最终脚本与同一输入锁。

| 实际 run | Profile | 题数 | 实测请求 | validate / run / replay | Top1 / nDCG |
| --- | --- | ---: | ---: | --- | --- |
| run-b | compat | 59 | 177 | 0 / 0 / 0 | 0 / 0 |
| run-b | native | 70 | 210 | 0 / 0 / 0 | 0 / 0 |
| run-c | compat | 59 | 177 | 0 / 0 / 0 | 0 / 0 |
| run-c | native | 70 | 210 | 0 / 0 / 0 | 0 / 0 |

共记录 **774 个真实查询请求**。这里的后端是按完整自然语言查询字面量运行的 rg 控制，零分不代表 CodeCortex 生产排名质量。baseline gate 的实际状态为 `baseline_recorded_not_quality_certified`。native 与 compat 题数不同，分别保留原始分母。

真实 Rust compare 的结果同样保留，没有调宽阈值或重跑挑选成功样本：

| 对照 | 实际 status / exit | 原始结果与限制 |
| --- | --- | --- |
| compat run-b → run-c | `inconclusive` / 1 | 两侧各 177 latency 样本，低于 policy 的 200；hardware provenance 缺失；p95 ratio 0.29244740082445525 |
| native run-b → run-c | `failed` / 1 | 两侧各 210 样本；p95 ratio 1.2127714005132988 超过原 policy 1.2；同时缺少 hardware provenance |

两者的 Top1 delta 与 family nDCG delta CI 都是 0。CI 中 59 个 independent units 是固定现有 scorer 的原始输出，不是本轮独立完成的题族审阅或资格认证。执行环境同时有本轮其他编译/测试活动，这些非绿输出用于验证状态传播和证据保留，不支撑稳定性能结论。

## 4. 可审计证据与验证

证据目录：`artifacts/checkpoints/p8-next-ten-20261008/compat/`。

- `control-runs.tar.gz`：完整保留 run-a 失败尝试、run-b/run-c 全量 raw、两份 comparator 原始结果及日志、输入 suite/query/source、Express MIT license、scorer、锁与 witness。
- `archive-manifest.json`：归档 SHA256、文件/字节总数、封存时逐成员校验结果以及完整原始成员清单的规范化 digest。
- `summary.json`：机器可读的身份、各 run 分母和状态、外部目标 `not_run`、完整对照结论。
- `tests.log`：本轮真实 Python 边界测试输出。

归档中的路径与 receipt 原始 argv 反映当时的执行目录。复现时将归档解包到自己的新目录，复制原锁并只重绑定 `input_root`、evaluator/backend/witness/scorer/policy 的绝对本地路径；保留全部预期 SHA256 与其余字段，提供同 SHA256 二进制。路径本身不计入比较身份，内容锁始终计入。不能通过改写预期 hash 把另一份输入称为原控制。解包后的旧 raw 可在相同平台和固定二进制下直接进入 compare；原有 run 目录不应作为新运行输出目录。

```sh
PYTHONDONTWRITEBYTECODE=1 python -m unittest discover \
  -s scripts/tests -p 'test_p8_compat.py' -v
```

16 个测试覆盖实际零退出伪 evaluator 无 raw、零退出 comparator 无结果、信号退出、query/binary/scorer 漂移、错误外部 commit、snapshot 冒充 Flask、实际 Git HEAD/脏目录、平台/glob policy 差异、source 清单差异、native/compat 混报、holdout 指针、符号链接、输出覆盖/重叠、不同身份阻断 compare 与 raw 保留。

`P8-003` 仍缺原外部输入的实际计数、源/题目/import lock、授权使用方式及固定 source 实际运行；V03/V04/V19 的完整外部证据也未取得。本轮没有读取 holdout 正文，没有新增独立人工题目审阅，也没有将公开控制算入外部套件或发布通过数量。


## 5. 显式文本与隐藏文件候选模式

新候选的 `local-text-hidden` 模式仅通过显式参数选择；不带参数的默认检查仍保留原配置断言。此模式只接受下列完整配置，compat/native 必须一致。多一个配置键、少一个开关、非布尔值、开启自动索引或添加 provider/query 配置都会被拒绝：

```json
{"auto_index":{"enabled":false},"indexing":{"include_text_files":true,"include_hidden_files":true}}
```

```sh
python scripts/p8_compat.py run --lock /absolute/text-hidden-lock.json \
  --output /absolute/new-text-hidden-run --configuration-profile local-text-hidden
```

该模式计入比较身份，仍使用原 `smoke` profile、固定输入与预算、真实 Rust scorer 和原 replay/gate。它不自动更换输入锁、裁剪未索引文件或把 readiness 缺口计为成功；文本与隐藏文件的准入仍受产品的系统、缓存、敏感路径、ignore、大小和内容检查约束。原模式的结果与新模式不属于同一比较身份。

候选应由固定的同一份干净源码分别构建产品与 evaluator，记录真实 Cargo 产物。产品构建脚本新增显式 `--release`，省略时仍使用 dev；evaluator 的 `build-runner --release` 入口沿用现有实现。两个回执都保留实际 Cargo profile、命令、完整 raw 日志、源码与工具链身份。`--release` 是调用方式记录，具体优化设置以 `actual_cargo_profile` 为准。编译成功本身不证明外部套件已经执行或通过。

本节是后续候选入口说明；前面章节的历史控制与其当时的 `not_run` 状态保持原样。首轮外部执行 `37735653177` 的 cc-switch 索引超时和 Flask 输入覆盖缺口均属于 `invalid_measurement`，两份 compat 未产出查询测量，native 未执行。它们的完整失败原件继续保留，不能据此声称排名通过；新模式的实际结果须由另一次完整运行及其回执给出。
