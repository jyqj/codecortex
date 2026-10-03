# 固定 PR101 tree：100k release 独立实测（未通过 ready）

只适用于源码 `574f7598662334c63e020da136c87f4f7281554d`（tree `7f9ea467c1705822aa403de7d273419ddfb45864`，核心 coldscan `c70c68f2ff9b4ac40c635652858f56d2d0a06518`）。不适用于后续集成 tree，不宣称完整 V20。既有独立 50k review `70c2a640160060e34cd530e18e02a8c95885063f` 未复跑。

最终有效隔离轮的冷索引完成，但 **backfill 在固定 300 秒期限内未达到 ready，退出码 1**。这是未完成的 100k 规模门，不得引用为 100k 全通过。核心判断交由父任务。

| 项目 | 最终隔离轮观测 |
|---|---|
| synthetic 规模 | 100,000 Rust 文件，每个一个函数；4,088,890 源码字节 |
| 固定生成方式 | 既有 `scripts/p7_release_resource_preparation.py:source(i,value=0)`，i=0..99999；seed/value=0，无 PRNG |
| cold index 墙时 / wire 时间 | 21.638297s / 21.638264s |
| 扫描 / 解析 / 新增 / 跳过 | 100,000 / 100,000 / 100,000 / 0；无 parse_errors |
| 最终运行总墙时 | 341.135547s（含生成、检查、300s drain 和 cleanup） |
| 失败后 files / symbols / chunks / document_manifest | 各 100,000 |
| 失败后 semantic_manifest | 29,696；outbox done=29,696 / pending=70,304 |
| 本地假模型输入 | entered=returned=29,696；每请求一个输入，没有真实 provider |
| 数据完整性 / FK | held 检查及失败后只读检查均 `ok` / 0 |
| Git co_change_edges / test_edges | 0 / 0（空 synthetic Git 仓库隔离父历史） |
| 产品根进程 sampled RSS 峰值 | 2032357376 bytes（1938.21 MiB） |
| 构建 | Rust 1.95.0，release，thin LTO，codegen-units=1，strip=true，opt-level=3，无 debug assertions；146.205s |
| Cargo 实际 features | semantic + semantic-http，--no-default-features，--locked |
| 二进制 SHA256 | `1dc3dbff22f9fda053c7e7eac6d11d27f0a7271cec28b902057e632be21b9b27` |

`failure-db.json` 是失败 cleanup 后的只读检查，不能替代成功 ready checkpoint。`last-observed-status.json`、完整 RPC/HTTP 与资源日志保留在 `live/n100000/`。cleanup 产品退出 0 只说明清理行为；预定的 **normal-exit/reopen 测试未执行**。

## 保留的失败与未跑项

- attempt01：嵌套 synthetic 目录继承父 Git 历史，515 条 co_change_edges；21.898s 冷索引结果 **不接纳** 为纯 synthetic 证据。发现后 SIGINT 停止，退出 1，原脚本、traceback、输入 manifest 与资源/RPC/HTTP 日志完整保留。
- attempt02：增加空 synthetic Git 仓库后，检查脚本表名写错为 `cochange_edges`，实际是 `co_change_edges`，KeyError 退出 1。原脚本、失败和未跑项完整保留。
- attempt03：仅修正表名，源码/产品二进制/seed/尺寸/超时/资源阈值保持相同。自然达到 300 秒 drain 失败，无超时调大或删去失败。
- 最终轮未跑：成功 ready 的全量 manifest 验证、4 请求重复/不同 query 并发检查、专门 normal 退出验证、重开稳定性。可复跑脚本已经包含这些有限检查，前置 ready 未满足便明确停止。
- 不执行：50k 重跑、heldout、真实/收费 provider、私源传给模型、完整 V20、无限压测、merge/force-push/deploy。全仓 clippy/test/fmt 未跑：本次仅新增证据，用户只授权 synthetic 且禁止 heldout，未改产品/版本/tasks/TODO。

## 资源边界

构建 target、Cargo/Rustup home、临时文件均隔离在本目录 `runtime/`。构建 900s；运行总体 900s；index RPC 300s；ready drain 300s；正常退出 15s。内存 cgroup guard 12 GiB、磁盘余量至少 4 GiB。预检 cgroup 上限 16 GiB、CPU 配额 4 核、可用盘约 30 GiB，最终轮未触发资源 guard，OOM 计数仍为 0。

请求采样间隔 20ms。此挂载的 `/proc/<pid>/task/<tid>/children` 不可读，尝试枚举的缺失错误逐样本保存；**无法给出完整进程树 RSS**。产品根进程 RSS、VmHWM、runner、假模型和 cgroup current 各自记录。采样非原子、有间隔、不是 PSS 或精确瞬时峰；已知进程 RSS 求和可能重复共享页。构建只采 cgroup memory，不冒称编译进程树 RSS。分阶段峰值、实际最大间隔与覆盖缺失计数见 `resource-analysis.json`。

## 审查与最小复跑交接

完整命令见 `commands.txt`；`build-receipt.json` 固定 sourceSHA/tree、Cargo 实际 artifact/profile/features、二进制 hash；`source-identity.json` 固定 lockfile/generator 身份；完整大型冷索引 JSON/summary 与安装日志使用无损 gzip 留存；`verification.json` 的 passed 仅指证据身份及失败保留通过，product_outcome 仍是 failed。

在固定源码的全新 checkout 中复制本目录脚本，使用新的空输出目录安装本地 Rust 1.95.0 后运行 `python3 build.py`、`python3 run.py`、`python3 analyze.py`、`python3 verify.py`。`run.py` 拒绝覆盖已有 case；既有输出和数据应保留。原始 synthetic 源码/DB/cache/构建二进制留在本地且由本目录 `.gitignore` 排除；逐文件 hash manifest 与可审查证据入库。

后续新 tree：在新的单独证据目录中明确更新 build/run/verify 三个脚本的 SHA 常量，重新记录 generator/lockfile 身份与 preregistration，使用全新的 target 重新构建，核对新二进制 hash，执行同一固定 100k 程序。不得套用本次结果；若要改变期限，须作为新的明确实验规范，保留本次失败。

## 远端交付状态

Git 只读核验 `refs/pull/101/head` 精确匹配固定 SHA。现有 `gh` 认证无效，`gh pr view 101` 的 GitHub GraphQL 请求返回 `Forbidden`，该 API 动作停止，未切换权限/通道。draft PR 因此阻塞；预备说明在 `PR-BODY.md`，提交/推送/远端分支核验的实际结果另见交付回执。
