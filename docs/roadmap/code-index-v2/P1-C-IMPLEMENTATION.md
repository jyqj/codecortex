# P1-C 实施与验证报告

2026-09-27。基线 HEAD `4514630dcd26481cf6dbc2aff38824ed71ef06da`，继续 P1-B 未提交工作。**P1-011–015 全部完成；累计35项done、157项todo；下一批P1-D，整个P1/G1尚未完成。** 没有提交、推送、PR、模型调用或全局 SDK/工具链修改。

## 1. 实际实现

| 任务 | 本轮交付 |
|---|---|
| P1-011 | 缓存域加入完整 SearchConfig、RankingConfig、政策版本及仓库规模档位；图缓存继续包含所有图限额与 token budget；硬集合与有序软提示分别验证；显式项目重载后配置生效 |
| P1-012 | typed SearchScopeExplain 串接详细检索、图缓存与 MCP；冷热/空结果可见 hard/soft/budget/ordering；只列提示数量而不枚举排除路径；数值 trace 与身份排序层分别说明 |
| P1-013 | 十四类型有效/未知字段与 sanitize 矩阵；真实 stdio 十四工具有效请求、额外字段拒绝和 hybrid/symbol 旧形态；未改生产工具参数或错误形态 |
| P1-014 | 六文件十八题双语作者回归集，四个意图类各两英/两中，加两道无答案；报告按 query_language/lexical_anchor 切片；不添加查询词典 |
| P1-015 | cc-eval ablate：独立源码快照与精确二因素四格矩阵、编译收据和 binary SHA-256 校验、共同输入/预算/提示、每条单因素边与原始工件；不存在生产旧行为开关 |

`SearchConfig` 指纹的遗漏在旧实现红测试中复现，但原来每个引擎已不可变持有配置，并没有因此证明实际发生跨引擎缓存串用。此次补齐显式契约和回归。配置重载测试调用 `CodeIndex::set_project`；不宣称已有自动配置热监听。

`search()` 保持 Arc 命中数组，`search_with_diagnostics()` 仍显式绕过结果缓存。MCP 图缓存保留 scope/grep 解释；其中计数属于结果生成时，不是缓存命中时重新执行。确定性的 cap 截断可以缓存，预过滤错误不进入结果缓存。未实施 P5 的跨 epoch 快照重试或统一 LaneOutcome。

范围解释还暴露了 caller-only prefix 与 file set 未在无 DSL 情况下物化交集的问题；现在无 DSL 也计算有效文件集和显式 empty，最终合法结果不靠放宽 scope。解释只给交集文件数量；长前缀有截断标志。HardScope 仍不是跨租户权限系统。

## 2. MCP 与证据分类

真实 SDK 对 unknown 字段的反序列化失败返回 `CallToolResult.is_error=true`；sanitize 对非法 mode 返回 JSON-RPC `-32602`。初版测试把二者假设成同一种 RPC 错误而失败，随后按实际旧契约修正测试，没有改生产实现迎合错误假设。

红证据 `red/`：缺失 grep_scan_cap 配置指纹、缺失 scope 解释（公开 API 与旧 P1-B 二进制）。开发测试还发现交集数量未物化，已修正。初次 tar 解包 Python 版本参数不支持、难度元信息超出既有 1–3 范围、Clippy 测试模块位置告警分别归类为准备/开发问题，不计作产品收益。

## 3. 双语回归与消融口径

双语样例在执行检索前阅读并冻结自编源文件，源 language=python，查询语种在 annotation.query_language；翻译和改写保持同一 query_family，不跨 split。无外部人工审阅或 holdout 认证。两个无答案问题单独进 gate，不参与有答案平均。查询自然语言效果无论好坏都保留，不通过添加目标词/答案路径修正分数。

消融使用 P1-B 作为共同不可变 scaffold，四格只更改 BM25 映射和预选到硬过滤两个确切片段，未重现整个历史 G0 实现。所有四格真实编译、每个 460 文件快照按精确 replacement 重算 SHA-256 验证，编译参数一致；源/二进制绑定是本地构建收据，不是密码学远程证明。实验代码仅在 artifacts 独立副本，生产没有退回错误行为。

两套数据合计三题 × 三次 × 四格=36次真实 MCP 请求。softscope 两题固定同一个无关 pinned 文件和 preselect limit=1；每格相同提示，后端没有 gold。BM25 一题不加该提示。八组原始运行均完成离线 metrics 回放。

消融观察：softscope 修复在 BM25 开/关两种背景下都使两道定向问题 Top-1/nDCG 从 0 到 1；BM25 修复在本次单题夹具中最终排名与 Top-1/nDCG 均不变，但公开 stage-a 明细确认强弱贡献方向修正。未根据零 delta 更改题库、权重或标准答案。不能将三道机制用例当成总体质量收益。

## 4. 最终验证与同输入比较

最终覆盖源码 SHA-256：`0d8c117b7ac95d997622de8a9ef09705b7c09b897620155051b471b7bcb2ae3e`。原始命令、退出码、日志和逐文件摘要在 `artifacts/benchmarks/p1c-20260927/final/validation.json`；配对运行后再次校验无漂移。

| 检查 | stable 1.97.0 | Rust 1.95.0 |
|---|---:|---:|
| all-targets + eval-http 严格 Clippy | exit0 | exit0 |
| workspace 全量，含 doctest | 1398 passed / 0 failed / 24 ignored | 同左 |
| cc-eval + eval-http | 104 passed / 0 failed / 18 ignored | 同左 |
| workspace/eval binaries | exit0 | exit0 |
| 显式 P1-C 契约/解释真实 MCP | 1 + 1 passed | 同左 |
| 显式 P1-B/P1-A/P0 真实 MCP | 2 / 2 / 1 passed | 同左 |

额外使用旧 P1-B 二进制重跑十四工具契约；旧新输入schema、未知字段拒绝形态的收据完全一致。不同测试命令有重叠，不相加；跳过不算成功。没有新增lint豁免或降低warnings。CI加入本批测试与数据锁，但远端Actions未运行。

| 固定数据 | 独立题/请求数（每版） | P1-B → P1-C Top-1 | P1-B → P1-C nDCG@10 |
|---|---:|---:|---:|
| 原七文件源码集 | 14 / 42 | 0.8571428571 → 同值 | 0.9379235538 → 同值 |
| 原三文件smoke | 11 / 33 | 0.7 → 同值 | 0.7 → 同值 |
| P1-B定位/范围集 | 8 / 24 | 1.0 → 同值 | 1.0 → 同值 |
| 新双语意图集 | 18 / 54 | 0.625 → 同值 | 0.65625 → 同值 |

八组运行均使用同一新runner和相同冻结输入，metrics与query-slices离线重算摘要一致。14题和18题正式compare均inconclusive，不声明加速或统计显著提升。当前CI源码锁的刷新是独立数据版本，不替换历史 frozen-inputs。

双语正答案16题：8个含标识符问题Top-1/nDCG均1；8个自然描述问题Top-1=.25、nDCG=.3125。其中英语自然描述4题=.5/.625，中文自然描述4题=0/0；这只是作者小样本，不代表所有中英文请求。按四意图类Top-1/nDCG分别为configuration .75/.75、api_usage .75/.75、symbol .5/.5、error .5/.625，所有前后delta均0。新两道无答案题两版通过，但原smoke S11仍失败，不能据此宣布通用无答案能力修复。

### 原14题的追加单因素归因

在未改题库的历史七文件14题上追加四格实验，另168次真实MCP请求。BM25单因素修正使R12 nDCG由.5变为.6309297536，全体平均+.0093521253；softscope开/关两种背景下相同。softscope单因素在该集指标delta为0。按7个query-family计算BM25均值差区间下界为0，正式比较仍inconclusive。这是在P1-B共同框架上关闭机制的反事实，不是本轮P1-C相对P1-B新增了这项提升。

合计机制集36次加冻结源码集168次=204次消融请求，12个cell结果全部离线回放。与正常配对的8组一起共20组回放；不把跨实验合计204次当同质性能样本。

Python/Rust两项mutation继续 equal=false/exit1；S11仍exit1，原始失败保留。代码变更相对入场未触及cc-db、cc-index或cc-parsers，不实施P2算法。

## 5. 保留限制

未运行 Linux/Windows/远端 Actions、真实 OCE 服务或 embedding。Python/Rust 签名更新旧目标 UID 仍归 P2；既有无答案弱相关结果不在本批静默删除。小样本/debug 查询不能认证 p95、峰值内存、100k 或发布性能。P1-D 的成本/并发/清洁/G1 总验收尚未完成。
