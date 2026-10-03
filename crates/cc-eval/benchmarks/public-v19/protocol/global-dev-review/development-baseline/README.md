# 四 repo 公开 DEV：固定 default 基线实际 run/replay

**结果：全部16个suite已尝试，但四repo完整测量无效。** 1671个scheduled rows中保留783行，均为Partial；Requests/Gin四个suite在index prepare失败，888行未运行。没有用validate代替排名，也没有改gold/scorer/生产代码、输入域或检索参数来消除失败。

## 排名前冻结与实际构建

固定PR67执行源码 `78b0ce52cb2ff4c53c53893b9f7dd469269806b8`，只物化该SHA的生产/evaluator源文件及Cargo锁（sparse checkout；没有物化旧语料/heldout题文）。实际offline Cargo compiler-artifact、完整构建JSONL/stderr和375个源码文件SHA保存在build/。默认feature，不启用semantic或eval-http，Rust1.95，dev opt0/debug assertions、debuginfo0；不是release性能构建。

- codecortex SHA256：`4e2f101137591c5bbc3bdc6d660cd7c2989e0b72fbacc0bb89d1a79b9ba8fe77`
- cc-eval SHA256：`04b546e45c75aca8ebfc9ee0c50f541aaab8fb3d53a620004d8c00982d2ba606`
- build receipt SHA256：`e56f82fe9fa1f17f4c9c783688134144bbd8d6d497cb74f7bcb42bd888f5af64`
- pre-ranking commit：`626e44d60f493089287b2a4c0f5dc1581dbd266f`，已先push核验，再开始pilot。
- preregistration receipt SHA256：`a17bab15d7ffcced8ed33d32fabed46a1c25f49da5aec12b29bc960595d742ee`

preregistration-receipt.json固定四repo作者suite/query/gold-answer/compat-expectation/source/config hashes、PR91 admission hash、280组件registry hash、protocol hashes、实际binary/build receipt、环境及计划。全部原suite repetitions3、warmup0、top_k10、timeout/seed/source domains不改。301native与256compat是同一批公开dev的两个评分投影，不是557独立问题。模型/版本/encoding space在这个无semantic default构建中not_applicable；没有real/paid provider调用。runtime env仅允许系统工具链/语言/临时目录字段，不转发API凭证；既有MCP adapter隔离HOME/config。可选process-tree probe固定关闭，native PID RSS原样保留，不声称连续峰值。

## 完整 pilot 与全量尝试

先选择现成TypeScript01-migration完整block（15native/12compat，45/36scheduled rows），未按结果挑选。81行均Partial、gate1，run/replay字节完全一致、无缺行/infra failure；按预先声明的continuation rule继续，不把质量失败当输入契约失败，也不调参。pilot仅检验契约，不进入full汇总。

| Repo | planned native/compat rows | observed native/compat rows | 实际结果 |
|---|---:|---:|---|
| Express |210/177|210/177|2 suites完整rows，全部Partial，gate1|
| Requests |273/249|0/0|2 suites index prepare失败，gate2|
| Gin |201/165|0/0|2 suites index prepare失败，gate2|
| TypeScript |219/177|219/177|10 suites完整rows，全部Partial，gate1|

Requests真实MCP错误：`-32602: src/requests/exceptions.py: invalid resolution manifest: invalid dependency key`。

Gin真实MCP错误：`-32602: context.go: invalid resolution manifest: conflicting duplicate call site call:385a3223e8cfb7ea`。

prepare/readiness未成功，因此未发出对应search；不能删除这些source文件、缩小域、绕过readiness或把未运行query记作零分观测。本任务不修复生产，后续若修需另一个明确固定源码版本、事先冻结输入/参数，再完整重跑；本失败基线保持原样。

## 可算报告与not_run

analysis/summary.json及完整raw归档保留原scorer的12个实际完成suite的Top1/nDCG/category诊断；所有Partial状态同时呈现，不宣称完整质量。举例，Express native的既有answerable MeanTop1=0.0847457627、nDCG10=0.0972817406，compat分别0.8135593220/0.7839612853；两profile的匹配规则与部分source域不同，不能作为候选minus基线的同指标增益。TypeScript五个完整block的双profile原分数也全部保留，不只挑最好block。

四repo宏微、family-balanced macro和candidate-minus-baseline paired intervals **not_run/invalid**：有888缺行，不能只保留幸存repo或把missing作为零分填充。case-means.jsonl保留每query既有scorer重复均值/适用数，缺观测为None；256个配对投影key中118完整、138缺测，没有把它们计成独立样本。现有raw的2000次未分层family bootstrap仅是原evaluator诊断，不冒充预注册10000次全局paired bootstrap；这里只一个产品arm，native/compat是不同公式，未进行双arm推断。

45个native no-answer queries计划135行，实际75行全部Partial，因此strict no_match+empty正确数0、错误75；另60行missing，四repo拒答accuracy无效而非0/135伪完整结果。既有scorer与strict audit在实际75行的差异0。

783个originating-work receipts、所有elapsed/normalized status、lane/budget/generation/raw source/facet/span相关输入与返回原样保留。analysis/lane-audit.json对所有783行既有lane receipts/status/truncation reasons作纯计数，semantic state全部disabled、effective policy全部local；不改限额、不把origin receipt当当前cache-hit工作量，也不发新search。2643 returned hits的既有source byte verifier结果为invalid0、unverified0；不由此宣称facet/graph正确。全部Partial RPC的783个elapsed观测约17.373–168.632ms、均值100.078ms，只是响应时间；既有Success/NoMatch latency summary无合格样本。成本是originating work，不能相加称作cache-hit当前工作量或token/API货币费用。未执行release/连续峰值/200样本tail性能认证。

required-facet coverage、graph correctness、Recall20、SymbolAccuracy、DuplicationRate未实现；full freshness、semantic ablation、正式heldout/六repo600评测not_run。formal600、cleanholdout、protected-body reads、live/paid providers均0。公开dev已有曝光，不转为heldout；没有读取旧/隔离/heldout question/gold。

## 原始证据与readback

public-development-raw.tar.gz保留pilot和full的1200个文件（约30.95MB原始字节，4.04MB压缩），含每次run/replay commands/stdout/stderr、全部manifest/queries/normalized/scores/metrics/costs/latency/resources/failures与864个实际raw search response（81pilot+783full）。缺行suite原prepare失败manifest也保留。retained-licenses/按admission哈希保留15份原license/NOTICE/第三方BSD证明文档，raw公开source片段不脱离这些notices。

archive SHA256：`ed873c1c105cffc91babf6b195d7f65a94e9d6b4f745b6616cd5aef2341148be`。

readback.py重新解压、核验所有1200文件byte/SHA，使用同actual evaluator对18个run（pilot2/full16）离线replay，全部文件不变、退出码仍匹配原1/2；analysis summary逐字节一致。readback receipt SHA256 `a96991861beee04f3b4c84f58e7be60bdb18bd411a4c899c56787d3430b7a14e`。这证明失败证据可重放，**不把gate1/2变成pass**。5项有意义的脚本回归通过（未准入读取先拒绝、semantic feature拒绝、Partial拒答规则、宏/微/组件权重、真实missing schedule不出全局分数）。未重跑全仓tests/clippy/fmt，也未再次发出search调参。

## 复现

在包含固定author commits/PR91 admission的此分支中：

```sh
git worktree add --no-checkout --detach /tmp/v19-dev-source-78b0 78b0ce52cb2ff4c53c53893b9f7dd469269806b8
git -C /tmp/v19-dev-source-78b0 sparse-checkout set --no-cone '/Cargo.toml' '/Cargo.lock' '/rust-toolchain.toml' '/crates/*/Cargo.toml' '/crates/*/src/' '/crates/*/build.rs' '/.cargo/config.toml'
git -C /tmp/v19-dev-source-78b0 read-tree -mu HEAD
python3 <this-directory>/build.py --worktree /tmp/v19-dev-source-78b0 --output <new-build-receipts> --binaries <new-frozen-binaries> --target <cargo-cache>
python3 <this-directory>/baseline.py prepare --inputs <new-inputs> --build-receipt <new-build-receipts>/build-receipt.json --plan <new-plan.json>
python3 <this-directory>/baseline.py execute --phase pilot --plan <new-plan.json> --output <new-pilot>
# 核对完整unique schedule/原run和replay一致，再按固定continuation rule进行full；不要以gate1否认已有Partial观测。
python3 <this-directory>/baseline.py execute --phase full --plan <new-plan.json> --output <new-full>
python3 <this-directory>/analyze.py --plan <new-plan.json> --run <new-full> --output <new-analysis>
```

新的编译路径可能改变binary bytes，必须用自己的实际compiler-artifact/build receipt，不借旧hash；源码和参数保持冻结。复跑自然不要求墙钟elapsed相同；保留raw的offline replay则要求所有文件hash相同。只做保留raw readback时，按原receipt建立对应exact copied binary路径，运行readback.py --output <new-readback.json>。5项测试中的实际完整性fixture使用/tmp/v19-development-full；其他workspace需先解压full到该路径，不能把skip说成已执行。没有GitHub/SDK权限拒绝后换路。

本PR仅追加development-baseline/脚本/证据，原三/四repoadmission和pre-ranking receipt不改；主registry、ledger由集成者同轮落账。本基线不能关闭V19质量gate，实际生产index blockers需要主任务按固定源码证据独立处理。
