# P2 find collection 完整性修复

独立 finding：`a17f05d674e82bc54aac6a664b2b69a87b276b36`，旧 product
`8d2b312c066f0514fb5cee555767955bf34708d0` / delivery
`892fd103f543424323e1d4e6691eca5774ede1a6`。

## 复现与最小修改

按原 commit delta 原样纳入独立 review 的 e010c9e/a17f05d；旧失败 tests/report/logs/manifest
在 `python-inventory-independent-20261004` 逐byte保留，没有改成旧树已通过。
新目录 `review_tests.rs` 与原 independent test 全文件逐byte相同，`run.py` 只把报告 source
字段改为实际 subject head/status；生成/测试/清理流程及 review_ filter 不变。

在未修产品上，新 runner 实际复现 12 passed / 1 failed / 0 ignored（exit101），两个
public API case 均准入；`baseline-tests.log` / `baseline-validation.json` 原样保存这次观测。
新增 public regression 在旧树也实测 0 passed / 1 failed（exit101），见
`baseline-public-regression.log`。日志仅去除末尾空行，断言/失败内容保留。

产品只改 `python_inventory.rs::supported_document`：find 一旦显式存在，必须自身是
object、只有 where，where 为非空至多32项的非空 string array，任一不完整项明确返回
`CaptureRefusal::Configuration`。另一个有效 package-dir 不能遮蔽缺失/空/default find。
normalized 单根、native directory 和所有 evidence 仍交原实现校验，无新 config parser。
legacy Python resolver/provenance、Loader、AST/model、qname/UID/extraction/search 无修改。

新增两项 public API tests；未弱化或修改旧断言：

- missing/empty where、scalar/object/非法 item/empty value、outside/multiple root、duplicate
  TOML key、selector、非法 find object 形状，各独立或配有效 package-dir 的 typed refusal；
  33项重复显式 root 仍超预算 refusal。
- standalone package-dir、find-only、同根 combined、重复明确 where values 均保留合法准入；
  raw config bytes、全部 directive/value（包括各 array index）和 model binding 逐项核验。
  重复显式值与重复 TOML key 不混淆，不丢失 supporting evidence。

## 当前 focused 结果

官方 Rust1.95.0、原 Cargo.lock、--locked、无新 dependency。实际执行 43 passed / 0 failed /
0 ignored：unchanged independent13 + public capture11 + 原组合6 + legacy provenance12 + drift1。
其中原红色 `review_contract_partial_find_must_refuse` 当前直接通过，不skip、不改断言。
实际临时 target 清理完成；source/log hashes 和 count 在 `checks.json`，independent实际 argv /
exit / subject bytes 在 `validation.json`（dirty test tree 的 source hashes，非旧已认证source）。

执行命令（env PATH=/workspace/.cargo/bin:$PATH，RUSTUP_HOME=/workspace/.rustup，
CARGO_HOME=/workspace/.cargo）：

```sh
python3 artifacts/checkpoints/python-inventory-partial-find-fix-20261004/run.py
cargo +1.95.0 test --locked -p cc-index --test python_inventory_capture --test python_identity_resource_integration
cargo +1.95.0 test --locked -p cc-index --lib python_provenance
cargo +1.95.0 test --locked -p cc-index --lib python_inventory_observed_midcapture_mutations_refuse
cargo +1.95.0 clippy --locked -p cc-index --lib --test python_inventory_capture -- -D warnings
cargo +1.95.0 fmt --all -- --check
```

上述 scoped lint/format通过；产品、docs、新证据 diff check 通过，原 review artifacts 中
raw output 的末尾空行按原样保留（不声称原 immutable evidence 无 whitespace finding）。

本次只修 P2，尚待同独审者 delta 接受。没有导入父 PR138 的 da896b5 guard/旧固定 manifest；
父 product50a493 的已审 source-union guard 不是当前新增 capture 的注册或认证。
后续接受/集成需要新显式 reviewed capture source version，本修复不抢先 claim 该版本。
无 broad/runtime/formal/publicDEV/100k/holdout/第三方代码执行或 merge/deploy。
OS atomicity、hostile mutation+restore、RSS/平台以及 persistence/cache/ingestion/publication/
DB/MCP/retrieval 边界保持原契约。
