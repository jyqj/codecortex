# P8-017/018/019：自身范围与最小剩余动作

固定主线 `a406a4968c46ab005225666e60e81acddaa77de8`。任务状态未改，`apply_now=false`，剩余 **28**。PR198仅三文档；本表不把其尚在执行的CI当终态。

| 原任务/claim | 已有精确证据 | 判定 | 下一最小动作 |
|---|---|---|---|
| P8-017 / 清临时旧branch/重复评分器/多份schema来源；运行路径只有一个事实与算法所有者 | `owner_review` | accepted_own_component | 无需再造删除或额外测试；保留已审ownership来源矩阵；PR198仅将矩阵落地。 |
| P8-017 / 删除有回归证据 | `owner_review`, `current_ci` | accepted_own_component | 无需重跑旧303+1、427/301或16命名控制。 |
| P8-017/P8-018 / 14工具/旧mode/参数schema-sanitize-dispatch-status/默认无网络无key | `current_ci`, `installer_and_declared_facts` | accepted_relevant_existing_controls | 复用下附精确原log行；无新live/provider或全矩阵要求。 |
| P8-017/P8-018/P8-019 / 新旧schema受控重建、cache不误读、default/semantic两包、SDK/MSRV | `platform_rollback`, `current_ci` | accepted_original_component_evidence | 引用已接受平台/回滚来源和本次current-schema/cache回归；等待各原task依赖按序关闭。 |
| P8-017 / depends_on P8-016 | `platform_rollback` | original_dependency_open | 等原P8-016依赖闭合；当前已接受自身范围不归零。 |
| P8-018 / 从schema/capabilities生成可核查事实；设计与实现分开 | `installer_and_declared_facts`, `current_ci` | accepted_own_component | 沿原p8_facts真实passed引用，不要求运行时导出不存在文件。 |
| P8-018 / 文档表数/schema/工具数不再漂移 | `PR198` | reviewed_fix_awaits_publication | root按已授权流程完成PR198普通文档整合；不增加代码/新数值。 |
| P8-018 / 安装/故障/默认离线说明与相关回归 | `installer_and_declared_facts`, `current_ci` | accepted_own_component | 无新发现安装说明实现缺口；复用原56+5与当前配置保留/无效不写入控制。 |
| P8-018 / CONTRIBUTING数字改动后的update-doc-baselines约定 | `current_ci` | documented_process_check_not_run | root明确处置这一贡献流程差异；不得以现CI或p8_facts冒称脚本通过。未来若需执行原脚本，先保持当前已授权测试范围。 |
| P8-018 / depends_on P8-017 | `owner_review` | original_dependency_open | 等017；文档修正整合与贡献约定处置后，自身无已识别生产缺口。 |
| P8-019 / 精确binary/checksum/manifest/raw/gates；历史run不被覆盖；latest只指向run | `archive`, `current_ci` | accepted_own_component | 无需新增归档实现或重复42控制；复用精确归档、无覆盖、latest条件和失败保持原件。 |
| P8-019 / 报告可重算；V04 raw到metrics/report及错误状态 | `archive`, `current_ci`, `PR198` | accepted_own_component_with_doc_clarification_pending_publication | 合入PR198副本/单run路径说明即可；没有原文要求再加一次组合E2E测试。 |
| P8-019 / SHA/dirty/binary/toolchain与旧测试/真实stdio；原fresh-target构建 | `platform_rollback`, `current_ci`, `archive` | accepted_applicable_original_evidence | 按原平台/构建/stdio收据分别引用，不把普通test profile说成cold release。 |
| P8-019 / depends_on P8-018 | `archive` | original_dependency_open | 等018；自身实现和相关既存控制没有新增已识别阻塞。 |

## 准确边界

- `current_ci` 是原 b8ab / run37994312848 / job114036304013 / checkout3f9d8f7…；其与main a406仅五个005进度docs差异由现成scope桥证明，整棵树并不相等。日志Git `dda53d175bbe34d0b285b462dd6ca8016dce1d56`。
- `owner_review` Git `0e5fd5de6599825486e60cea567572c8010fd23a`；原删除、lane收口、16命名控制/35重叠名称不相加成新测试数。
- `platform_rollback` 是原275e/run37890757129的八格及277f/schema24↔25实测，已在tasks accepted component证据中。永久commit `cd92ce4b931e770994770ba3dba6fd0a47a505ea`，路径见机器map；不是a406重新测试。
- `archive` 原42归档控制及native replay结果已存在；文档澄清不是新增必须完成的组合E2E准入门。

## update-doc-baselines 实际缺口

CONTRIBUTING确有数字改动后运行该脚本的约定。PR195/196/197三份已有完整原日志没有脚本命令或基线输出；当前CI也不调用它，因此明确 `not_run`。脚本固定执行全workspace/all-targets并比TEST_PLAN的测试/语料计数，不检查本次oracle表数。当前CI明列故障/GC/WAL等执行范围限制，不能为了这一文档单句修正自行突破。脚本还只打印DRIFT而不返回该变量作为退出码，所以今后实际exit0也不能单独当无漂移证据。

最小处置是由root在当前PR记录该贡献流程差异及确切范围，保留真实事实核查和现有授权CI；若未来要刷新TEST_PLAN全量测试数字，再明确对应执行范围，不能把mock或其他命令stdout塞给原脚本。该记录不新增task原定义没有的100k/发行门，也不自动撤销已有V18/V21组件证据。

此处没有运行任何产品、测试、原validator、guard、replay、外部provider或研究；没有修改tasks/ref。
