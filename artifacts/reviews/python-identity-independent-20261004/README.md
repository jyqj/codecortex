# Python AST identity adapter 独立审查

结论：**发现 R1，暂不接受实现完整性**。未发现本切片启用生产 identity、改写既有 extraction/qname/UID，或执行原 Python bytes。R1 是合法源码误拒绝，不是错误 identity 泄漏。

审查生产实现：`c5091acf45fcd001f17e9822555e1db503ec7ed9`。
审查最终文档/测试基线：`6ed481e22960244a78c2706e4866a71f2a6278a4`。
用户给定 base：`e4a8df4cbc6dfae29af8cb0eac9ee83fbba4d696`。
最终文档相对实现仅修改 `docs/internals/python-identity-adapter-v1.md`；审查读取了完整文档与完整 `python_identity.rs`。本提交只增加独立 tests、fixture 和本报告/验证日志。

## R1（P2）：合法 leading trivia/BOM 被 root 覆盖假设拒绝

位置：`crates/cc-parsers/src/python_identity.rs:93`。

锁定 tree-sitter-python 0.23.6 的 recovery-free `module` 节点不保证 `start_byte()==0`。实际证据：

| 原 bytes | 实际 module span | has_error | adapter |
|---|---|---|---|
| `\n\ndef f(): pass\n` | `[2,16)` | false | UnsupportedAst |
| `  # lead\r\n\r\ndef f(): pass\r\n  ` | `[2,29)` | false | UnsupportedAst |
| UTF-8 BOM + `def f(): pass\r\n` | `[3,18)` | false | UnsupportedAst |
| ` \t\r\n\r\n` | `[6,6)` | false | UnsupportedAst |
| UTF-8 BOM only | `[3,3)` | false | UnsupportedAst |

因此普通 leading blank lines 的文件中，完全有效的顶层声明也不能获得 identity；whitespace-only 无声明文件也误归为 unsupported shape。对照空 bytes、开头无缩进的 comment-only、leading comment 的正常声明、无尾换行及 trailing spaces/CRLF 均成功，说明问题是 root-start 假设而不是 digest 或 declaration traversal。

建议作者取消对 module start 覆盖全部 bytes 的要求，或按锁定 grammar 明确验证允许的 leading trivia；保持原 bytes ownership、全文件错误拒绝和 digest 原样计算。禁止为修复而 trim/重写 bytes 或重解析 source snippet。BOM admission 可以明确形成契约，但普通 blank lines 不应被当作 unsupported AST。

独立测试 `r1_valid_leading_trivia_and_bom_are_rejected_by_root_coverage_check` 是**当前缺陷 characterization**，并非期望行为；通过不表示 R1 修复。作者修复后应把这些声明样例改为成功的精确 byte assertions。

## 其他验证范围及结果

新自编 `nested.py` 为 UTF-8 comment + 全 CRLF，包含 decorated class、decorated async method、条件内 decorated local function、local class 的 decorated method，以及 if/else duplicate declarations。手写固定 witnesses 比较每个完整 Class/Function ancestry、name/span、wrapper span。grammar declaration span 在这些语句中止于 CR 前；首轮手算包含 CR 的 end off-by-one 已纠正，不是实现缺陷。

固定 source digest 为 `08fc7ab660a3744baff4ffd57b0ff87b3c3810632e61054babcdb3f09161f520`。纯模型对固定 witnesses 与实际 inputs 得到同一结果；local class/method 保留所有 Function 祖先并返回 LocalDeclaration，条件重复声明 address 相同而 fingerprint 不同。fixture package marker 含 `raise RuntimeError`，从未执行；configured root 是模型明确允许的 fixture assertion，不是生产 config/capture 验证，也不代表 runtime importability。

独立 malformed fixtures 验证先有 valid sibling 时仍 whole-file 拒绝；空 body/comment-only body、hard keyword recovery-free shapes 返回 UnsupportedAst。另有真实匿名 `MISSING ")"`（`class C(A, meta:`，带 child method）证据，root.has_error 为 true，整个文件为 SyntaxError。soft match/case/type 保留。strings/comments/lambda 不制造声明。

source diff 审查显示仅新模块及一行 pub mod 注册；没有 production caller。既有 extraction、qname、UID、DB/MCP/retrieval 没有本切片改动。此判断是 scoped source comparison，未运行 broad/runtime suites。

## 预算实际相位

- 参数/path 校验后、UTF-8 与 parser 前检查 source_bytes；独立 invalid UTF-8 超 source cap 先返回 SourceLimit。
- timeout 经现有 parse_tree 的 parser.set_timeout_micros 设置，parse(None) 返回 Parse error；1 µs + 自编 400000-byte assignment fixture 实测返回 Parse error。一次观测不保证 OS 硬 deadline。
- root error/shape 检查及 digest 后才进入 node/depth accounting；所有匿名节点计数，root depth=1。真实完整 tree 的 count/depth 精确边界成功、少一拒绝；syntax rejection 可优先于 traversal cap。
- segment() 内先分配当前 name String，之后检查 declaration/累计 ancestry segment/text 输出预算，再 push 和 clone 输出 ancestry/path/digest。output_text_bytes 是累计返回文本 clone 预算；不限制此前 segment-name 分配。临时祖先 name 受 source-byte cap 约束，digest 固定 64-byte text 也在输出预算检查前生成。
- declaration/segment/text 精确边界成功、少一 whole-file OutputLimit；所有失败丢弃临时 output，无 partial identity。零 timeout 和过长 path 为 InvalidParams。

source cap 只限制 parser 输入；node/depth/output caps 均在 tree 构造后，不能证明 parser peak RSS 或硬 memory isolation。没有 RSS/吞吐测量。未导入、修改或验证独立待审 resource API `08733fa26504e9792586513ebe81075f058b586e`；本模型 new/resolve fixture 成功不证明其预算或组合兼容性。

## 检查

workspace/repo/ancestor 未发现 AGENTS.md；`.agents`/`.codex` 无适用文件。读取 CONTRIBUTING 和 source-declaration identity contract。没有适用 coding/review skill；本报告是既有 repository review 文档，不创建 Library/Page。

官方 `rustc 1.95.0 (59807616e 2026-04-14)`；原 Cargo.lock 未改变，SHA256 `ea6b177872de3749702e2cd967f4504efff5bcc67cd2a2459205d182243d1000`。无新依赖。

```sh
PATH=/workspace/.cargo/bin:$PATH RUSTUP_HOME=/workspace/.rustup CARGO_HOME=/workspace/.cargo
cargo +1.95.0 test --locked -p cc-parsers --test python_identity_independent --test python_identity_adapter
cargo +1.95.0 clippy --locked -p cc-parsers --lib --test python_identity_independent --test python_identity_adapter -- -D warnings
cargo +1.95.0 build --locked -p cc-parsers --lib
rustfmt +1.95.0 --edition 2021 --check --config skip_children=true crates/cc-parsers/tests/python_identity_independent.rs
git diff --cached --check
```

fixture-local .gitattributes 显式保留 CRLF，并仅允许 CR 行结束；日志去除最后的空白行。staged diff whitespace 检查通过。

最终 author 6 + independent 6 passed，0 failed/ignored；lint/build/format/diff 通过。原探索 probes 未保留，只保留确定 byte witnesses。没有运行 excluded runtime、broad suites、public gold、formalDEV、100k、private42 或 GCWALfaults；未 merge/deploy。
