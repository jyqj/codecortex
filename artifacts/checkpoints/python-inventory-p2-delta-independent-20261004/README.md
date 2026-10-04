# Python inventory P2 独立 delta 复核

结论：**接受固定 source `e3b932ed4b1e197022c0902fd4c11af3e87ae87e` 的 P2 修复**。在该配置准入 delta 内没有发现新缺陷。该结论不是完整 capture certification、release、生产 DB/MCP 输出或父 CI registry 认证。

原 finding/review SHA：`a17f05d674e82bc54aac6a664b2b69a87b276b36`；原 reviewed source：`8d2b312c066f0514fb5cee555767955bf34708d0`。历史 `artifacts/checkpoints/python-inventory-independent-20261004/` 的红色 report、tests、logs、manifest、publication receipt **逐 byte 保留**，未改写为旧 source 已通过。当前新绿色证据独立存放在本目录。

## 修改核验

正常 origin fetch 得到上述固定 source，并从该精确 commit 建立独立 review worktree。重读完整 capture 契约、实际 `supported_document` delta、新增 public tests、修复 README/CONTRIBUTING。没有 AGENTS.md 或 `.agents/.codex` 指令。沿用此前已读取的 Agent Architecture skill 中证据/声明分离原则，无子 agent。

相对原 product，crate/Cargo.lock 变化仅：

- `python_inventory.rs`：**只改 opt-in `supported_document`**。每个出现的 find 要独立满足 object + 仅 where + where 为非空、最多32项 array + 每项非空 string。package-dir 无法再掩盖缺失/空/非法 find。unsupported key 继续拒绝。
- `tests/python_inventory_capture.rs`：新增两项 public regression/control tests，原测试断言未改变。

其余 capture 前缀/后缀、native walk/verify、Loader、legacy `python::build`/provenance/resolver、AST/model、extraction/search/UID/qname 和 Cargo.lock 全部不变。没有增加另一 TOML parser。root lexical normalization、单根一致性、目录实际存在、全部原始 evidence 仍交旧 provenance/bridge 校验。缺失 where/empty where 在 opt-in 层前置拒绝，未改变旧默认政策。相关 exact Git diff、hash 与 prefix/suffix byte equality 在 `static-review.json`。

## 独立实际复现

官方 Rust `1.95.0 (59807616e 2026-04-14)`、原 Cargo.lock、`--locked`，无新 dependency。独立执行 **38 passed / 0 failed / 0 ignored**，不是照录作者所报告的43：

- 原13项 independent tests 全文件逐 byte 不变 + 新2项独立 public API controls：15 passed；原 `review_contract_partial_find_must_refuse` 不 skip、不改断言，两个 exact counterexample 均真实 public API 返回 `Configuration`。
- 实际 product `python_inventory_capture`：11 passed，包括作者新增的两项 exactsource public tests。
- 既有 scoped `python_provenance` library tests：12 passed（391其他测试 filtered），含 legacy resolution 忽略 additive evidence 的原检查。

新自编真实临时文件系统 public controls（未执行任何 captured Python）：

- standalone package-dir、find-only、同根 package-dir + 多项 where、重复明确 where value 均成功；核验原 config bytes、每项 directive（包括 array index）/value、完整 evidence JSON 与 config digest model binding。
- missing/empty where、scalar/object where、非法/空元素、multiple实际存在 root、duplicate TOML key、unsupported selector、非 object find、outside/absent root、33项 repeated where，分别单独或配有效 package-dir 均 `Configuration` refusal。重复明确 root values 和重复 TOML key 不混淆。
- unchanged13 继续实际运行原 capture count/bytes/depth/path/output exact-one-over、AST/model 限制、aliases、scope/marker/collision、十二种 mutation/mixed metadata；不扩大解释其证明能力。

复现命令：

```sh
python3 artifacts/checkpoints/python-inventory-p2-delta-independent-20261004/run.py
```

该 runner 逐 byte 复制 subject 原源码前缀，追加独立 test modules，引用原 Loader/provenance 等支持源码；结束清理临时 tests target。只运行 `review_` filter，3项其他生成 module tests filtered。原13 fixture 文件在新目录按原样复制（hash相同），额外 `delta_tests.rs` 仅增加独立 public cases。完整 argv/exit/subject/fixture/Cargo.lock SHA256：`validation.json`。原13与新2实际日志：`tests.log`；产品/legacy命令与exit：`focused-commands.json`、`product-capture.log`、`legacy-provenance.log`；新 Rust fixture format check 与toolchain在 `static-review.json`。新日志仅去除末尾空行；历史红色日志原样保留。

## 结论边界与交付

P2 在这个固定 source 已关闭；原 source 的红色历史结论依旧有效。没有导入或改写父 CI current registry/旧认证 manifest；没有建立新的 persistence、versioned ingestion、publication、DB/MCP/retrieval 输出协议。

仍仅 opt-in/Linux，真实 mount拒绝分支未测试；没有 bind mount/系统安全更改、hostile writer atomicity、RSS硬限制或hard deadline声明。最终 verify 不是持续锁。EntireProject 的 `.git`/cache 保守 scope、预算/portable alias 实际仓拒绝、正常 Git churn 导致 binding 失效仍按原契约，不能称稳定仓identity。

没有 formal DEV/100k、public gold、第三方项目执行、excluded old runtime/post_index、broad suites、private42、GC WAL faults、merge/deploy。此次 review **只新增本目录 tests/runner/report/evidence**，product和旧红色 report无修改。仅正常 Git commit/push；遵守此前 PR API Forbidden，**本次 PR API 尝试0次**。
