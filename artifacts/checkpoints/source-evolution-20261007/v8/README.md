# P7 reviewed source union v8

This source-only registry reconstructs 776 crate/Cargo inputs from the immutable v3 base and six disjoint, explicitly reviewed deltas. Existing validation-work and Python capture changes remain pinned; semantic coverage/retention, GC unlink accounting, public-strategy evaluation and worker contention are separately reviewed additions.

The 23 changed paths are copied from their fixed source commits after review. No current-source inventory is treated as self-approval. Author execution scopes and original failures remain in their checkpoints; neither isolated sources nor this product commit inherit whole-task, live, quality, scale or release approval.

The legacy CI selector changes explicitly, and 19 existing default stdio consumers use the verified default binary snapshot. Original gate commands and thresholds remain intact. A separate fresh-target P7 workflow executes the current engineering matrix and keeps raw logs. Actual source controls, final review and CI are recorded separately after execution.

实际源码准入 CLI 与 14 项完整性控制均通过；这 14 项包含正控和拒绝控制，不能全部称作负控。固定原日志、命令与源码绑定见 `verification.json`，最终独审见 `reviewed-source-v8-review.json`。这些结果不认证最终组合的产品行为或整个 G7。
