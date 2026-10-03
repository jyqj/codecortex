# Express V19 独立作者候选分片

此分片只提供新版本 corpus 候选；独立审查完成数为 **0**。作者 B 不签独立 review。基线是 PR60 `bc8e22bd6b4f85774e7b84a3e6a30da8dc4a3b29`，公共源码是 `expressjs/express@7ef98448f8b38099ab1ded55e458538ad47a51e7`，不得追 HEAD。

`provenance/source-lock.json` 记录原始锁、MIT 许可证 SHA256、7 个允许文件的 Git blob/SHA256/大小与完整排除路径。仅纳入原字节 `index.js`、`lib/*.js`；所有第三方依赖实现、示例、测试、资源、生成物与其他文件不参与检索。许可证在 `license/LICENSE`，其固定 SHA256 为 `95a5762890e5c1c9808921cef095661fc482c5e1f0bba31446ac85595df6237c`。

`blocks/block-020` 是首个 20 家族代表块。问题、答案及逐条精确证据在 `questions.jsonl`，gold 不在 source 内。每行是 evaluator Query 合同，不改共享 registry；suite 的 source 是原字节快照，因此 `commit:null`，真实上游 SHA 在每条证据及 provenance 中绑定。`suite.source.digest` 与 query lock 由本基线 evaluator 的显式 freeze 计算 BLAKE3；额外审计哈希是 SHA256。

稳定 family ID 不随措辞、排序变化。相关事实通过 `related_family_cluster` 绑定，用其 SHA256 前 64 位模 4，0 为 holdout，其余 dev；这是本分片候选 75/25 概率分配，实际数见 inventory，尚非全局冻结。任何改写必须复用 family/cluster。向生产调参者仅报告 inventory 数量、哈希、覆盖和阻碍；不要传递 holdout 问题或 gold 正文。

`source_evidence` 保存固定 SHA、路径、符号、1-based inclusive 行范围、0-based half-open 字节范围、span 文本与哈希。答案组表达 facet/替代答案候选。`chain_edges` 记录关系及支持证据；当前 evaluator 无原生链评分，且多 primary 组不等于所有 facet 都被命中。no-answer 仅在明确列出的 7 文件范围成立，由完整源阅读、邻近正证据/外部边界和字面检查支撑；字面检查不是通用语义缺失证明，独立 reviewer 必须审查。

## 复现

从仓库根目录，在临时目录克隆公开 Express，并 checkout 上述固定 SHA。构建当前 PR60 合同的 evaluator（公共 crates.io 构建依赖不属于检索源），不要启用真实 provider 或运行 search backend：

```sh
PATH=/workspace/.cargo/bin:$PATH RUSTUP_HOME=/workspace/.rustup CARGO_HOME=/workspace/.cargo cargo build -p cc-eval --bin cc-eval --locked --target-dir /tmp/express-eval-build
python3 crates/cc-eval/benchmarks/public-v19/express/scripts/verify.py --block block-020 --evaluator /tmp/express-eval-build/debug/cc-eval --upstream /tmp/express-v19
```

`author.py 20` 可重建首块未 freeze 的作者输入；重建后必须显式 `cc-eval freeze --suite .../suite.json` 再 validate。正常 verify 不刷新 lock。`review/*validation.json` 保留源比对、精确 span 与实际 Rust validate 结果；它不是独立 gold 审查或质量验收。离线构建缺 `reqwest` 的记录和随后公共依赖构建记录保留。

## 待独立复核

按 PR60 轮换建议交由作者 C 复核：许可证与源锁、scope 的第三方边界、每条答案语义与 span、facet 可接受替代、链边、no-answer 检查充分性、跨 family/跨仓模板泄漏与 split 关联。使用 `review/reviewer-template.json` 的空 receipt，由审查者独立提交；作者不能填签名。此分片不代表旧 306 raw 恢复、live 质量结论或 V19 门禁关闭。
