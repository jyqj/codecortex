# PR #161：e95 到 34aa 只读 delta 审查

结论：不是纯档案更新。新 head 34aa30d7bad4b5069934aa008c9394f514995a4b 相对 e95fd750d2f5052d0308c970cd5032c48b765cde 前进 12 commits，共改 869 files：859 档案、8 crates、2 scripts。当前 base 为 55902428d49b09bb4a90cee6ccccf985680618bb，PR 仍 Draft。本审查没有批准合并或改变任何 TODO。

## 源码与验证变化

- 生产 reconcile 复用 prepare 内 budget+1 的 dependency window，满足原 admission 上界时省去第二次 pending SQL，越界保留原 SQL fallback；旧 e95 的运行结果不能验证新分配/读生命周期和性能。
- Native oracle 新增 64/8/1 行、64KiB pending payload INSERT batching；原 projection/排序/重复行/15 表和数值预算保持，但这是实际执行算法变化，64KiB 不等于总 RSS 上界。
- Legacy percentile 调用回到等价的本地 ceil(n*q)-1 公式，共享 owner 和额外 legacy 交叉测试没有随 M6 带入；不据此虚构分位数数值回退。
- P8 scale 的 Option 条件是逻辑等价替换，原预算不变。
- ProjectSession 差异仅 cfg(test) initializer witness 与取消测试：删去取消前交接负控，已有测试从 worker witness/5s 回到 try_lock loop/2s。release 路径未改，但不能声称被删除的测试仍存在或已通过。
- v15 verifier 只更新 PRODUCT / REVIEW / REGISTRY_SHA256；138 个 validation_inputs 完全相同，registry 的 1087 native inputs 中 8 个变化。新记录明确只给 source admission，保留运行、质量和任务验收。

## 未变项与身份边界

整个 docs、.github、tests、Cargo.toml/lock 和根 README 的 Git object 与 e95 相同。tasks.json blob f36b757b0d75ac9b495be039b35b187e8d4ff788 逐字未变，仍 163 done /29 剩余。新 crates/Cargo 的 Git object 与注册 D0 完全一致，crates tree 却不同于当前 a23；当前 a23 原件不能重标 34aa，e95 也不能。

PR body 仍称当前发布 head/PINS6 是 e95，并指向旧 runtime run 37859755918，应更新当前指针并保留旧证据身份。真正的新 head runtime run 是 37878738892；本次快照仍 queued，不能沿用旧 head 验收。新 head 7 workflows 当时为 3 queued、2 in_progress、2 success，明细在 JSON。

GitHub compare 只返回前 300 个 artifact 文件，本审查用 Mac 已有 Git objects 的完整 diff 和远端 root/recursive tree 相互核对，才得出 869 文件全量统计。未下载或执行这些归档中的脚本。原 D0 行政停止/失败/缺失样本保持原状态；这份源码集成不能恢复或补换原研究。

完整记录：[pr161-e95-to34aa-delta-review.json](pr161-e95-to34aa-delta-review.json)；8 个 crate 与 2 个 scripts 的完整补丁：[pr161-e95-to34aa-source.diff](pr161-e95-to34aa-source.diff)。
