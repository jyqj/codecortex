Title: review: independently verify PR122 finite retry liveness

固定 PR122 head cec4ad8ef47c6c3fe86cae6a35e357935b94a0ec 的独立限定复审。全新真实 CodeIndex/runtime fixture 与自有 provider 在相同 acceptance oracle 下对照旧基线，验证普通一次 ServerError 后其他 ready 文档继续完成，保留失败项退避。

验证：修复版7 tests通过，18个width0/2、4/12/33文档与前中后失败场景通过；旧版相同oracle退出101（4 passed / 3 failed）。包含预算16、不可提前退避、close/join/cancel/token证据，保留PR120原始反例与全部初稿失败。仅提交 artifacts/checkpoints/independent-retry-liveness-review-20261003/，不改生产/CI/中央TODO，不整合PR121，不声称组合验收。

交付限制：GitHub GraphQL返回Forbidden后停止API对应动作；此描述为准备好的draft内容，未创建PR。
