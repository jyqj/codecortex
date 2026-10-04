# Packing 终态集中整合（2026-10-03 / 04）

产品固定 source 为 `90858afae647a513537bf118932a7ba5020ee98b`，packing docs 起点为
`156e3ac13ddc6aa2b0208c8805c8c79bd2a0a38d`。独立集成分支
`integration/packing-evidence-20261003` 从此起点建立，保留 cpp49e033/sourceaa271e5、
PR136 Python、PR132 qname/schema25 与 PR131base88f2 的完整产品历史。
相对 packing 起点八个 crate/src、各 Cargo.toml、Cargo.lock 逐字未变。

## 来源及历史证据

- PR135 `591c246c6f57b230138dd3c4c6b0753d1d75c775`：schema guard 与真实24 fixture，
  MODULE_CAPABILITIES 25/model3，p1d 实际9 statements/10 rows 与缺身份读取合同。
- PR134 最终独审 `29a29ed0fccba074e299c2e8d7e04d1427ab184f`：完整原 review prefix，
  保留 P1/P2 bounded fixed PASS 与此前两轮 REJECT、native gold/output、source manifests。
  当前严格 positiveproof 测试证明 Class/Method/原始字节/SQL存活，历史 absentqname
  红断言仅在原 docs/history 保留，不导入过时 active target，不删失败证据。
- DB 独审 `2d5aec12e8dbd54b5a223d93aae048c8f18b2cbb`：完整 prefix、真实v24数据库、
  自编13tests。原报告仍原样记录 schema3fail 与 SQL成本范围/AST authority 限制。
- Packing 独审 `c0876c82a5d04d4068f0e3e3ef0a205c1a2207ce`：原 prefix、两个targets
  和压缩完整收据原样导入。root 完整阅读并接受 bounded scoped-search packing 切面。
  39labels、真实legacy/generation producers、Partial/noresurrection 和指定宽度15962/余38
  有效；不推及全部数字/unscopedcontext/公开DEV/100k。
- PYGO opt-in 登记 `974ccf5f90a1062512c9a645078e4948ebad5a8e`、候选
  `97c478cdb05d2bb85f852ff42af5452b00ba6e3a`、独审
  `47706907868007f71246f74674164263019bba79`：约1.5 MB原目录精确导入；loader
  验证要求完整固定artifact文件，故保留候选gold作为证据，不替换旧gold或运行评分。
  Requests历史broad function有依据，root新taxonomy只适用于PYGO显式opt-in；JS、
  historical default/scorer、历史样本规模不变，无新独立样本。

底层 merge commits 只发生在新集成branch。tasks.json 仅合并实施备注，保留双方历史段落，
追加 SQL scope/authority TODO 与后来独审接受，再由 code_index_plan.py --write 生成。
状态仍192任务=150done/41todo/1in_progress，整体V19/P7/quality开放。

## 实跑与保留缺口

官方Rust1.95，原Cargo.lock，--locked -j2，独立target `/workspace/target-packing-integration`。
原运行全命令/退出码/日志为 checks.json/checks/；后续修正与新独审target为
final-checks.json/final-checks/。最终不同测试覆盖如下（DB index7在lint修正后重跑）：

| 精确范围 | passed | failed | ignored |
| --- | ---: | ---: | ---: |
| schema guard | 4 | 0 | 0 |
| p1d_cost | 5 | 0 | 1 |
| 原 p5e_priority_pressure | 2 | 0 | 0 |
| 作者 packing_scope_v1 | 3 | 0 | 0 |
| DB独审 index7/server6 | 13 | 0 | 0 |
| parser独审最终3+leaf1+owner1+publicproof1 | 6 | 0 | 0 |
| packing独审 boundary3/stage1 | 4 | 0 | 0 |
| 不重复覆盖 | 37 | 0 | 1 |

实际Rust执行44 passed（含DB index7再次执行），不是44个独立测试；ignored为显式stdio
opt-in，未当作通过。stage measurement helper只读旧receipt的历史独审结果保留，本次不跑。
Python schema/source guard、mutation9、plan，以及PYGO loader12均通过；source guard内嵌
9个mutation运行与单独mutation命令有重叠，不当作独立样本。

首次完整strict workspace/all-targets clippy在导入DB测试报四处
cloned_ref_to_slice_refs；checks/clippy.log 原样保存。只将四处单元素clone切片改为
std::slice::from_ref，不改断言/产品。原测试完整字节保存在original-tests/，manifest绑定
原hash和当前hash；验证器按这四处精确转换校验，不允许任意test漂移。最终clippy/fmt结果
见final-checks，CI workflow和-D warnings门槛不改。未stage时的diff检查不含untracked
PYGO目录；完整156e3ac..8189abd范围检查另见full-range-whitespace.log，exit2，只有固定
原许可证Werkzeug LICENSE的CRLF（29行）提示whitespace。该文件与97c478c原字节/hash
完全一致，保留许可原证据，不把完整范围宣称whitespace全通过。

PYGO第一次loader因为缺原input Git对象在setUpClass失败、0tests，保留
initial-pygo-missing-object.json；从normal origin取声明的精确pins后12tests通过，没有
产品/loader修改、替代credentials或新评分。

不运行脚本update-doc-baselines.sh，因为它隐含workspace broad suite；本包只记录精确
范围，不改历史全仓基线数字。不运行被排除旧worker或包含它的broadworkspace/server、
GC/WAL/kill/staging/EROFS/private42export、publicDEV/100k；后两者须固定组合源码后新session。
作者591/0/1、历史packing search298/bounded41/2等仅继承，不冒充本次复跑。

## 身份验证与CI

source-manifest.json完整列出731 crate/Cargo inputs（387 product）、246原审查/导入文件，
source commit、git blob、SHA256、bytes、类别及状态。scripts/verify_packing_integration.py
验证每个输入真实原Git字节、产品完整inventory和证据fixity；允许的唯一DB测试转换也逐字验证。
scripts/verify_fixed_e3_integration.py继续校验原e3 manifest在固定历史SHA的708输入及58原
import身份，并增加当前packing清单校验；不把历史e3 manifest覆盖成新source。浅checkout
只会从normal origin fetch缺失的明确provenance SHAs。所有workflow字节等于156e3ac。

SQL成本只含originating hydration命名语句，final manifest/load_on(None) cold/warm读取及
warm当前请求总成本不计；QUERY_EXECUTION.md明确说明，额外validation-work receipt和
完整AST authority/taxonomy待办开放。组合验证不替代独审、不关闭质量或发行gate。

普通push和一次draft publication的实际结果、最终docs head及exact-head CI另记录PUBLICATION.md；
不merge/no forcepush/deploy，Forbidden即停止，不重试旧PR或绕路。
