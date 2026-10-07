# Query target V2：裸点分查询回到 ambiguous fallback

修复 independent review `56fd54e26f3f9c388fdcf0a15543b43dd5f26590` 的 P2。
生产基线 `c1aa6607aadbf27eb86ba63816982cf48ae277d0`；V2 控制先冻结在 `f8dd834`，
随后才修改生产。原始 public metrics 仍 **FAIL**；broad prose 未解决。

## 源码 delta 与语法边界

生产行为只有 `query_target.rs::member_name` 的一行 delta：纯 `::` split 不再
包含 dot split。现有 identifier 校验使普通 dot / 混合 dot:: 链落入 ambiguous。
不使用 extension denylist、filesystem 存在判断、capitalization 或语言/库词规则。
保留旧 name: 优先、whole-query kind/name、method NAME on CONTAINER、methods on，
保留纯 A::B::member 和严格单层 (*Receiver).member。后两者只识别最后名字，
仍不解析真实 qualifier 或限定 receiver，也不能区分同名不同 kind。

旧 `query-target-20261004/` **所有文件未修改**，包括其 28 条矩阵、模型和实际结果。
V2 保留每条旧 historical_v1_expected_model，并明确把 qualified-dot/qualified-field
从旧 named 期望改为本版本 fallback；独立测试记录这些 superseded 期望。
没有重写旧期望成成功，也没有把旧 broad-prose 失败宣称为修复。

## 两条 reviewer 反例的真实回放

reviewer fixture 从其 supplement 原字节引用，只读取本次 bounded 独立审查证据。
`reviewer/before/results.json` 在精确 c1aa660 生产源上重新编译/链接后执行，
`reviewer/after/results.json` 在 V2 源上执行真实 CodeIndex、in-process MCP wire 和
未经修改的 Rust normalizer/source verifier；不是独立 stdio 子进程测试。

| 查询 / hit | c1aa660 before | V2 after | 结果 |
| --- | ---: | ---: | --- |
| Beacon.spec.py / class Beacon | 0.4864227119347454 | 0.6664227119347454 | 恢复 +0.18；class 回到第一 |
| Beacon.spec.py / function py | 0.6562776873656182 | 0.6562776873656182 | 不变；不再抢第一 |
| Vessel.yaml / class Vessel 首片 | 0.419123464984373 | 0.599123464984373 | 恢复历史 class bonus |

两条查询的 engine/MCP 保留 raw hit、score_trace 与 reviewer independently archived
37dd042 baseline **完全一致**；before 也精确复现 reviewer c1aa660 反例数值。
所有共同 hit 的 non-name 分量不变，既有正确身份/正文保留。

45 条主矩阵及 11 条 reviewer 控制合计 56 条，均跑 before/after 两种 API。
33 条明确语法控制逐 hit 不变，覆盖直接 class/interface/type/callable、DSL、
同名不同 kind、split class fragments、methods-on、::、pointer receiver。
`class Reservoir` 的准确片段仍受 boost；broad prose 的 Lantern 仍第一。
`Lantern.Ignite`/`Lantern.Ready` 现在按 legacy tokens 处理；这是 V2 有意收紧语法，
不承诺它们仍排除 receiver bonus，也不制造未索引的 property。

`check.py` 检查 968 个 returned score traces，483 个共同 hit 的 identity/text/fused
score 和每个 non-name trace component 逐值一致。42 次 trace 改变只涉及既有 name
bonus；最大 trace 回放误差 `2.220446049250313e-16`，使用既有 1e-9 容差。
所有返回 evidence 经 fresh source verification。MCP 在 reviewer colon/mixed 控制的
两臂各少一个 retained hit（共四次），prefix 一致；explicit-kind-dot 的两臂还存在
metadata compaction 差异，normalized identity/source 和所有 raw score 字段一致。
完整差异保留在 comparison.json，不将 retained subset 描述为完整检索。

## 验证、绑定与复现

官方 Rust/cargo 1.95.0，默认 features，原始 Cargo.lock，cargo 命令 `--locked`。
六个 selected query_target 测试通过（其他 298 个过滤），fmt/diff checks 通过。
没有 DEV/gold/scorer/parser/ranking weights/budget/formal rerun 改动，没有 broad suite
或 excluded post_index runtime。没有修改 cc-model prototype 或中央 tasks 状态。
已阅读 CONTRIBUTING.md；仓库/workspace 没有 AGENTS.md 或本地 `.agents/skills`。
先前 draft API 已被 Forbidden 拒绝，本轮没有重试、没有 merge/deploy。

`receipts/` 保留 build/test/check 日志（仅去掉末尾空白行）；`binding.json` 绑定源码、
原 lock、frozen model、fixture、driver、binary hash 和证据。before 先构建 c1aa660
生产（freeze-only commit f8dd834），再编译 driver；随后才改源码并构建 after。

```sh
export PATH=/workspace/.cargo/bin:$PATH
export CARGO_HOME=/workspace/.cargo RUSTUP_HOME=/workspace/.rustup
cargo +1.95.0 build --locked -p cc-eval --lib
cargo +1.95.0 test --locked -p cc-search query_target --lib
# rustc 的本次实际 --extern rlib 参数见 binding.json；用对应版本 library 链接。
# driver 参数为 controls-root fresh-fixture-root fresh-output-root；两个目的必须不存在。
# 主矩阵 controls-root 为本目录，reviewer 矩阵为本目录/reviewer。
python3 artifacts/controls/query-target-v2-20261004/check.py
```

checker 还需正常 origin 提供的 reviewer commit git object，用其两条 archived baseline
作独立恢复检查。实际运行不使用 public queries/gold 或自定义 scorer。
