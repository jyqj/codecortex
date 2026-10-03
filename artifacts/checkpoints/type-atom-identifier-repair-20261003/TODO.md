# 单一修复清单

- [x] 核对工作区指令、CONTRIBUTING 和 PR103 固定源码。
- [x] 取得冻结独立负证据 `7f650a5f8338e0d1ad2d8e8592ae31c89b34047c`，保留原 BOUNDED_REJECT。
- [x] 在 synthetic 源码复现合法类型边/name bucket 回归。
- [x] 最小修改 `type_atoms`；保留 `resolution_name_keys` 非空过滤和严格 validator。
- [x] 同一套专属测试完成原版 → PR103 → 修复的隔离 target 三方验证，保留失败日志。
- [x] 限定 resolver/model 测试、两 crate clippy、全仓 fmt check 完成。
- [x] commit、push 并核对 remote SHA/tree：修复提交 `778b20deed43db6c11a310a58910ba4b0ed2f3ad`，tree `9dcc68cce1ebdd64b522b930daad6c98f29f5b57`，fetch 后与 local 一致。
- [ ] 为本修复创建 draft PR：已尝试；GitHub `Post https://api.github.com/graphql: Forbidden` 阻塞，按委派指令停止，不改权限/credentials，不换访问通道。可审阅文案保存在 `PR_BODY.md`，原始错误为 `draft-pr.log`。

主集成 owner 后续清单建议（本 session 不认领）：固定 SHA 的独立复验、原 Requests 20 文件/2416 deps 来源核对、全量复现/测试/clippy、旧 cache 升级迁移。19 Python/2420 deps 补充语料不可合并为原语料通过记录。
