# 核心 packing scope-v1 修复与定向证据

结果：原 `p5e_priority_pressure` 在指定 base 两项失败，在 fixed 两项通过；默认真实 MCP **带 `path_prefix=scope` 的 search** 在 Fix/Refactor/Trace 保留完整 impl/API，Fix/Refactor 保留完整 test body。原两份 noise、所有 caps、排名、断言均未修改。

- base：`49e0330754e6451e7eb5e91863f432ce7eaf816c`（包含 product `aa271e52b9c2aa52e05696af96ac52116963e57e` 的 Python/C++ 修复）。
- fixed source：`90858afae647a513537bf118932a7ba5020ee98b`；最终 evidence/docs head 由本分支后续提交提供，产品树与此 fixed source 一致。
- 独立诊断来源：`cffdbf6bb5b04569d63720f07a2fe645a9fbe31a`；复用 stage recorder 逐字一致，SHA256 `7a5ce9855106cb7f10f713ea64127db5a609006e329979dab0db5abd2b3a4aea`。
- 本环境无 AGENTS.md 或 `.agents/skills` 文件。Rust 1.95.0，全部 cargo `--locked -j 2`。

## 产品变化与证明范围

`budget.rs` 仅增加 26 行精确匹配映射；三个 producer 抽取原始说明为共享常量，未改变其值或逻辑。预算触发时，先将已知 selector/freshness 说明替换为 `cue_window:v1`、`disk_generation:v1`、`disk_files:v1`，再沿原过程删除正文。完整语义见 `docs/internals/QUERY_EXECUTION.md`：validated twice-top-k literal cue 不是全局唯一或 exact identity；bounded per-file disk + optimistic full-read generation 不是 atomic filesystem snapshot；legacy per-file scope 不升级为 generation proof。未知/近似标签不改字节；标签有版本，已紧凑标签重打包不再变化。

未修改 source/document/qname、numeric counts/budgets、priority、ranking、cost、Partial、scorer/gold、microfixture/noise 或阈值。Python/C++ parser owner、CI metadata 均未修改。七个 hydrated candidates 的 path、chunk_id、text、score_trace、rerank_score、qname、document、source_evidence 在 baseline/fixed 全部相等；实际源码/span/digest 均经 normalizer 校验。原 failure log 完整保留。

范围是原合同的 scoped public search 默认预算门。stage recorder 另记录无 path-prefix 的 public context：Fix/Refactor 会选择 `outside/forbidden.py`，未保留 `scope/impl.py`；baseline 与 fixed 的该路径列表相同。context 不提供 path-prefix 参数，此处不把 scoped search 的通过推广为所有 unscoped context 都必含某指定文件。Trace context 保有 impl/API。没有索引/排行修复或 general completeness 宣称。

双 pack 仍是设计债：核心 pack 后 handler 加入约 199 bytes 的 resolution freshness，再 pack。此包仅提供已知说明余量，未重构 pipeline。

## 实际 Rust/serde_json 字节数

这些值来自 Rust 实际序列化，不是 pretty JSON 文件大小或 Python 重序列化。记录器复制的组装逻辑与真实核心严格比较 hits/selection/omissions，final-boundary replay 与真实 MCP 严格比较 hits/omissions；独立 elapsed/generation/cache 观测不伪造成逐字相等。

| source / intent | prepack | core packed | dispatch before | dispatch after | public bytes / hits / omissions |
| --- | ---: | ---: | ---: | ---: | --- |
| baseline fix | 39635 | 15978 | 16177 | 15216 | 15216 / 4 / 3 |
| baseline refactor | 39680 | 15833 | 16032 | 15070 | 15070 / 4 / 3 |
| baseline trace | 39545 | 15952 | 16151 | 15976 | 15976 / 5 / 2 |
| fixed fix | 39636 | 15959 | 16158 | 15833 | 15833 / 5 / 2 |
| fixed refactor | 39681 | 15839 | 16038 | 15863 | 15863 / 5 / 2 |
| fixed trace | 39546 | 15965 | 16164 | 15982 | 15982 / 5 / 2 |

## fixed 的最终公开响应重打包矩阵

所有成功项保留的 source/text/proof 是完整原件；按原断言验证 score/rank、facet/support 顺序与诚实 Partial。以下数值为 `packing.used_bytes`，原合同验证其等于 Rust 实际 serialized size。

| intent | cap | used bytes | hits | omissions | outcome |
| --- | ---: | ---: | ---: | ---: | --- |
| fix | 16000 | 15833 | 5 | 2 | Partial |
| fix | 14000 | 13862 | 3 | 4 | Partial |
| fix | 12000 | 11858 | 2 | 5 | Partial |
| fix | 10000 | 9962 | 1 | 6 | Partial |
| fix | 8000 | 7970 | 0 | 7 | Partial |
| fix | 5000 | — | — | — | `search error: required context metadata exceeds output budget` |
| refactor | 16000 | 15863 | 5 | 2 | Partial |
| refactor | 14000 | 13892 | 3 | 4 | Partial |
| refactor | 12000 | 11888 | 2 | 5 | Partial |
| refactor | 10000 | 9992 | 1 | 6 | Partial |
| refactor | 8000 | 8000 | 0 | 7 | Partial |
| refactor | 5000 | — | — | — | `search error: required context metadata exceeds output budget` |
| trace | 16000 | 15982 | 5 | 2 | Partial |
| trace | 14000 | 13873 | 3 | 4 | Partial |
| trace | 12000 | 11964 | 2 | 5 | Partial |
| trace | 10000 | 9924 | 1 | 6 | Partial |
| trace | 8000 | 7932 | 0 | 7 | Partial |
| trace | 5000 | — | — | — | `search error: required context metadata exceeds output budget` |

## 位宽、未知说明与边界

原 short/wide（elapsed 9 / 9,999,999；epochs 9 / 1,234,567；incarnation 16 bytes）和 unknown policy-label 实验实际通过。新增最大合成 stress：complete lanes 的 elapsed_us、source generation 的 index/evidence epochs、resolution freshness 所有 epoch 数值均设为 `u64::MAX`；incarnation 为 16 个 255；恢复 policy 长说明以确保压缩压力。保留 impl 与完整 source/document/qname，原 cost/真实 SQL 数值未改，所有注入字段和完整 lane receipts 比较相等。**serialized 15,962 bytes / margin 38 bytes**，而非仅针对本次 36-byte overflow；这是合成 presentation 宽度实验，不是测量成本/世代，也不声称任意所有 caller 数字字段同时 20 位都有相同余量。

新增三项回归还覆盖：已知 scope 只在压力下压缩；两个 freshness scope 不互相升级；near-known 后缀和尾空格逐字保留；二次 pack 全 JSON 相等；8k 后提高到 16k 不复活旧正文或旧遗漏计数；零 references/nodes/spans 仍保有 fitting impl，大小自计。原 8k reference-only 保持 Partial，5k 明确错误。

## 验证与复跑

| check | result |
| --- | --- |
| base 原 p5e_priority_pressure | 0 passed / 2 failed，exit 101，完整 log 保留 |
| fixed 原 p5e_priority_pressure | 2 passed |
| fixed packing_scope_v1 | 3 passed |
| baseline/fixed stage recorder | 各 1 passed；fixed 无 evidence env 也 1 passed |
| explicit saved-receipt Rust measurement | baseline/fixed JSON 各 1 passed |
| cc-search --lib | 298 passed |
| bounded-context selected suites | 41 passed / 2 ignored（explicit stdio / benchmark opt-in，不当作已运行） |
| clippy cc-search --all-targets | exit 0，-D warnings |
| clippy cc-eval 精确十个 test targets | exit 0，-D warnings |
| cargo fmt --all -- --check；git diff --check | exit 0 |

bounded suites：p5c_budget(9)、p5c_hydration(11)、p5c_selection(8)、p5d_contract(1)、p5e_context_facets(2)、p5e_path_domain(6)、p7_partial_budget(4)。没有 broad cc-index/cc-server/workspace test suite，没有运行旧 post_index_worker_crosses_pages_and_reopen_reuses_artifacts、GC/WAL/kill/staging/EROFS、private42export 或 publicDEV/scale。

功能测试 baseline 与 fixed 使用 `/workspace/target-packing-baseline`、`/workspace/target-packing-fixed`，证据目录分开且写入拒绝覆盖。baseline 功能记录完成后，额外只读 JSON measurement helper 在已改源码工作区执行，使用早前 baseline target，仅读原始 saved JSON；它不是一次新 baseline 产品测试，产物日志单列 `measurement/baseline-saved-json.log`。fixed 功能测试始终独立 target，不共享 baseline 产品输出。

复跑：设置 `PATH=/workspace/.cargo/bin:$PATH RUSTUP_HOME=/workspace/.rustup CARGO_HOME=/workspace/.cargo`，`CARGO_TARGET_DIR` 指向新的独立目录，`P5E_PRIORITY_EVIDENCE` / `P5E_STAGE_EVIDENCE` 指向新的不可覆盖目录。依次执行：

```sh
cargo test --locked -j 2 -p cc-eval --test p5e_priority_pressure -- --nocapture
cargo test --locked -j 2 -p cc-eval --test packing_scope_v1 -- --nocapture
cargo test --locked -j 2 -p cc-eval --test diag_p5e_stage_receipts_20261003 -- diag_p5e_stage_receipts_same_microfixture --nocapture
cargo test --locked -j 2 -p cc-eval --test diag_p5e_stage_receipts_20261003 -- diag_p5e_measure_saved_receipts --ignored --nocapture
```

`summary.json` 是可读阶段/矩阵索引；`full-receipts.tar.gz` 保存 baseline/fixed 全阶段、合同 JSON 与完整日志，`manifest.json` 保存各原始文件 SHA256、固定 source 和工具入口。tasks.json 只追加 P5-019 的实施备注，再生成 TODO；不改变任务状态，不关闭 V19 或质量父项。等待独立验收，未 merge/deploy。
