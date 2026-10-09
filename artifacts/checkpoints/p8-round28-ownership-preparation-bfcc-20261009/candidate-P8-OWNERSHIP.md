# P8-017：已核实现归属与必要兼容矩阵

本页记录 `51138c236cf2ed47858e2306f9c7bfd3600aaef9` 的有限源码核查，
对应完整 tree `24df8be11955fda1f2e320de483e9eb4ddd1ffb3`。
它为 P8-017 的“一个事实与算法所有者，保留必要外部 wire 兼容”提供可追溯清单。
本页没有删除代码，也没有将有限核查扩写为全仓无重复实现证明。

## 已收口的命名范围

| 范围 | 当前实现归属和调用关系 | 保留的行为 |
|---|---|---|
| 百分位统计 | [`benchmark/statistics.rs`](../../../crates/cc-eval/src/benchmark/statistics.rs) 的 `nearest_rank` 供 distribution、quantile interval、[`report.rs`](../../../crates/cc-eval/src/report.rs) 和 `legacy_latency_ns` 共用；[`p8_runtime.py`](../../../scripts/p8_runtime.py) 复制实际 Rust statistics 输出中的 `legacy_ns`，不在 Python 再算该组百分位。 | 毫秒、微秒与 legacy 纳秒各保留输入单位；纳秒统计包含全部 offered terminal outcomes，不过滤拒绝和失败，也不先舍入为微秒。空样本的不同 wire 默认值由各适配入口保留。 |
| 默认检索通道目录 | [`lanes.rs`](../../../crates/cc-search/src/lanes.rs) 的借用 `default_lane_registry` 同时供执行列表与 [`QueryPolicy::resolve`](../../../crates/cc-search/src/query_policy.rs) 使用。`default_lanes().to_vec()` 是执行列表，不是另一份目录定义。 | exact_symbol、path、lexical、grep、graph 的顺序及 policy 义务不变；semantic 仍按实际策略另行追加。局部 ablation 清空执行 Vec 不会删除 policy 目录。 |
| 当前数据库 DDL | [`index_migrate.rs`](../../../crates/cc-db/src/index_migrate.rs) 的 `FULL_SCHEMA_SQL` 包含 `sql/index_v1.sql`；当前 schema 的两个物理索引维护语句经 [`direct_writer::selected_index_statements`](../../../crates/cc-db/src/direct_writer.rs) 从同一 SQL 提取。 | 缓存的是选定 SQL 文本，不是连接、行或 schema 判定结果。当前版本只维护物理索引；不改逻辑数据、incarnation 或 epoch。非零旧版本继续按版本不匹配要求重建。 |
| 版本化 benchmark 评分 | [`benchmark/metrics.rs`](../../../crates/cc-eval/src/benchmark/metrics.rs) 的 `score` 按 [`ScoreProfile`](../../../crates/cc-eval/src/benchmark/schema.rs) 分派；compatibility 与 native 共用其 linear-gain DCG/nDCG helper。 | `oce-compat-v1` 的 expected-files 顺序与 native 的 answer groups、symbol/span 条件并不等价。两个版本化评分分支应保留，不能为消除“两个函数”而合并分母或改变评分。 |

任务历史记录中“默认 lane 尚有两份目录”描述的是当时源码。后续 lane-registry 证据已记录收口；
历史记录无需改写。以上结论只覆盖表中明确命名的实现和调用者。

## 必须区别处理的兼容与对照

| 路径或数据 | 类别 | 当前保留理由 |
|---|---|---|
| [`resolver/helpers.rs`](../../../crates/cc-index/src/resolver/helpers.rs) 的 `legacy_prefix`、`legacy_candidates` | `cfg(test)` 内独立参考实现 | 对照路径分段、并列候选、重复与顺序；不是生产 fallback。删除它们会丢失旧行为对照。 |
| [`resolver/catalog.rs`](../../../crates/cc-index/src/resolver/catalog.rs) 的 `legacy_exports` | `cfg(test)` 内独立参考实现 | 验证别名冲突及 remove/re-add 后的导出注册，不能与被测实现合并成共同来源。 |
| [`ci_schema_guard_contract.rs`](../../../crates/cc-db/tests/ci_schema_guard_contract.rs) 的历史 schema 夹具 | 原版本回归输入 | v21/v24 等历史定义用于检验不兼容库重建和当前数据不被误改，不是当前生产 schema 的另一所有者。 |
| [`benchmark/normalizer.rs`](../../../crates/cc-eval/src/benchmark/normalizer.rs) 的旧公开输出与版本化 packing 解析 | 外部 wire / 历史结果读取 | 保留已知版本的 omission 与 partial 真值；未知或畸形的新 receipt 仍报协议错误，读取旧版本不提升为 complete。 |
| `SearchHit` 中 legacy score slots、compat/native profile 名 | 对外序列化契约 | 内部实现收口不授权改旧 wire 标签、字段、版本或历史 comparator 身份。 |

兼容路径的删除必须对应原任务要求的旧行为回归证据。本清单不把 test-only oracle、历史输入和公开 wire
适配器认作待删除的生产重复实现，也不改变原任务定义或验收门。

## 现成回归证据及适用范围

[PR196 原 CI](https://github.com/jyqj/codecortex/actions/runs/37985696710/job/114007017987)
实际检出 `dac7d9c20e7586baa96648194f41c4383c3e166d`，完整 tree 与上述 main tree 相同。
保留原 test-merge 身份；这里没有重新执行测试。原完整 decoded log 的 Git blob 为
`907b9a33cb22d7a13b947e4fac6aaa121c594d55`，SHA-256 为
`9ccadfda3f610035295f27e229fbdbd4ac5b5f570b84168cfd112b33fa041370`。

| 命名范围 | 原日志已通过的控制（节选，不是新测试计划） |
|---|---|
| 统计与单位 | `distribution_and_interval_keep_one_nearest_rank_convention`；`preserves_empty_single_and_submicrosecond_wire_values`；`preserves_nearest_rank_boundaries_without_rounding_to_microseconds` |
| 检索目录与公开 policy | `default_lanes_registry_keeps_fusion_order`；`registry_cleanup_preserves_policy_wire_and_fingerprint` |
| 物理 schema 维护 | `current_schema_identity_index_preserves_rows_and_uses_indexed_fk_cascade`；`fifo_physical_index_preserves_existing_current_logical_data`；`v21_file_database_requires_reindex` |
| resolver 独立对照 | `export_registration_matches_legacy_after_remove_and_readd`；`export_registration_matches_legacy_for_alias_collisions`；`one_pass_distance_preserves_original_ties_duplicates_and_order`；`streamed_prefix_matches_original_path_segment_semantics` |
| versioned scoring / partial | `compatibility_is_linear_gain`；`compatibility_repeated_path_does_not_spend_rank`；`partial_empty_is_not_a_correct_no_answer`；`malformed_packing_status_cannot_be_interpreted_as_complete` |

完整日志中的 ignored 测量仍是 ignored，普通默认回归也不代表全部 feature、平台或发行验证。
本表复用已有实际回归，不要求因新增本文而重跑规模研究、holdout 或 live provider。

## 仍未闭合的原范围

P8-017 的原依赖 P8-016 尚未完成。本页没有签署完整 V18（MCP/配置兼容）或
V21（回滚/发行/文档）验收；其原定义仍见 [06-VALIDATION.md](06-VALIDATION.md)。
已有命名控制应按原 source 和实际执行范围复用，未取得的验收证据继续保持未完成。

本次核查未发现表中需要再删除的生产重复实现，未据此宣称其它路径已经全部审毕。
oracle 的 scratch/public-output 兼容恢复仍以独立 PR195 的源码和实际验收为准，
不属于 `51138c2` 已发布实现或本表已验收内容。
P8-018 的事实生成与安装说明入口仍是 [P8-FACTS.md](P8-FACTS.md)；本页不修改其受管表、
任何任务状态、受保护定义、依赖或生成导航。
