# 原预算测试执行 commit 的补充历史

`budget-source.bundle` 保存真实执行四项测试时的本地 commit
`b8991b652dad464b14256bd867a3d7deae6846d0`，其唯一前置是已发布的
PR #146 head `2a75e65d01a3722155e7a1858d7e0e9c8a5558cf`。
原较大的 `../history/local-evidence-history.bundle` 保持原字节，本文件单独追加。

在仓库根目录恢复：

```sh
git fetch origin 2a75e65d01a3722155e7a1858d7e0e9c8a5558cf
git bundle verify artifacts/checkpoints/p8-next-ten-20261008/history-main-merge/budget-source.bundle
git fetch artifacts/checkpoints/p8-next-ten-20261008/history-main-merge/budget-source.bundle refs/p8-evidence/main-merge-source:refs/remotes/p8-evidence/main-merge-source
```

已实际执行 `git bundle verify` 并退出 0；本补充没有另跑全新 clone 的恢复测试。
全部 785 项 crate/Cargo 输入与远端固定 source `8e12c388…` 相同；完整 tree
和 commit 身份仍分别保留。该 bundle 提供历史对象，不授予源码或任务验收。

## 完整合流验证与审查时的 delivery

`validated-delivery.bundle` 另保存原固定 `0e197ff3fa376e702e1a324ca33c73acb19a5893`。
18 个 v12 控制、4 个历史方法、完整 v12 CLI 和最后独立交付审查均使用这个
真实本地版本。其 tree 为 `62ad27783f738c2f708d347d25c7231a3f436ca3`。

```sh
git fetch origin 2a75e65d01a3722155e7a1858d7e0e9c8a5558cf
git fetch origin d53a4972af92fd10a5cddb9f15ffdf06414b3d54
git bundle verify artifacts/checkpoints/p8-next-ten-20261008/history-main-merge/validated-delivery.bundle
git fetch artifacts/checkpoints/p8-next-ten-20261008/history-main-merge/validated-delivery.bundle refs/p8-evidence/main-merge-validated:refs/remotes/p8-evidence/main-merge-validated
```

本补充实际 `git bundle verify` 退出 0，明确列出两个远端前置；未另跑新 clone
恢复。最后的 GitHub API 发布 commit 还会包含这些验证记录，因此另有真实的
commit/tree 身份；上述原执行版本不被覆盖或改名。
