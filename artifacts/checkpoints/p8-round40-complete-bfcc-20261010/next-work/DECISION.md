# 原任务关键路径与可实施工作

当前仍为 **164/192 done、28 remaining，本次累计完成1项**。本记录只追加事实与开发建议，不执行任务状态变更。

正式 wide cohort `38013753078/a1` 的100k rep0在03:54:45以failure结束；完整artifact清单没有该测量片，原日志单次请求为404 BlobNotFound。原aggregate明确failed，缺146片；其中100k rep0缺失，另外145个注册槽因preflight失败而未启动。API同时保留测量step7为in_progress、上传step8为pending的返回，不能据此猜测native exit、停止阶段、OOM、磁盘或deadline原因。

此前接受的四片、41观测原样保留。原五rep0路线尚未完成；不能以四片、failed aggregate或不同run的诊断代替第五片。九项输入继续apply_now=false，旧pending原件不回写。

## 原依赖为何仍指向006

原tasks对象`adb44ee70a4c129bfb443d4ecf678fb934c8ea6b`明确：depends_on是硬集成依赖；未启用条件任务的todo/blocked/deferred不算done。

| 原任务范围 | 精确依赖或缺口 | 目前可做、不能做的事 |
|---|---|---|
| 006→007→008→009→010；007、010→011→012→013；012、013→016 | 006五片路线失败，后续组件已有证据但硬依赖未闭合 | 保留已接受组件；实施具体源改进，不以准备材料计done |
| 016→017→018→019 | 017–019自身证据齐，018原baseline真实0且3历史DRIFT已补证 | 无需虚构新测试或改历史表；仍待原依赖 |
| P8-014，tasks:12872 | 依赖013；需要明确授权/预算的真实LLM旁证 | 原条款不硬限商业付费模型；获明确授权的固定local模型审公开DEV原则可行。但真实judge/revision/出处/人审仍缺，现18离线fixture控制不替代它，也不能消除013依赖 |
| P7-018与P8-015，tasks:10962、12923 | 真provider授权、实际返回/用量；015另需live holdout、模型revision与013 | 当前排除live/heldout，不能用fake、声明或deferred算完成 |
| P8-020，tasks:13287；06-VALIDATION:58 | 所选发布范围blocker=0，原P8硬依赖与G8证据 | M4-local可不启用015条件，不可跳过006；19个评审工具控制不是发布通过 |
| P9的ANN/LSP/rerank三条根链，tasks:13364以后 | 根任务依赖020，后续是逐项收益决策 | deferred不算实现done；现在扩展这些分支不能解除006，也无原证据支持为计数引入新服务 |

全部28项中，27项为006本身或存在到006的硬依赖路径；唯一例外P7-018受当前未满足的live条件限制。没有识别到现在可独立结项的原TODO。

## 现在可落实的两项工程工作

1. **先实现sorted cursor文本借用。** 当前`oracle/streaming.rs:228–232`把scratch排序后的每行再次读成String，`:358–374`随即只借str比较、hash和生成至多3个例子。原已接受100k cold/no-op行数对应两侧13,307,986个行值、10,302,684,584字节canonical正文的再次物化。这是明确源码工作量，不是耗时占比或本次失败归因。可以单独提交普通开发候选：A游标step/文本转换先于B；两行在各自游标有效期内比较并hash；保存的例子仍拥有内容；不使用unsafe延长生命周期。保留完整A+B双spool、15表、重复、Value/signedzero判等、所有预算、错误顺序、报告字段。最小相关验证为原13项streaming集成控制、必要游标错误等价控制以及正常fmt/Clippy。没有新速度门或强制150实验；也不预称能通过5小时。

2. **修补未来full preflight的运行中证据保全。** 原wide workflow:110已经`if: always()`上传失败片，因此再次加always没有用。当前真实缺口是100k没有上传原片、日志也不可得。可把已经审查过的capture监督器适配到未来100k preflight，原driver argv/profile/5h/512MiB完全不变，progress名字独立，按offset保存raw/stderr并以实际上传ACK推进。保留最终whole原片与原validator，prefix永不计样本；host丢失仍只能保证已上传前缀。这项改进解决可诊断性，不是性能修复或006完成。

不建议立即重写filesystem continuation、提高scratch cache、删full control/parity或恢复旧witness。现有token/catalog缓存与64/8/1scratch batch已经实现，当前失败原因未知；这些范围较大且缺少足以支持最小正确补丁的当前证据。

## 后续真正验收仍需什么

此前已独立接受的原任务与release范围区分继续有效：描述性五rep0不冒称N30/全1500/G8通过。此次失败不能使五片条件变成四片；任何新source的替代任务证据应在执行前固定完整五规模、原九阶段/15表parity、1k五fanout和旧回归适用范围，保留原失败与独立研究身份。当前诊断只可提供自身已观察事实，不拼入本cohort。没有发现新的、能凭现有缺失结果立即关闭006的合法路径。

完整28项原字段、依赖路径、代码身份、旧证据来源及失败通知见同目录JSON。此次只读本地既有原文和源码；没有新网络请求、产品/validator/测试运行、ZIP/CRC、研究触发或任务/ref修改。
