# P0-020 / G0 收口

2026-09-27，基线 HEAD 4514630dcd26481cf6dbc2aff38824ed71ef06da，验证对象为包含前轮 P0 与本轮 lint 修复的未提交工作树。P1-A 改动发生在本验收完成之后，不能拿此证据替代 P1-A 的最终验证。

## 已完成验证

最终原始证据：`artifacts/benchmarks/p0-g0-20260927/accepted-v2/validation.json`。记录全部覆盖源码摘要、命令、日志摘要、环境与退出码。Rust 1.95.0 和当前 stable（1.97.0）分别通过：全仓/all-targets/可选 eval-http 的严格 Clippy（-D warnings）；workspace 完整测试 1350 passed / 0 failed / 17 ignored（含 doctest）；eval-http 测试 69 passed / 0 failed / 11 ignored；workspace binaries 构建；显式使用各自工具链产品 binary 的真实 MCP stdio 测试 1 passed。不同命令计数有重叠，不能相加当独立用例。

`cargo fmt --all -- --check` 通过。两套工具链独立 target；1.95.0 是显式版本安装，rustup default 前后不变。SDK 15.4 仅以子进程 SDKROOT/RUSTFLAGS/RUSTDOCFLAGS 使用，不修改全局 Xcode/SDK。所有失败尝试保留；只有 accepted-v2 的冻结源码证据用于完成标记。

## 修改边界

五项原阻塞以根因修正：两处降序 sort_by_key、无意义的 Option 解引用移除、未使用 content_carry_budget 删除、分析阶段位置参数改为 AnalysisInputs 并删除未用 parsed_file_paths 参数。额外 Rust1.95 Clippy 检出三处 C/C++ 匹配 guard 和一处 oracle 布尔条件，均进行等价改写。全仓格式检查暴露的旧格式差异一并规范，没有添加 lint allow、降低 -D warnings 或修改排序权重。

## 限定

G0=本地严格检查与最低版本认证通过，不等于 GitHub Actions/Linux/全部发布认证通过。没有运行外部模型或 OCE 服务。B01/B02/B03/B15 的原产品失败仍存在于 G0 基线，并不阻止可信测量工具完成；它们由后续阶段修复。二进制和固定源码输入保存在同一 evidence 根目录，供 P1 配对比较，证据包不分发平台二进制。

任务 P0-020 已在进入 P1-A 之前更新为 done；tasks.json 是状态源。
