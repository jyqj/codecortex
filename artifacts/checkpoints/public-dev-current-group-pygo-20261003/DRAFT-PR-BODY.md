固定 PR131 产品 `88f2cf099c8b81f3acef485fd5ac9b01c63ce790` 上重跑 PR91 已准入 Requests/Gin 公开 DEV，补齐旧基线中本组因 index prepare 失败而未执行的 888 scheduled rows。本任务只追加 group-pygo 的证据与附录，产品、suite/query/gold/source/config/scorer/阈值/统计计划不变。

4 suites：Requests native273/compat249、Gin native201/compat165，scheduled/executed888、missing0、Partial888、NoMatch0、Success0；各套 run/replay exit1。60 native no-answer 行全部 strict rejection 错误。旧 invaliddependency/duplicatecallsite 准备阻塞未复现；这只证明本组本轮准备成功，全部 Partial 仍为质量失败，不认证完整质量。

验证：official Cargo.lock + installed direct Rust1.95.0 默认 features 构建；708 crates/Cargo 文件固定且与 e3 产品相同；admission/hash 冻结、排名前 commit/push、每套唯一完整 schedule、初次 replay 与 965 文件 archive readback/replay/analysis 全部逐字节一致。只保留本轮授权公开 DEV raw/summary/selftools/notices，没有整库 source/binary/db；旧证据、中央 TODO、其它 owner 不动。

范围仅 Requests91native/83compat + Gin67native/55compat；不是四 repo16suites/1671全量，不是 holdout/正式600题。原 TypeScript pilot、Express/TypeScript、全四repoaggregate、双产品paired intervals、live/paid/semantic/release均 not_run。详见 `artifacts/checkpoints/public-dev-current-group-pygo-20261003/APPENDIX.md` 与 aggregate/readback receipts。

这是依赖 PR131 固定产品的证据分支。未运行用户明确排除的故障 tests；未访问拒绝源或私有诊断。
