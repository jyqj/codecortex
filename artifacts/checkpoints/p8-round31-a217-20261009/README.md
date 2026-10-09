# P8 第 31 轮固定证据

截止时间为 **2026-10-09 12:34:58 UTC**。原 192 项任务仍为 **163 已完成 / 29 未完成 / 本轮新增关闭 0**。本提交只增加公开审查资料，不改产品、工作流、任务定义或状态。

| 固定来源 | 截止时的实际事实 | 保留的限制 |
|---|---|---|
| C `3ffcefc3b28ee1a4ed80caecebd7208a45c3e302` | 6 个 runtime 原件已接受；8 个平台格与独立执行的原 collector 已接受 | 官方 collector 仍 queued；平台是选定冷构建与 stdio，不是八次完整 workspace suite |
| C 原规模 run `37910924354` | 4/150 分片、41/1500 样本已接受 | 完整规模未完成，原预算和分母不变 |
| 历史来源 173 `275e8799d4947d297329073eaa3ca675d3fd0777` | 原 100k deadline 失败及小原始收据保留 | 不写成 C 的失败或通过，不把缺失结果填为成功 |
| LG2 `4d18dcdb34b5d7a277566b57801cc882d8b1eb61` | 原 v15 实际 exit 0；正常 push 后新 run `37930392164` queued | LG 首次 fmt 失败仍保留；修复版本编译、feature controls 和四档 mixed 尚无通过信用 |
| PR180 G4 `832f79b8702c6cdfb567b7cf949508cd4589f286` | 原 v15、任务计划检查通过；正常 fast-forward；7 个新 CI queued | 新组合尚无 Rust/native 执行信用；已有 P5 精确回归只属于原 P5 |

`round31-public-evidence.tar.gz` 为 179 个正规 USTAR 成员的确定性 gzip：原 payload 共 9,629,307 B，压缩包 2,263,977 B。完整包 SHA256 为 `bca9f1017421de6947b9a44bb4ce068e80f99ec907437aeb7e3fba42855b4c40`，Git blob 为 `a46f45c47112fb2f212863ea02107b29a95efa44`。成员排序固定，uid/gid/mtime 为零，保留实际源文件模式。每成员已重开核对原字节、SHA256、Git blob 和模式；第二次独立序列化逐字相同。

四份冻结选集在 `manifests/`，完整成员清单为 `round31-public-evidence-inventory.json`，构建输入和读回收据分别为 `round31-public-bundle-inputs.json`、`round31-public-evidence-readback.json`。成员内容包括当前 C 第八格与完整独立汇总、C soak 的统计/15 表 oracle/清理原结果、LG2 准入及身份副本、M4 窄审与历史 173 原失败。部分关键原结果在 `details/` 提供原字节副本，完整资料仍以包和清单为准。

原 R28–R30 已选来源没有重新打包。R30 storage-root helper/同行记录，以及已固定在 LG2 提交中的首次失败日志和 16 份修复记录，按包内固定 commit/path/OID 索引引用。R30 的时间误述由追加 errata 保留并更正：最后轮询为 queued，后续完整原日志显示其截止时已进入 setup；本轮不修改旧包，也不把本轮失败倒写成上一轮已知结果。

归档不含原 ZIP、可执行文件、SQLite、完整 workload raw、私有传输 capture 或签名链接。当前原 ZIP 的实际到期与长期保管另由最终 benchmark 交付清单处理；本包不能替代完整 raw 保管，也不把来源准入当成任务完成。

恢复时先按上述完整 SHA 校验 gzip，再按 inventory 逐成员路径、大小、SHA256、Git blob、模式核对。仅解包到新空目录，拒绝不在清单的成员、非正规文件或路径穿越。无需重新执行 native 工作负载即可核验归档保真。`build_public_round31_bundle.py` 沿用 R30 的原字节读取/正规 USTAR/重开验证，只更新轮次、对应 G2 准入收据和精确实际 URI 检测；代码中的 `library://` 正则文字不会被误判为实际私有 URI。
