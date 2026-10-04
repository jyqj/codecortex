# Python identity R1 修复与正向 byte 回归 v1

修复前 adapter source：`c5091acf45fcd001f17e9822555e1db503ec7ed9`；文档基线：
`6ed481e22960244a78c2706e4866a71f2a6278a4`。
完整阅读独立审查 `c2b2a22e0536e163d1f9f7353f10ddf400dfd854` 的 README、source 与
`python_identity_independent.rs`。接受 P2 R1：root.start_byte()==0 假设误拒合法 leading
blank lines、缩进 comment、BOM 和 whitespace-only 文件。它是误拒绝，不是错误身份泄漏。

## 修复

仅替换 adapter 的 module root 覆盖检查：kind 必须为 module，byte extent 必须在 source 内
且不反向（start <= end <= source.len()）；允许非零 start 和空 root。原 has_error 检查仍在
shape 检查前，遍历仍检查 ERROR/MISSING 与 unsupported 必需 field。未修改 digest、名称/
声明 span 或 ancestry 推导；没有 trim、normalize、源码片段重解析或外部 tree assertion。

BOM 的接受依据为原 Cargo.lock 的 tree-sitter-python 0.23.6 行为，仅是锁定 grammar 的
语法证据，不声明 CPython 完整编译合法性、runtime importability 或模块导出。整个原 byte
slice 继续借用为 UTF-8 str 并传给同一 parse_tree 调用；原始 source_bytes 限制在 parse 前
检查，digest 覆盖全部 leading/trailing trivia/BOM。

## 分别保留旧证据与新正向回归

原独立 characterization、失败报告与日志完整保存在 immutable review commit
`c2b2a22e0536e163d1f9f7353f10ddf400dfd854`：
`crates/cc-parsers/tests/python_identity_independent.rs` 中的
`r1_valid_leading_trivia_and_bom_are_rejected_by_root_coverage_check` 记录旧行为，不是成功契约。
本修复不编辑/覆盖该 commit 的报告或旧 assertion，也不把旧 characterization 的通过解释为修复。

新正向版本为 `crates/cc-parsers/tests/python_identity_trivia_r1.rs` 与
`tests/fixtures/python_identity_r1/WITNESSES.md`。先固定 byte witnesses，再对旧实现运行：
1 passed / 4 failed（exit 101），原始失败输出保存在 `before-tests.log`（仅去掉末尾空行）。
之后实施检查修复，相同正向 tests 全部通过。修复前日志的行号对应尚未 rustfmt 的新版测试。

5 个新增 scoped tests 覆盖：原审查三个 declaration 样例精确 absolute spans 与模型比较；
empty/blank/whitespace/comment-only/BOM-only 正常 empty Vec；BOM + 缩进 comment + blank
CRLF prefix 后的 nested decorated class/async method 精确 wrapper/name spans；leading/trailing
trivia 在 digest 中仍有效；prefix 后 malformed sibling、真实 anonymous MISSING 和 unsupported
empty-body/keyword whole-file 拒绝；原完整 bytes source cap 与 declaration/segment/text/work/depth
budgets。configured root 仍是自编 fixture assertion，未调用生产 capture/config，未执行 Python。

原作者 6 tests 保持原样并通过；本 branch 不导入独立 characterization suite 或尚未审查模型
resource source `08733fa26504e9792586513ebe81075f058b586e`，未声称资源 API 组合成功。
当前 delta 仅 parser root 检查、新 scoped tests/witnesses 与相关 docs/evidence。

## 验证与发布边界

检查适用 AGENTS/SKILL/.agents/.codex，无新规则；继续遵循 CONTRIBUTING/source identity contract
及用户 scoped 限制。官方 Rust 1.95.0；原 Cargo.lock SHA256
`ea6b177872de3749702e2cd967f4504efff5bcc67cd2a2459205d182243d1000` 未改变；无新依赖。
以下使用 PATH=/workspace/.cargo/bin:$PATH、RUSTUP_HOME=/workspace/.rustup、CARGO_HOME=/workspace/.cargo：

```sh
cargo +1.95.0 test --locked -p cc-parsers --test python_identity_adapter --test python_identity_trivia_r1
cargo +1.95.0 clippy --locked -p cc-parsers --lib --test python_identity_adapter --test python_identity_trivia_r1 -- -D warnings
cargo +1.95.0 build --locked -p cc-parsers --lib
rustfmt +1.95.0 --edition 2021 --check --config skip_children=true crates/cc-parsers/src/python_identity.rs crates/cc-parsers/tests/python_identity_trivia_r1.rs
git diff --check
```

最终 11 passed / 0 failed / 0 ignored；lint/build/format/diff 均通过，收据见同目录 after-tests.log、
clippy.log、build.log。未修改模型/production extraction/qname/UID/DB/MCP/retrieval；身份输出
仍禁用。excluded old post_index/broad/private42/GCWALfaults/formalDEV/100k 未运行。
正常 commit/push；之前唯一 draft API Forbidden 不重试、不 reroute，无 merge/deploy。
