# P5E 独立核心回归诊断（固定 source / 相同微型 fixture）

结论：PR135 的两处失败是 **默认公开 search 的最终二次打包丢失 impl.py**。parser/index、retrieval、hydration、selection 均保有完整实现，第一次核心打包也保有。附加真实 `resolution_freshness` 后再次应用同一 16,000-byte cap，新增 qname 的序列化开销使 Fix/Refactor 的最后余量不足。PR136 `f4df9a8` 同样失败，尚未解决。所有产品源码及 PR135/136 owner 文件未修改；本包只新增 tests/report/evidence。

## 固定来源与结果

| 来源 | 固定 SHA | 冻结 P5E 两测试 | 阶段诊断 |
|---|---|---|---|
| base | `88f2cf099c8b81f3acef485fd5ac9b01c63ce790` | 2 passed，exit 0 | 1 passed |
| PR135 fixed | `591c246c6f57b230138dd3c4c6b0753d1d75c775` | 2 failed，exit 101 | 1 passed |
| PR136 product | `f4df9a83514e6ba901143547368ce4a3bee21940` | 同两处失败，exit 101 | 1 passed |

PR135 fixed 与 `4e3e5355d8e4c642574eb84bdf73b7373bf3c4a7` 的六个产品 crate/src 树无差异。PR136 与 docshead `dbeefb478a996b7dd093b3c2eddde571e2c68178` 的相同产品目录也无差异。均用 Rust 1.95.0、`--locked -j 2`、各自独立 `target-p5e-{base,current,pr136}`，没有共享编译输出。

三份冻结合同测试来自 PR135 fixed 的原始文件，**同两份 noise，未改变任何源码内容、caps、断言或排名**。base 原来的 fixture 可能不同，因此比较运行的是新增独立测试文件 `diag_p5e_frozen_contract_20261003.rs`；current 则直接运行原始 `p5e_priority_pressure`。current 的原始失败位置确为 line 135 和 line 305。PR136 使用同一冻结文件，失败也在同两行。current 的 numeric-width 和 unknown-label 实验被前置 impl 缺失阻断，不能声称它们已通过或已执行；base 两个实验完整通过，原始 JSON 留存。

## 逐阶段证据

七个 scoped 候选的实际排名在三个 source、三种 intent 上一致：support 第一段、api、impl、support 第二段、noise_0、noise_1、test。impl 是 rank 3，完整正文含 `return value + 731`。

- `impl-indexed-symbols.json`：真实数据库可读到 `repair_widget` 的 indexed symbol。真实 retrieval 输出中已有完整 impl source/document/proof；hydrator 没有删除它。
- `*-retrieval.json` / `*-hydrated.json`：七个候选均保有；无 source omission，scope/源码校验通过。
- `*-selected.json`：七个都入选，`limit_omitted=0`、`source_covered_omissions=0`、`unmet_facets=[]`。implementation facet anchor 是 **support 第一段**，不是 impl；impl 没有 `evidence_priority`。这在 base/current/PR136 完全相同。
- `*-prepack.json`：完整未打包 envelope，包括原始 lanes、完整 proof、cost、scope、selection、graph、源文本。
- `*-core-packed.json` 与 `*-real-core.json`：真实核心第一次 pack 后仍有 impl；Fix/Refactor retained 5、omitted 2。
- `*-dispatch-before-pack.json`：上述真实核心输出附加当前稳定 DB 的真实 resolution freshness。
- `*-dispatch-after-pack.json` 与 `*-public-search.json`：Fix/Refactor 在 current/PR136 只剩 4 hits、omitted 3；impl 被降成无 body 的 document reference，`packing.partial=true`。base retained 5、omitted 2，仍有 impl。Trace 三个 source 均 retained 5，仍有 impl。

阶段诊断使用公开 SearchEngine、EvidenceHydrator、selector、CodeIndex/MCP API。为了观测无公开 hook 的 prepack envelope，只在新测试文件复制 `assemble_context_once` 的组装逻辑，不改产品；三个固定 source 的原函数均为 9,606 bytes、SHA256 `ce02a7d10c319586f11307be957208767c51577e0076c9efc2fd1e4e5f681f84`。镜像输出与真实核心严格比较 hits（含 score_trace、rerank_score、text、proof/qname）、selection 和 omitted_hits；真实核心 + dispatch observation 重放的最终 hits/omitted_hits 再与真实 MCP 比较。全体保留 hit 都经过 normalizer 的磁盘源码/span/digest 校验。独立执行的 elapsed_us、随机 incarnation 和 cache 状态如实保留，不伪造逐字相等或延迟结论。

## 实际预算与字节数

fixture 属 Tiny：`max_output_chars=18000`、`token_budget=4000`，有效 cap 是 `min(18000,4000*4)=16000`。以下数字由同一 Rust/serde_json 序列化实际保存的数据测量，非 pretty JSON 文件大小或 Python 浮点重序列化。

| source / intent | prepack | core packed | dispatch before pack | dispatch after pack | 真实 MCP bytes / hits / omissions |
|---|---:|---:|---:|---:|---|
| base Fix | 39288 | 15857 | 16056 | 15881 | 15881 / 5 / 2 |
| base Refactor | 39333 | 15887 | 16086 | 15911 | 15911 / 5 / 2 |
| base Trace | 39198 | 15835 | 16034 | 15859 | 15859 / 5 / 2 |
| current Fix | 39638 | 15981 | 16180 | 15219 | 15219 / 4 / 3 |
| current Refactor | 39683 | 15836 | 16036 | 15074 | 15074 / 4 / 3 |
| current Trace | 39548 | 15955 | 16154 | 15979 | 15980 / 5 / 2 |
| PR136 Fix | 39637 | 15980 | 16179 | 15218 | 15218 / 4 / 3 |
| PR136 Refactor | 39682 | 15835 | 16034 | 15072 | 15072 / 4 / 3 |
| PR136 Trace | 39547 | 15954 | 16153 | 15978 | 15978 / 5 / 2 |

每次稳定 dispatch observation 增加约 199 bytes；表中 `real-core` 是独立执行，Refactor 比镜像 core 多 1 byte 属真实 elapsed_us 宽度变化。Trace final 与 public 差 1 byte 同理，hits/遗漏严格相同。

新增 qname 的精确字段开销（JSON 键、值、逗号）：support 第1段 37、impl 24、support 第2段 36、noise_0 25、noise_1 25、test 29，共 **176 bytes**；核心留下的 Fix/Refactor 五个 hits 上共 **126 bytes**。prepack 的重复节点 metadata 还重复这些字段，但产品 pack 先去掉重复节点；最终压力重点是 retained machine hits。

SQL receipt **没有新增 schema 字段**。真实 source identity 验证使 hydration SQL 从 base 的 statements/rows/vm_steps `1/7/425` 变为 current/PR136 的 `26/31/1203`。序列化该组计数只增加 **4 bytes**（1→26、7→31、425→1203、其余相同）。这是真实成本，必须保留；不是需要删除的假计数。cold originating retrieval 为 storage_reads 7、utf8_bytes 588；cache-hit 行的 cost 按 originating_work 保存，不能当作总 query SQL/总 I/O。

## 根因与最小修复候选（尚未实施）

`cc-server/src/engine.rs` 第一次 `budget::pack` 先消费了大部分余量；`handlers/context.rs::finalize_search_response` 随后附加 freshness，再 `pack_value`。后者先尝试已知 prose/重复 projection 压缩，再清空旧 optional references，最后从低优先级 body 删除。因为 support 已满足 broad implementation facet，impl 属低优先级 hit，删除顺序符合当前 selector/packer 的实现。

这不是 parser 漏索引、retrieval 漏召回、selection 排除 impl，也不是排名改变。**触发条件是 qname 增加字节后的 finalpack 边界压力；facet 的宽泛语义决定牺牲对象。** 现有 honest Partial 与 document reference 已正确表示 body omitted，但这不能满足原测试指定完整 impl 的业务门。

单独优先删除旧 references 不够：current Refactor 的 core 已无 references，附加 freshness 后仍有 16036 bytes（超过 36）；final 值删除 impl 后虽然留下较大空隙，那是完整 hit/完整 proof 必须整体保留的结果，不能通过裁 source/body/proof 假装 fitting。

建议 root 首先选择 **`cc-search/src/selection/budget.rs` 的已知说明字符串等价紧凑化**，在删除 body 前提供最终 metadata 的余量；不删除 qname、不省 source/document identity、不改真实 cost、不调 caps。候选是 selector 的已知 `source_support_scope` 与 source_freshness 的已知 `scope` 说明，投影为有文档定义的短 schema 标签，保留“两倍 top_k 窗口中的 literal cue、非全局唯一/非 exact identity”、“逐文件磁盘检查 + optimistic generation、非 atomic filesystem snapshot”等语义。未知 caller labels 必须原样保留。需要在最大合成数字宽度上量化余量，不能只瞄准本次 36-byte 差额。此处只是修复建议，没有修改产品或提交模拟绿灯输出。

若 root 选择改变 selector 的 target protection，需要单独定义：如何用已有可信 identity/字面 program cue 区分直接 repair implementation 与调用它的 support，且不改变 score/ranking、不从 fixture filename 或专用 magic text 推导优先级。当前 broad implementation facet 已履约，因此不能声称 selector 自身发生回归。

## 原测试的业务门与独立验收

原测试要求：同一微型项目在默认公开响应里保留 `repair_widget` 的完整实现、interface、Fix/Refactor test body，然后在既定更小 caps 下验证已被真正 selector 标记的 facets/support 比 incidental 优先。第一层是 **默认 API 的目标源码可消费性**，第二层是 **whole-source priority packing**。这不是“任意所有命中永远必须进默认响应”的承诺，也不是“缺任意低优先级 hit 就不允许 Partial”；不过当前两-noise 小 fixture 在 base 已满足明确指定目标的门，增加 identity 不能通过弱化该门掩盖回归。不得自行缩 noise、增预算或把 required body 换成 support body。

root 授权修复后的独立验收至少应包含：

1. 新 product SHA 独立 target，原两-noise冻结合同全部通过，默认 Fix/Refactor/Trace 含 impl/interface 和所需 test 完整正文；qname/document/source proof 留存且可核对。
2. 三 intent 的真实公开 MCP→原 16000/14000/12000/10000/8000/5000 cap 全矩阵；保留 ranking/text；omission 仍诚实 Partial，或原明确预算错误。下游 repack 不能复活先前丢失的 impl，因此必须先核对公开前置门。
3. 原短/宽 metadata（elapsed_us、index/evidence epochs、16-byte incarnation）及 unknown labels 测试，未知解释不被任意缩写，numeric cost 与原始 originating cost 完全一致。
4. 阶段诊断确保 core/final 相同源码证据可追踪；核对新修复改变的是预期 packing/serialization 边界而非 retrieval/gold/ranking。

## 文件、复跑与边界

- `summary.json`：完整阶段路径/priority/qname/packing/cap 结果及 cost，可机器阅读。
- `*-serialized-sizes.json`：每个原始 envelope 的 Rust 实际字节数与各顶层字段大小。
- `full-receipts.tar.gz`：三个固定 source 的全部阶段 JSON、public context/search、完整 proof/lanes/omissions/budgets、完整 cap 矩阵、原冻结合同产物和原始 targeted test logs。`manifest.json` 给出 SHA256 与固定源码。解压到新目录即可逐个检查。
- 新增测试：`diag_p5e_frozen_contract_20261003.rs` 是原合同逐字副本；`diag_p5e_stage_receipts_20261003.rs` 是独立 stage 记录器（默认不设置证据环境变量也能执行）。ignored helper 只对已保存 JSON 进行 Rust 字节测量，已在三个独立 target 上显式执行并通过。

复跑应在每个 source 的新 worktree 复制这两个新测试，以全新 evidence 目录分别运行下列精确入口（失败后的 stage 诊断要单独执行，不能让 cargo 在失败合同后跳过）：

```sh
CARGO_TARGET_DIR=/absolute/unique-target-for-this-source \
P5E_PRIORITY_EVIDENCE=/absolute/new-evidence/contract \
cargo test --locked -j 2 -p cc-eval --test diag_p5e_frozen_contract_20261003 -- --nocapture

CARGO_TARGET_DIR=/absolute/unique-target-for-this-source \
P5E_STAGE_EVIDENCE=/absolute/new-evidence/stages \
cargo test --locked -j 2 -p cc-eval --test diag_p5e_stage_receipts_20261003 -- diag_p5e_stage_receipts_same_microfixture --nocapture

CARGO_TARGET_DIR=/absolute/unique-target-for-this-source \
P5E_STAGE_EVIDENCE=/absolute/new-evidence/stages \
cargo test --locked -j 2 -p cc-eval --test diag_p5e_stage_receipts_20261003 -- diag_p5e_measure_saved_receipts --ignored --nocapture
```

本环境额外设置 `RUSTUP_HOME=/workspace/.rustup`、`CARGO_HOME=/workspace/.cargo`、PATH 指向已有工具链；没有改 HOME。原始 evidence 有不可覆盖断言，复跑必须使用新目录。没有运行 broad suites、旧 `post_index_worker_crosses_pages_and_reopen_reuses_artifacts`、GC/WAL/kill/staging 压力、private localdiag/42export 或 publicDEV gold/规模读取与运行。未使用 gh auth probe；ordinary origin fetch 成功。GitHub PR metadata 的只读 GraphQL 请求返回 `Forbidden`，因此停止后续 PR API 操作，不绕路；尚未创建 draft。产品修复等待 root 指定具体切面，未 merge/deploy。

最终提交文件的 targeted clippy（只两个新测试 target，`-D warnings`）通过；rustfmt 与 diff whitespace 检查通过。最终 stage test 在不设置证据环境变量时也通过，避免把记录器配置当作 CI 必需条件。相关日志附在本目录。
