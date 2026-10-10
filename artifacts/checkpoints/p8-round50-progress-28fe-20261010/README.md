# P8 第50轮进展证据

冻结时间：2026-10-10T04:58:00Z

## 权威账本

- main: `b9b089bb4eae072affe9326681d4980eae15fd84`
- main tree: `6461938788665701cfa4fa095a064a3a404ec492`
- tasks blob: `adb44ee70a4c129bfb443d4ecf678fb934c8ea6b`
- 192 total / 164 done / 15 in_progress / 12 todo / 1 blocked
- 剩余 28
- 本轮新增完成原始 TODO：0
- 相对 163 基线累计新增：1（仅 P8-005）
- 下一个原始任务：P8-006，保持 in_progress

## 规模研究终态

full-wide run 38013753078 已于 2026-10-10T03:54:59Z 结束为 completed/failure。固定身份为 source `9cc6bf49f6dd81e4069a8004eed49addba0ab79b`、tree `6461938788665701cfa4fa095a064a3a404ec492`、profile `scale_wide_dirty_v1`。1k/5k/10k/50k 各仅 rep0 成功；100k job 114100438875 失败；aggregate 114125450702 失败；measure 114125451149 跳过。

matrix artifact 11658738280：ZIP 580 bytes，SHA-256 `716be1cef60ff03736583ca7bca4dd75be435c44fc6936aa801df6cf57a4f32b`，CRC 通过。matrix.json 为 status=failed、passed=false、release_certification=not_run，缺失 146 个矩阵位置。因此仅 4/150 成功，1500 个复合观测、成功汇总、测量与 release certification 均未完成。旧研究与本研究不合池、不拼接、不重标。

## 诊断运行

run 38012409915 在冻结时仍 in_progress。checkpoint 08–12 已完整接收并通过 GitHub digest、本地 SHA-256 与 ZIP CRC 核验。连续原始前缀由 [0,14714549) 延伸到 [0,16145239)，新增 1,430,690 bytes；capture_faults=[]，但 native_EOF_claimed=false、supervisor_terminal_observed=false，尾部未知。

API 阶段已完成并通过：9 次 incremental 299.008236s、full control 797.075890s、parity 1040.236485s，15 表相等，scratch peak 5,984,473,088 bytes。Config 已完成 17 次 incremental 共 378.067165s 与 full control 1232.572187s；config parity/stage_finished 尚未出现。最大单 PID 离散 RSS 快照 11,078,230,016 bytes 不是完整进程树或 OOM 证明。终态、exit、EOF、shard.json、measurement_complete 与 observer-after 仍缺。

## PR 管理

新 PR #201 起始 head `750e5b33a0cbc7702d8c85827763c9a76d4b46ff` 的 CI 确认有一处 rustfmt 阻断。外部协作者已在本轮将 head 推进到 `f97c5068d056969705e8387ba1f37d83847f5bee`，实际文件已展开 ScaleShard 字面量，并更新独立审查绑定；新同 head CI 已启动但未完成。未覆盖其提交，PR 继续 Draft/HOLD，不合并，不授予原始 TODO 信用。

本轮未 dispatch、rerun 或 cancel 任何规模研究，未运行收费 provider/holdout，未修改任务状态。
