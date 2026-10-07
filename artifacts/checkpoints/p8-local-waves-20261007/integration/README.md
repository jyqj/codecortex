# P8 最终本地组合与远端准入

本目录分开记录实际运行、独立审阅和远端发布；完整结构化结果见 `delivery-preparation.json`。

## 最终组合验证

实际测试源码固定为 `853385b7ccb2780818f9f8e8a83791f1c197efcf`。同一独占构建目录、同一工作树清理cc-eval缓存后，5个指定集成目标共 **33 passed / 0 failed**；cc-eval全目标 `--no-deps -D warnings` Clippy通过，workspace格式检查通过。原始日志、环境、源码与3个实际binary摘要见 `artifacts/checkpoints/p8-combined-validation-20261007/`。这不是全workspace测试或正式性能认证。

源码准入扩展阶段的完整67项测试通过；加入P8固定delta后，21项reviewed-source测试与777输入直接验证通过。远端重新绑定后，又使用实际可fetch的R1/R2 pins执行直接验证，退出0。三者的来源和日志分别保留，不将早先执行重标成后来的commit。39个发布证据CLI合同、事实漂移检查与4个TODO生成视图检查通过。

## 可fetch的来源与审查

终端缺少push凭据，已使用有写入权限的GitHub Git Data接口创建实际远端源码 `18499879a8ba3197e599cfd2e9ae96bcb657d65b`，并创建独立审查提交 `c7766540612f1156f0a8afbdead20e934c98066c`。根代理执行远端fetch，独立审阅者检查获取的Git对象：父关系、17路径差异以及777个受保护输入，共9,972,292字节，全部与原已审/已测本地源码相等。原review和测试来源保留，当前guard只绑定实际远端R1/R2。

R2提交说明中的“reviewer fetched”是分工简写不准确：fetch由根代理执行，独立审阅者读取并核对已获取的对象；固定review JSON已准确记录分工。此处订正该说明，不改写既有Git对象。

最终远端全树还包含Python工具、文档、所有成功/失败raw和原作者Git历史增量包。历史包用于重放原始本地commit，CI不会从包导入对象并授予批准。CI现有30个命名步骤全部保留，变化只为两条新增本地Python检查和明确的v8 selector；GitHub CI结果查看实际PR head，不从上述局部验证推断。

## 仍然开放的范围

原native live-child测试在此环境仍因PID/proc归属不一致失败，原日志与独立诊断保留。早期共享Cargo缓存错误、scale 120秒timeout和两次中断的scope Clippy都保留原结果；后续组合成功是独立新记录。全1k到100k矩阵、长时soak、真实provider、heldout、跨平台安装和发布验收未执行。原十项任务均为in_progress，三轮推进4/3/3，正式未完成仍42项。
