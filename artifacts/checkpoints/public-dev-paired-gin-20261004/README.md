# Gin / Go fixed public DEV paired evidence (2026-10-04)

**阶段：配对运行与封存完成；产品 quality FAIL / 全部 Partial；无当前 cert。**

baseline `88f2cf099c8b81f3acef485fd5ac9b01c63ce790` 对 candidate `37dd042eaa1209a86e0cafdcd92ae77e036e76f5`。候选八 crate/src 与 Cargo/lock 原字节等于产品 `90858afae647a513537bf118932a7ba5020ee98b`；评测器源码两边相同。官方 Rust/cargo 1.95、normal proxy、原 lock、default features，独立 checkout/target/copied binaries/TMPDIR/cache/index，无混编。

两个版本都显式加载 `public-dev-pygo-declaration-kind-v2`：Gin native 67（55 answerable + 12 no-answer）、compat 原 55；每套3次重复，原 seed20261003 / warmup0 / top_k10 / timeout30000 / hybrid / 原 engine_config。原 schedule 固定于 `fff0931e5b470aa3b9111d6ce55444f9b40f98cf`。loader 要求完整 Requests/Gin source map，实际物化后核验，**仅执行 Gin**。native 81 kind delta；没有新独立样本。旧 default gold 未改，无 future holdout 读取。

| Arm / profile | scheduled / executed / missing / error | Partial | prepare/index/readiness | gate / replay exit |
|---|---:|---:|---|---|
| baseline / native | 201 / 201 / 0 / 0 | 201 | 53/53 ready, no pending/failed/unknown | gate_failed / 1 |
| baseline / compat | 165 / 165 / 0 / 0 | 165 | 53/53 ready, no pending/failed/unknown | gate_failed / 1 |
| candidate / native | 201 / 201 / 0 / 0 | 201 | 53/53 ready, no pending/failed/unknown | gate_failed / 1 |
| candidate / compat | 165 / 165 / 0 / 0 | 165 | 53/53 ready, no pending/failed/unknown | gate_failed / 1 |

所有原 run/replay exit1；runner 编排成功不等于 quality 成功。source proof 返回 hits：baseline native819/compat660，candidate native822/compat663；invalid0/unverified0。每边 native strict no-answer **0/36**，12个case均三次失败。scheduled inventory、每个case/重复/status/metric/cost原记录见归档与 analysis，不补缺项、不best-of。

| Same-profile metric (answerable query mean) | Baseline | Candidate | Candidate − baseline |
|---|---:|---:|---:|
| native top1 | 0.490909091 | 0.381818182 | -0.109090909 |
| native ndcg10 | 0.481058673 | 0.448182206 | -0.032876467 |
| native recall5 | 0.454444444 | 0.457474747 | 0.003030303 |
| native recall10 | 0.454444444 | 0.457474747 | 0.003030303 |
| native mrr10 | 0.559090909 | 0.492424242 | -0.066666667 |
| native span_precision | 0.211067454 | 0.201219353 | -0.009848101 |
| native span_recall | 0.439131998 | 0.440997243 | 0.001865246 |
| compat top1 | 0.800000000 | 0.836363636 | 0.036363636 |
| compat ndcg10 | 0.814702811 | 0.838074336 | 0.023371526 |
| compat recall5 | unavailable | unavailable | unavailable |
| compat recall10 | unavailable | unavailable | unavailable |
| compat mrr10 | unavailable | unavailable | unavailable |
| compat span_precision | unavailable | unavailable | unavailable |
| compat span_recall | unavailable | unavailable | unavailable |

同输入 native Top-1/nDCG/MRR下降，compat Top-1/nDCG上升，只能分别描述；不能以不同评分公式之差作提升因果。固定54个answerable相关组件，family-balanced native Top-1 delta −0.111111111、nDCG delta −0.029506911；compat对应 +0.037037037 / +0.016870919。完整 query/category/group 聚合与逐case paired delta 在 `analysis/`，缺指标保持 unavailable。旧 scorer 的2000-draw诊断不替代预登记10000 paired cluster CI；该 CI、Recall20/SymbolAccuracy/DuplicationRate/facet/graph仍未实现，不宣统计确认。

Partial 原因诊断见 `partial-diagnostics.json`：两边 lexical、graph 每行均partial；path native180/201、compat144/165 partial；packing native198/201、compat165/165 partial；grep均complete。观测resolution freshness均ready，不将其提升为整体Complete。没有改变默认预算/knobs或以gold实现修复。

每行originating retrieval-work receipt齐全（每边366/366），完整costs原样归档；所有数值计数/denominator见summary，各case cost见observed-rows。originating work不是当前cache-hit实际工作。RPC walltime仅cloud诊断，不能支持performance因果、release/tail/100k认证。无live/paid provider。

完整raw/original output、dev query snapshot、source-proof excerpts 与Gin MIT LICENSE封存在 `public-development-paired-raw.tar.gz`；SHA256 `86a88f17993f0adf1e728879fc048cb44204d3fe2060a7140d75c53be6682613`。793 files / 25836871 bytes，解包后四套分别用所属binary replay，全部字节一致；summary/case/paired/observed分析复算逐字节一致。`raw-artifact-manifest.json`、`safety-license-audit.json`与`readback-receipt.json`保存核验。没有归档整个第三方source、binary、cache、DB、private/secret；实际source库存由固定Git entries与哈希引用。

前置诊断保留：初始checkout为旧ff458bc、请求对象缺失，normal Git fetch补齐；初次直接rustup默认/home路径只读失败，使用已配置/workspace官方toolchain成功；初次loader缺原author/review Git对象，normal fetch成功。Python blake3模块普通缺失，使用已构建官方Rust evaluator在scratch算BLAKE3并断言只有queries_digest值变化；没有替换lock/compiler、绕accessdenied或修改scorer。未找到AGENTS.md/.agents/skills，workspace .agents为空；排除所有用户禁止的suite/旧故障/私有/100k测试。12 admission tests pass，独立同输入锁与exact product/evaluator src核验成功。

历史四repo301native/256compat/280相关组、旧1671全Partial/qualityFAIL原样保持。本次是已暴露 public DEV 的122个profile case pair、732 measured rows，不是600、holdout或新的独立样本；cleanholdout0。中央tasks状态不关闭，无merge/deploy/CI green。

待办：保留 native 下降及 no-answer/Partial 为真实新证据；若父任务另开实现/默认覆盖或预算研究，需独立授权与预登记，不在本任务看结果调参。当前产品质量仍失败；预登记cluster CI未实现。交付使用正常push/draft，Forbidden时停止对应动作，无新CI声明。

复现脚本 `selftools/paired.py` 分build/prepare/execute；每次新执行需新隔离路径，原run输出不可覆写。`selftools/analyze.py <unpacked-archive-root> <new-analysis-dir>`只分析已有原输出，不检索。构建receipt、manifest/input lock/binary pins、original-schedule与前置协议均在本prefix。
