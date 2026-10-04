# P2 修复 delta 的独立复核

结论：**PASS（仅就本次有界 P2 修复）**，精确生产源
`54b2b92cfca035ef1de2b4d6478f314231fd5511`。没有发现本次修复新增的真实缺陷。
原审查 `56fd54e26f3f9c388fdcf0a15543b43dd5f26590` 的文件名误识别已消除。
原始 public metrics 仍 **FAIL**，broad prose 回归仍未解决；未进行 public rerun。

已完整阅读 `artifacts/controls/query-target-v2-20261004/README.md`。
独立核对 V2 model/fixture 的 frozen.sha256 及每个 byte 与 `f8dd834` 一致，且该
freeze commit 的生产 crates/Cargo.lock 与 c1aa660 相同，确认先冻结后修复。
生产 diff 仅 query_target.rs，运行时仅将 dot/:: 混合 split 改成纯 :: split；
无 extension、大小写、语言词、filesystem 存在判断或其他启发式规则。

## 独立真实回放

复用本审查自己编写的三语言 fixture 和原 driver；本目录新冻结 70 条 V2 模型，
保存 historical_review_expected，未修改或覆盖旧矩阵、旧结果及旧 P2 报告。
在精确 fixed-source detached worktree 构建实际 cc-eval/cc-server/cc-search，再把
原 reviewer driver 链接到此次构建，70 条均经真实 CodeIndex + in-process MCP
wire + 未改动 Rust normalizer/source verifier。不是 stdio 子进程测试。

| 原 P2 反例 | 固定后独立结果 | 与原始 37dd042 基线 |
| --- | --- | --- |
| Beacon.spec.py / class Beacon | 0.6664227119347454，第一名 | 两 API 的全部 retained raw hits/trace 逐值一致 |
| Beacon.spec.py / function py | 0.6562776873656182，第二名 | 不变，保留历史 token bonus |
| Vessel.yaml / class Vessel 首片 | 0.599123464984373 | 两 API 的全部 retained raw hits/trace 逐值一致 |

所有 bare-dot、mixed-dot/::、module-like-dot 都是 legacy fallback，包括未知后缀。
纯 `lights::Vessel` / `lights::Vessel::pulse` 及严格 `(*BeaconGo).pulse` 继续 final-name
hint；不是 receiver constraint。显式 name: 优先、kind/name、method NAME on CONTAINER、
methods on 语法不变，29 条 supported-hint 控制结果与 c1aa660 逐 hit 一致。
41 条 fallback 控制与原基线的共同 retained 前缀逐 hit 一致（含全部 malformed、
ambiguous prose、context augmentation 控制）。neargrammar/direct type/direct callable/
同名不同 kind、语言中立 parser kind、split type、Unicode/case、DSL、exact-tier 均复核。

102 个共同 exact-target hit 保留身份层及**基线的历史 bonus eligibility**。
`Café.résumé` 的 résumé 在 c1aa660 获得的 Named bonus 是 V1 新语法授予，并非基线
历史 token bonus；V2 fallback 后它可失去这部分 bonus，但 exact-target 身份保留，
与原基线逐 hit 一致。不能错误地要求 V1 所有新增 bonus 也受到豁免。
独立 syntax probe 对本目录 70 条预先声明模型全部通过。

`check.py` 校验 684 个 fresh-source-verified returned hit 与 score trace；678 个
c1aa660/fixed 共同 hit 的正文、身份、fused score 和每个 non-name 分量完全不变；
19 次 trace 改变只影响既有 name bonus，最大误差 `1.1102230246251565e-16`。
224 次非-exempt hint 资格检查通过；source/score/reasons consistency 同时检查。
两 API 有 10 条 fixed control 的 packing hit 数差异，保留前缀一致。与原基线对比时
`qualified-mixed` 的 engine 固定版保留 6 个、历史 5 个；共同前缀逐值相同，MCP 都
保留 5 个。记录此 retained-count 差异，不声称整行完全相同或验证未返回候选的完整性。
所有详细结果见 results.json / comparison.json。

## 验证及执行界限

官方 Rust/cargo 1.95.0、默认 features、原始 Cargo.lock、--locked；6 个选定
query_target 测试通过，298 个其他测试过滤；fixed-source fmt 检查通过。
两次 review driver 初始链接因 cached artifact / host serde_json variant 不匹配而失败，
失败日志完整保留。最终使用 cargo --message-format=json 的 target-profile 精确
artifact，成功链接并执行；binding.json 绑定 fixed 源 hash、lock、driver、binary 和 argv。
这不是切换工具链或绕开失败测试。syntax observer 的 unused helper 警告单独留档。

生产及原作者 controls 未改；旧 broad-prose Lantern 第一和缺少 Ready property chunk
结果原样保留。未调整 weights、budgets、parser、normalizer、gold、scorer；未执行
DEV/holdout/100k、广泛套件、post_index runtime、private42、GC/WAL faults。
未 merge/deploy，也未重试先前 Forbidden 的 draft/API 操作。

```sh
export PATH=/workspace/.cargo/bin:$PATH
export CARGO_HOME=/workspace/.cargo RUSTUP_HOME=/workspace/.rustup
# /tmp/query-review-fixed 为精确 54b2b92 detached worktree。
cargo +1.95.0 build --locked -p cc-eval --lib --message-format=json
# 在 fixed worktree 编译，按 binding.json 链接原 reviewer driver。
# 不要按 mtime 选择 rlib；不要选择 host-only serde_json build dependency。
/tmp/query-review-v2-driver artifacts/controls/query-target-independent-20261004/delta-v2 \
  /tmp/new-v2-review-fixture /tmp/new-v2-review-output
python3 artifacts/controls/query-target-independent-20261004/delta-v2/check.py
```

本目录的独立报告只是原 P2 的固定源复核，不将原 56fd54e 的历史 REQUEST CHANGES
改写成当时已成功，也不将修复解释为整体 public 搜索质量通过。
