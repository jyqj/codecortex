# A23失联后的两条路径与首次执行前的恢复设计（仅提案）

状态：prepared_for_non_author_review；无study、重跑、label、ref或任务变更。本文不宣布旧A23或新候选通过。原任务仍192总数、163完成、29剩余，本轮关闭0。

## 建议与选择依据

推荐路线B：先用正在运行的G2工程100k的真实终态判断工程可行性，完成PR173/174的各自审查并正常整合，再完成新P→R→G源码准入，在最后完整固定SHA上运行原150片/1500测量。G2工程小样本只用于判断该工程版本是否可执行，不能填新正式总体，也不能因为它更快而宣称A23变快。

路线A“保留A23四片作为最终N150，再另建controller补146片”目前没有严格兼容原合同的实现。不能以controller与measuredsource分离为理由绕过P8-SCALE.md278–281的完整SHA与重新build要求。它保留在比较中用于说明成本与阻断，不作为可批准执行路径。原文没有永远禁止未来恢复，但这不等于旧A23已经允许更换合同或采样选择规则。

若新的最终研究需要对runner失联作有界恢复，必须把恢复规则、实际native命令、driver/build绑定及原始记录保全方式，在其首次执行前纳入同一个完整固定提交并独立审查。不能先启动，再用新controller或重命名study重置失败/次数。当前不建议立即并行启动A23恢复与新源码完整研究。

## 已实际取得的故障事实

- 原run37872522779，attempt1，source a23bb72d3c954f385b99fe81ce9189885c208557，tree58147c952505c44da1f41eb4b9c31643f2303b96。
- 100k rep0 job113638100879于06:12:02UTC为completed/failure。官方check唯一annotation首句为“The hosted runner lost communication with the server.”；其后列CPU/内存/网络等可能原因，未确定具体根因。
- measurement step仍记in_progress、上传pending是已保留的API内容；不能把这种不完整收尾解释为native exit0、deadline3或纯基础设施免责。native是否完成、退出值、原阶段计数和latency均unknown。
- 原aggregate job113695707497已completed/failure；原matrix.json明确缺146个坐标、passed=false。原生预算18,000,000ms是否曾耗尽不能由job墙钟猜测。
- 当前原11artifacts只含build、5capacity、4有效shard与failedmatrix；没有100kshard。原4片为1k/5k/10k/50k的rep0，共41个已观测measurement identity。
- 原measure job113695708822为skipped/steps=[]。这是145个原计划坐标未开始的证据，不是145个各自存在的job，更不是145次失败native执行。
- 以上从只读run37893190079/job113698501180完整log还原六份原JSON，并逐份复算其bytes/SHA；同时只读核原jobs/artifact列表。没有新增下载或执行native。

## 原合同与路线A的具体阻断

原固定A23：
1. docs/roadmap/code-index-v2/P8-SCALE.md:67要求新输出、旧run和失败不重写；267–276要求五首片全成功后才启动145，并明确不自动重跑失败样本。
2. 同文件278–281要求完整执行固定完整GitSHA；即使只改变说明或workflow，也不能重标旧build receipt，须在新固定提交重新build并保留旧失败。
3. scripts/p8_scale_matrix.py:311–333严格核build/source/driver/helper/二进制；375–431每片绑定该build；754–819核原完整raw；861–946要求同source/binary/profile/input、150唯一坐标、50组各N30。源码和预算门不应改。
4. .github/workflows/p8-scale.yml的build/capacity/shard/matrix artifact命名仅含run_id，不含attempt。现代码没有跨attempt的前瞻资格、完整账本或选择器。
5. 09-BENCHMARK.md:130/136/173/183/191要求输入锁、保留无效/取消/超时与失败、禁止best-of、不覆盖旧run。

因此：
- 在新提交登记controller/协议后重build，会生成新source/driver/build-receipt身份；旧四片不能通过原validator与它混合。
- 继续用A23旧build虽然可能让某次原run_shard执行，但并未自动满足“前瞻协议与完整执行SHA一同固定”的要求。这不是证明原注册矩阵已恢复。
- 原生GitHub单job重跑也不是现成答案：它没有当前所需的每attempt工件命名/失联分类/历史选择协议，不能仅凭按钮存在推定旧run合法收齐。
- 把新controller作为完全独立的新登记profile并事后采用旧四片，需要明确修订/批准原合同适用性，已超出本提案的不改原门范围。本文不请求这种豁免。
- 四片依然完整保留，可做诊断、阶段比较和失败研究的事实依据，绝不删除；路线B不会将其计入新的N150。

## 两条路径的实际成本

| 路径 | 可以沿用 | 必须新取得 | 当前阻断/风险 |
|---|---|---|---|
| A23四片＋新增恢复协议 | 原A23四片、旧build及所有已审runtime/platform事实，身份不变 | 至少100k恢复1片＋145首次执行及完整外账本 | 新协议/完整SHA重build规则与旧四片混合不兼容；尚无合规闭合实现，不能启动 |
| 最终新源码完整研究（推荐） | 原失败历史、已审源改动与测试作为review依据；同域证据仅按独立适用性判定引用 | 新完整SHA的build；原5首片＋145；最终原聚合与全部实际适用证据 | 先固定实际组合源码与新准入；G2工程100k未完成时不预测成功；不能把任何A23/G2样本改标签填新总体 |

保留四片只减少150中的4个测量分片；它不能弥补source锁缺口，也不值得为节省这4片牺牲原合同。新源码的reconcile窗口/写入批处理可能改变耗时和资源，旧A23 runtime/RSS/scale不能自动认证新产品。另一方面，不应无理由重跑所有旧证据：对最终组合中仅test/cfg(test)或不相关域的差异，应先明确逐域适用性；需要实际运行的源变化再列最小集合，不能仅因GitSHA不同宣布所有语义历史失效。

## 可在新研究首次启动前实现的最小有界恢复协议

此节是候选设计，不是已批准原项目要求，也没有对A23生效。新完整固定提交必须包含全部规则与执行代码；若首跑未纳入本协议，不能事后声称存在。

### 1. 身份与总体分开

冻结150个计划cell=(scale, global_repetition)，原5×30、九主阶段与1k独占五fanout保持。每个cell的注册plan通过原registered_plan构造并保存原字节摘要，不手写替代它。

每个实际执行attempt绑定：
- study完整GitSHA、workflow文件SHA、原driver/helper SHA、native source manifest、build原artifact ID/ZIP SHA/完整build.json SHA与binary SHA/BLAKE3；
- cell、ordinal、parent-attempt identity、repository ID、实际run_id/run_attempt/job_id；
- preregistered quota与资格类别、用于准入的前驱API/check/annotations/raw artifact摘要、准入时刻及实际controller源；
- execution_requested、实际原raw证明的native_started/terminal/outcome（无证据为null）、artifact集合及SHA、所有失败和unknown。

计划slot不是执行attempt。未创建job的slot保job_id=null、not_started；已出现的attempt即使工件过期/删除也永久存在。不能将queued job自动started_at当native开始。

### 2. 允许补充的类别与禁止条件

建议前瞻配额为每cell至多2个supplemental attempts、全研究至多300个supplemental attempts；这是此次建议的有限调度选择，不是原task硬门，不意味着达到配额后项目永远不得修复。此配额不授权同时创建这些job。

仅当前一attempt已官方终态，并有固定官方runner lost-communication/shutdown记录且本次已收完整可用证据中未见native/correctness/parity/source/evidence/budget失败，才可分类为eligible_unobserved_interruption。资格含义是允许另一次观察，不是证明前次pure-infra或产品成功。

任何已知原生deadline/输出预算耗尽、parity不同、closure未完成、source或build不一致、输入/原件损坏，都保持失败并阻断通过；最后一条runner annotation不能覆盖较早失败。已成功有效cell不可重测；未终态attempt不可叠跑；同cell不可两个active；同一lineage更名不能重置次数。其他pre-native错误没有自动豁免，需具体证据和原协议范围判断。

每次开始前重新核前驱状态与所有晚到原件，先保准入记录再启动原driver。拒绝的准入也留原件。用原runner/capacity helper及原预算；只允许其原白名单host preparation，不新增宿主清理或提高deadline。

### 3. 选择规则与晚到证据

选择与运行快慢无关：采用每cell最早完整有效attempt。只有前序attempt按冻结规则可补充且不存在已知禁止失败时，才允许后序入选。不得成功后再测，不挑更快host或更好结果。

补充启动后原件晚到时先重算：
- 若早期attempt的完整原raw有效，恢复其优先选择；后来attempt仍在全账本，不能删掉或两次计N。
- 若早期或任何已见attempt暴露native/parity/budget/source/evidence失败，撤销该cell原coverage资格并输出blocked；后来成功不洗掉它。
- 身份冲突、未知额外attempt或尚未调和的partial收件阻断最终coverage；普通当前API短暂缺页属于可恢复incomplete，不能误写永久产品失败。
- 所有已知失败/撤销/原artifact身份跨snapshot单调保留；每次receipt绑定前驱digest，不能省略previous来清历史。
- 承诺仅覆盖已登记workflow/branch及已发现lineage的完整API范围，不虚称证明任意其他命名实验不存在。

### 4. 计数与结果

分开输出：
- expected_cells=150；selected_valid_cells；selected_unique_measurements=1500目标；每组N30及原host/CPU/kernel strata；
- 全部真实execution attempts及其valid/failed/unobserved/cancelled/not_started分类；
- 已知失败数、未知终态数、source/预算/原件错误、deadline-censored记录；没有原native值时null，禁止填0；
- 原native latency只从原raw取得，job墙钟不当native latency；原可靠性事件不从分母移出。

若举例采用旧A23四片（本路线尚不兼容）：原只有5个实际slice请求，145未开始；随后1次100k补充＋145首次执行会产生151个执行attempt，而非150个全成功attempt。150selected不能抹掉第151个原失联记录；它的9个计划measurement也不能被捏造为9个已测失败latency。

完整覆盖结果只表示原有效样本总体已收齐且所有原断言通过，不等于每个历史attempt成功，也不自动关闭G8/任务。已知产品失败不得在可靠性旁表中降级为不影响门的注释。

### 5. 最小代码/文件变动边界

保持原native生产代码、registered_plan、run_shard、validate_build、validate_shard、combine、oracle、seed和所有预算原样。

若root选择新研究带恢复，最少需要：
1. 新研究registration JSON：固定整体source与150坐标、build身份、有限前瞻配额、资格/选择/晚到规则。
2. 一个小型外层admit/receive模块：官方身份与先前事实核对、append-only attempt ledger、确定性选择；最终把恰好150个选定原目录交原aggregate，自己不实现第二套raw统计或oracle。
3. 一份受审workflow：原build一次、原5首片门、原145计划；工件名字包含study/cell/ordinal/run/attempt，原失败全部always保全；任何重执行先经admit，绝不用overwrite覆盖工件。平台原生retry的实际job/下游行为须在准备时按官方语义核实，不能凭猜测扩散运行。
4. 少量有意义控制：成功cell重测拒绝；失联但此前native错误拒绝；未终态/双active拒绝；跨wave次数不重置；晚到成功优先原attempt且不双计；晚到失败撤销；缺页/partial与已知错误分离；原150/1500聚合仍缺片/重复拒绝。

现有D0 receiver可借鉴已独审的hash/ZIP/history设计，但其D0 source、特殊前驱及固定attempt身份不能直接用于A23或新study。不要机械复制整个大receiver并声称已适配。上述代码尚未实现/批准；没有反复编写新通用框架的需要。

## 推荐的下一步顺序

1. 继续保留并等现有G2工程100k真实终态；不加新并行规模任务。终态成功只证明它自己的工程scope，不认证新study。
2. 正常整合173/174，按现成组合地图完成实际P/R/G和原源码门，确认固定最终源码与适用证据。保留原A23/Mac失败及所有旧研究。
3. root在首次执行前决定使用原无恢复workflow，或先审上面有限恢复设计并将其一同冻结。若不加入，就诚实承担原workflow遇失联仍failed的语义，不事后加例外。
4. 唯一固定新study先跑原5首片；全部原断言通过后才启145。若原生预算/correctness失败则停止，具体修产品后另固定source，不能追认恢复为pass。
5. 收齐后用原aggregate一次性重算150/1500/50组N30，并核完整attempt账本与后续证据；结合已准备十项ledger逐条闭合硬依赖。只有实际满足原条款才能将005..013/016关闭，届时173done/19remaining，而非现在。

本报告是方案作者交root的待审建议。未发布协议、未触发run、未改refs/labels、未改source/任务/预算。

## 固定只读依据索引

既有政策报告：Gitblob d7c01152ea3fedfa558a84e71780c04953ec921f。本文补充审查其泛化continuation建议在A23完整SHA条款下的实际可行性，不改写旧报告；现在明确路线A未获严格兼容性证明。

- P8-SCALE.md完整路径docs/roadmap/code-index-v2/P8-SCALE.md，A23 blob d23c80666c8885ee7274cdffc3c023e6a0ef9e08。
- 09-BENCHMARK.md完整路径docs/roadmap/code-index-v2/09-BENCHMARK.md，A23 blob5ced0c455e7e774a9d95da23187c64dcefc37314。
- 原workflow blob74f1b64ed447ef24da584f1c1f8d090d2fa1e03e；matrix driver b4ba4b57f8d23a16128ef82927e78799ae70f886；capacity helper16d150d837e98c900b49de2059aa4ca74fe2f02f。
- 只读终态收件run37893190079/attempt1/job113698501180，controller53fbfbdc86faacdcd241f0c619b5110e3aba259d；原完整log55,915B/SHA25630d26ce703c47f83b97735f82c144eb6ff293121237bfc11df469d7c4ea19e13。
- PR173+174只读组合地图Gitblob2c957f172d845afa816a4728e0d59753b5225aef，仅准备1089输入/44delta/139validation，并非实际组合提交或执行批准。

| 终态收件打印的原JSON | bytes | SHA256 |
|---|---:|---|
| original-run | 11863 | 00b06a562e1a0a0611b940727bd10f51cb6c85d2d2df9e0efae082d32714cb4c |
| original-job | 2482 | 66cd42ba7db2be8284c6c223e936923aaee48eb0e898b39f7eea489d295fd2ac |
| original-check | 3051 | f18aac4ecc5b2f70e0458fc2d374e7bd07d1693937ae2d0a2b0549bd01585b8c |
| original-annotations | 452 | bc1ad4395886b208c0d13e493a95e17c657285fdf659c4f8c26021e1add629bc |
| original-artifacts | 7955 | 8defbbe2ce5c1653102ed11d19ecc770023cd571ebe4bf5d39e5d9e9bc30288c |
| original-failed-matrix-member | 1978 | c14cf58287f4cb49cd942e5b26150ab46156375a19157d17426eb8301d2ae6ea |

这些六份raw_utf8的UTF8字节已各自验证同原打印收据，不从解释摘要重造原件。

| 原A23工件 | ID | bytes | 官方ZIP SHA256 |
|---|---:|---:|---|
| p8-scale-matrix-37872522779 | 11599136237 | 580 | 6585b064492fba4b0f1cd8f10cfeb304ee63fa15851ede907c20a012780902e3 |
| p8-scale-shard-50000-0-37872522779 | 11593548201 | 2964759 | d0163f3421ea9abf3d544edad56a9386aec2db55fef0aa11155e30e10a27b06d |
| p8-scale-shard-10000-0-37872522779 | 11591693031 | 853798 | d5a73580784d5bb247adf0d0e6d189f45215600df62bc94e4ca88bdf0ab6a444 |
| p8-scale-build-37872522779 | 11591482043 | 9214568 | d84d66e9048aa2849d037af9df06fcc34423ca7a4e7937f3e79e4d77b441afed |
| p8-scale-shard-5000-0-37872522779 | 11591464502 | 583603 | cfc34907c79a13ceede8ad7a892af2d0861ddd2b978ba36105ad26fba2da010f |
| p8-scale-shard-1000-0-37872522779 | 11591367982 | 952631 | a5eeda7a03131fd0d5cebfd26bbcdfb0ff07db32f303593219665f0aa7d2b5b6 |

此索引不冒称本次重新深读ZIP；原四片和build已有独审，本文只核官方身份与原failedmatrix读回。任何真实新执行仍需完整build/shard校验。
