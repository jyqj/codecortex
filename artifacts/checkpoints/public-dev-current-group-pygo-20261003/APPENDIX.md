# Requests / Gin 公开 DEV 当前固定产品重跑附录

仅 group-pygo：Requests 91 native/83 compat，Gin 67 native/55 compat；4 suites、158 native/138 compat、888 scheduled rows。不是四 repo 16 suites/1671 全量，也不是 holdout/正式 600 题。公开 DEV 已在旧基线曝光。

产品固定 PR131 `88f2cf099c8b81f3acef485fd5ac9b01c63ce790`；crates/Cargo 共708文件与 `e3c04fed903c4d0e3c26b5d7cf0e055a6f7c5207` 相同。协议 PR91 `5385f5a7a2a875c6d5cbd049bdde039bf71bbf32`，admission SHA256 `b4388ca60612f6734719b348bf30e05dd0752b7b754f47cc078b590e35c1e425`。原 author commits 的 admitted suite/query/gold/source 字节逐一核验；未更新 gold、排名、查询、阈值、统计计划或产品。原正负例、source domain、timeout30000ms、top_k10、repetitions3、warmup0、seed20261003、default nonsemantic/local hybrid、quality profile 原样。TypeScript 原 pilot 不属本组，not_run；不另选 pilot。

排名前 receipt 已在 `877065ba3387895d7aac880068e3e2f07f7ac107` commit/push 并核对 remote。实际当前二进制由官方 Cargo.lock、installed direct Rust1.95.0、默认 features 构建。两次离线 cache preparation 缺依赖日志保留，随后正常平台代理 Cargo --locked 构建成功；未使用 mirror/env-i/权限改动。normal platform environment 保留；仅沿用原可选 process-tree probe=0，native PID RSS 独立。没有以 build 成功替代质量。

| Suite | scheduled | executed | missing | Partial | NoMatch | Success | MeanTop1 | Mean nDCG10 | run/replay exit |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| requests native | 273 | 273 | 0 | 273 | 0 | 0 | 0.0000000000 | 0.0000000000 | 1/1 |
| requests compat | 249 | 249 | 0 | 249 | 0 | 0 | 0.7710843373 | 0.7769516268 | 1/1 |
| gin native | 201 | 201 | 0 | 201 | 0 | 0 | 0.0727272727 | 0.0945003959 | 1/1 |
| gin compat | 165 | 165 | 0 | 165 | 0 | 0 | 0.8000000000 | 0.8147028107 | 1/1 |

888 行全为 Partial；四套 gate1。完整 schedule/replay 仅证明测量完整性，**质量没有通过**。表中分数只是原 scorer 的可计算描述值，不能将 compat 高分或 Partial 当 complete retrieval 成功；native/compat 为同批问题的不同公式投影。原每-query 三重复均值、micro/repository/category/family-balanced 描述值在 analysis/，仅本组两 repo；138 paired answerable projections 完整，不是双产品对照。没有将 scheduled repetitions 当独立问题。

Requests invaliddependency / Gin duplicatecallsite 的旧 index prepare 阻塞在本轮未复现，4 suites 都准备成功并产生 rows。未重试 search；旧 raw/失败/306missing 证据未修改，也不据本组推断其余两个 repo 或关闭 V19。

native no-answer20题/60行全部 Partial，strict no_match+empty 正确0、错误60、missing0；原 scorer 与 strict audit 差异0。2616 returned hits：原 source verifier invalid0/unverified0；这不是 facet/graph 正确性认证。

lane originating receipts：exact_symbol complete888、grep complete888、graph partial888（graph_expansion_limit）、lexical partial888，path complete42/partial846。semantic disabled888、effective local888。完整 lane reason、成本可用性、响应 elapsed、category 等诊断全部保留。originating work 不能当 current cache-hit work，质量 profile 的 elapsed 不能作 release/tail/continuous-peak 认证。

not_run：Express/TypeScript、原 TypeScript pilot、全四 repo aggregate、candidate-minus-baseline 10000 次 paired intervals（仅一个产品 arm）、semantic/live/paid providers、full freshness、release performance。required-facet coverage、graph correctness、Recall20、SymbolAccuracy、DuplicationRate 仍 not_implemented；保留证据，不用代理指标。formal600/cleanholdout/protected-body reads/live-provider calls/paid-provider calls 均0；未访问 serde/vite 拒绝源、旧 denied42/private localdiag、semantic_runtime EROFS 目标或 GC/WAL/kill/staging 故障 tests。

新 raw 归档仅本轮 run/replay evidence（包括已准入公开 DEV query/gold 与返回 source 片段），不含整库 source、binary 或 db。retained-notices.tar.gz 保留本组9份已准入 license/NOTICE/归属文档原字节；不归档旧源码证明文件。readback-receipt.json 核验解压、4套离线 replay 和 analysis 重算均逐字节一致；exit1 保持失败，不包装为 pass。

未重跑全仓 tests/clippy/fmt：本次仅 frozen DEV baseline，用户明确排除故障 tests，不改产品；验证为 locked build、hash intake、完整 schedule、run/replay、archive readback/source inventory。

发布：本组证据 commit `ed5428efd2b60e4e71aac540ec0c5a68af1a1ece` 已 push，origin remote SHA 实际核对一致。`gh pr view131` 元数据读取返回 GraphQL Forbidden 后停止，未替换通道。授权 draft PR 创建仅尝试一次，同样 `Post https://api.github.com/graphql: Forbidden`、exit1；没有 PR，已停止创建，不使用 REST/browser/其它账号替代。实际 command/stdout/stderr 在 draft-pr-receipt.json。可用交付为已 push 分支 `public-dev-current-group-pygo-20261003`，等待 GitHub 授权恢复；不把 push 成功说成 PR 成功。
