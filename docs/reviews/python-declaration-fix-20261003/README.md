# Python 声明类别与 decorated class 修复

基于 PR132 `fd5146baf5a62f977ce5ed4682c16f485aef9da3`。独审来源
PR134 `fb8ca8ae3b2cebf7d08255d37032158daafa63f2`；其原证据目录
`../qname-parser-public-20261003/` 逐字保留。这里的新运行不覆盖旧失败证据。

实际声明节点决定 class/function 分派；函数提取器自身拒绝 class。
装饰 class 只产生一个 wrapper 坐标的 Class，真实 body、nested class、
method 及 decorator 表达式分别遍历一次。qname、UID 与最近 lexical parent
的 canonical symbol ID 一致；Python 未产生 scope catalog，保留空 scope_id。
Boundary 只有在 AST 声明类别与名称兼容时才接受 hint，拒绝后仍可尝试
inner-node hint，最后由 AST 决定类别。保留合法 Method 与其它语言的原分类。

本修复未改 scorer、gold、生产预算、Cargo.lock、schema 或 DB 写入逻辑。
仅处理声明与 source-bound 身份真实性；不关闭 V19 或 P7 整体任务。

原公开红例要求 class 的 qname 必须缺失。在正确的完整 class proof 出现后，
这一缺失断言会继续失败；原测试与运行失败保留，新公开测试额外要求正确
Class/Method、qname、exact source bytes 和 SQL 存续证明，不能以丢弃身份代替修复。

## 冻结源码验证

源码 SHA：`f4df9a83514e6ba901143547368ce4a3bee21940`；后续提交只增加
证据、复验脚本与任务备注，536 个 Rust/source manifest 项核验未变。
官方直装 `rustc 1.95.0 (59807616e 2026-04-14)`，官方 Cargo.lock、
`--locked`、正常继承的网络/proxy；独立 `/workspace/scratch/python-kind/`
下的 toolchain、Cargo/target/index 缓存。没有尝试受限旧 toolchain 路径。
本仓库及 `/workspace/.agents` 无 AGENTS.md 或本地 skill。

| 冻结源码检查 | 结果 |
| --- | --- |
| cc-parsers lib + 全部 parser integration | 248 / 0 / 0 |
| index 的原独审 parser、自有声明、既有 identity/owner 四 targets | 14 / 0 / 0 |
| public 的自有声明、既有 lifecycle/diagnostic 三 targets | 10 / 0 / 0 |
| cc-search lib | 298 / 0 / 0 |
| workspace fmt check；三个相关 crate all-targets strict clippy | PASS |
| 原公开红例原样重跑 | 1 PASS / 1 FAIL；失败为缺失 qname 断言，保留 exit 101 |

前四项共 570 PASS、0 FAIL、0 ignored。`checks.json` 记录实际命令、退出码与
日志哈希。原 parser 红例来自 PR134 的原始四测试，纳入 index target 原样
重跑并全部通过。原公开红例先前在 Function 分类断言失败；现在通过该断言，
在 `metadata.qname` 必须缺失处失败。`checks/original-public-red-replay.log`
保留完整输出，明确不算 PASS。新公开测试不是宽松省略：要求独立 Outer、
Inner、pulse 结果、正确 qname/Class/Method、exact source bytes、完整 owner
endpoints 与 SQL-surviving symbol、持久化 identity row。

覆盖无装饰/多装饰、nested/conditional/function-local class、staticmethod、
property、async 的既有独审案例、Unicode/CRLF、canonical parent ID/UID、
真实 decorator 引用/调用及字符串负例。错误 class↔callable、Method↔Function、
同类错误 name 的 hints 均不能改写真实 AST；拒绝 wrapper hint 后仍接受合法
inner hint。Rust/TS/Go/Java/C++ 的 Class、Method、Function、Interface、
TypeAlias、Namespace 回归通过。局部 class 若仍包含在符合既有预算的完整
function chunk 内，测试核对 parser 的完整 canonical AST boundary；不会凭
没有独立 chunk 伪造 qname proof。小/大 decorated class 的独立 member chunk
另有直接断言；生产预算与 chunk partition 逻辑未改。

## 真实公开输出与原字段

`fixed/` 保存真实完整 engine 与 MCP JSON-RPC backend envelopes，包括原始
decorated-class fixture 的 Outer/Inner 查询；非模拟填入字段，也不冒充
产品子进程 stdio 测试。两查询在两 API 上都保留以下三个结果：

| name | kind | qname |
| --- | --- | --- |
| Outer | class | Outer |
| Inner | class | Outer.Inner |
| pulse | method | Outer.Inner.pulse |

`compare.py` 对照原独审 product 输出，普通方法全部 5 hit 在两个 API 的每个
字段都逐值相同，包括完整 metadata、scores、score_trace、source/document
identity 与 qname。只比较原 hit 字段；运行时间/随机 generation 等完整 envelope
非确定性字段不宣称相等。完整 envelopes 没有裁剪。

`native_driver.rs` 与原独审逐字相同，真实调用未经修改的 `cc-eval` normalizer、
source verifier 与 native scorer；原 micro-gold/wrong-gold 文件逐字相同。
engine/MCP correct recall10=1、wrong=0；旧 missing=0 基线证据原样保留，
没有重跑或重命名旧快照。`field-parity.log` 保存全部比较结果。

## 复现与范围

```sh
python3 docs/reviews/python-declaration-fix-20261003/run_checks.py
python3 docs/reviews/python-declaration-fix-20261003/run_native.py \
  /workspace/codecortex /workspace/codecortex/docs/reviews/python-declaration-fix-20261003/fixed fixed
python3 docs/reviews/python-declaration-fix-20261003/compare.py
python3 scripts/code_index_plan.py
```

`source-manifest.json` 固定 base/review/product/source SHA、536 source hashes、
旧审查目录 hashes、新证据 hashes。原审查 REJECT/log/output 均保持 PR134 字节。
`earlier-runs/` 保留早期自有 fixture 的失败与修正过程：Go declaration 缺少
fallback name、默认完整 function chunk 未独立拆出 local class、bare decorator
引用遗漏、SQL 测试错误使用不存在的 qname 列，以及一次编辑脚本的匹配失败。
前三者的边界/引用修正或符合既有 chunk 模型的测试校正均体现在冻结源码；
SQL 检查改为读取实际 `record_json`。这些早期日志不冒充冻结源码 PASS。

生产只改 Python parser/references 和必要 boundary guard；没有编辑 schema_guard
worker 的测试或 DB reviewer 的内容。未运行包含
`post_index_worker_crosses_pages_and_reopen_reuses_artifacts` 的 broad suites、
旧 GC/WAL/kill/staging/EROFS/private localdiag/42export，也未运行 public DEV、
holdout、provider、规模/发行认证。任务状态与验收字段未变；tasks.json 的
P7-011 新增实施备注，05-TODO.md 按权威任务文件生成并严格核验。
独立最终审阅与 V19/父项仍开放；没有 merge/deploy。
