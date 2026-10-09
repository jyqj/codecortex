# 第 7 轮：与 main f7 的源码整合及工程回归

原任务仍为 **192 项、163 done、29 项剩余；本轮新增完成 0 项**。用户要求的至少 10 项完全完成尚未达到。源码审查、工程测试、证据追加和 PR 管理均不折算成原 TODO 完成。

## 固定来源

| 身份 | 不可变提交 |
|---|---|
| 本轮组合产品 P3 | `034982202bdccc90b4938c6a6d6a56a36d4678af` |
| P3 tree | `7e5ea7972eef08fe2a1874ed8d2b9af977d26358` |
| 第一个 parent：双方任务证据合并 M900 | `3d571fad90f0c10030417d29faabcfe7c841972f` |
| 第二个 parent：已合 #175/#179 的 main | `f7a003636b4902f569404f4e493e1fff7630093e` |
| 既有空输入准备产品 P2 | `b4fef72211e5967f4fba729d25d0ca2958094fd5` |
| 第 1–6 轮固定发布 A | `73885e5f98a56c0dd6e5120f7907a2559398a7ac` |

P3 保留 P2 的四个 Rust 文件和 f7 的四个新 Rust 文件，各自逐 Git blob 不变。三个分支的产品修改路径不交叠；当前 v15 registry/verifier 是唯一共同修改的两个文件。P3 先完整继承 f7 的这两个文件，随后独立 review 提交仅新增本目录的证据，再由后续 binding 提交只更新当前两个文件及四个身份常量。

`BASE`、`VERSION`、validation roots/exclusions、六份 frozen v14 文件、历史证明和 CI 保持原约束。新绑定只准入被独立审查的源码及验证输入，不授予性能、规模、任务完成或发行认证。

## 本轮实际工程执行

`engineering-checks/receipt.json` 与六份原日志记录了固定 P3 的实际命令、环境、起止源码和结果。六条命令的前后 HEAD 均为 P3，前后完整 1,091 项产品输入哈希相同。

| 实际检查 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | exit 0 |
| 全 workspace、全 targets、locked/offline 的严格 Clippy | exit 0 |
| `cc-index --all-targets` | 23 targets；594 passed、0 failed、3 个既有 ignored |
| `cc-eval --lib legacy_latency_ns_tests` | 2 passed |
| `cc-eval --bin p8-runtime-statistics` | 5 passed |
| `cc-eval --lib integration_fixtures_and_corpus` | 1 passed |

corpus 本次显式选择原测试所在的 lib target；实际 argv 完整保留，没有声称执行旧的更宽 Cargo 调用。本轮没有执行 unfiltered workspace 全套。旧 P2 的 workspace exit 101、旧 G native 181 和旧 GitHub CI 均保留在原 A 中，并保留各自源码、运行和结果身份；它们没有改标为 P3 的执行。

`semantic-composition-review.json` 保存静态语义与实际 P3 对象核对，保留其形成时工程结果尚未封存的 null 字段。后续完成的实际工程收据及 `engineering-results-independent-review.json` 单独记录终态。

## 文档及归档完整性

main900 的 10 条 evidence 与 A 的 11 条 evidence 全部保留，顺序为原共同祖先内容、main 后缀、A 后缀。所有原定义、状态、依赖和导航保持不变。合并任务文件 SHA-256 为 `8e4742e00ee114fac549c86d560c6776f3384b4f0ad30179365b5a6ebbe678c3`。

`main900-integration/` 保留原 plan write、无参 plan 和 historical 的 stdout/stderr、真实执行收据及独立合并证明。这三条命令实际运行在 **HEAD=A 的 prospective merge 工作树**；不是后来 M900 或 P3 的新执行。

P3 的 21,912 项既有 artifact 恰为两个父提交的 mode/type/OID 并集；21,749 项共同内容完全相同，A 侧 138 项和 f7 侧 25 项全部保留。此目录只新增本轮证据，不覆盖任何旧原件。`archive-manifest.json` 覆盖本目录除自身之外的全部文件。

## 仍未满足的原验收

P8-005/P8-006 仍需完整、同一原研究来源的 150 分片与 1,500 phase samples。P8-007/P8-010 的原性能规范还要求完整 queue/service、DB lock wait 和 worker contention 归因；本轮 #175 的纳秒统计字段不补出原执行中未采集的产品指标。原始研究与其他 owner 的候选保持独立，没有拼接来源、重新标记失败或新派发规模研究。

第 1–6 轮的原始证据、完整缺口及源码位置图见 [既有发布](../p8-empty-input-preparation-20261009-50c/publication/README.md)。本轮与后续 binding 执行的准确信息以原收据及 PR #181 当前提交为准。
