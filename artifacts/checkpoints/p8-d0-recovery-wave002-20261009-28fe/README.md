# D0 continuation wave002：唯一前瞻补充与容量调用修复

这是尚待非作者审查和 root 发布决定的候选。它仅登记 `(100000, repetition 8, ordinal 2)`，使用新的独立 branch 和 package；已发布的 wave001 / ad840759、原 D0 注册及全部失败记录不变。测量源码仍为 d0cb69c601e530dcef738af0c2dffb8d3b8bcf28，原 150 格 / 1,500 样本、seed、原 release binary、18,000,000 ms / 512 MiB / 15 表等预算和原 validate_shard / combine 标准不变。

## 已观察到的精确故障与前瞻协议

wave001 原 run37865643378 / job113611597284 的官方唯一失败 step 是容量准备；执行 step 明确 skipped。原 ZIP11588924279（60,953 B，SHA69639897e691c708dba91be441a7807df7d076ac12aea9afd7e8a280a439ef01）仅有 admission、capacity、recheck 三个目录及 35 成员，无 execution / started / shard 原件。原 capacity receipt 在检查 script 是否属于 workspace 的守卫处 exit2，错误为 `capacity observer belongs to another checkout`。原可用 88,661,061,632 B 超过原要求 30,509,367,296 B；原 helper 没有进入 SDK 清理，也没有进入 native driver。

原 workflow 已 `working-directory: measured`，但容量进程继承的 GITHUB_WORKSPACE 仍是上层 controller + measured 工作区，因此原 script==workspace/scripts 守卫正确拒绝。候选只在该条容量 Python 命令前加进程级 `GITHUB_WORKSPACE="$GITHUB_WORKSPACE/measured"`。此赋值不影响其他 step，原 helper、Git HEAD/blob 校验、容量阈值、固定 SDK 白名单及 fresh hosted VM 删除守卫逐字保持。

这不是原 143/shutdown 分类。新增 wave-v2 前瞻分类 `pre_native_capacity_observer_checkout_mismatch` 只适用于固定的这一条已核原件，policy SHA20d94bc4798c2a6277fb7782307c868283b581b545460ac12d82ced2ce4163e8。其 prior receiver SHAeccffe7a89abdf3686306ddf0e54bab190fe429abb57861cd5a35b1ea8311d4a 中的 wave001 `state=invalid` 和两条原 `known_errors` 保留；仅追加本次核验的控制失败解释。未知 native 结果、耗时及费用保持 null，旧失败永不被选择为覆盖。

## 启动与证据约束

- 所有原 150 初始 identity、完整原注册、原 snapshot 及完整 151 条 prior attempts 都不可替换。新 receipt 分别列 150 条 initial、151 条 prior 和包含当前实际 job 的 152 条总数；当前 job 的存在不被当作 native 已执行。
- 原 rep8 的 143/shutdown 仍必须精确重读原完整 log，且无后来出现的原 shard / 已知失败。wave001 的 current canonical job、完整 log、唯一 artifact ID / digest / size 和内嵌原 ZIP 均逐次核对。任何变化、额外 native 错误、原状态回退、过往错误或 late revocation 不能被忽略。
- 每次 admit、execute 之前及最终 recheck 都重读原 run / jobs / artifacts 以及两个登记 branch 的完整 workflow run 集合。新 branch 只能有一个本 wave run、其 Actions attempt 必须是 1；旧001重跑或重复002push会阻断。固定全 lineage concurrency group 保留，cancel-in-progress=false，单格串行；不调度其他 149 格。
- 初始 ordinal0、supplement ordinal1 和本次 ordinal2 永久累计，per-cell 最多 2 个补充 / 全150格最多300个补充，不通过改名重置。原最大20与补充串行1的声明上界仍分栏为21，不称实际独占环境。
- 原 ZIP 仅以其精确字节的 base64 文本保存在候选中，解码后 SHA 和长度必须与官方 artifact 相同。程序不解包写文件，只读取所有已固定成员核 CRC 与原 receipt。没有重新打包原 ZIP。

## 覆盖、失败与本地验证的边界

本 controller 的 coverage 永远 false。它不能将旧 D0 study 改成 passed，不能自动解除原 capture001 SHA993496adfbb7c1237f550a7e439ffc16ca5cd43d40ffdd5834e38d310cb5744d 的待独立调和门，也不能关闭任何原 TODO。新整体覆盖须由另行独审的 receiver v6 / 原 validate_shard / combine 在全 150 格和全 attempts / late evidence 门通过后认定。

本地原 capacity helper 验证在明确的本地 host-path adapter 下执行真实 prepare body、真实 Git HEAD/blob 和真实 disk_usage，禁止 preparation / SDK 命令。错误 workspace 在原身份守卫处失败；正确 workspace 通过原 D0 源码身份检查后，因为本地可用仅 48,517,120 B，按原阈值返回 not_run_capacity_unavailable。保留首次预期 capacity_available 断言失败及两个原 receipt，没有把它改成 hosted VM 容量成功，也没有降阈值或清理活动数据库。原 CLI 在真实非 hosted 本地环境先行拒绝。

新控制使用真实固定 wave001 原件，并将尚未发布的当前 wave002 API 身份明确标作 synthetic。这些控制仅验证准入、历史、路径与注册约束，不产生任何 native 测量或原 TODO 完成。
