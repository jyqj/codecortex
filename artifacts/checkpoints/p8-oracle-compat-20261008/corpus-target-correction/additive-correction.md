## 追加更正：P8-002 的问题数与 family 口径

`artifacts/checkpoints/p8-corpus-lock-review-20261008/README.md` 中的 “formal 600-family acceptance target” 属于解释错误，现明确撤回该验收口径。原始权威 `docs/roadmap/code-index-v2/09-BENCHMARK.md` 第 7 节 D2 的原文是：“6 个以上真实仓库，覆盖 Rust/JS/TS/Python/Go 与至少一个 mixed monorepo；初始约 600 个审阅问题”。`tasks.json` 的 P8-002 要求扩至规划语言/仓库覆盖、复核 gold，并透明报告题数、类别、语言、能力缺口和实际审阅范围；未要求 600 个独立 family。

该文第 7 节的独立复核要求针对每题源码和 gold 的审阅。第 5 节的 query_family 分割规则与第 10 节的 repository/query-family 分层 bootstrap 用于防止改写泄漏并处理统计相关性，不能据此把“约 600 个审阅问题”改成“600 个独立统计样本”的硬阈值。`04-PHASES.md` 的 P8 阶段计划、`06-VALIDATION.md` 的 V19 原验证条目及统计门说明均未增加该阈值。

旧脚本 `scripts/p8_corpus_audit.py` 输出中的 `formal_family_target=600`、`formal_family_shortfall=600` 是硬编码的派生解释，`formal_accepted_families=0` 也不能用作对该不存在门槛的验收计数。旧 JSON、旧脚本、旧 README 和原测量保留，后续结项不得使用这些字段或相同的文档表述提高原始门槛。本次审计 `new_independent_gold_reviews=0` 表示这次机械重验没有新增独立语义复核，不撤销历史回执已经接受的开发语料审阅范围。

实际保留的 4 仓、301 条 native DEV、256 条 compat、301 个 family 标签、287 个本地组件及 280 个保守全局相关组件分别按原证据报告；它们不直接等于统计独立样本数。P8-002 的当前缺口依据是原规划中的 Rust 与 mixed monorepo 覆盖、相应真实源准入和逐题 gold 复核，以及原 V02/V19 所需的类别、输入锁和质量证据。问题规模按“初始约 600 个经审阅问题”的原口径透明报告；本更正不根据一个新造的 600-family 缺口拒绝结项，也不据现有行数单独宣称完整认证。
