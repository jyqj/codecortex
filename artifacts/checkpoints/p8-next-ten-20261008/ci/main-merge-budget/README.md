# 合流后的四项 artifact budget 定向验证

本记录来自固定源码 `b8991b652dad464b14256bd867a3d7deae6846d0` 的**一次实际运行**。该源码以已发布 PR #146 head `2a75e65d01a3722155e7a1858d7e0e9c8a5558cf` 为父提交，整合新 main `d53a4972af92fd10a5cddb9f15ffdf06414b3d54` 中两项预算测试。

源码差异仅为 `crates/cc-eval/src/benchmark/p8_load.rs` 的 `cfg(test)` 模块新增 43 行。双方四个原测试名称和断言全部保留；新 main 的两个测试仅适配现有匿名临时文件 helper。生产 CAS 循环、统计共用算法、独立 nearest-rank oracle 和 worker 就绪修复均未因本次整合改变。

## 实际结果与范围

- Rust / Cargo 1.95.0，显式 Linux host `x86_64-unknown-linux-gnu`、dev profile，`--features semantic --locked --offline`。
- 复用先前私有 semantic target：225 个 Cargo compiler artifacts 中 **224 fresh，1 个新编译的 cc-eval unit-test binary**。这不是冷构建。
- 只选择 `benchmark::p8_load::artifact_budget_tests`：**4 passed、0 failed、0 ignored、46 filtered out**，测试耗时 0.01 秒；整个命令退出 0、耗时约 19.82 秒。
- 实际覆盖双方原有的整数溢出、预算边界、并发完整预留、余量填满和成功计数核对；没有运行其他筛除用例。
- 执行前后 785 项源码输入清单逐字节一致。规范化 input map SHA256 为 `4c9aeb9cac2eea382d2dbce6d85220af3e10f6483b4c1f696b7874ecaff0fdb4`。
- 实际测试 binary SHA256 为 `416eeb6c42c6effb6af27ff3e3661e8b3fbce419334987a27659df73c72fdbba`。二进制本体不放入此目录；其真实 compiler-artifact、路径、大小及哈希见 `test-result.json`。

`cargo-budget.stderr` 原样保留 Cargo 的 `failed to save last-use data` 警告及 `database or disk is full` 原因：overlay 空间不足导致全局缓存使用元数据写入失败。构建和测试实际成功，但本记录不宣称通过全仓或 warnings-clean CI 门。运行期间 tmpfs 最低可用 545,890,304 bytes，没有触发预设的 200 MiB 停止线。

## 原始记录

`raw-file-manifest.json` 列出从 `/dev/shm/codecortex-p8-merge-budget-ebacee` 逐字节复制的全部 9 个原件及哈希；包含完整命令、环境、stdout、stderr、执行前后源码清单、工具链版本与测试 binary 绑定。原 stdout 末尾空行也保留，没有为格式检查改写日志。本目录的归档操作没有再次运行任何测试。

本结果限定于合流后四项预算测试，不能把旧 head 的 P7 CI 成功重标到新源码，也不增加 TODO 完成数或授予发布验收。
