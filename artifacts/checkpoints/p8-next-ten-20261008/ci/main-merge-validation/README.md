# 合流版本的源码与历史控制实测

全部成功执行均绑定本地固定 delivery
`0e197ff3fa376e702e1a324ca33c73acb19a5893`，执行前后 HEAD 相同。
完整 tree 为 `62ad27783f738c2f708d347d25c7231a3f436ca3`；
其原 Git 身份保存在 `../../history-main-merge/validated-delivery.bundle`。

| 执行 | 真实结果 | 原始文件 |
|---|---|---|
| v12 新控制 | 18 tests，92.554 秒，exit 0 | `v12-tests.log`、`v12-tests-execution.json` |
| 4 个原历史 selector 方法 | 4 tests，236.308 秒，exit 0 | `legacy-ci-controls-retry.stderr.log`、`legacy-ci-controls-retry-result.json` |
| 完整 v12 CLI | 209.670 秒，exit 0，stderr 0 字节 | `v12-cli-resumed.json`、`v12-cli-resumed-execution.json` |

完整 CLI 原命令没有缓存、超时或跳过逻辑。它实际执行 v11 与 main 原 joint22
两套完整证明，再核固定 `8e12c388…` / `3056a14c…` 的双来源审查：
785 项当前输入、相对 v11 的 1 处差异、相对 joint22 的 4 处差异，以及当前 CI。
18 项控制包含原 main CI 方法的原 AST 与真实 guard 负向测试。

原执行会话发生中断：第一份历史方法日志只留下前三项 `ok`，没有完整 runner
终态；第一份 CLI 两份日志均为空，也没有退出收据。分别保留
`legacy-ci-controls-interrupted.json` 和 `v12-cli-interruption.json`，退出码为未知。
后续用不同文件名保存补跑；已完成的 18 项控制没有重跑。

`raw-file-manifest.json` 逐项记录原始文件长度和 SHA-256。以上为固定版本的本地
来源及 workflow 验证，不代替发布后新 head 的完整 GitHub CI，也不改变任务验收。
