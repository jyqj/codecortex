# Wave002 receipt 与原 wave001 的差异

本文件描述冻结 controller 的输出契约，不是假造的实际 wave002 运行收据。尚未发布或执行 wave002；实际 run / job / head 必须在登记后的唯一 GitHub run 中读取。

| 字段 | Wave001 | Wave002 |
|---|---|---|
| `phase` / 成功 status | admit / admitted_wave_only；execute / supplemental_raw_validated_pending_external_attempt_readback；recheck / predecessor_still_eligible | 保持原三阶段值 |
| `registration_sha256` / `controller_script_sha256` / `wave_id` / `wave` | 原001登记和原controller | 新冻结登记和controller；唯一 wave002 / 100k / rep8 / ordinal2 |
| `observation.all_initial_attempts` | 原150条 | 仍为原150条，完整初始身份不变；保留当前官方状态及以前已见错误 |
| `observation.all_prior_attempts` | 无 | 原150条 + 原ordinal1失败，共151条 |
| `observation.prior_supplemental_attempts` | 无 | 固定原ordinal1一条，始终 invalid 且保留两个原错误、原artifact/log身份及 native_outcome=null |
| `observation.predecessor` | 原rep8中断的分类摘要 | 原ordinal1完整失败行，额外含精确 policy 的 `control_failure` 与本次重核为真的 `control_failure_currently_verified` |
| `observation.original_predecessor` | 无 | 原rep8的完整ordinal0行；143、root_cause=unknown、native_outcome=null，必须仍无额外错误/晚到raw |
| `observation.current_wave_attempt` | 无 | 从当前官方唯一job取得完整身份；测量是否开始和native结果仍为null，不从job存在或时间推断 |
| 尝试分母 | 原150条 + 当前波次另行接收 | prior151、包含当前job共152、当前累计supplement2 分栏；coverage分母仍原150格 / 1500样本 |
| 原receiver / partial | 无完整receiver | 绑定 eccffe7a… 原151条receiver及993496ad…未调和capture；不能自动解除partial门 |
| `coverage_accepted` / 原TODO | false / 0 | 保持false / 0 |

admit、execute前及recheck的 observation 都重新核验两个登记 branch 的完整 run 清单、原 D0 全150官方jobs/artifacts/原rep8日志、原001唯一job/日志/唯一archive及固定build。execute成功仍须原 driver 返回0、原 validate_shard 完整通过、原plan和source保持一致；`native_measurement_started=true` 只在该原raw核验完成后写入。driver启动请求时仍为null。

原 `main`（包含 driver argv、运行和原验证器调用）的语法树与 wave001 完全相同。变更的是 load / observe 和固定 ordinal2 的前驱准入：原ordinal1已被允许的前驱仍必须为原143中断；仅这次拟议ordinal2可使用精确、当前重新核验的 pre-native capacity 控制失败。原两条 known_errors 没有删除或改成143。

失败和recheck撤销仍通过原异常 / 最终receipt路径落盘；没有第三次supplement回退路径。最终完整覆盖由另行独审的receiver核所有actual attempts及late evidence，原D0 study不会被此新范围翻为passed。
