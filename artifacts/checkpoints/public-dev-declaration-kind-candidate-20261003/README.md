# 独立公开 DEV 声明 kind v2 候选 — 2026-10-03

`public-dev-declaration-kind-v2-candidate`，状态 **not_admitted**。
固定开发基线 PR133 `6c1416109003bcff0c1911307a4af5bd48870517`，原准入
PR91 `5385f5a7a2a875c6d5cbd049bdde039bf71bbf32` 不变。

授权 taxonomy owner 决定采用原 proposal 的声明分类。166 条 source-backed
alternative 的 `function` 变为 `method`：Requests 85 条、Gin 81 条，涉及
58/43 行。Requests 的历史宽泛 function 不被判错；Gin 原约定仍未确定。
完整历史与新规则见 [协议补充](PROTOCOL-SUPPLEMENT.md)。

## 交付物

- `change-manifest.json`：完整逐项原绑定、精确 kind token delta、before/after
  raw 哈希与 unchanged source binding，及原行/answers/文件派生哈希。
- `candidate-gold.json`：只包含 166 条 alternative 覆盖层，不复制 query 正文。
- `aggregate.json`：聚合计数和安全范围；无独立样本增量、准入增量或评分结果。
- `input-manifest.json`：固定 Git 输入及原 review pins。
- `license-retention-manifest.json` 和 `retained-licenses/`：九个已授权公开法律文件。
- `candidate.py`：离线重放、精确字节校验与产物校验。
- `test_candidate.py`：自编边界与负例；`tests.log`、`validation-receipt.json`
  记录实际验证，`artifact-manifest.json` 绑定所有交付文件（自身除外）。

## 重放

需要固定公开 Git 对象已在本地、Python 3.12 和标准 Go 编译器。不得为缺失输入
访问其它 repo、private/old42、被拒资料或 live provider。原 review 只读，scratch
仅位于本目录。缺失输入即失败；不以当前产品返回替代证据。

```sh
D=artifacts/checkpoints/public-dev-declaration-kind-candidate-20261003
R=artifacts/checkpoints/public-dev-kind-review-20261003
mkdir -p "$D/.scratch"
GOCACHE="$PWD/$D/.scratch/go-cache" GO111MODULE=off \
  go build -o "$D/.scratch/go-taxonomy" "$R/go_taxonomy.go"
python "$D/test_candidate.py"
python "$D/candidate.py"
```

本环境 `/usr/bin/go` 不是 Go 编译器，因此实际使用原 review 已 pin 的官方
`go1.23.12.linux-amd64.tar.gz`，SHA256
`d3847fef834e9db11bf64e3fb34db9c04db14e068eeb064f49af747010454f90`，
在本目录 ignored scratch 中下载、验证、构建；未访问新的 source corpus。
重放不要求 compiler binary 哈希跨路径一致，独立 helper source 与 Go archive
哈希固定。`--write` 可以在本目录重新派生五个 JSON 文件和法律副本；正常命令
按已交付文件的完整字节验证，不能靠改写 manifest 让漂移通过。

改动独占本目录。`/workspace/.agents`、`.codex` 为空，固定基线未发现 AGENTS.md
或 SKILL.md。原审查区、原 gold/source-map/admission、raw/score/default selector
及产品主链未修改。301/256/280 历史规模不成为新的独立样本。
Express/TypeScript 未自动改为新 taxonomy，四 repo 分数不合并。

12 个自编测试通过，完整 source/owner 重放及候选字节检查通过。
Rust 1.95 的 `cargo fmt --all -- --check` 通过；offline clippy 因本地 index
缺失 `libc` 在解析依赖时退出 101，workspace/corpus tests 因相同依赖阻塞未运行。
没有通过新的产品检索或网络补依赖来替代候选检查。环境初始 cargo PATH/rustup
home 问题已用现有工具链路径解决，详情与日志见验证凭据。

候选交付后停止，等待 root 独审；draft PR 不等于准入，不 merge/deploy。
