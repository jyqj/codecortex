# 同缺陷独立复验：BOUNDED_PASS

本结论仅适用于固定修复 SHA **`671063b11af8cb40a0d526098de82e684dd24aca`**。
远端分支 `fix/type-atom-identifier-regression` 与该 SHA 一致。生产实现相对 `da5b05ee08d84fd336d5a98f25da7cae176daebf` 只改变 `helpers.rs::type_atoms`；此外有作者测试修改/新增，但作者 7/7、97/1 ignored、model 3 的计数没有用于独立证据。

先前 `da5b05ee08d84fd336d5a98f25da7cae176daebf` 的 **BOUNDED_REJECT** 和四类反例仍然有效，原报告和结果未改写。本次在新子目录追加固定新 SHA 的复验。

## 三版本重放

同一份独立 harness 对三个固定生产源码快照运行；所有用例使用新数据库和 `full=true`，实际扫描/解析数量均核验，不能以退出码掩盖空扫描。

| 类型/用例 | PR99 b2572345 | 拒绝版 da5b05ee | 修复版 671063b1 |
|---|---|---|---|
| `tuple[int, ...]`、`tuple[Packet, ...]` | 严格空-key 错误 | 成功 | 成功 |
| Python `_`、`__`、`℘`、`℮` 的 uses_type | 各 1 | 各 0 | 各 1，目标 UID 非空 |
| TypeScript `$` 的 uses_type | 1 | 0 | 1，目标 UID 非空 |
| Unicode continuation `a·b`、`Á` 的 uses_type | 各 1 | 各 1 | 各 1，目标 UID 非空 |
| 非 variadic、Unicode union、qualified controls | 成功 | 成功 | 成功 |

新修复版七个合法标识符用例均保留实际落库的 `name_bucket`。Python 名称经过 `ast.parse` 与 `str.isidentifier()` 独立核验。原四类反例重放和额外 Unicode 边界均保留在三套 `*-legal-identifiers/` 目录。

组合字符用例 `Á` 的 bucket 按既有协议为小写 `á`；harness 记录的 literal `name_dependency=false` 只表示没有大写字面 key，独立 verifier 直接核验实际 lowercase bucket 存在，并验证目标类型边 UID，未删除或改写原始观察。

还验证了六组 qualified/generic/union/Unicode/空白 atoms、七组 ASCII 语法标点、七组 resolution name-key 查询，以及十五组特殊标识符/未知 Unicode 拼写/组合类型边界。`...` 不产生 type atom 或落库依赖；`resolution_name_keys("...")` 保留非空原拼写，不产生空 leaf key。未知非 ASCII 拼写 `…`、`⚙` 依新实现的保守约定保留，不宣称完整标识符语法验证。

显式空 dependency 在构造、normalize 和序列化往返后仍被严格 validator 拒绝。`cc-model/src/resolution.rs`、`cc-db/src/resolution_dependency_store.rs` 在拒绝版与修复版之间 Git blob 完全相同，name-key 过滤和数据库读写严格校验没有回退。

## 构建身份核验

三版本分别使用本目录 `targets/old`、`targets/rejected`、`targets/repaired`，开始构建前均不存在。没有使用先前共享 `target` 的二进制。每套 `cargo build --offline --locked --message-format=json-render-diagnostics` 均成功，核心四个 crate 和 harness 的 compiler-artifact 均 `fresh=false`。

执行路径来自该次 cargo JSON 返回的 executable，并核验路径归属于对应独立 target。三套使用同一 harness 源码；执行前后核对 binary SHA256，实际核心 crate manifest_path 和 artifact 路径也检查为对应固定快照。全部六次运行返回 0。

修复版实际执行二进制 SHA256：
`e055251c2d9a17d2457176bea8b1283f3a7a834a6016a70c4ba718a35e3f38c3`。

固定生产 helper Git blob：`6d1f5c90aa27e573dcf2d885cee7fbeea96371c8`。
完整身份数据见 `build-execution-receipt.json`、三套 `build-*.jsonl`、`source-manifest.json`。

私有 helper 的直接 atoms controls 仍使用固定函数的机械提取，只调整可见性；端到端索引通过未修改的固定生产源码 crate 执行。源码快照、目标目录和数据库保留本地并忽略提交，提交可重建的独立 harness、脚本、输入和观察数据。

## 验证与边界

`python3 .../recheck-671063b/verify.py` 已返回 0，生成 `verdict.json`。它交叉核验三版本负例、边界、实际落库的类型边/依赖、严格校验、构建身份和未变的校验路径；完整本地复验需保留已生成的目标与数据库。

重建命令：

```bash
python3 artifacts/checkpoints/independent-requests-fix-20261003/recheck-671063b/setup.py
python3 artifacts/checkpoints/independent-requests-fix-20261003/recheck-671063b/run.py
python3 artifacts/checkpoints/independent-requests-fix-20261003/recheck-671063b/verify.py
```

重复重建需在全新副本执行；run.py 明确拒绝已存在的 target，以确保不会使用旧产物。

**BOUNDED_PASS** 只接受此固定 SHA 对同一 ellipsis/合法类型名缺陷的修复。原固定 Requests 20 文件/2416 deps 的身份、全语料索引与旧 cache 迁移由集成 session 负责，本次不重验，也不以这些 fresh fixtures 的通过代替迁移验收。

没有读取隔离 holdout/public-v19 内容、调用真实 provider 或修改生产、版本、ledger、gold。先前被拒绝的 draft PR 创建不重试、不换通道。仅允许本专属目录的本地提交和普通推送；完成后停止。
