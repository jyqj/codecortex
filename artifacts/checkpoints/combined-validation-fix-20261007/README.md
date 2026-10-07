# 最终 A+B+packing 修复组合验证

固定被测源码 `ec2b3b4015de61d1e098ab05a99e76d0291f06ca`，完整 768 个 crate/Cargo 输入见 [source-sha256.json](source-sha256.json)。
[validation.json](validation.json) 记录实际命令、退出码、日志摘要、测试数量、环境和前后源码一致性；作者执行 48 passed / 0 failed / 1 个既有显式 stdio ignored，fmt 通过。
其中 cc-eval 26 项通过，cc-index 22 项通过。旧 A、B 和早期组合测试与本组有重叠，不相加为唯一测试总数。

本目录保留原样的命令执行脚本、收据、完整源码映射与小日志。13 个重复的大型观察 JSON 已逐字节核对后仅在主证据目录保留一份：
[packing 修复证据存储说明](../validation-work-packing-budget-fix-20261007/STORAGE.md) 的 `raw-observations.tar.gz` 内，成员路径为 `combined-check/original-boundary/…` 和 `combined-check/packing-scope/…`。
该目录原始 `checkpoint-sha256.json` 保留每个观察文件的 SHA-256，`verify_storage.py` 只读核验磁盘文件与归档成员，不执行或任意解包载荷。

本组没有运行完整 workspace、公开质量、100k、live provider、非 Linux 或远端 CI。固定源码准入和最终提交 CI 是后续独立检查；本收据不把 P7 整项状态升级为完成。
