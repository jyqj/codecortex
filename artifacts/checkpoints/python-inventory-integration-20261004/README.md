# Reviewed Python inventory capture 组合集成

Integration base：PR138 当前 `2d297de0e5c7776c358ab49ab2072659ac03c8fc`。
Fixed product（精确 capture + 新组合 fixture）：`780502322816e4fd1d61d8f77a98d7d6b635d9fb`。
仅接受 opt-in Linux/EntireProject 的有界 capture→AST→model 声明准入；runtime publication、
release、质量、规模与完整 P7/V19 仍不在本次验收内。新分支
`integration/python-inventory-capture-v2`，正常 origin 交付，不覆盖 PR138/139，不 merge/deploy。

## 精确 source 和 review 历史

Root 完整读取 `docs/internals/python-inventory-capture-v1.md`、原独审 full report 和
P2 delta report。最终采纳 PR139 `e3b932ed4b1e197022c0902fd4c11af3e87ae87e` 的精确源码。

| 内容 | 原提交 | 集成提交 |
| --- | --- | --- |
| capture 原实现 | `8d2b312c066f0514fb5cee555767955bf34708d0` | `d0b9b2a` |
| 作者原交付/回执 | `892fd103f543424323e1d4e6691eca5774ede1a6` | `f070280` |
| full 独审/P2 finding | `e010c9e010147f47abaa2b7278e2aabe5d687ddd` | `b45ac98` |
| 原 review delivery | `a17f05d674e82bc54aac6a664b2b69a87b276b36` | `d0d784d` |
| final capture / P2 fix | `e3b932ed4b1e197022c0902fd4c11af3e87ae87e` | `f816e77` |
| fixed delta 独审接受 | `1fe4dc5f44f53e215e3b6cc99c756bd9b62b0a18` | `f152aa8` |

各 commit delta 的全部路径和 bytes 已比对，完整 SHA 映射在 `provenance.json`。
PR139 的 full review cherry-pick `eee56e3a86608ee9211f235642b3962ec3de6777` 与原独审
e010 的 fixture/report bytes 由原 review delivery 和本次 source guard 核验。
原 request-changes/report/logs/publication receipt、修复前再次复现的红色证据、fixed delta
review 全部原样保留，未将原 source8d 的失败改成通过。原
`review_contract_partial_find_must_refuse` 全文件/断言不改，现在在组合树实际通过。

对 approved product `50a4933e48ef20b16401ac8f75c386a660aa8e5c` 的生产变化只有
`project_model/mod.rs` 的一行 export 和新 `python_inventory` 实现/native 模块。
capture 五路径 delta（含其 tests）逐 byte 等于 final sourcee3b；新三项 integration fixture
独立新增。旧 Loader、provenance、模型、AST adapter、parser extraction、UID/qname/search、
DB/MCP/retrieval 输出、scorer/gold/weights/budgets 及 Cargo.lock 均不变。

## 当前 source registry v2

`scripts/current-source-registry-v2.json` 显式登记 `python-inventory-20261004-v2`。
它独立重建 v1 approved50a493 的全部历史 source/机械 lint/R1 patch union，然后叠加精确
capture delta（含原/最终 SHA256）和固定 product780 的新 fixture。完整 764 个 crate/Cargo/
lock inputs 的路径和 SHA256 均登记；最终独立比对固定 product、tracked/disk inventory 与
每个 byte。45 个 contract/作者/full/delta review 记录同样固定 source/hash 并核对现有文件。

registry SHA256：`6bce6b8c8b3a7925bf03dd1559113012d4b9fc650d4d667106ae8c4aab731172`。
guard 的 commit/version/registry hash 都是显式常量，无 HEAD 或 latest admission。
v1 registry、approved50a493 manifest、原 verifier 与其负控保持原样。

CI 明确选择：

```sh
python3 scripts/verify_current_source_v2.py --source-version python-inventory-20261004-v2
```

30 项 source controls（原 v1 16 + 新 v2 14）使用临时副本，实际拒绝 unknown tracked/disk
addition、各 capture/fixture/Cargo/lock removal/mutation、旧 source mutation、symlink、
旧/未知 registry、HEAD fixture pin、错误 capture/review hash、完整 manifest 的添加/遗漏。
不改产品文件做负控，也不将新 checkout 的 digest 自动升级为 approved source。

## 新实际 filesystem 闭环及 replay

`crates/cc-index/tests/python_inventory_resource_integration.rs` 的三个自编 fixtures 创建
真实临时项目，直接调用实际 capture API，随后对同一只读 bytes 调用真实 AST 和 borrowed
model，逐项比较返回 outcome；无任意 inventory 断言替代 filesystem 捕获。

- 新 constellation package、BOM/CRLF/trivia、nested class/method 与 conditional duplicate
  用独立 literal name byte spans、config digest/三项原始 evidence、module/owner/marker
  binding 和 occurrence fingerprint 核验；完整 `.git/HEAD`、opaque cache、ignore marker 和
  空目录均在 scope 内，Python 不执行。
- `.git/HEAD` 改动使新 binding 改变，旧 capture bytes/fingerprint 保持不可变；实际 marker
  removal 和 module/package collision 分别得到 typed NamespaceAncestry/Collision outcome。
- 累计 JSON output exact/one-over、AST declaration refusal、模型 identifier refusal、有效
  package-dir 配 partial find 的 Configuration refusal 均 fail closed；旧成功 capture 不变。

`replay.py` 在当前树逐 byte 复制 subject 前缀，追加原13 independent + 新2 delta fixtures，
只运行其 review_ tests，结束清理临时 target。本目录两 fixture 与原 review 全文件字节一致，
原历史 runner/report/log 不改。当前15 passed / 0 failed / 0 ignored；3项非 review module
tests 正常 scope-filtered。当前输出独立写 `tests.log`/`validation.json`，不覆盖历史红色日志。

## 验证与证据层级

官方 Rust1.95.0、原 lock、`--locked`；严格 workspace all-target Clippy/build、fmt、source/
module architecture、v2 current union 与历史 packing/e3 guard、计划 generator 均见
`checks.json` 和逐命令 logs。Rust focused 合计112项通过/0失败/0ignored：index20、drift1、
legacy provenance12、model43、parser21、独审 replay15。filtered 项未记为执行或通过。

all-target build 已通过后，初次 scoped test compilation 遇 disk full，未执行 tests；
`initial-checks.json`/`initial-index-compile.log` 保留原 failure。只清理 workspace 内可重建
incremental compiler cache，源码、产品 binary、历史证据、权限/OS 配置不变；原 argv 重跑
通过。sandbox 当时因磁盘满无法启动，自动审批允许这个限定 cache cleanup，无权限拒绝。

PR138 精确 base2d 的 historical CI [37174586283](https://github.com/jyqj/codecortex/actions/runs/37174586283)
由父核验 head，root 回读 check/msrv/security 全部 success 和每个 check step；原结果在
`historical-base-ci.json`，只属于 base，不证明新 capture integration。当前新 PR/CI 的实际
head/run/state 单独记交付回执。保留 da896b source version guard，以及 2d297 的 P0 anchor
`277f2490fad3fa30f2812b5547bad033867c9ea5` materializer：14 个 harmless 历史负控和六个
unchanged locks 仅 validate，没有检索评测、relock 或新 quality certification。

metadata 仅改 `tasks.json` 的 P7-014 implementation_notes，再运行 generator `--write` 和
无参数 check；192 tasks / 150 done / 41 todo / 1 in_progress 不变，没有手写生成 TODO 或
parent status closure。固定 product、精确 provenance、当前 docs/交付 SHA 分开记录。

## 保留的边界

Linux/statx mount-ID、trusted root canonical alias、native ASCII portable/no-follow、nlink/
dev/ino/casefold alias 与完整 `.git`/cache/no-exclusions/EntireProject 政策原样。正常 Git churn
使 binding 失效；大仓 scope 超预算直接 refusal，不能缩 scope 或改 scanner exclusion 重试。
没有实际 bind-mount fixture 或 OS 设置变更，也未认证跨平台/恶意 FUSE/remote metadata。
前后完整重检不构成跨文件原子 snapshot、持续锁、hostile mutation+restore 防御、hard RSS
或 hard deadline；parser/TOML 临时分配和 allocator capacity 不升级为硬内存上限。

仍未实现 cache invalidation/版本化 ingestion 重新推导/persistence/publication guard 或
optional DB/MCP/retrieval identity 协议。历史 packing/e3/P0 corpus 和旧100k不认证本 source。
formal DEV/100k、excluded old runtime/post_index/broad/private42/GCWALfaults 均 not_run。
public quality/Gin/P7/V19 保持 OPEN；无 merge/deploy。
