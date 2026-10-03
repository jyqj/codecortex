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
所选运行时文件未见 generated/vendored 声明；`packages.py` 将安装的 urllib3/idna/字符探测
模块映射为兼容别名，不包含这些依赖的实现。`_types.py` 是仓库维护的类型声明，
不是由工具生成的依赖包。LICENSE/NOTICE 保持原始字节；不下载 certifi CA bundle、
urllib3、idna、chardet、charset_normalizer 的实现到源码分片。

## 数据合同与审查

`questions/specifications.json` 是人工读源码撰写的独立事实/工作流规格。
`scripts/author.py` 只由 AST 定位真实 symbol，生成原始 UTF-8 byte span，
不调用检索或 Requests。Query 使用真实 evaluator schema，多个必需 facet 是独立
AnswerGroup，同组 alternatives 才表示可替代答案；没有用无关答案填充 alternatives。
`gold/{dev,holdout}.json` 绑定 commit/path/symbol、1-based inclusive line span、
0-based half-open byte span、片段 hash、语义依据和显式跨文件边。方法 gold 采用完整实现
span（排除 overload stub），审查者可进一步缩窄。边绑定调用/数据表达式的精确字节。
无答案只声明指定模块或实现内不存在的功能；有源码范围、相邻实际功能证据和可复查
AST/literal 检查，不按排名判空。literal 检查只保证审查锚点完整性，语义否定还需交叉审查。

Suite 的 `source.commit=null` 是本地导入 snapshot 合同：此目录不是 requests Git checkout。
真实 upstream SHA 在 provenance、每个 Query annotation 和 gold 中独立绑定；suite
源锁是 evaluator 实际使用的 BLAKE3 文件清单 hash，不把 CodeCortex HEAD 当 upstream SHA。
开发 Suite 与 holdout Suite 分开；调参者只应读取 dev。整个分片面向另一作者交叉审查，
不要向主生产调参者转发 holdout 问题/gold/specifications 正文。

家族 ID 稳定；相关家族用 `cluster` 合并。按 cluster SHA256 排序，通过确定性 subset-sum
选取本批次 25% holdout，余下 75% dev，关联家族不能跨 split。扩展批次可能重新分配
候选 split，必须在任何排名前完成；不宣称全局冻结。最终集成仍需全局去重和泄露审查。
`review/preparation.json` 只记录候选计数、覆盖和 hashes；无作者独立签字。

## 复现（仓库根目录）

```sh
python3 crates/cc-eval/benchmarks/public-v19/requests/scripts/reproduce_source.py
# 仅显式重新创作时运行 author，再用实际 evaluator freeze 建立 BLAKE3 locks。
python3 crates/cc-eval/benchmarks/public-v19/requests/scripts/verify.py target/debug/cc-eval
```

`verify.py` 检查 source/license/AST spans/edge 字节/无答案范围并运行两次 evaluator
`validate`；不索引、不检索、不修改 gold。它更新本分片完整性 receipt。evaluator
schema 和 cross-field 校验来自本基线真实 Rust 类型与 manifest/validation 实现。
`provenance/build-receipt.json` 记录实际 compiler-artifact、binary hash 和命令。
suite top_k=10 只是候选合同合法配置，主集成者仍须在测量前注册正式统计和 metric 方案。
