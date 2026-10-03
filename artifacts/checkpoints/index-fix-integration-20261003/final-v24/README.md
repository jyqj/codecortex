# 固定修复集成：schema24 / manifest3

固定生产源码 `9ebdb155c64e094b3d774be41dbe7ab8c4e222c1`，基于PR101 `574f7598662334c63e020da136c87f4f7281554d`，组合Go102和Requests修复最终 `671063b11af8cb40a0d526098de82e684dd24aca`。Requests新独立复验 `9e0c34aa5d487aa026953300b9a54ddbd5ca37b2` 为BOUNDED_PASS；原103的BOUNDED_REJECT与负证据保留。Go独立review104/0647889、cold块独立review70c2a64范围不扩大。

为何再升版本：公开中间执行树57bedba曾生成schema23/manifest2缺边。修复后同版本bf10b64真实重开五库仍no-op缺边0，而同binary fresh/full各恢复1边。该对照已确认，父批准schema24/manifest3；代码只用既有成熟重建机制，没有新增算法fingerprint或unsupported指标。

`identifier-migrations.json`：PR101真实v22/v1→v24/v3及中间真实v23/v2→v24/v3，共10路径，源码hash不变，中间原源码mtime不变。每次重开观察files0/新incarnation，然后full:false正常解析；五种合法符号各恢复/保留1条target UID非空type edge和name_bucket；再重开no-op parsed0/skipped1且DB状态稳定。未手工删库改key。初始五库SQLite快照仍不可变保留。

`pinned-boundaries.json`：保留v22及v23真实旧query handle与温缓存，新default产品升级后，旧handle读新generation/新源码，旧v1/v2 manifest reader均明确拒绝v3。v23 reader是修复后尚未bump的bf10b64产物，打开57bedba实际生成的库而不full重建。4次local search，0真实provider。

`public-index-receipt.json`：Requests锁定20源码/配置、Gin固定context.go，旧PR101负例保留，新版实际index成功；两旧正常cache升级/no-op及nested/variadic后续索引成功。`gin-repository.json`保留完整Gin109文件真实升级和稳定no-op。源码锁与许可证沿用上层已验证身份。

构建：same-v23与v24为两个全新独立target，default features，源码/Cargo字节hash与compiler-artifact身份、binary hash见build-receipts。default产品执行使用单独保留的binary，避免all-features test覆盖。Rust1.95.0。全features strict clippy/fmt通过。

全features no-fail-fast完整结果2474passed/4failed/68ignored，148 suites，未增加skip或ignore。fixture500ms失败在其它构建结束后单独原样重跑1/0；semantic lifecycle同5s超时在PR101同features复现；semantic_runtime明确cache put Read-only filesystem，停止对应写入，不改权限/换输出根绕过；另runtime review等artifact2s超时尚未独立归因。完整原始失败/通过日志与SHA见validation-log-manifest。不能称整个CI全绿。

父新告知的GC/WAL风险仅写ledger边界，不追加fault执行；不宣称完整crash、100k、V20、heldout或live质量通过。已被明确Forbidden的PR动作不重试/换入口；本轮只提交/推送集成树和证据，不merge/deploy。

Default核心检查 cc-model/cc-parsers/cc-index 全targets通过，另 ci_schema_guard_contract/semantic_schema迁移9项通过；精确计数见validation-log-manifest。未补跑被拦GC/WAL故障执行。整个all-features运行仍记4失败，不以单独fixture通过抹去原失败。
