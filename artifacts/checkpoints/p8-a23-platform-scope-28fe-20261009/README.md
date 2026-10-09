# A23 实际受验平台范围与已知 Mac 失败

本文件和 `scope.json` 只整理现有原件的适用范围，供非作者评审及集成者确认；不改变 tasks、源码、原测试、预算或门槛，不授予发布许可。固定产品是 A23 `a23bb72d3c954f385b99fe81ce9189885c208557`，tree `58147c952505c44da1f41eb4b9c31643f2303b96`。

原 P8-012 要求 Linux/macOS × MSRV/stable × default/semantic 的全新 target，并明确“SDK blocker解决或发布平台范围明确，缓存测试不替冷构建”。所有相关任务的“相关旧功能回归通过”条件继续有效；所声明范围内的已知基础测试失败仍是阻断。本说明采用原有的“平台范围明确”路径，不创设自动豁免。

原平台 run `37871838952` 的八格均为 **fresh release build + 实际 stdio smoke**，来源与 binary/bundle 由已完成 A2 纯数据读回独审绑定。正向范围精确到下列原工件，不外推所有主机或 SDK 组合：

| 原平台 | 工具链标签 / 实际 rustc | 包 | 原 artifact |
|---|---|---|---|
| Linux | 1.95 / 1.95.0 | default | 11591733636 |
| Linux | 1.95 / 1.95.0 | semantic | 11592442010 |
| Linux | stable / 1.99.0 | default | 11592750594 |
| Linux | stable / 1.99.0 | semantic | 11592990763 |
| macOS | 1.95 / 1.95.0 | default | 11591229630 |
| macOS | 1.95 / 1.95.0 | semantic | 11591108235 |
| macOS | stable / 1.99.0 | default | 11591159319 |
| macOS | stable / 1.99.0 | semantic | 11592465349 |

公开 A2 摘要没有逐格抄出 OS 版本、架构、target triple、Cargo版本、Xcode 和 SDK；这些字段标为 `unknown_from_available_review_projection`，值为 null，不填0、不猜测，也不声称底层原收据没有记录。每格原 ZIP 摘要、binary/bundle 摘要和执行耗时均在 JSON 中保留。后来的 SDK26.5 单测信息绝不反贴到这八格。八格构建/smoke不冒称整个 workspace 测试通过。

原 supervisor 单测另列：run `37891034688`、job `113691785319`，真实 macOS26.6.2/ARM64、Rust/Cargo1.95、Xcode_26.6.app 选择路径、`xcrun --show-sdk-version` 实测26.5。固定 A23 前后 HEAD/tree、两原文件字节和 Git clean 相符；唯一原 `subprocess_descendant_cannot_hold_stderr_past_worker_deadline` 为1 passed、0 failed、0 ignored、11 filtered，Cargo exit0。0.12s是test harness输出的舍入时长，不是内部worker精确耗时；98.530306833s含构建。原250ms、<2s、exit0及stderr_complete断言未改。实际测试二进制 SHA为 `f4be9957278dacac55da1cb616a8e4ef947a8cffb77c33ecc9bb2e1e15cf3d95`。此记录只证明该实际host/environment上的一个原case，不证明Mac整个workspace。

旧用户Mac失败仍为 **failed / root cause unknown**：相同A23、macOS26.6.2/arm64、Rust1.95，显式 `SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX15.4.sdk`。原 `cargo test --workspace` exit101，p8_scale目标11 passed/1 failed；同一原Mach-O隔离exact执行仍exit101。原binary SHA为 `d6cc286b3d031bd796b41a29c82669b51fa0011e1ce8f4d9fe86943859196ad7`。两原命令、完整stdout/stderr、source收据、原deadline/summary记录及20个原Git文件定位均保留，doctests未到。这个具体失败配置未获本次正向范围认证；它未被改成not_run或passed，SDK27的更早链接失败也没有覆盖它。

主机、SDK、构建与环境均有差异，不能把SDK认定为原因，不能排除所有SDK15.4，也不能宣布所有SDK26.5可用。若继续声明支持旧失败配置，必须保留该阻断并在实际对应配置解决；本说明不能代替修复。若采用这里明确的已有受验配置范围，应由独立评审和集成者确认，并保持旧失败对用户可见。

原八格要求、完整150分片/1500唯一测量、每规模30重复、原aggregate和全部硬依赖仍需满足。此文件不刷新规模运行进度，也不新增wholeworkspace或额外平台hardgate。G8保持not_certified，release保持not_release；原总账192项、163已完成、29未完成，本说明完成0项原TODO。

主要固定依据：A2独审 `784238713bb0fb24c778017925ebd22ffe6fec10`；新原单测独审 `4b2fe67b3cc26ba05070af5b640573bd05f84823`；旧Mac失败独审 `62d2d1960ab0f15432ba869225951988a80ed843`。其完整摘要、原日志/官方artifact定位及A1首次数据读回失败均在 `scope.json` 中绑定。
