# 第 33 轮：修补后的独立诊断已准入并注册

本目录保存新源码 P `aa8c86b168290e6133040072613aae8240d5b1f6`、R `5c53bb909efef3fb842b5fbc39a45dec4054ba40`、G `9cc6bf49f6dd81e4069a8004eed49addba0ab79b` 的实际创建、独立审查、原始准入与发布记录。G tree 为 `6461938788665701cfa4fa095a064a3a404ec492`。

修补只把诊断目录的初始化移入 runner 上的第一个普通步骤，原 wrapper、测试、driver、产品及原规模协议保持。原始 `bcc3` 版本启动失败的完整记录位于上次第 32 轮归档，不被新成功准入覆盖。

原 v15 于 2026-10-10 01:11:26.889497–01:13:07.611579 UTC 实际退出 0；原 task-plan 于 01:13:08.404604–01:13:08.468992 UTC 实际退出 0。每次均有 1444 项选定输入及真实 HEAD/index 前后一致；完整原件 ZIP 随附。

[PR200](https://github.com/jyqj/codecortex/pull/200) 已按 lease 从 bcc3 快进到 9cc6。随后仅为修补后的新源重新触发独立诊断 label；没有调用旧 run 的 rerun 接口。[新 run 38012409915 / attempt 1](https://github.com/jyqj/codecortex/actions/runs/38012409915) 已由 GitHub 接受，事件为 pull_request。在 01:15:08 UTC 的原注册观察中，build job `114095124901` 已创建并排队，尚无 native 执行结论。

## 原件接收与后续任务

当前绑定与两个只读接收脚本已按实际新 source/run 冻结并独立审查。注册时的 ELF、diagnose job 和 artifact custody 仍未知；脚本与原 validator 尚未执行。后续接收实际原件时，不将 ACK、原始字节前缀、完整 native 结果和任务验收互相代替。

九项原始任务的未来落账清单保持 applyfalse。其原始包已从不可变 Git 工件恢复，未重跑测量。只有 P8-006 的实际完整验收收据到齐后，才可按原依赖顺序更新 006、007–013、016。计数目前仍为 **192 总项 / 164 完成 / 28 剩余，本次新增原始完成仅 1 项**。

只读性能审计解释了原 G8 的 batch 与 resume 顺序、实际缓存分支和嵌套计时边界，没有改动产品或实施猜测优化。每 stage 的 full control 和完整 parity 均为原协议必需。

PR199 在 01:11:49 UTC 的原观察中为 25 项成功、2 项条件跳过和 1 项原 soak 运行中。最终合并准备只使用既有工程门槛；此目录不宣称它已经合并，也不预写 soak 成功。
