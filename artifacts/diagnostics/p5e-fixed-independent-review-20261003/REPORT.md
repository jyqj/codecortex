# 固定 packing scope-v1 修复的独立续审

**建议 root 给予 bounded scoped-search packing 修复范围内的接受结论。** 未发现阻断该切面的新问题：原冻结两-noise / 固定 caps 合同 2/2 通过，独立边界 3/3 通过，独立真实阶段复验 1/1 通过。未知标签、真实 Partial、qname/source/document identity、成本和排序保持原语义；legacy 文件收据未变成 generation 证明。

该结论仅覆盖既定微型 fixture、默认 **`path_prefix=scope` 的 search** 以及记录的预算边界，不推广为无 path-prefix context 的指定文件保证、任意所有数字同时最大位宽保证，或 publicDEV/规模/整体质量父项验收。没有 merge/deploy。

## 固定源、范围和独立性

- 产品 source：`90858afae647a513537bf118932a7ba5020ee98b`。
- 实测 checkout：`156e3ac13ddc6aa2b0208c8805c8c79bd2a0a38d`；与产品 source 的八个 crate/src 目录完全相同。
- 变更起点：最终 C++ fix head `49e0330754e6451e7eb5e91863f432ce7eaf816c`；未覆盖该 parser 修复。
- 独立 target：`/workspace/target-p5e-independent-fixed`；未复用之前 base/current/PR136 或作者编译 target。
- Rust 1.95.0；`--locked -j 2`。本次固定 tree 未找到 AGENTS.md/.skills，已重读 CONTRIBUTING.md，按委派范围使用精确入口。
- 新测试 prefix：`diag_p5e_fix_review_boundary_20261003.rs`、`diag_p5e_fix_review_stage_20261003.rs`；后者逐字复制此前独审 stage recorder，SHA256 `7a5ce9855106cb7f10f713ea64127db5a609006e329979dab0db5abd2b3a4aea`。未修改任何作者/产品/原测试文件。
- `p5e_priority_pressure.rs` 与原 current `591c246...` 及 C++ fix `49e033...` 的文件 SHA256 都是 `33b1b67d09c054ff1c22a1099d653d9258b96c142a2777e2e35f47556439d0e3`。原两份 noise、源码、gold、断言、caps 均没有改变。

此前 base 2/2、current 0/2、PR136 0/2 的失败/成功记录与原报告保持不可变，本轮没有重跑或改写。新档案在独立 prefix 追加；summary/manifest 保存旧证据哈希与关联提交 `cffdbf6bb5b04569d63720f07a2fe645a9fbe31a`。

## 标签语义与静态切面

核对 `49e033..90858af` 的完整产品差分：`budget.rs` 只增加精确映射，三个 producer 抽取原始完整说明为常量，说明值与运行逻辑不变。selector 的 owner/signature、literal cue、两倍 top_k 窗口、scope、priority 资格及排序逻辑没有改变；SourceVerifier/Hydrator 的源校验与 generation 检查没有改变。预算配置没有改变。

| 实际 producer 字段 | 紧凑标签 | 核对后的语义边界 |
| --- | --- | --- |
| `selection.source_support_scope` | `cue_window:v1` | 仅已验证 `2*top_k` 窗口内的字面 cue。窗口中的单文件证据不等于全仓唯一或 exact identity；原 owner/signature/query-token 资格仍适用。 |
| Hydrator `source_freshness.scope` | `disk_generation:v1` | 有界逐文件磁盘校验与 optimistic full read generation 检查；不是 atomic filesystem snapshot，不保证文件校验后不会变化。 |
| SourceVerifier `source_freshness.scope` | `disk_files:v1` | 有界逐文件磁盘校验；不是 atomic filesystem/whole-query snapshot，也不声称 full read generation。 |

`QUERY_EXECUTION.md` 的表明确定义了上述限制，且解释标签不增强证明。代码只访问三个确切 JSON pointer，并仅在 `as_str()==Some(known)` 时替换；不做 trim、case folding、前缀匹配或类型 coercion。该段位于预算压缩分支，未在无压力的早退分支执行。两个 freshness 常量不同，第二轮替换不会把第一次的短标签再次映射。

逐字段 UTF-8 说明节约量：cue 96→13（83 bytes）；generation 105→18（87 bytes）；legacy files 84→13（71 bytes）。因此此微型现代 generation envelope 共减少 170 bytes 的说明开销，而 qname/proof/cost 不减少。

## 真正默认公开 scoped search 与真实阶段

独立边界测试调用真实 MCP，未提供 token/output budget 覆盖。Tiny 默认仍为 token_budget 4000、configured_max_bytes 18000、有效 cap 16000。

| intent | 独立默认门 public bytes | hits / omitted | 完整 impl/API | 完整 test |
| --- | ---: | --- | --- | --- |
| Fix | 15839 | 5 / 2 | 是 | 是 |
| Refactor | 15868 | 5 / 2 | 是 | 是 |
| Trace | 15987 | 5 / 2 | 是 | 原合同不要求 |

所有 retained source 的路径/span/digest 都通过 normalizer 与磁盘校验；impl 的 qname 为 `repair_widget`。source/text/document/proof/score_trace/rerank_score 与重打包前逐字段相等，保留顺序不变。omitted=2 的响应仍为真实 Partial，未宣称完整。

独立 stage recorder 则在另一真实 fixture/session 取得：

| intent | prepack | core packed | dispatch before | dispatch after | public bytes / hits / omitted |
| --- | ---: | ---: | ---: | ---: | --- |
| Fix | 39638 | 15961 | 16160 | 15835 | 15835 / 5 / 2 |
| Refactor | 39683 | 15841 | 16040 | 15865 | 15865 / 5 / 2 |
| Trace | 39548 | 15967 | 16166 | 15984 | 15984 / 5 / 2 |

prepack→core→真实 public 全程有 impl；原缺口发生的 finalpack 现在门闭合。stage 镜像与真实 core 严格匹配 hits/selection/omitted_hits，附加实际稳定 DB freshness 的 final replay 与真实 MCP 匹配 hits/omitted_hits。数字由 Rust/serde_json 对保存的 JSON 测量。不同独立 session 的 elapsed_us/incarnation 字节差异如实保留，不能用字节微差推测功能或成本差异。

另对原 current 阶段记录与新 fixed 的七个 hydrated candidates 比较：三 intent 的 path/chunk_id/text/score_trace/rerank_score/qname/document/source_evidence/source_freshness 投影完全相同。没有用更好的 ranking/source 替换来掩盖 pressure 回归。

原完整 cap 矩阵在冻结测试中实际执行通过；独立 stage 再保留各 cap 输出：

| intent | 16000 | 14000 | 12000 | 10000 | 8000 | 5000 |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| Fix | 15835 | 13864 | 11860 | 9964 | 7972 | 明确 metadata budget error |
| Refactor | 15865 | 13894 | 11890 | 9994 | 7986 | 同上 |
| Trace | 15984 | 13875 | 11966 | 9926 | 7934 | 同上 |

各成功项均为 Partial。hits 从 5→3→2→1→0，omitted_hits 从 2→4→5→6→7，8k 同时诚实记录 graph node omission。原声明 caps 未改变。

## 独立负控与实际 producer 边界

新三测试覆盖以下有意义的独立切面，没有重跑作者的 packing_scope_v1/search298/bounded41 suites：

1. 三种 domain 各 13 个样例，共 39 个：精确 known、caller prefix/suffix、前后空格、换行、大写、NBSP、已紧凑值、null/array/object、另一个 domain 的 known string 放在错误 pointer。只有正确 pointer 的精确字符串映射；近似值/非字符串/错误域值逐字保持。另一个未知 pointer 下相同 known 描述也保留。每项都有实际预算压力、源校验、真实成本/状态保持及重复 pack 全 JSON 相等断言。
2. 原始公开响应以 16000 重打包后再 16000：全部 JSON 相等。每个 intent 进一步经历 14000→8000→16000→18000→14000→16000；所有有效 cap 均不超过原默认 16000（token_budget 保持 4000），高 cap 不复活此前 hits，遗漏计数单调且 Partial 不消失，原 qname/proof/cost/rank 保持。
3. 用真实 indexed fixture 的 SourceVerifier 对 retained hits 校验，产生实际 legacy diagnostics；用真实 EvidenceHydrator、原默认 query deadline 与实际 ReadGeneration 产生 generation diagnostics。两者分别只映射为 disk_files / disk_generation。legacy 输出仍没有 generation/hydrator 字段，generation 对象逐字保持，top-level resolution observation 未改变。再让真实 legacy verifier检查一个未索引路径，得到实际 omitted_files / partial=true；压缩后完整遗漏事实与所有 numeric counts 原样保留。这是 **真实 producer diagnostics 的序列化边界实验**，不是声称新增或调用了 legacy-only public MCP 路由。

负控输入/输出全部保存，没有把错误值塞入 gold 或修改产品身份验证来讨绿。

## 合成位宽与 context 限制

独立复验了指定 presentation stress：complete lane elapsed_us、source generation index/evidence epochs、resolution 内既有数字 epoch 字段为 u64::MAX，incarnation 为 16 个 255；恢复完整 known policy 说明以触发压缩。impl 仍有完整正文、qname、source/document proof；cost counters 未改变，所有注入字段和 lane receipt 保留，第二 pack 完全相等。**实测 15962 bytes，余 38 bytes。**

这只证明指定 timing/epoch/incarnation 字段组合，不证明所有计数任意同时 20 位、任意 caller metadata 或所有 fixture 都能保留所有正文。没有扩大 numeric-width 宣称；full receipts 显式标记 synthetic，不当作测量世代/成本。

实际无 path-prefix 的 public context 另行记录：Fix/Refactor 的 hits 含 `outside/forbidden.py`，不含 `scope/impl.py`；Trace 同时含 outside 与 impl。它们仍是有效无硬 scope 的真实源码结果。不能把 scoped search 门通过转化为 unscoped context 都必含指定 scope impl 的承诺。此前 baseline/current 的对应路径列表与本次相同，修复没有改变检索/选择范围。

## 验证、证据和交接

- 原冻结 `p5e_priority_pressure`：2 passed，exit 0。
- 新独立 boundary：3 passed，exit 0。
- 新 prefix stage：1 passed，exit 0；JSON measurement helper 显式执行 1 passed。
- 仅两个新 test targets 的 clippy：exit 0，`-D warnings`。
- 新测试 rustfmt / staged whitespace 检查通过。

`summary.json` 给出阶段/完整 cap/真实 producer/旧证据关联；`serialized-sizes.json` 是 Rust 大小账；`full-receipts.tar.gz` 保存原合同新输出、独立阶段完整 proof/lanes/omissions/cost/budgets、三测试全部正负控输入输出与原始最终日志；`manifest.json` 给出 source 和 SHA256。

精确复跑入口为原合同、新两个 test targets 和显式 ignored measurement helper，证据目录必须新建且不可覆盖：

```sh
CARGO_TARGET_DIR=/absolute/new-fixed-target P5E_PRIORITY_EVIDENCE=/absolute/new-review/contract \
cargo test --locked -j 2 -p cc-eval --test p5e_priority_pressure -- --nocapture
CARGO_TARGET_DIR=/absolute/new-fixed-target P5E_FIX_REVIEW_EVIDENCE=/absolute/new-review/boundary \
cargo test --locked -j 2 -p cc-eval --test diag_p5e_fix_review_boundary_20261003 -- --nocapture
CARGO_TARGET_DIR=/absolute/new-fixed-target P5E_STAGE_EVIDENCE=/absolute/new-review/stages \
cargo test --locked -j 2 -p cc-eval --test diag_p5e_fix_review_stage_20261003 -- diag_p5e_stage_receipts_same_microfixture --nocapture
CARGO_TARGET_DIR=/absolute/new-fixed-target P5E_STAGE_EVIDENCE=/absolute/new-review/stages \
cargo test --locked -j 2 -p cc-eval --test diag_p5e_fix_review_stage_20261003 -- diag_p5e_measure_saved_receipts --ignored --nocapture
```

本环境使用已有 `/workspace/.cargo` / `.rustup`，未修改 HOME。无 broad suites、publicDEV gold/规模、旧拒测试或 GC/WAL/kill/staging/EROFS/private42export 工作。只新增本 prefix tests/report/evidence，通过 normal origin push 交接；未重试已 Forbidden 的 PR API，未 merge/deploy。
