# P5-019 待运行正式消融计划

状态：仅准备；生产/测试源码静止，等 owner 完成 P5-D，且 S11/Partial 最终修复后的代码重新冻结。

## 控制变量与输入

`prepare_factorial.py` 接受独立 accepted source，不修改工作树；每个 disabled control 必须在 reference 恰好出现一次，复制全 Cargo/crates 闭包后只改对应文本；现有 Rust `ablation::validate` 独立核验全部源字节/build options/binary digest。

- path：关闭 PathLane；不改变预选、exact 或 lexical。
- exact：关闭 ExactSymbolLane；不改变 path 或 lexical。
- facet_selector：仅关闭 intent-driven facet reservation（以 Locate policy 替代），保留 overlap 排重/排名/source authority。不能称为完全关闭所有 selection。
- 8 cell 完整factorial，同一配置/seed/输入，逐 one-factor edge 对照。不能只用 on vs全off解释单因。

四套既有51题开发gold不改（如 accepted source 改动，source suite须另栏声明 current-source re-lock：不是历史同source输入；历史冻结4suite继续保留复验）：smoke/exact/intents/source，3 repeats，8 cells，总1224请求；保留全部 gate/Partial/S11，不能使用这批已反复调参题认证 holdout。

## 必须补强独立质量断言

`facet-plan.json` 是独立手写多facet机制题源，不覆盖既有gold。`driver/` 是artifact-only Cargo包，使用公开McpStdio，same args输入、不把gold传给产品；每cell每题30重复，共720请求。

- path/exact要求 src/a.rs 中原始函数正文。
- fix要求 implementation + test + interface；只含路径/引用或错byte-span不能通过。
- 逐hit用现有normalizer独立读取原件校验 source hash/slice/span；每facet marker必须位于valid正文，要求整段必需实现/测试函数/接口定义而非名字出现。
- 存全部raw/status/facet覆盖/span验证/latency/originating work；Partial/error/缺facet均fail。失败不回写标签。

这些题是有限开发机制；如果候选本身未覆盖全部要求，应保留红灯，交owner修正式机制。不能改facet/top_k来凑绿。

## 混合负载

4个 `mixed-cN.json` 固定同一源/queries/seed/budget，C1/4/8/16各90 reads+30 full rebuilds。同一真实rmcp session，所有 offered/started/finished/client queue/e2e均保留。每读仍要求原始有效span和全部facet；不以删facet换快。

限制：这是immutable-source重建争用，不是mutation freshness；in-process公开dispatch，不是stdio进程隔离性能认证。线程值复用sampler有界probe，关探针时null；before/after不代表峰值。少于200同类样本不称稳定尾部/p99；100k、RSS、真实provider不由本计划通过。

## 运行与冻结

1. owner/auditor先批准控制变量、最终source hash与资源预算；不与P5-D计时测试争用。
2. `python3 prepare_factorial.py --accepted-source <frozen> --output <new-run> --sdkroot <verified-sdk> --build`（8次顺序release，jobs2，共用隔离target）。
3. 同accepted source构建cc-eval runner，执行 `cc-eval ablate --plan <new-run>/plan.json --output <new-quality>`。
4. artifact driver以release构建并锁Cargo.lock/driver source/runner binary；对每cell执行facets，C1/4/8/16执行mixed。所有原始命令/exit/stdout/stderr/hash留收据。
5. source/二进制/输入结束后再hash核验；failed不能写done。后续正式源码改动后必须重新跑。

## 修订：真实stdio混合与聚类报告

根据owner/auditor提出的G5预注册，mixed配置提升为1000 admitted Rust文件、每C每binary300reads+30full builds（100 repeats×3题，build_every=10），50ms offered interval暂待批准。artifact driver新增 `mixed-stdio <frozen-binary> <mixed-plan> <new-output>`，同一RunningService实际子进程，固定有界C工人真实并发read/build；能够baseline/candidate相同harness/version配对。in-process `mixed`仅保留机制回归，不能替此正式配对。

`cluster_report.py --ablation <quality>/ablation.json --output <new-report>` 以dataset/repository与query_family层级抽样10000次、seed1905、95%CI，原重复先按case归并、翻译同family；其他三套dataset为synthetic corpus单位，不冒充真实多仓。

`driver replay-facets <source-plan-directory> <facet-run> <new-report>` 从全部raw重算status/facet/span和每题latency/失败数。first-after-build与repeated-same-process分开；originating work不等真实cache-hit成本。运行结束复核admitted源原件未漂移。

这些修订仍为准备，不是正式run；最终source/hash/预算/资源采样需owner预注册和auditor批准后执行。
