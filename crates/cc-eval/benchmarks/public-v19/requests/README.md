# Requests V19 候选作者分片

基线 PR60 `bc8e22bd6b4f85774e7b84a3e6a30da8dc4a3b29`。公开来源
`psf/requests@611c6162cbc4ac2020a2f91c7cfa4f3abf9bbb60`；Apache-2.0，
LICENSE SHA256 `09e8a9bcec8067104652c168685ab0931e7868f9c8284b66f5ae6edae5f1130b`。
全部内容为新版本 candidate，独立审查数量为 0；作者的完整性验证不构成独立 gold review，
不构成排名、历史 306 raw 恢复或 live 质量结论。主集成者独占公共 registry。

## 来源与许可范围

保留 19 个 `src/requests/*.py`、`pyproject.toml`、LICENSE 和 NOTICE；
`provenance/inventory.json` 逐项锁定完整公开 tarball 的 128 个文件并记录排除原因。
tests/docs/assets/ext/CI 等不索引、不打包；无二进制或私密代码。
所选运行时文件未见 generated 标记或 vendored 目录；`packages.py` 将安装的 urllib3/idna/字符探测
模块映射为兼容别名，不包含这些依赖的实现。`_types.py` 是仓库维护的类型声明，
不是由工具生成的依赖包。`utils.py` 的 `parse_list_header`、`parse_dict_header`、
`unquote_header_value` 带 Werkzeug 许可使用来源注释，原样保留，全部排除在 gold 定位范围外；
详见 `license/inclusion-review.json`。为保持真实上游完整文件 byte span，源码文件仍原样包含
这三段。已核得历史 Requests 引入提交 `9966017a…` 与固定 BSD-licensed Werkzeug
`d902d2c0…`，三函数归一化实现 AST 完全一致。完整 BSD LICENSE/AUTHORS、
原始 copyright/permission notices 和历史 Requests ISC LICENSE/NOTICE 均保留于
`license/werkzeug-0.6.2/`；详见 `license/THIRD-PARTY-NOTICES.md` 与 `lineage.json`。
当前 evaluator 不支持按函数排除索引；原始完整 source bytes 的准入有明确许可链和归属
材料，但不声称分片无第三方来源，独立许可接受仍为 pending，gold 排除决定保持不变。
LICENSE/NOTICE 保持原始字节；不下载 certifi CA bundle、
urllib3、idna、chardet、charset_normalizer 的实现到源码分片。

## 数据合同与审查

`questions/specifications.json` 是人工读源码撰写的公开 dev 事实/工作流规格。
`scripts/author.py` 只由 AST 定位真实 symbol，生成原始 UTF-8 byte span，
不调用检索或 Requests。Query 使用真实 evaluator schema，多个必需 facet 是独立
AnswerGroup，同组 alternatives 才表示可替代答案；没有用无关答案填充 alternatives。
`gold/dev.json` 绑定 commit/path/symbol、1-based inclusive line span、
0-based half-open byte span、片段 hash、语义依据和显式跨文件边。方法 gold 采用完整实现
span（排除 overload stub），审查者可进一步缩窄。边绑定调用/数据表达式的精确字节。
无答案只声明指定模块或实现内不存在的功能；有源码范围、相邻实际功能证据和可复查
AST/literal 检查，不按排名判空。literal 检查只保证审查锚点完整性，语义否定还需交叉审查。

Suite 的 `source.commit=null` 是本地导入 snapshot 合同：此目录不是 requests Git checkout。
真实 upstream SHA 在 provenance、每个 Query annotation 和 gold 中独立绑定；suite
源锁是 evaluator 实际使用的 BLAKE3 文件清单 hash，不把 CodeCortex HEAD 当 upstream SHA。
公开 Suite 仅包含 dev native 和 dev compat；compat 按真实合同只投影 answerable paths，
不含无答案，不代表 span/facet/chain 得分。第一个 AnswerGroup 为 grade3 primary，
其余必需证据为 grade2 secondary；`annotations.v19.facets` 显式关联 required group。
graph_constraints 绑定 caller→callee 精确 source evidence；当前 evaluator 不提供正式
facet/graph correctness metric。全部输出保持 pending，作者不签独立审查。

## Protocol-v1 迁移和实际阻塞

采用 PR65 fixed `03abe24950f1fa89de3cce0a4e1d44d01e9aa0d6` 的共同协议；本分片
`provenance/protocol/` 只是该资源的原始只读镜像，未改共享 protocol。
前协议先发布首批 20 个候选；其后写到 100，全部尚未排名或独立审查。
按原始撰写顺序一次性迁移到 `v19.requests.f0001`…`f0100`，query ID 加 `.en01`，
`review/id-migration.json` 保留旧 ID/Query/gold exact-byte hashes 和旧/新 split 标签。
为遵循 hash/count-only holdout 报告，旧语义 ID 用 SHA256 映射，不重新公开其正文。

原有相关任务关系保守合并为 **86 个拟议 global components**，66 dev /20 expected holdout；
作者只提出本分片关系，最终语义等价和跨仓去重由 designated global reviewer 裁定。
共享 helper 的独立入口定位与调用链虽然题目不同，尚不能自动认证为独立样本。
所以 **100 是候选 ID 数量，独立组件距离 100 仍差 14，独立接受为 0**。
类别草稿目标 20exact/API、20behavior、15facets、20crossfile、10config/error、15hardnegative
达到，但不能用类别总数覆盖组件短缺。无翻译/paraphrase 被计为新家族。

使用协议固定公式 SHA256(`codecortex-public-v19-split-v1\n`+canonical component) 的
前64 bits <2^62 为 holdout；这是期望75/25，不按配额重平衡，不挑 ID。
本次恰好得到75dev/25拟holdout行，公开75native-dev，67compat-dev（排除8no-answer）。
所有关联成员同 split；relations 尚未全局冻结，不能开始正式排名。

协议到达前的首批20正文已经公开，旧100草稿亦在共享工作区中；其中25拟holdout全部
记录 `holdout_custody_blocked`，2条还有首批公共 Git 历史暴露。删除不恢复秘密性。
原始记录仅保留于本云工作区忽略目录 `.preprotocol-local/` 用于迁移证据，**这不是
独立受限托管，不能保证其他共享工作区参与者无法读取**。没有新 holdout 正文写入
本次公共投影；`review/holdout-commitments.json` 是从迁移草稿在内存计算的 hash/count，
不是受限存储的存在证明。正式可用 holdout=0；confirmatory holdout 25行因污染隔离，
需要独立 custodian 裁定访问历史和后续处置，不能更换 split/ID 伪装成 untouched。
不得把忽略目录或旧 Git holdout/specifications 正文发给主生产调参者。

独立审查、真实托管、适配来源许可准入和全局等价裁定均待完成。因此首批虽有作者
完整性/输入校验和 draft PR，尚不是协议定义的 reviewed complete20/100 block。
本作者停在候选准备和 migration，不用重复题补足14组件，也不签自己的审查。

## 复现（仓库根目录）

```sh
python3 crates/cc-eval/benchmarks/public-v19/requests/scripts/reproduce_source.py
python3 crates/cc-eval/benchmarks/public-v19/requests/scripts/verify.py target/debug/cc-eval
python3 crates/cc-eval/benchmarks/public-v19/requests/provenance/protocol/check.py \
  --shard requests:native=crates/cc-eval/benchmarks/public-v19/requests/queries.native.dev.jsonl \
  --shard requests:compat=crates/cc-eval/benchmarks/public-v19/requests/queries.compat.dev.jsonl \
  --relations crates/cc-eval/benchmarks/public-v19/requests/relations.json \
  --evaluator target/debug/cc-eval \
  --suite crates/cc-eval/benchmarks/public-v19/requests/suite-native-dev.json \
  --suite crates/cc-eval/benchmarks/public-v19/requests/suite-compat-dev.json \
  --output crates/cc-eval/benchmarks/public-v19/requests/provenance/protocol-check.json
```

`verify.py` 检查 source/license/AST spans/edge 字节/无答案范围并运行两次 evaluator
`validate`；不索引、不检索、不修改 gold。它更新本分片完整性 receipt。evaluator
schema 和 cross-field 校验来自本基线真实 Rust 类型与 manifest/validation 实现。
`provenance/build-receipt.json` 记录实际 compiler-artifact、binary hash 和命令。
原旧 author 配额生成入口已禁用；`migrate_protocol.py` 需原始本地草稿且不能解决 custody，
不在缺少那些真实字节时重建或伪造 hash。公开 dev 可由 pinned source/spec/gold 复核。
Suite top_k=10/repetitions3/warmup0/seed20261003/timeout30000 与协议一致。
公开 corpus-receipt/source-manifest/review-receipt 是 candidate 数量与 hash 交接入口。
