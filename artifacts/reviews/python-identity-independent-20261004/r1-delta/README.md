# R1 修复的独立 delta 审查

结论：对 fixed source **`7d49beb6c220b6992d8e8e6001342f7a2fa213f6` 有界接受；原 R1 已修复**，本次 scoped delta 未发现新缺陷。此结论不关闭其他产品质量门禁，也不证明生产接线或 CPython runtime importability。

原审查提交 `c2b2a22e0536e163d1f9f7353f10ddf400dfd854` 针对实现 `c5091acf45fcd001f17e9822555e1db503ec7ed9`、文档 `6ed481e22960244a78c2706e4866a71f2a6278a4` 的 R1 finding 仍有效。原 README、tests.log、clippy.log、build.log、测试及 fixture 在新 review branch 原样保留，未覆盖历史失败结论。新 review 从 fixed source 建立，重放原独立 review commit，再只新增本 delta tests/report/receipts；无需 force-push 或本地 merge。

## 精确修复与信任边界

完整读取 fixed source、author 新 `python_identity_trivia_r1.rs`、fixture witnesses、fix README、更新后的完整 adapter contract。相对 `6ed481e`，唯一 production change 是 module root 检查从必须覆盖 source 改为 `start <= end <= source.len()`。root.has_error 仍先拒绝，迭代遍历仍检查 ERROR/MISSING，segment 必需 fields/body/keyword 策略未变。

fixed source 和本 review 的 `python_identity.rs` git blob 相同：`c906ea829c2233599f8e5875e2499cddda4d469a`。UTF-8 str 仍借用 original bytes，传给原 parse_tree 一次；没有 trim/normalize、源码片段重解析、外部 tree assertion、qname/line 推断或原 Python 执行。digest 覆盖原完整 slice，name/wrapper spans 仍是绝对 byte offsets。无需让 module node 覆盖 trivia 才能证明 source ownership，因为 tree 在此函数内由同一 bytes 创建。

`parse_common.rs`、Python extraction、parsers lib、Cargo.lock 和 cc-model 相对原 docs head 均未改。未导入/验证待独立审查 resource API `08733fa26504e9792586513ebe81075f058b586e`，未新增 identity production caller、DB/MCP/retrieval 输出或更改 qname/UID。

## 独立正向证据

新增 `crates/cc-parsers/tests/python_identity_r1_independent_delta.rs`，4 tests：

- 原 R1 三个声明反例（leading blanks、缩进 comment + CRLF、BOM）全部成功；逐一固定原绝对 declaration/name spans，和纯模型固定 witnesses 比较。原 whitespace-only/BOM-only 以及 empty/comment-only 对照返回 empty Vec。原无尾换行/trailing trivia/leading unindented comment 对照仍成功。
- 对原独立 7-declaration nested UTF-8/CRLF fixture 添加四种 prefixes，包括 BOM + UTF-8 comment。每段 Class/Function 名称、完整 ancestry、decorator span/name span 均只移动 prefix.len() bytes；尾 trivia 不扩大声明 span。纯模型保留 local class/method 的全部 Function 祖先与 LocalDeclaration；模块仍 review.probe。完整 digest 随前/后 trivia 变化，新增尾换行使旧 assertion 返回 StaleSource。
- 所有 prefixes 后，先有 valid sibling 的 ERROR、真实匿名 MISSING `)`、decorator recovery 仍 whole-file SyntaxError；空 class/function/comment-only body/hard keyword 仍 whole-file UnsupportedAst。prefix 后 invalid UTF-8 仍 InvalidUtf8，不产生部分声明。
- prefixed original bytes 的 source/node/depth/declaration/segment/text 精确边界成功，少一全部 typed unavailable。用独立递归 grammar-node metrics 验证匿名节点/depth；whitespace/BOM-only root 的 node/depth=1 可以成功，source cap 少一仍 SourceLimit。

原 independent target 的其余 5 tests 同时通过（包括精确 handwritten nested witnesses、真实 MISSING 和 timeout fixture）；author original 6 + author R1 5 同时通过。配置 root 是自编 fixture assertion，package marker 含 raise RuntimeError 且未执行；不冒充 production config/capture 验证或 runtime importability。

预算相位未改变：source cap 在 UTF-8/parser 前，timeout 是 tree-sitter best-effort；node/depth/output caps 在 tree 建立后；segment-name String 先分配，再检查 output budgets，再 clone 输出。没有 parser peak RSS、硬 deadline 或 OS memory isolation 保证，未将纯模型预算升级为 parser allocation 证明。

## scoped 验证

本轮检查 workspace/repo AGENTS 未发现适用文件；此前已读取 CONTRIBUTING/source identity contract，无适用 coding/review skill。官方 `rustc 1.95.0 (59807616e 2026-04-14)`，原 Cargo.lock SHA256 `ea6b177872de3749702e2cd967f4504efff5bcc67cd2a2459205d182243d1000`，无新依赖。

命令均使用 `PATH=/workspace/.cargo/bin:$PATH RUSTUP_HOME=/workspace/.rustup CARGO_HOME=/workspace/.cargo`：

```sh
cargo +1.95.0 test --locked -p cc-parsers --test python_identity_adapter --test python_identity_trivia_r1 --test python_identity_independent --test python_identity_r1_independent_delta -- --skip r1_valid_leading_trivia_and_bom_are_rejected_by_root_coverage_check
cargo +1.95.0 clippy --locked -p cc-parsers --lib --test python_identity_adapter --test python_identity_trivia_r1 --test python_identity_independent --test python_identity_r1_independent_delta -- -D warnings
cargo +1.95.0 build --locked -p cc-parsers --lib
rustfmt +1.95.0 --edition 2021 --check --config skip_children=true crates/cc-parsers/tests/python_identity_r1_independent_delta.rs
git diff --cached --check
```

结果：**20 passed、0 failed、0 ignored、1 filtered out**。唯一过滤的是保留原样、明确断言旧 rejection 的历史 characterization；它对原 source 的结果已由 immutable 原报告记录，不能拿它的当前失败要求修复回退，也不能拿旧通过宣称修复。当前所有五个原 R1 counterexamples 均在新 delta 正向断言中实际执行。clippy/build/format/staged whitespace 检查通过；日志仅去掉 EOF 空白行。

未运行 broad/excluded runtime、公用 gold、formalDEV、100k、private42/GCWALfaults；未 merge/deploy。正常 origin commit/push，不重试之前被 Forbidden 拒绝的 draft API，不 reroute。
