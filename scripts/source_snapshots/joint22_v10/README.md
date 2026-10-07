# 新主线 joint22 原始证明快照

`verify_reviewed_source_v12.py` 同时执行两个独立的固定来源证明，再核验它们到最终产品的独立审查记录。历史运行结果保留实际 source、binary、平台和失败状态；本次来源合流不授予质量、100k、跨平台、冷构建或发布验收。

## 固定来源

| 角色 | 固定 commit |
| --- | --- |
| 原 PR #146 交付快照 | `2a75e65d01a3722155e7a1858d7e0e9c8a5558cf` |
| v11 已证明产品 | `65dd32934b3f8cb3ff4f5f5deb154a431a8c09e3` |
| 合入 PR #145 的新 main 快照 | `d53a4972af92fd10a5cddb9f15ffdf06414b3d54` |
| 新 main 的 joint22 已证明产品 | `d77a2143cdb82e722b1d1c62851298c43e707b65` |
| 最终双来源产品 | `8e12c3884edbb2743eb2aee82fafa285e8b28ef7` |
| 最终产品独立审查 | `3056a14ccc3496b4e5c9ff1e3746bf6cf33a1b95` |

原 main 使用与旧 P8 guard 相同的文件名。为保留双方原件，新 main 的文件以下列别名保存，所有字节均直接来自固定 `d53a4972`；不改写模块内部常量或 import。

| 当前路径 | 新 main 原路径 |
| --- | --- |
| `scripts/verify_reviewed_source_joint22_v10.py` | `scripts/verify_reviewed_source.py` |
| `scripts/reviewed-source-joint22-v10.json` | `scripts/reviewed-source-registry.json` |
| 本目录 `test_reviewed_source.py.txt` | `tests/source_integrity/test_reviewed_source.py` |
| 本目录 `.github/workflows/ci.yml` | `.github/workflows/ci.yml` |

`v12.verify_snapshots()` 将上述原件及原 v11 脚本、registry 与固定 Git blob 比较，并拒绝文件改动或符号链接。旧 v10/v11 继续执行其自身的历史快照、完整证明和独立审查核验。

## 实际执行与合流边界

v12 先执行原 `v11.approved_union()`；该调用继续完整执行原 v10、两条旧 P7/P8 证明及 v1/v2/v3。之后，v12 通过原接口 `joint.load_registry(显式别名路径)` 加载新 main 的 registry，并实际执行未修改的 `joint.approved_union(registry, root)`。joint22 的六个审查组、22 项联合分区、互不重叠约束、历史审查文件与完整 785 项输入核验均正常执行。

显式 registry 路径是必要条件。别名模块的默认 `REGISTRY` 仍指旧原名，调用默认 `joint.load_registry()` 会因读取旧 P8 registry 而失败。v12 不修改该默认值，不替换原 guard 函数，不通过只核对脚本 hash 跳过实际证明。

随后，v12 使用本目录作为显式 root，实际调用原 `joint.verify_ci(root)` 核验 11 KB 的历史 workflow。当前 workflow 则由原 v11 workflow 仅迁移一次 v12 selector 得出，并继续核验原 P7 engineering workflow 不变。

独立审查的 `paths` 绑定 v11 → 最终产品的唯一 `p8_load.rs` 变化；`joint_paths` 绑定 joint22 → 最终产品的四个变化：`p8_load.rs`、`statistics.rs`、`p8_measurements.rs`、`p7_worker_contention.rs`。两组均要求精确路径集合、每个 before/after SHA256、固定审查 source/base/joint_base/verdict，最后核验全部 785 项输入。registry 固定完整输入摘要为 `4c9aeb9cac2eea382d2dbce6d85220af3e10f6483b4c1f696b7874ecaff0fdb4`（按路径排序的紧凑 path→SHA256 JSON，无结尾换行）。

相对 v11 的唯一新增内容是 `cfg(test)` 中的两个预算回归；相对 joint22 的其余三项差异延续已独审的 nearest-rank 清理、独立测量 oracle 和 worker readiness 顺序修复。这些来源声明与实际 Rust 测试或远端 CI 运行证明分开记录。

## 新控制与原 CI 负对照

`tests/source_integrity/test_reviewed_source_v12.py` 检查任一完整证明失败不能被另一证明掩盖、任一来源库存或字节不符、浮动 pin、审查记录漂移或缺少父来源、额外差异、before/after 不符、完整清单缺项及当前工作树自授权等拒绝条件。

原测试文件以 `.py.txt` 保留，避免自动发现时误导入旧同名模块。新测试从固定文件提取唯一的 `test_ci_only_allows_reviewed_migrations_and_local_p8_checks` 方法，保持该方法 AST 完全不变并实际执行。适配器只给无参数的正向调用提供本目录作为 root；每个负向 fixture 都传给原 `joint.verify_ci(root)`。原方法的新增 workflow 尾注、historical-v2 恰好一次、改回旧 E3、跳过 historical 检查、私有 binary 绑定恰好 20 次、改回共享 `$PWD/target/debug` 均执行。该适配器仅存在于测试层，不修改 guard。

这表示执行了原 CI 方法的全部控制，不表示重新执行了该历史快照内的全部 16 个测试方法。原工作树中的既有历史测试仍保留，由最终 CI 照常执行。

在已整合固定产品和最终 workflow 的 checkout 内运行：

```bash
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests/source_integrity -p test_reviewed_source_v12.py -v
PYTHONDONTWRITEBYTECODE=1 python3 scripts/verify_reviewed_source_v12.py --source-version p8-joint-source-20261008-v12
```

测试使用标准 `tempfile`，可通过 `TMPDIR` 指向私有 tmpfs 目录，避免复制完整 checkout 或创建 Rust target。上述命令是验证入口；实际运行时间、输出及通过状态由最终交付的原始日志记录。
