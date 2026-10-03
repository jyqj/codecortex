# Evidence checkpoint TODO

- [x] 读取/确认 AGENTS 与 .agents/skills；固定 PR122 head、旧基线和真实 compiler。
- [x] 全新自有 CodeIndex/runtime fixture，同一 acceptance oracle；两源独立 target、features/source/binary hashes。
- [x] width0/2 × 前中后失败 × 4/12/33；33文档单轮预算16；其他 ready 完成与不可提前退避。
- [x] runtime close / physical join / started attempts；queue closed NeedsRetry；unstarted cancel / stale token；普通 Err join。
- [x] 保留 PR120 原始12文档反例、所有初稿失败、not-run；完成独立最小复现实验、报告。
- [ ] 新分支 commit / push / remote SHA（见交付 receipt，交付时更新）。
- [ ] draft PR：明确 GitHub GraphQL Forbidden，停止 API 动作；未换身份/路线重试，未创建。

限定结论：固定 head retry-liveness 通过，不整合 PR121，不声称组合验收。交付完成即停止。
