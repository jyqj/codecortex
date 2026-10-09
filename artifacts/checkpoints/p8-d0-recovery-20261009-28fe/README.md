# D0 continuation：仅第一格补充的控制器

本目录是新的、独立登记的 continuation 调度控制器，基于 G4 `260f596582f2d82b8d7c707b61a6b8b6a43b069f`。被测源码始终为 D0 `d0cb69c601e530dcef738af0c2dffb8d3b8bcf28`，原 build artifact 始终为 `11582571291` / run `37846370300`。它不是旧 ca72 study 的重试入口，不修改旧 `failure_policy`、原 native driver/helper、seed、150 个身份/1500 样本、15表 oracle 或各预算。

当前程序硬锁 **wave-001：100000文件、repetition 8、supplemental ordinal 1**。没有全150矩阵，没有自动追加其他格，没有实现后续wave的发布/调度。新工作流仅在专用分支push时运行，Actions workflow attempt必须为1，固定并发组不取消旧任务；运行前完整检查该固定branch/workflow的run历史必须仅有此次run。重复push/原生rerun/其他未登记run会阻断。本协议每格最多两个supp、全150理论至多300supp是前瞻自选配额，不是项目永久硬门，也不是运行300次的授权。此控制器只会执行上述一格一次；后续wave必须另登记具体cell/ordinal、纳入实际累计历史并独审新实现。

## 原总体和中断状态

`initial-identities.json` 冻结原run `37854240827` 的全部150个初始cell/job身份，包括当时queued/running。原完整API页、原注册和rep8原日志逐字保留；map由既有官方分页而来，source路径和SHA可追溯。Job started_at字段不单独当作测量已启动。

原rep8 job `113580044384` 的官方终态与全文日志证明exit143和runner shutdown，原shard上传skipped；具体原因和未上传的native结果仍unknown。只有这个可观测 `runner_interrupted_unobserved` 类别候选可准入。其他已知native、parity、source、证据、deadline/预算失败不可补过；先出现产品错误再shutdown也不可免责。原ca72 study永久保留其原规则下failed/incomplete，不由新成功改写。

目前prior supplemental ledger为空，指本新受控lineage的登记初值：包装时专用branch ref不存在，执行时须查询固定branch/workflow的完整run历史并要求只有此次首次run。**这不是扫描证明整个仓库所有任意未登记实验都不存在。** 外部未登记attempt不能被采用；若后来发现与本lineage有关的未声明attempt，当前准入依据失效，不能改名重置次数或挑最好结果。通用`check_ledger`控制ordinal连续、同格无双active、全局jobID唯一、原150完整、累计配额和失败/成功不得替换；首wave主路径不会把它包装成已经具备跨所有未来wave收件的完整系统。

## 准入和实际执行

执行前两次读取官方原run/attempt1全部jobs、全部artifact元数据和rep8全文日志，绑定固定repo、branch、path、controller、jobIDs。未知额外job/artifact、分页改变/截断、重复身份、原log字节改变或后来出现原rep8 shard均失败。API token只发固定api.github.com仓库路径；job日志重定向去认证，只接受有界HTTPS Azure Blob日志地址。所有收到的原JSON/log及其来源/摘要保留，非目标已完成失败仍显式未分类、native未知，不记成无错误。

在真实D0 checkout中核原1087 inputs、observer/helper、原build receipt与binary，调用**原**`validate_build`，执行一次完整原rep8，然后调用**原**`validate_shard`重放该格。原seed12648430、N30、shard count30、18,000,000ms、scale_capacity_v1及原cap preparation保持。原所有阶段/预算/完整parity都由原代码执行；不改失败阈值、不重封/覆盖旧run、不用599或其他source分片填D0。

`started.json`只记录driver launch请求，native是否启动保持null；只有实际原完整raw validator通过，才记native measurement started=true。driver失败、Popen失败或raw缺失不虚构native数量、时延或成功。每个目录独占创建，stdout/stderr、实际driver退出及失败收据保留。

## 明确尚未完成的验收边界

本程序只产出首wave准入、原单格raw、本地重放及job末即时recheck。`coverage_accepted`始终false；即便单格成功，也只记`pending_external_attempt_readback`。本轮**没有实现**未来所有wave的完整外部收件、150格完整覆盖认证或长期迟到证据监控。

Job末recheck发现迟到原件或错误会追加revocation收据并非零退出，不修改此前原收据。原件发布后的未来迟到证据仍需最终外部收件器再次审查，并可吊销准入/coverage；一次job末检查不能证明未来永不出现新证据。最终收件须核实际current controller commit、run/job/attempt、所有上传/遗漏、前驱recheck、原150全部有效观测与所有失败/unknown/补充attempt账本，然后才可形成新continuation coverage结论。执行次数与150个覆盖身份分母分开，host/CPU/kernel继续分层，不算pooled stable p99或因果提速。

新补充为串行一格；旧study仍可并行20格，因此本协议独立声明调度上界21而不冒称原20格调度不变。未宣称隔离资源测量。只要原native/parity/source/evidence/预算失败已知，就不能借后续success翻绿。源码、runtime、原硬依赖和TODO关闭仍分开；本目录不关闭任务，原192/163/29账本不变。

## 控制与发布

`test_recovery.py` 使用冻结的真实初始API/rep8日志作为正向证据，再构造明确标注的合成篡改反例；不会执行产品、规模矩阵或外部API。这些控制只验证控制器安全边界。作者尚未发布/注册远端commit或触发任何重试。主智能体必须在独审通过后核实际新增commit/tree/registration字节，再正常push专用分支，仅启动此明确已准入的一格。原PR产品分支和原ca72 guard保持不变。
