# 四 repo 公开 dev 差量：TypeScript 与本地 pair 裁定

本版本基于Gin PR90 / `1bb86a6afaf64f8e922f0b71d42d5d6801567e52`，其前置为两repo PR86 / `9213fef97c8bd32843eab6815227f7d78142ab80`。原两repo和三repo回执不变，分别继续由默认check_admission与--include-gin复现。本四repo版本运行独立check_four_repo.py，只有当前公开dev，无任何ranking或生产执行。

TypeScript固定作者 `4c01f627bd6df73e70bfe1dbdc42b2a9234e797a`；first20独立review `d6a41bbcf76b5dc15e4924e54c7375f673d714f7`，remaining53独立review `704c0fd0901c85c51eaaa265bfc1aab49ccb3341`。aggregate SHA256 `1da27a0dbe1a75d277f60daff2e043ce414f433ef1d03c62bd7e88d4255d05f5`。20+53 source-fact接受记录与73条当前原始native行哈希一一绑定，10份当前dev文件哈希与review输入绑定，first20及upstream来源回执在两review提交中逐字节相同。保留52accept/1component-only needschange原事实，不改独立review历史。

## 本地组件裁定

pair-adjudication.json只存opaque family ID、当前原始行SHA、共享必要obligation的source-span SHA与受控reason code。唯一需要裁定的两条当前公开dev，一条的sole API obligation是另一条required chain facet；实际源码证明必要obligation复用。额外链路不能建立独立性，因此保守合并为一组件，canonical取两ID字典序最小。不是宣称完整任务等价，不删题、不改作者global_family或split，不把公开dev换名当heldout。71已独立通过组件加这一个保守pair组件得到72 local correlation components；73 source facts继续全部可用于开发快照。原needschange回执保持原样，裁定单列sidecar，不将其重新标成原reviewer签署accept。

审查所有73个当前公开dev问题，并对与三HTTP repo的任务/模板/事实作保守对照。新增10项人工跨repo决策仅锁定当前完整记录哈希和reason codes，跨repo证明同任务等价合并0，无新增跨repocorrelation边。共享词汇、文件主题或通用定位/trace/absence模板不足以认定相同任务。TypeScript内部共享必要obligation单独保守关联；完整语义独立性仍未证明。

## 来源与可执行 suite admission

20个实际TypeScript手写API/AST文件与固定manifest sizes/SHA、first20来源审查23条official-source/license witness相符；root Apache-2.0、完整NOTICE、vscode-uri MIT三文件字节/路径哈希保持。upstream source SHA `ed4807212c28c90777c1d7ef2bf8e47af5d08519`，official-source receipt SHA256 `e7d952edcaba0e3d04779d4fa2342f0a478551becf7ed94a387530a082fc0679`。不声称整个repo主语言或clean Git checkout，不下载或准入排除的generated/vendor/dependency文件体。这里只重放既有独立官方来源证据，没有另行下载upstream。

仅读固定blocks/01-migration、02、03、04、05的queries.native.dev.jsonl / queries.compat.dev.jsonl，以及对应suite.*.candidate.json；严格query路径allowlist在加载前拒绝越界。根questions/candidates.jsonl、旧/隔离/heldout question或gold体均未加载；27blockedheldout未读、未准入。compat只包含59 positive projections，其source集合可以是本block native集合的子集；每份suite实际source/query BLAKE3 digest由 unchanged offline cc-eval validate核验，不改写作者输入来通过检查。

TypeScript10个实际evaluator validate、5个protocol checks均通过，98个gold spans边界/UTF8通过，annotations中的来源字节/跨度/file hash再次核验。四repo总16actual validates、8protocol checks，301 native /256 compat，100 source files、476 gold spans、179固定输入哈希，errors={}。跨repo33801pairs normalized exact duplicates0；30人工pair decisions，7总cross-repo conservative edges，287local→280global correlation components。registry SHA256 `25866458ea556b347294c209b1cc986c9538d581d1b96f91ceef714d04c3d639`。280不是正式独立600问题数。

formal600、cleanholdout、正式完整20块、ranking、live/provider、protected body reads均0；serde/vite来源阻塞、六repo目标、custody/独立性/facet/graph统计仍open。没有改production、scorer、作者gold/注释/分割、主registry或权威ledger。

## 复现与验证

```sh
python3 crates/cc-eval/benchmarks/public-v19/protocol/global-dev-review/check_four_repo.py \
  --evaluator <exact-receipted-cc-eval-binary> --output /tmp/v19-four-repo-admission
python3 -m unittest discover -s crates/cc-eval/benchmarks/public-v19/protocol/global-dev-review -p 'test_*.py'
```

默认沿用原83a6b54 offline evaluator源码/Cargo/实际binary构建回执，替代构建可显式--evaluator-build-receipt，规则同PR86。只用既有validate，不执行ranking、Rust重编译、全仓测试或live/provider。18回归通过（原13+四repo真实准入、TS source篡改、额外内容错误不能由pair裁定消除、TS越界query先拒绝、pair source witness篡改）。原两repo回执不变的回归仍执行；三repo版本也继续保持独立复现。

initial-gating-receipt.json保留早期检查器误报：compat source子集被错误要求完全相等，official witness路径哈希错误遗漏source/前缀。修正两处检查器后按独立review原规则核验，无题文、gold、来源或原审查的放宽/修改。最终receipt errors={}。

admission.json的repo_results.typescript.dev_suite_entries给出固定作者SHA下的10个可用dev suite入口。它们名字仍是candidate，开发准入裁定在此sidecar；不改作者名称为自签accept。集成顺序PR86→PR90→本四repo差量，权威清单由集成者落账，本任务未写共享ledger。
