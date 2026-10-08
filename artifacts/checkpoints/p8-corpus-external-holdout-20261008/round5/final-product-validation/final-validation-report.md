# 固定整合 P 的本地定向验证

源码提交为 `1ed3c7df574db6d450f79ef29d5148f68556b3db`，tree 为 `d16aa044e56c9347e6b4ff75d783cc4317f15087`。独占工作树保持干净；六次记录命令的全部 1,196 个源码与验证/策略输入在执行前后完全一致，其中 crates/Cargo 1,082 项、验证与策略 114 项。

## 结果

| 验证 | 实际结果 | 命令耗时 |
|---|---:|---:|
| 新产品构建 | Cargo exit 0，源码/工具链一致 | 46.828 秒 |
| coverage diagnostics 完整 target | 9 通过、0 忽略 | 39.271 秒 |
| 显式选入的真实 MCP readiness | 2 通过、0 忽略 | 2.476 秒 |
| watcher notify 事件分类 | 2 通过、0 忽略 | 19.691 秒 |
| CI 原 workspace/all-targets Clippy | exit 0，警告作为错误 | 56.690 秒 |

测试共 **13 通过、0 失败、0 忽略**。watcher 只运行指定的两个方法，另 256 个方法被过滤；这里不把过滤项算为执行。完整方法 ID、argv、环境、前后源码清单和原日志摘要保存在 `final-validation-summary.json` 及每条命令的收据中。

## 实际产品与控制

产品保存为 `product/codecortex`，SHA-256 为 `f149c9afc5df9d464c68a46d618040ef91a39d847a6c8dc641e7764c52a0dcd1`，大小 67,117,368 字节。原始 Cargo JSONL 与 stderr 完整保留。构建前仅清除了 task-owned target-scale 中八个工作区包的可再生编译缓存；第三方依赖缓存继续使用。六个实际参与默认产品构建的工作区包产生七份 Cargo 产物，全部明确 `fresh=false`。复制后的产品字节在两项真实 stdio 测试前后保持不变。

真实 profile 为 dev，`opt_level=0`、`debuginfo=0`、`debug_assertions=true`、`test=false`，它用于功能回归验证。此构建不宣称优化 release 或性能结果。

coverage target 保留原七个控制，并检验默认隐藏排除与启用后实际准入但尚未索引时的未知原因。MCP target 使用明确指定的新产品子进程，显式 `--ignored` 运行原本默认忽略的两项：137 个文件跨至少三页、引号路径精确保留，以及相同数量但错误实际路径被拒绝。watcher 控制调用真实 notify 事件分类函数，覆盖新增/修改/删除和受保护路径、用户忽略规则；没有把它写成操作系统订阅时序的完整认证。

Clippy 命令为 `cargo clippy --workspace --all-targets --locked -- -D warnings`，沿用 CI 的全工作区与全部 target 范围。它检查默认 feature 配置下的编译/lint，不执行所有测试。

## 保留范围

此前 P615 完整本地测试中的 sampler 与 500 ms fixture 两项失败、原日志及 issue #155 继续保留。这次定向成功不覆盖那些结果。local 2d 的 19 项 scanner/config 控制和 12 项 profile/binding Python 控制保留各自原始源码身份。

最终四 pins 的 source-integrity guard、原完整 174 个方法、其他 CI feature 配置及原 TODO 验收由主代理和最终 CI 单独记录。此任务没有读取/执行任何新的评测或 holdout 题体，也没有更改 gold、任务账本、远端分支或 PR。
