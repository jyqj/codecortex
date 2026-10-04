# 完整 Python inventory opt-in 交付

Fixed product/source：`8d2b312c066f0514fb5cee555767955bf34708d0`。
Reviewed base product：`50a4933e48ef20b16401ac8f75c386a660aa8e5c`；
base delivery：`ba6197771da5425166fb0386d46c74f4a05e8273`。
API/范围/alias/budget/一致性/后续边界见
`docs/internals/python-inventory-capture-v1.md`。

实际 source hashes 和命令/exit 在 `validation.json`；日志只去除末尾空行。
官方 Rust1.95 --locked：新9 filesystem integration + 1 deterministic drift +
原focused81 = 91 passed / 0 failed / 0 ignored。strict scoped Clippy、fmt 通过；
最终 committed-range diff check 通过。没有执行 excluded/broad/eval/第三方项目代码。
所有 AST/model 和生产 qname/UID/parser extraction/search 实现与 reviewed base 相同。

## Metadata 和正常 origin 交付

无冲突原样 cherry-pick PR138 docs-only `bf1e47ee1a355ef2634761a980546492fe877f7b`
为 `f2744e6`，tasks.json/生成 TODO 与其逐byte相同，然后只给 P7-014 权威
implementation_notes 添加本 slice 记录。通过 `scripts/code_index_plan.py --write` 生成
并运行无参数 check，192 tasks / done150 / todo41 / in_progress1 完全不变。
未手写生成 TODO，也没有翻 parent 状态或声称独审已完成。

正常 origin fetch、产品 commit/push、metadata commit/push 成功。
创建时 head：`063adeada4720b0fdab07493494bca0ebdcd47e2`。
唯一 draft attempt 通过已有 GitHub connector 成功：
[PR139](https://github.com/jyqj/codecortex/pull/139)，base 为
`integration/python-resource-contract-20261004` / `bf1e47e`，draft=true、merged=false。
完整请求与 normalized response 在 `publication.json`。初响应 mergeable=false，
只记这一工具观测，不称已可合并或已修复冲突；没有另一次 draft attempt/权限 reroute。
CLI `gh auth status` 独立诊断显示其 GH_TOKEN invalid；没有用 CLI 创建 PR 或改 credentials。
实际正常 git push 和已有 connector draft 操作均成功，无实际写操作权限拒绝。

本回执后正常 origin 推送仅包含 docs/evidence，fixed source hashes 未变；最终 delivery SHA
在交付消息及 origin ref 中核验。无 merge/deploy；尚待独立审查。

## 未完成的边界

仅 Linux/statx mount ID，实际 bind-mount fixture 未运行，不宣称其他平台/不可信
FUSE/remote filesystem 支持。完整 inventory 和 bytes 前后重检不是跨文件 OS atomic
snapshot，不能证明抵抗 hostile mutation+restore 或硬 RSS/deadline。
新 API 仍仅在内存，未接 production persistence/cache/versioned ingestion/
publication guard/DB/MCP/retrieval optional identity 协议；旧行为完全未启用 identity 输出。
