# P8-017 独立复核：限定统计路径去重

结论：**接受本次两文件重构的有限范围；P8-017 的完整验收未完成。** 机器可读证据见 [review.json](review.json)。本轮没有重新运行已完成 Rust 测试，也没有修改 Rust、任务或 schema 文件。

## 已确认的变更与语义

固定本地 source `3f6cca74c1c9da16cd5eca818c1bce57659d1db0` 与远端 source `78ae91eeae6edae6bea29c27f24b251773341c00` 中，`benchmark::statistics` 的 `distribution` 和 `quantile_interval` 现在共享一个私有 `nearest_rank` 实现。该模块内只有一个 `ceil(q*N)` 取秩表达式，三个调用分别为 p50、p95 和 interval 点估计。

空样本返回 `None`；`quantile_interval` 保留原有 `q ∈ (0,1)` 限制，对零、负零、越界值、NaN 和无穷值仍返回 `None`。排序、ceil 后饱和减一与上界夹取保持原式，`get/copied` 替代已保证边界内的索引。interval 函数除点估计委托外逐字节一致，二项分布 order-statistic 上下界与无界 `None` 语义没有改变。

均值 `bootstrap` 是另一过程，其完整函数体与 2000 次 draws、`draws[49]` / `draws[1949]` 端点逐字节保持，**没有调用 `nearest_rank`**。`quantile_interval` 使用二项分布次序统计量，也不是 bootstrap 重采样。

新增测试使用独立整数 `div_ceil` oracle，覆盖长度 1、2、3、19、20、21、199、200、201 的乱序、重复和零值样本。其余既有 measurement 测试逐字节保留。

## 原始执行与来源核验

- 对两份固定 Git commit 的完整 **785 项输入、10,111,841 bytes** 独立核对成员清单及逐文件 SHA256，并与主工作树实际字节相对照：均无差异。
- [source manifest](../validation/current-product-source-inputs.json) 的文件 SHA256 为 `229acb8d2e7bed3f92c715f02ac5a22fed00b9baac04cfde6d06e1e442559db3`；规范化内容 digest 与固定 `7f992a0e` 独立 source review 一致。
- [原始 p8-regression.log](../validation/p8-regression.log) SHA256 为 `dffd2577e87abad2857bfcb7a3679edb4b0cdccf2a014b01857446f3e04d6839`。measurement 段 **12 passed、0 failed、0 ignored**；全部 12 个名称与固定源码中的同步/异步测试逐项一致，包含新增 oracle。
- log、source manifest 与 [build witness](../validation/cc-eval-build-witness.json) 的绑定相符。该 witness 明确是构建后的 source/binary 一致性证据，不是冷构建或密码学编译器证明。
- 固定 [source-review.json](../source-review.json) 对此重构的函数名及边界措辞准确，未发现“均值 bootstrap 也统一调用”的声明。

## 仍未清理的具体路径与原验收

固定源码的 `cc-eval/src/bench.rs:117` 仍有独立 percentile 取秩式，`cc-eval/src/report.rs:34,66` 仍有两处 p95 取秩式；`lib.rs` 继续公开导出这两个模块。这些路径与本次重构前相同，因此“只有一个算法所有者”只能限定在上述 `benchmark::statistics` 两条路径，不能扩展为整个 `cc-eval` 已去重。

原任务仍要求清理临时旧 branch、重复评分器、多份 schema 来源，保留必要外部 wire 兼容，并满足 V18/V21 和 P8-016 前置。该完整盘点、删除和兼容回归均未在本轮认证，保持 `not_run/not_certified`。本次有限重构没有发现新增阻断性正确性问题；它不构成全任务、性能或发布认证。
