# Requests 索引修复独立验收：BOUNDED_REJECT

固定被验收提交：`da5b05ee08d84fd336d5a98f25da7cae176daebf`（PR103）。
执行源：`ee1988521e2125f86d2ff0aff8559dccc2a417b0`；两者 cc-index/src 和 cc-model/src 的 Git tree 完全相同。
基线：`ace2bc7983be2955831c9384e44d1bdd0749c909`（PR92）。
旧负例：`b25723458a77edd2207ed1a52dceb0ea1009cb93`（PR99）。
远端三个 PR head 在开始和结束均与指定 SHA 一致，结束原始记录见 `remote-shas.txt`。

## 阻断发现：合法类型名被过滤

`type_atoms` 新增的 `a.chars().any(char::is_alphanumeric)` 不等价于“存在合法标识符”。以下输入均被生产解析器正常解析、索引没有 parse_errors。Python 三项还经过本环境 Python AST 与 `str.isidentifier()` 独立验证。

| 合法类型名 | 旧版 uses_type 边 | 固定版 uses_type 边 | 旧 → 固定 name_bucket |
|---|---:|---:|---|
| Python `_` | 1 | 0 | 存在 → 缺失 |
| Python `__` | 1 | 0 | 存在 → 缺失 |
| TypeScript `$` | 1 | 0 | 存在 → 存在（其他解析途径保留） |
| Python Unicode `℘`（U+2118，Other_ID_Start） | 1 | 0 | 存在 → 缺失 |

最小重现：

```python
class _: pass
def consume(item: _):
    pass
```

旧版 `type_atoms("_") == ["_"]`，固定版返回空数组；生产 full fresh indexing 同时证明对应类型边和依赖消失。`℘` 直接证伪“合法 Unicode 类型不丢依赖”。证据见 `legal-old/legal-identifiers.json`、`legal-fixed/legal-identifiers.json` 及对应原始 sample 文件。可重建的独立 harness 为 `harness.rs`；没有修改生产实现。

建议修复方向：对 ellipsis/分隔符做有边界的排除，保守保留未知名称，尤其不能把下划线、美元符号及 Unicode Other_ID_Start 一概当作无效类型。此建议未实施；生产修复属于其他 session。

## 已独立通过的限定项

- 两个不同 Python variadic tuple 负例在 PR99 均报 `invalid resolution manifest: invalid dependency key`；固定版均成功。非 variadic、qualified 和 Unicode union controls 两版均成功。
- 新版独立测试的六组 qualified/generic/union/Unicode/多种空白类型 atoms、七组标点、七组 name keys 按预期通过。qualified exact/leaf name bucket、命名 variadic 类型、Unicode union 的依赖通过实际落库验证。
- 构造、normalize 后及序列化往返后的显式空 dependency key 均被严格 validator 拒绝。PR92→PR103 在四个相关 crate 的生产 src 范围内只改动 helpers.rs 与 resolution.rs；数据库写入和读取的严格校验未变。
- 独立公开 Requests 源 `psf/requests@611c6162cbc4ac2020a2f91c7cfa4f3abf9bbb60` 的完整 src/requests：物理文件 20 个（19 个 Python + py.typed）。旧版 fresh index 在 exceptions.py 失败；固定版 fresh index 扫描/解析 19 个 Python 文件，359 symbols、520 chunks、2420 dependencies、19 个经严格验证的 manifests、0 parse errors、0 empty keys。

Requests 来源从公开 Git 获取，并以 `requests-source-manifest.json` 锁定每个文件 SHA256，保留 LICENSE。**原指定“20 文件 / 2416 deps”语料的固定 SHA 与可读来源未获得，不能声称验证的是原语料，2420 与 2416 的差异也未归因为已确认的合理变化。** 本项只是独立补充证据。

## 验证与重建

环境：Linux，rustc 1.95.0；只调用本地索引器，不调用 provider。

运行 `python3 artifacts/checkpoints/independent-requests-fix-20261003/verify.py` 可重验保存的独立结果、旧负例、四项回归及 full-index 数字；返回 0 表示证据内部断言成立，**不表示修复获接受**。

`setup.py` 从两个固定 Git 提交提取仅 Cargo 与 src 构建文件，跳过 benchmark/holdout；每个源码文件哈希见 `source-manifest.json`。私有 helper 的直接 controls 使用原函数的机械提取副本，仅改变可见性以独立调用；生产 end-to-end 验证通过未经修改的 cc-index/cc-db/cc-model/cc-parsers 源码路径依赖执行。

```bash
python3 artifacts/checkpoints/independent-requests-fix-20261003/setup.py
export CARGO_HOME=/workspace/.cargo RUSTUP_HOME=/workspace/.rustup
export PATH=/workspace/.cargo/bin:$PATH
export CARGO_TARGET_DIR=/workspace/codecortex/target
cargo build --offline --manifest-path artifacts/checkpoints/independent-requests-fix-20261003/harness-old/Cargo.toml
cargo build --offline --manifest-path artifacts/checkpoints/independent-requests-fix-20261003/harness-fixed/Cargo.toml
# 每次选新的输出目录；harness 拒绝复用已有数据库。
target/debug/independent-requests-old old artifacts/checkpoints/independent-requests-fix-20261003/replay-old-controls
target/debug/independent-requests-fixed fixed artifacts/checkpoints/independent-requests-fix-20261003/replay-fixed-controls
target/debug/independent-requests-old old artifacts/checkpoints/independent-requests-fix-20261003/replay-legal-old legal-identifiers
target/debug/independent-requests-fixed fixed artifacts/checkpoints/independent-requests-fix-20261003/replay-legal-fixed legal-identifiers
```

完整 Requests 模式第四个参数为保存的 `requests-main-source/src/requests` 的绝对路径，输出 full-index.json；旧版因严格验证失败，保存 error 后以 panic/101 退出，是预期负例。成功模式还需核验 report.parse_errors 和 scanned/parsed/manifests 数，不能仅凭进程退出码判定。

构建初期缺少离线锁定依赖，随后只从 crates.io 下载声明依赖；最终两套构建以 `--offline --locked` 成功。初版 harness 的相对路径与控制组分类错误在最终独立证据运行前已纠正；早期失败运行保留在本地但不计入验收证据。主要结果以 old-controls、fixed-controls、legal-old、legal-fixed、requests-old、requests-fixed 为准。

## 边界与剩余阻塞

结论 **BOUNDED_REJECT** 仅针对这两个生产函数的修复与合法类型依赖保留要求。阻断原因是已证实的四项类型边回归，其中三项也丢失依赖。

另一个未完成项是原固定 Requests 20 文件/2416 deps 语料身份核验。缓存升级/迁移未执行，fresh pass 不代表迁移通过。作者 563 pass 未计入任何独立证据。

仓库与固定提交未发现 AGENTS.md 或 .agents/skill 指令；/workspace/.agents 与 .codex 为空，已阅读 CONTRIBUTING.md。未读取 public-v19 内容/holdout；未改 production、版本、ledger、gold；验收执行阶段未 commit、push、merge、force-push 或 deploy。只新增指定独立验收目录并停止测试。随后收到同任务的证据交付授权，仅允许此目录独立分支 commit/push/draft PR；该授权不改变 BOUNDED_REJECT、四类反例、原语料身份未确认与迁移未执行的结论。
