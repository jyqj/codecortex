# Requests fixed paired public DEV, 2026-10-04

真实阶段：**executed_and_replayed_quality_failed**。这是新、独立 worker 的
Requests/Python 语义质量配对证据；未改产品、gold、scorer、normalizer、provider、
预算或默认 knobs。未 merge/deploy，未宣称 CI green 或当前产品 certification。
中央 tasks 状态保持原样。低分完整交付，不改 Partial 为 Complete。

## 固定身份与准入

baseline `88f2cf099c8b81f3acef485fd5ac9b01c63ce790`；candidate 组合
`37dd042eaa1209a86e0cafdcd92ae77e036e76f5`，8 crate src/Cargo/lock 与
产品 `90858afae647a513537bf118932a7ba5020ee98b` 的 diff 为空。
登记 `974ccf5f90a1062512c9a645078e4948ebad5a8e`、候选
`97c478cdb05d2bb85f852ff42af5452b00ba6e3a`、独审
`47706907868007f71246f74674164263019bba79`；显式加载
`public-dev-pygo-declaration-kind-v2`。登记 loader 原样保留，12 tests 本 worker
实际通过。`intake.json` 绑定登记原字节与历史 raw 完整 965 文件核验。

原准入 PR91 `5385f5a7a2a875c6d5cbd049bdde039bf71bbf32` 的 admission hash
`b4388ca60612f6734719b348bf30e05dd0752b7b754f47cc078b590e35c1e425`
由原 loader 验证。Requests author `e49f9ada5826b4206d2128f3bfb8d31603ff42fa`。
native SHA256 `7847fbe8326977121e964ee3729d47ce4855e7ce51b32c3918d8bd5ef77c3730`；
compat `0a0203c010b8a448bc2d447ff8c27ff9d4ab99b405b23d14094400d9c9c31edc`。
完整 source 字节从实际隔离输入目录回读，再交原 loader 校验；Gin 仅满足原 loader
完整准入，未执行 Gin 检索。两侧输入锁、suite 字节和实际执行 query/repetition 顺序
完全一致。原 source digest 与历史 inventory 相同，parser errors=[]。

两个 checkout 分别从固定提交实际 build `cargo +1.95.0 build --locked -p cc-server
-p cc-eval`，使用官方 1.95.0、原 lock、普通官方 fetch/proxy，不混编。
各自 target/binaries、materialized corpus、index 和检索 cache 隔离；仅共享官方
Cargo 依赖下载 cache。`binding-{base,candidate}.json` 保存每个产品源文件和
binary SHA256，完整构建日志保存成功与早期 package-name 失败诊断。

原 PYGO raw 来源 `fff0931e5b470aa3b9111d6ce55444f9b40f98cf`。
仅用其原 Requests schedule：native 91、compat 83，repetitions=3、seed=20261003、
top_k=10、warmup=0、timeout=30000ms、mcp-stdio/quality，原 engine config 不变。
native suite 仅按登记更新 BLAKE3 queries_digest；compat 原输入不变。
原 cc-eval 在 query loop 前完整实际 prepare/index/readiness 成功。

## 同输入结果

| 边 | 模式 | scheduled/executed | missing/error | Partial | Top1 | nDCG10 | 门禁 | replay |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- |
| baseline | v2 native | 273/273 | 0/0 | 273 | 0 | 0 | gate_failed, exit 1 | byte-identical, exit 1 |
| candidate | v2 native | 273/273 | 0/0 | 273 | 0 | 0 | gate_failed, exit 1 | byte-identical, exit 1 |
| baseline | 原 compat | 249/249 | 0/0 | 249 | 0.7710843373493976 | 0.7769516267731055 | gate_failed, exit 1 | byte-identical, exit 1 |
| candidate | 原 compat | 249/249 | 0/0 | 249 | 0.7710843373493976 | 0.7769516267731055 | gate_failed, exit 1 | byte-identical, exit 1 |

两种模式的 candidate-minus-baseline Top1/nDCG delta 均为 0。
全部四个 run 的 packing.partial=true；invalid/unverified normalized hits 均为 0。
native 含 8 no-answer query，24 次 no-answer_correct=0；answer 指标 MRR/recall=0，
span precision=0.37956728965012027、span recall=0.48510768966827983，两侧相同。
compat 未提供的 native 指标保持 null，未补值。原 scorer 的 family CI/类别均值
保留在 aggregate/runs 中；没有另造独立样本或宣称配对优势。

诊断观察：native baseline 603 retained normalized hits、qname nonnull=0；candidate
609 hits、qname nonnull=597。compat 分别 536/0 与 543/531。仅是原输出身份与
可用性观察，不能据此改写质量结论。每次原始 retrieval cost 和阶段结构完整保留，
aggregate 提供数值计数与观察覆盖；未把耗时或成本差异当性能因果。

历史 301 native / 256 compat / 280 correlated groups 的四 repo DEV 范围不变，
clean holdout=0、new independent samples=0。旧 1671 次 all Partial / quality FAIL
未覆盖或删除。历史 v1 classification 有依据；未声称错标，未拿旧 v1 分数作本次
paired baseline，也未拿 compat/native 的评分口径差异作为提升。

## 安全归档与复核

- `paired-requests-raw.tar.gz`：四个完整原始 run 的 raw、normalized、queries、
  scores、costs、manifest、prepare/readiness、gate/report、资源与延迟原输出；
  Requests 的 8 个原法律文件及 source/binary/input receipts。
- `raw-artifact-manifest.json`：每文件 bytes/SHA256 与 archive SHA256，封存后逐文件
  回读核验。未导出完整原仓 source tree、binary、DB、秘密或 future holdout。
- `case-stage-cost-score.jsonl`：1044 次 scheduled/executed 的 ID、repetition、真实
  status、raw identity、原 metric 与原 retrieval cost。无 duplicate/best-of/missing 填值。
- `runs.json`：实际命令、终端 exit、全部输出哈希、各阶段记录；`aggregate.json`
  汇总全部原指标、null 分母、cost 观察覆盖与事实性身份诊断。
- `runner.py` / `seal.py`：本 prefix 自有一次性编排与封存程序；原 product/runner
  adapter 不改。原 loader/test 可原样重跑；retrieval 已完成，不重复选择最好结果。

## 后续 TODO（不改变中央任务状态）

- 由 root 评估全部 packing Partial 与 native 质量 0 的诊断证据；若另开产品工作，
  必须单独授权与固定 protocol，不能用本 gold 推动本包产品实现。
- 当前组合 CI 先前 Forbidden 仍未确认；本包没有新增 CI 通过声明。
- full V19 / clean holdout / 100k certification 均不由本 Requests DEV 配对关闭。
