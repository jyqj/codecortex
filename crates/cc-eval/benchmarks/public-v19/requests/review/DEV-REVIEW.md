# 独立审查入口：仅公开 dev

审查域：`queries.native.dev.jsonl`、`queries.compat.dev.jsonl`、`gold/dev.json`、
`questions/specifications.json` 与 source-manifest 中 admitted 源文件。
不要读取 `.preprotocol-local/`、旧 Git 中 holdout/specifications 正文或任何拟 holdout
材料；那些资料不是本次 dev 审查输入。公开 hash/count 迁移记录可核对，不据此接受 holdout。
本入口包含作者准备材料，**不是独立审查签字或已接受数据**。

独立 reviewer 应逐家族核对：固定 requests SHA 和 source file hash；问题条件与字面行为；
gold path/symbol/[start,end) byte span 及1-based line span；primary/secondary 分工、facet
required group 与 graph caller/callee/方向/表达式证据；无答案的明确范围及完整源码否定依据。
不要运行搜索排名或读调参结果，也不要按召回好坏改 gold。

新增16任务的 `independence`/`independence_proposal` 是作者关于新事实的说明；独立 reviewer
须与现有 dev 对照，若只是同事实改写则归并/隔离并保留原记录，不凭 singleton 标签接受。
分享 symbol 或文件本身不证明等价或独立；涉及旧拟 holdout 的全局关系由独立 custodian /
global reviewer 在其授权范围裁定，只向主集成者报告 hash/count/status。

先运行公共 dev 校验：

```sh
python3 crates/cc-eval/benchmarks/public-v19/requests/scripts/verify_license.py
python3 crates/cc-eval/benchmarks/public-v19/requests/scripts/verify.py target/debug/cc-eval
```

许可核对：主 Requests Apache LICENSE 原始 hash；三个适配 helper 的历史引入及三路 AST
匹配、Werkzeug BSD LICENSE/AUTHORS、源文件 notice；历史 Requests 证据自身 ISC LICENSE/
NOTICE/AUTHORS 亦保留。全部许可证据在 `license/`，不包含评测 holdout。

审查结果由另一个作者在其自己的 review 分支/授权路径签署；作者 ID 必须与 reviewer ID
不同，绑定协议 SHA、来源 SHA 和审查输入精确 hash，逐项 accepted/quarantine/disagreement
及理由。不能自动以 verify/protocol checker 的通过替代语义 review。当前 accepted=0。
主公共 registry/任务总账仍由唯一集成者拥有，本分片作者不落全局接受账。
