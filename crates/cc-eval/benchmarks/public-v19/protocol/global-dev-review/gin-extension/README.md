# Gin public dev delta（开发快照准入）

本差量基于两 repo PR86 / `9213fef97c8bd32843eab6815227f7d78142ab80`。原两 repo 回执、决策和组件表保留；默认运行仍逐字节复现原 `evidence/admission.json`。使用 `--include-gin` 才启用本差量。

固定 Gin 作者 `949a9471f552d496e85b27c460c7b76536f13803`，来源审查 `ba79bcfc3f2bcc622297ef53b5de37ce205a5c89`，repair 审查 `5aafcda4a5098c8407d9373eafc3185e59932550`。只读当前公开67 native /55 compat；66本地 components，33受阻holdout正文没有读取或准入。TypeScript不在本三repo版本准入范围；其后续独立审查与pair裁定归另一个四repo差量。

repair回执绑定两份当前dev文件SHA256；本检查逐行重放4个修正accept和63个保留accept的原始行（含换行）哈希，重放4个修正compat行，并把63个保留accept与原独立审查的body-free行哈希连接。审查者必须与原作者不同。原审查仍有历史4项needschange；当前4项修正不改写旧记录。没有读取聚合gold、历史/隔离题文、任何heldout题文，也没有执行作者或审查者脚本。

来源回执对原作者快照77d8707完成upstream Git原点核验，本检查通过当前source-manifest、source-lock和全部53文件字节/Git blob一致性，将该核验绑定到当前949a947快照。来源回执SHA256为 `9c64ebcfdf6879a5f209e1ec48e7c7292a48c744d8e8c2200d4366b060823427`；MIT license SHA256为 `b104efb2c7700691650f27034e8541c5ae0ed9af54d884287f19a7467ca2fe7f`。这是审查者的upstream下载证据重放，本任务没有另行下载upstream；当前suite仍是commit=null文件快照，不宣称当前快照是clean Git checkout。

三repo合计228 native /197 compat，80 source files，378 gold spans，126固定输入文件哈希；6个实际离线evaluator validate与3个protocol check通过。Gin自身127个answer spans通过边界/UTF8检查，当前annotations中的source evidence再与固定来源字节和SHA核验。实际evaluator仍绑定原offline构建/源码回执，不运行ranking/provider/生产检索。

当前公开dev的跨repo pair检查17157项，normalized exact duplicates 0。原10项人工决策保留，新10项任务/模板/事实判定记录当前row和source-span哈希，5项保守相关边；7条总相关边把215本地components投影为208 correlation components。registry SHA256为 `74b94a1f157089d18cc05613befaacb51e893660f849d7762201baf7b206a8f3`。已证明的跨repo同任务等价合并0；208不是已证明独立问题数，更不是600问题正式验收。

开发快照准入状态 `development_admitted_snapshot_scope_only`，errors={}。formal600、cleanholdout、正式完整20块均0。不同语言任务的相关事实仅用于保守correlation分析；不修改评分分母、作者题文、split或gold，不据此关闭全语料独立性、custody、facet/graph统计和质量gate。

复现：

```sh
python3 crates/cc-eval/benchmarks/public-v19/protocol/global-dev-review/check_admission.py \
  --include-gin --evaluator <exact-receipted-cc-eval-binary> --output /tmp/v19-gin-admission
python3 -m unittest discover -s crates/cc-eval/benchmarks/public-v19/protocol/global-dev-review -p test_admission.py
```

本轮13测试通过：原8项保留，新增三repo真实准入、upstream证明篡改、行承诺篡改、Gin越界query路径拒绝、原两repo回执不变。仅运行Python检查与既有actual evaluator validate；未重新构建Rust、运行全仓测试或live/排名。

可用Gin当前dev suite入口：

- `949a9471f552d496e85b27c460c7b76536f13803:crates/cc-eval/benchmarks/public-v19/gin/suite-native-dev.json`
- `949a9471f552d496e85b27c460c7b76536f13803:crates/cc-eval/benchmarks/public-v19/gin/suite-compat-dev.json`

集成者需先采用PR86，再采用本Gin差量；权威ledger由集成者更新，本任务不写主ledger。
