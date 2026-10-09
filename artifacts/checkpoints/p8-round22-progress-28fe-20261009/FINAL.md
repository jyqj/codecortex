# 第22轮完成：原Mac回归、平台范围与规模失败终态

原任务总账保持 **192项 / 163 done / 29未完成**（16 in_progress、12 todo、1 blocked）。本轮新增完全完成 **0项**；P8-005至P8-013及P8-016仍需完整原验收。

## 本轮完成的工作

- PR #172 经自身 check/security/MSRV 实际成功及独立归档审查后，由本 root 正常合并。实际合并 a6b51ffc7d02a798b0230b9f7ec5ccda1b5757ed 保留此前 main 和 #171 原Mac失败。
- 固定 A23 的原 Mac 单测 run37891034688/job113691785319 实际1 passed、0 failed、0 ignored、11 filtered；原断言不变，实际 SDK26.5。测试框架0.12s只属于这一单项测试。
- 原八格冷构建/smoke与新原Mac单项测试的适用范围经作者、非作者和 root 确认。旧用户Mac/SDK15.4失败仍保留、原因未知；范围仅绑定A23。
- 实际发现 main 在06:04:11Z由账户jyqj于本会话之外合并PR #170，前进到60e5ce3ccc86f5cccb40f6f9c32bbc7b77bf1e9d。它含源码改动，A23测量没有重标为此源码。
- PR #173 的局部surface窗口复用经独立源码与实际CI审查，未发现该有限优化的源码阻断；自己的普通CI三job通过，06:23快照原soak仍运行。
- PR #174 的一次有界helper准备及平台cfg修整经独立语义审查。既有Mac工程执行2712 passed/0 failed/73 ignored；该执行以真实工作树身份保留，自己的PR CI仍需结束。本PR确实改变了fixture前提，旧A23冷启动失败没有被称作已修复。
- 已修正两PR正文中的过时规模进度，及PR #174尚无因果证明的“race”措辞。

## 原A23规模研究实际失败

原run37872522779的100k首片job113638100879于06:12:02Z失败。官方唯一annotation确认hosted runner与服务器失去通信；CPU/内存/网络等仅是官方列举的可能性，实际根因及native终态仍未知。原完整job日志首次取回为BlobNotFound，后续shard上传未完成。

原aggregate job113695707497于06:14:24Z失败。原artifact11599136237为580字节，ZIP摘要6585b064492fba4b0f1cd8f10cfeb304ee63fa15851ede907c20a012780902e3；内含原matrix.json明确passed=false/status=failed，精确缺146片。原sample_count字段不存在。

一次无checkout/无native执行的只读collector run37893190079/job113698501180取得原run/job/check/annotation/artifact索引及固定580B ZIP，完整原字节、日志、独审和root核对均保存于../p8-a23-scale-terminal-28fe-20261009/。收件成功不改变原研究失败。

## 下一轮

第23轮将推进已实现且经审查的工程PR，准备两PR不相交源码与source registry的正确集成，并根据实际失联事实和原执行政策准备有界后续执行方案。原150片、1500唯一测量、每规模30重复、硬依赖和失败保留要求继续有效；当前没有新启动规模研究或替换旧失败。
