# P7 / P8 合并源码独立审查

结论为 **accepted_scoped**。本目录只保存固定 Rust delta 的独立静态审查和完整
PRODUCT 字节清单；新合并 CI 尚待执行，不在这里写成通过。

| 对象 | 固定身份与范围 |
|---|---|
| 独立分区 source | `3359647e81ea73b9ee98a56d6753f1cfa741b3d2` |
| 分区 base | `6d02d77f018a5965a6f289b0b43558ed4b9f8322` |
| 分区 tree | `f128d45c4167e7015a26236deac586c93c8b6fb6` |
| 分区范围 | 恰好 22 个 Rust 变更路径，781 个完整受保护输入 |
| 完整 PRODUCT | `d77a2143cdb82e722b1d1c62851298c43e707b65` |
| PRODUCT tree | `647605286a3706dcda4df064d40356de4d62385b` |
| PRODUCT parents | 主线 `b951f27d3ed50b7755bc2456c6425355f753ec17`、P8 `40d54460f38ed13810f9898bc7257bc2cdd7fbad` |
| PRODUCT 范围 | 785 个受保护输入；不是上述 781 输入分区源码的相同树 |

`review.json` 固定 source/base、22 路径 before/after SHA-256、独立审查方法和分区
规则。`product-inputs-d77a2143.json` 是完整 PRODUCT 的逐路径模式、Git 对象和
SHA-256 清单，摘要为
`f62b219cfde5404ea2d94a4d43acf559415c3ba7331faed6bfbaf8ffd9eda1fc`。

## 保持分区不相交

主线策略组的 6 路径与 P8 的 17 路径只在 `cc-eval.rs` 交叉。新的
`p7_strategy_p8_engineering` 联合组取其 22 路径并集；原策略和 P8 的 source/review
记录保留为历史，不能作为两个相交的活动组重复批准 CLI。

以下主线五组的固定 source、review、review path 与全部原字节保持，联合组与它们
没有交叉：`gc_unlink_accounting`、`python_capture_revalidation`、
`semantic_coverage_retention`、`validation_work`、`worker_contention_measurements`。
结果是 **6 个不相交分区，39 个批准路径**。

独立读取了五组实际 source/review 对象，核对主线 registry 的 before/after 与 review
摘要，再从固定全局 base 加五组原字节和新联合组，逐字节重建出完整 785 输入 PRODUCT。
联合组每条路径在显式 `6d02d77` base 的旧字节，也与组合时已接受的旧字节一致。
这个字节重建不代替完整历史准入程序的执行，其他五组没有在此重新宣称运行验收。

## 唯一重叠文件的审查

合并 CLI 的 SHA-256 为
`2de5e8cfc3fa8e8fffd572c0a9df4449cea6c46f12fcfa542127dc4a9a0074cc`。
本审阅者读取两侧到合并结果的 diff，并从原 P8 CLI 加回主线原有的两段代码，独立重建
出相同字节：P7 的 `AblateStrategies` 参数枚举与执行分支，P8 的 `io::Write` 和
Compare 独占创建输出、失败收据、flush 与退出码逻辑均保留，其他 CLI 字节不变。

其余 5 个 P7 策略路径与主线及原固定 source 相同；其余 16 个 P8 路径与此前已审
R3 相同。本审阅者已完整读取策略 runner、reporting、两份测试、新 export 与原独立
review，并延续此前全部 P8 审查。策略使用的 `Summary`、`summarize` 和原统计函数
字节保持，P8 只增加测量产物及统计功能；完整原 canonical oracle 也保持。

## 验证与后续准入边界

- 本审阅者没有运行 Rust 或合并 CI；原测试记录仍归属其真实 source/binary。
- 最终 registry 必须分别绑定联合组 source、实际发布的独立 review 和完整 PRODUCT，
  不能把 781 输入的分区 source 当作 785 输入的产品。
- 保留现有 disjoint、精确 delta、before/after、review 固定字节和完整 PRODUCT 校验；
  联合组只使用此前已独立审过的显式 base 支持，不增加例外。
- 保留主线 historical-integrations-v2 wrapper、其测试、P7 engineering workflow 和
  19 个隔离的默认产品路径。主 CI 仅增加既有 P8 两条 Python 检查并更新明确 selector。
- 原 P7/P8、完整质量、100k、发布或性能认证不会因为源码合并而自动关闭；不从 Git
  bundle 授予源码准入。
