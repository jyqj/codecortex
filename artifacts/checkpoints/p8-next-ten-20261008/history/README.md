# 原始本地执行身份的可还原快照

本批多个真实回执绑定原始本地 Git commit，例如 G8 的 `f99b2dc2…` / `2d060834…` 任务状态，以及 worker 实跑的 `83dae5a6…`。通过 API 发布相同 tree 会产生不同 commit ID，不能把这些旧执行身份重标为新远端提交。

`local-evidence-history.bundle` 因此保留这些本地提交及其祖先，包含原始作者、时间、parent 和 tree。它是一份增量 Git bundle，4,198,102 bytes，SHA256：

`c1667a78ac8a8a5f6b27eb016c024a0ea0fb15c3ea077c063436a4a03c60730c`

固定 root 快照为 `2dc4ad6e7bbd03184cf58f17594b7e69431e3bab`；完整 advertised refs、各工作分支快照和两个前置提交列在 `metadata.json`。原 `git bundle verify` 退出 0，日志保留。这份包不包含自身和后续发布说明，也不包含外部语料仓库；并发更新的 PR #145 不作为本地快照混入。

## 在新 clone 中读取原回执

在仓库根目录执行。下列 fetch 只增加 `refs/remotes/p8-evidence/` 下的引用，不切换当前工作树。第二次运行 G8 工具时应选择新的、尚不存在的输出目录。

```bash
git fetch --no-tags origin eb7cdc55aa94c8d6865bed14fa37fff08080af33 ae906513886bef4501d0a1d2ecffd68ef76c3f5d
git bundle verify artifacts/checkpoints/p8-next-ten-20261008/history/local-evidence-history.bundle
git fetch --no-tags artifacts/checkpoints/p8-next-ten-20261008/history/local-evidence-history.bundle 'refs/p8-evidence/*:refs/remotes/p8-evidence/*'
PYTHONDONTWRITEBYTECODE=1 python scripts/p8_release_review.py \
  --manifest artifacts/checkpoints/p8-next-ten-20261008/release-review/final-state-2d060834/input-manifest.json \
  --output /tmp/p8-g8-review-restored
```

最后一个命令应退出 1：实际生成证据缺口总账，152 done / 22 in_progress / 17 todo / 1 blocked，剩余 40，local / semantic 继续 blocked。它不授予 G8 接受。

上述恢复流程已在新的独立 bare 仓库实际验证：仅获取两个 depth=1 前置提交后，f99 / 2d / 83 三个本地执行对象均不存在；导入后 22 个 refs 和三个 commit 均可读取。一次原 G8 CLI 退出 1，新 stdout 与原 stdout、receipt 与原 receipt 分别逐字节相同；13 份记录也匹配原始字节。仓库没有 shared/alternates/硬链接对象，临时峰值 223,866,880 bytes。完整命令与结果见 [独立恢复验证](../history-restore-review/verification.json)。

此历史包供审阅和重放固定回执使用。当前源码准入仍由公开固定 source/review、v11 完整旧证明和逐文件清单决定；不能从历史包中的任意分支自动获得源码批准，也不以 commit 存在证明测试、质量或发布成功。
