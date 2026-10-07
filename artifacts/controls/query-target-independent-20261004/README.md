# Query target 独立有界审查

结论：**REQUEST CHANGES（一个 P2 文件名误识别）**。审查对象为
`c1aa6607aadbf27eb86ba63816982cf48ae277d0`，基线为
`37dd042eaa1209a86e0cafdcd92ae77e036e76f5`；原作者控制集在
`443ca67a45aec1b15939f746c082d1f6d5bf57f2` 冻结，早于生产改动。
原始 public metrics 仍为 **FAIL**。本审查没有修复或重新认证 broad prose Gin 回归。

## P2：点分文件名被当成 member hint

位置：`crates/cc-search/src/query_target.rs:105-106`，`member_name` 对所有
`::`/`.` 分隔的合法 identifier 链取最后一段，没有区分文件名。
`Beacon.spec.py` 被解析为 `named:py:any`；`Vessel.yaml` 被解析为
`named:yaml:any`。这并非要求语义 receiver 解析，而是 whole-query 词法门控
把常见文件名拼写当成了明确 member 目标。

新增独立 Python fixture 同时有 class `Beacon` 和 function `py`：

| 查询 / hit | 修改前 score | 修改后 score | 实际效果 |
| --- | ---: | ---: | --- |
| `Beacon.spec.py` / class Beacon | 0.6664227119347454 | 0.4864227119347454 | 唯一变化为移除 +0.18，第一名变为无关的 function py |
| `Beacon.spec.py` / function py | 0.6562776873656182 | 0.6562776873656182 | 保留 bonus，成为第一名 |
| `Vessel.yaml` / class Vessel 首片 | 0.599123464984373 | 0.419123464984373 | 无 exact-target 豁免，准确 type 片段失去 bonus |

engine 与实际 in-process MCP wire 的保留前缀一致；所有返回证据均由未经改动的
Rust normalizer 做 fresh source verification。完整结果在 `supplement/`，语法观察
在 `syntax-observations.json`。这两个反例没有依赖 public queries 或 scorer。
已有文件查询 `beacon.py` / `vessel.rs` 则命中独立 exact-target tier，保留历史
bonus，所以仅测试真实存在的文件名会掩盖这个问题。

建议维护者在后续实现里明确文件名和 qualified grammar 的词法边界，或收紧并准确
记录接受范围；本审查未改实现。`lights.Beacon` 与模块/type 限定本身存在词法歧义，
不将缺少语义解析另列缺陷。未知 qualifier 和混合 `lights::Vessel.pulse` 均只是
final-name hint，不是硬 receiver 约束；README 对此描述准确。

## 其他审查结果

- 新增 64 条控制及随后单独冻结的 6 条补充控制，使用自行编写的 Go/Python/Rust
  fixture。每条均在冻结生产基线及实现版本执行真实 CodeIndex build/search 和
  in-process MCP duplex JSON-RPC；不将此称为独立 stdio 子进程测试。
- `class Beacon`、`method pulse`、`function pulse`、`class pulse` 验证直接类型、
  callable 及同名不同 kind，且各自第一名符合目标。Go struct / Rust struct 为
  class，trait/interface 为 interface，alias 为 type_alias，impl/receiver 为 method；
  语法 kind 和实际 parser 输出相符。Rust Vessel 的多个准确片段都保留 class bonus。
- `name:` 优先于剩余 prose / qualified hint；`kind:` 仍是既有硬过滤。
  重复 name 最后一个生效。quoted DSL、相邻 token、未闭合引号接受行为来自原 DSL，
  未发现新修改引入的 quote/filter 解析问题。相互矛盾的 `kind:class method pulse`
  不给 class pulse 新 bonus，符合 hint 及硬过滤同时存在的设计。
- ASCII 大小写、预组合 Unicode 标识符、İ 的 lowercase expansion 均被观察。
  combining mark / decomposed spelling 落入历史 fallback，未引入 NFC/NFKC 或新的
  Unicode equivalence。保守 grammar 不覆盖所有语言合法标识符，这是现有边界。
- 23 条 neargrammar 控制覆盖末尾 prose、调用括号、泛型、引号、空段、混合错误标点、
  pointer 多段、空白、arrow、qualified kind-name、qualified on-container、零宽字符。
  未将这些不完整语法升级为明确目标；其中 `class _` 是 grammar 合法但 fixture 无匹配。
- primary DSL 在 augmentation 之前解析。四条真实 conversation 控制表明明确
  `method pulse` 不接受 context 的 Beacon bonus；不完整 `method` 不被 context pulse
  补全为明确目标。模糊 query 继续使用 augmented legacy tokens，context 可给 Beacon
  bonus，这是明确保留的历史行为。任务入口静态检查：task_symbols fallback 和 MCP
  context fallback 把原 task 原样传入 search，不另造 target；本次没有单独执行 context
  工具的 direct-symbol 路径，不将该路径描述为动态验证。
- exact-target identity 排序层未变。102 个共同 exact-tier hit 检查保留原 bonus eligibility；
  新 Named hint 可新增 bonus，不被错误当作与 before 逐字相同。官方选定 seam 测试同时
  覆盖构造的 kind/receiver 不一致 exact_identity 豁免及优先排序。

## 验证证据与界限

`check.py` 校验：70 个 before/after 对照、1365 个 source-verified hit 和 score trace、
662 个共同 hit 的身份/正文/所有非 name 分量保持不变、86 次仅 bonus 改变、30 条
fallback 结果逐 hit 一致、261 次非 exempt 的门控资格检查。trace 最大误差
`1.1102230246251565e-16`，遵守产品的 `1e-9 * max(1, abs(score))` 容差。
MCP 有 19 次 arm/control 少保留一个 hit 的 packing 差异，保留前缀一致，详情见
`comparison.json`。保留子集不能证明未返回 candidate 的完整性。

原作者 `query-target-20261004/` 整体不变；冻结构件逐 byte 对照冻结 commit。
旧 `ambiguous-prose` 在两版本逐 hit 相同且 Lantern 仍居首；`qualified-field` 仍没有
Ready property chunk。未凭这个 gate 宣称 broad prose 已修复或制造缺失 chunk。

官方 rustc/cargo **1.95.0**，默认 features、原始 Cargo.lock、全部 cargo 命令带
`--locked`。before 在独立 detached worktree `443ca67` 编译（crates 和 Cargo.lock 与
base 相同），after 在精确实现 commit 编译，Rust driver 用实际 rlib 链接。
`binding.json` 记录 compiler argv、commit 和 lock digest；`receipts/` 保留 build、
run、toolchain、check、selected tests、format 日志。选定 query_target 测试 4/4 通过，
298 个其他测试 filtered out；未运行旧 post_index runtime、广泛 suite、private42、
GC/WAL fault、formal DEV/holdout/100k 或 public rerun。

production base→implementation 仅触及 query_target 模块、plan 字段/门控、模块注册和
选择 seam 测试。无 weights、budget、parser、gold、normalizer、scorer、Cargo.lock 或
public-query tuning 改动。本审查只新增本目录，不修改 production 或原控制产物。
仓库/workspace 没有 AGENTS.md 或本地 `.agents/skills`；已阅读 CONTRIBUTING.md。
Rustup 初始默认 home 为只读而失败，后续均使用已有 `/workspace/.rustup`，未换工具链。

## 重现

```sh
export PATH=/workspace/.cargo/bin:$PATH
export CARGO_HOME=/workspace/.cargo RUSTUP_HOME=/workspace/.rustup
cargo +1.95.0 build --locked -p cc-eval --lib
# driver.rs 需要 cc_eval、cc_server、cc_model、serde_json 的本次实际 rlib。
# 参考 binding.json 的 --extern argv；在别的构建上不要假设 hash 文件名不变。
# 编译后，运行时两个目的目录必须不存在：
/tmp/query-review-after-driver artifacts/controls/query-target-independent-20261004 \
  /tmp/new-query-review-fixture /tmp/new-query-review-results
# supplement 同理单独运行。before 必须用基线库重新链接，不能拿 after binary 代替。
cargo +1.95.0 test --locked -p cc-search query_target --lib
python3 artifacts/controls/query-target-independent-20261004/check.py
```

语法 probe 直接包含未经改动的 dsl.rs/query_target.rs，仅观察 recognizer，不能替代
engine/MCP controls。其 unused DSL helper 警告已留在 log，不是 production lint 结论。
未 merge、deploy 或调整 implementation。
