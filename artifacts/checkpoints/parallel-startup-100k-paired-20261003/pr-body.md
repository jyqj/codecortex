两版固定源码、原 Cargo.lock 与 semantic-http release，在同一环境按原 synthetic100k 协议 baseline → candidate 各实测一次，均未在 300 秒内 ready，性能验收未通过。基线 / 候选 cold 28.730 / 30.183 秒；deadline manifest 27,656 / 66,867，失败 cleanup tail 28,672 / 67,584，不把 tail 算作通过。

status 错误均为0；失败 EOF 清理均正常退出。ready 后查询、并发、正常退出认证和 reopen 均未运行。完整源码/二进制/协议 hash、构建、边界快照、HTTP 并发日志解释、RSS/CPU/IO、阶段记录与限制保留在本 checkpoint。产品、CI及历史失败只读，无调参、追加测量或统计显著性声明。

仅新增 artifacts/checkpoints/parallel-startup-100k-paired-20261003/ 证据；README.md 提供结果与复核入口。compare.py / supplement.py 为证据重放，不运行产品。
