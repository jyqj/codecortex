# Python AST / 模型资源契约：有界组合

Integration base：`84d5d57978cfaa3a0dd7f63e39d7c7288b614d95`。
旧 production anchor：`e4a8df4cbc6dfae29af8cb0eac9ee83fbba4d696`。
Fixed product（实现 + 新组合测试 + R1最小迁移）：`50a4933e48ef20b16401ac8f75c386a660aa8e5c`。
这不是 release/quality/规模认证，也未启用生产 identity 输出。

## 精确采纳及历史

`source-guard.json` 保存每个原提交与集成提交、全部路径映射。按 commit delta 顺序 cherry-pick，
没有从 stale source 覆盖整仓，两个生产模块保持已独立接受 source 的逐byte实现。

| 内容 | 原始提交 | 集成提交 |
| --- | --- | --- |
| 模型资源契约 | `08733fa26504e9792586513ebe81075f058b586e` | `120af8f` |
| 资源独立接受 | `a9050fa7123a671dc0d9e2c1b61533cfbee8612d` | `1302ea3` |
| AST原实现 | `c5091acf45fcd001f17e9822555e1db503ec7ed9` | `53d1803` |
| AST组合边界文档 | `6ed481e22960244a78c2706e4866a71f2a6278a4` | `b832e23` |
| AST final / R1修复 | `7d49beb6c220b6992d8e8e6001342f7a2fa213f6` | `7b4904f` |
| 原独审/R1 finding | `c2b2a22e0536e163d1f9f7353f10ddf400dfd854` | `003abc1` |
| fixed独审有界接受 | `16d1344a72f749f995670c995c537330f2d9403a` | `3f435e6` |

原author/review报告、日志、fixture和历史freeze原样保留。原报告的尚待资源独审、R1拒绝和
fixed独审采用skip的叙述是各自时间点的历史事实；当前状态由本文、manifest和当前日志说明。
原独审仅接受纯模型逻辑准入；AST独审仅接受固定grammar语法断言和预算相位。两者均未认证
production capture、hard RSS、runtime importability或100k。

R1迁移见 `migration.json` / `r1-migration.patch`。原测试完整文件在 `003abc1` 的版本历史和
`python_identity_independent.rs.original` 中逐byte相同，原source及SHA256可核。
当前只改原一项characterization的名称/解释与旧UnsupportedAst断言，换成原五个witness的
成功/空成功、完整digest、name和固定absolute declaration/name spans；保留其余五tests。
没有skip、ignore、过滤旧R1名称或把旧错误断言当当前契约。迁移后21个parser tests全部实际执行。

## 新自编功能闭环及边界

`crates/cc-index/tests/python_identity_resource_integration.rs` 新增6tests，未借用旧fixture：

- 真实 `discover`/Loader 读取原config，绑定原path/digest/directive，验证publication重读及parse
  cache复用；仅在test内转换这个单根单指令自编fixture，不发布通用config→identity转换。
- 对相同BOM/CRLF原bytes调用真实AST adapter，再用明确全部7维 `DeclarationLimits` 和
  `DeclarationSnapshot<&BTreeMap<String, Vec<u8>>>` 共享不可变inventory派生普通包、嵌套class/
  method，保持全部Function祖先使local class/method明确Unavailable；if/else重复地址相同、
  occurrence fingerprint不同。包marker含raise，未执行Python。
- initializer模块不追加__init__，root initializer明确Unavailable；leading blanks、缩进comment/
  CRLF、BOM三组固定absolute byte spans贯穿AST→模型；完整digest不trim/normalize。
- config bytes变更使旧capture.verify失败、原root digest拒绝；重捕获更新绑定。
  marker/source/无关空文件使旧binding失效；旧source input为StaleSource，marker删除为
  NamespaceAncestry，module/package collision拒绝。失败不改变原borrowed snapshot。
- adapter source/node/depth/declaration/segment/text各预算whole-file拒绝；模型7维分别拒绝，
  typed resource refusal和Unavailable不混淆；syntax sibling错误无部分输出。
- config-only捕获与完整fixture的Python provenance相同，即使caller catalog漏掉磁盘marker。
  该负向证明实际执行，明确不能由Explicit/captured_document声称完整源码capture。
  inferred config也不进入identity准入。现有lexical qname/UID独立核验并保持。

这里inventory是完整自编fixture的caller assertion：文件由test创建、原bytes map共享借用。
生产config capture不是完整/native-safe source inventory。source/node/depth/work/output预算和
模型logical memory admission各自显式配置；后者不追溯AST/capture之前的分配，前者的node/
output预算在tree建立后才生效；timeout为best-effort。未认证hard RSS、OS deadline或累计输出。

## 当前验证

官方Rust 1.95.0、`--locked`、原Cargo.lock、不加依赖。逐命令argv/exit/time在
`validation.json`，原始当前日志在本目录；保留初次新6tests日志作为独立6次额外执行记录。
组合总数是96项通过、0失败、0ignored：model43、parser21、index13（新6 + 原taxonomy3 +
provenance独审4）、provenance library12、resolver1、query6。701个unrelated library/target
项目仅被正常scope过滤；没有过滤R1或执行excluded旧post_index/runtime/broad套件。
四个API probes要求正确Rust错误码，防止伪compile-fail。

全workspace all-target strict Clippy `-D warnings`、all-target build、fmt、source architecture
和精确source guard结果见最终 `validation.json` / `source-guard.json`。build/lint只编译targets。
`validate.py` 不含全仓test、doc baseline脚本或formal评测调用：`update-doc-baselines.sh`
内部会执行全仓all-target tests，与本轮排除约束冲突，所以不运行，也不更新全仓测试基线。
中央TODO只加本切片事实，不翻父项状态；保留现有vec→array机械lint修复。

## 最小下一项功能集成

先独立实现并审查 **Python完整/native-safe inventory capture→可拒绝的identity admission**：
明确owner/授权扫描scope，证明完整regular-file列表、native spelling/重复key/alias及symlink/race
政策，预算预检且捕获后重检，绑定完整config directives、package marker absence/collision；
任一不完整/unsupported/default拒绝snapshot，不能把当前config-only证据升级成完整capture。
复用本AST adapter和显式模型limits，在同源bytes上返回typed derived/unavailable，保持未接线。

随后分别设计累计输出/缓存失效策略（目前whole inventory保守失效与重hash成本）、版本化
input ingestion重新推导和未知版本拒绝、production additive optional identity persistence及
同snapshot publication guard，再独立opt-in DB/MCP/retrieval版本化协议。旧identity absent可读，
UID/qname不变；gold/scorer命名空间迁移须独立新协议，不能改旧精确答案。

新产品不继承先前100k通过。formalDEV/100k/private42/GCWALfaults全部not_run；query权重/
budgets/scorer/gold、parser extraction及生产DB/MCP/retrieval均不改。四仓publicquality FAIL、
Gin broad-prose、完整P7/V19仍OPEN。当前新组合尚无额外独立复审，不冒充父最终accept。
无merge/deploy。正常origin push与唯一draft attempt结果另见PUBLICATION.md。
