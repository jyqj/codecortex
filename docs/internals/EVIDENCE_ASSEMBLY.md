# 上下文证据集合与完整 JSON 预算

本页描述 P5-C 当前代码，不凭文件存在推定验收。状态以 tasks.json 和对应冻结证据为准。默认 code_index_context 已接入最终 EvidenceHydrator 与结构化预算；旧 SourceVerifier 作为复用的磁盘验证组件保留。G5/M2 和完整检索质量不是本页的完成声明。

**集成边界：**结构化预算适用于完整 code_index_context 结果对象；其他符号数组和非上下文工具保持原协议。早期预算版本把低排名引用放在主要正文之前，造成五道源码题退化；失败与当时回退工件保留在 p5c-20260929-evidence-assembly。当前版本修正优先级并恢复默认/真实 stdio 集成测试。当前验证证据写入 p5c-20260929-completion，不覆盖旧失败。

## 持久化读代际

ReadGeneration 包括持久化 index_incarnation、index_epoch、evidence_epoch，以及尚未启用时为 None 的 semantic_epoch。身份存入已有 metadata，不增加业务表或强迫 schema21 重建。可写 schema-open 执行 INSERT OR IGNORE 补齐旧库身份；正常重开保持身份。已存在但损坏的身份或 epoch 报错，不被改成零或自动换身份。

完成的 staging 数据库在发布前更新 incarnation，并沿用原来的递增 epoch 规则。两个库即使 epoch 相同，也不能共享普通/图检索结果缓存。读前后比较完整 ReadGeneration，沿用最多三次重试与已有取消控制；语义响应也必须匹配同一读代际。语义 epoch 的 None 不是伪造的就绪零值。

结果缓存显式包含 incarnation、相关 epoch、query/config/policy/spec；上下文的宽候选窗口与传统 top-k 图检索另有布尔缓存域。旧正文 LRU 仍只是解码提示：失效时清除，复用前由现有 DB hydration 与当前 source proof 比较；它不是来源权威。任意外部进程不走项目发布协议的文件替换、文件系统原子快照以及未来向量发布不在此声明内。

## 排名与选择分开

search_context_candidates 返回原有有界 rerank window，不扩大通道预算。EvidenceHydrator 对该窗口统一验证后，coverage selector 才选择输出集合，所有相关性分数及 score trace 保持不变。

Locate 不强制文件多样性；Fix/Test/Refactor 保留至少一半的高排名锚点，再从两倍请求排名窗口内补测试与接口 facet。Trace 使用实现/接口职责。输出按原排名排序。没有找到的 facet 明确登记为 unmet，不用不相关远端片段硬凑。

重叠命名空间为文件路径加 source snapshot，区间使用原始 byte span。相同正文但不同文件或版本不合并；部分重叠但仍贡献新字节的片段保留。只有已经选中的证据才可以覆盖并抑制另一个候选，未入选片段不能成为去重依据。报告分别给出输入集合与选中集合的区间并集、冗余字节、覆盖省略和名额省略。此统计发生在最终输出预算之前，预算省略另计。

## 最终来源与装配读面

EvidenceHydrator 复用受限磁盘读取，并在选择与预算之前批量取得当前 chunk/document 行；另调用与普通 hydration 相同的完整 record 验证器，暖缓存不能掩盖损坏的 record_json，且不为此重新解压源码 blob。chunk_id、规范路径、存储语言、HardScope、完整 DocumentRef、原始 ChunkSource 必须一致；再检查磁盘原字节、snapshot identity、连续 span 和显示行号。每文件只构建一次验证身份和行索引。稳定代的坏身份、正文、坐标或范围是错误；磁盘变动、删除、读取预算不足是带原因的明确省略，不拿旧行号解释新文本。

图节点是关系描述，不作为原始源码片段。它们的存储语言、硬范围和文件新鲜度也必须通过检查；带未经验证 backing/span 声明的图描述被拒绝。原 symbol_extract 只提取查询名称，不读取正文，因此最终校验接在实际 engine/query_handle/handler 组合根，不新增第二套符号提取器。

整次同步装配复用 with_stable_generation，最多三次外层尝试；内部检索保留自己的三次有限重试，最坏至多九次内部尝试，共用最初 deadline。装配、预算和最终返回均在 generation 核对之内。异步 handler 附加新鲜度后再次计数并核对接受代际；改变则返回可重试错误。最终上下文不写入普通完整结果缓存。每文件磁盘观察仍不是文件系统原子快照。

## 结构化预算（默认 code_index_context 路径）

BudgetPacker 的单位是紧凑序列化的 code_index_context 结果对象：包括机器命中、节点、跨度、JSON 转义、metadata、最终附加 freshness 和预算收据。上限为 min(repo output byte limit, token_budget * 4)。token_estimate 是整个对象字节数除四向上取整，不是模型 tokenizer 的精确计数；不声称 JSON-RPC 传输帧字节也属于该对象。

预算收据本身也计数，used_bytes 与实际重新序列化长度一致。保留 JSON 浮点往返精度，避免 Value/typed envelope 转换改变分数或字节收据。未超预算时保留完整结果。

超预算时按次序压缩重复展示、重复节点说明和可省略的诊断正文。完整源码只存 machine_pack.hits；节点可引用该条命中，而不是重复正文。通道的完整 candidates 可转换为独立 lane_receipts 字段，保留真实执行状态、覆盖范围和 candidate_count，明确 candidate_details_omitted。省略候选详情不伪装成返回了零候选的 complete 通道；该投影有独立严格类型。

仍超预算时，先移除已在完整机器命中中保存的重复节点投影，再从低排名正文开始降级。正文集合确定后才使用剩余空间放引用，不能牺牲仍可容纳的正文去给引用保留配额。引用优先覆盖正文未包含的文件，其次选择命名实体，同组保持原排名。完整文档引用放不下时，可降为带原文件 digest 和召回诊断的 file_reference；它没有 chunk/行号或正文声明，读取时必须重新核验。高体积的图描述和诊断详情同样不能挤掉可容纳的最后主要正文；其省略明确记录。首项没有无界豁免：超大的单个首项可变成带版本/位置的 document_reference。引用保留原命中的符号名、类型、语言和文档/位置，作为可操作的提纲；文件召回理由至多保留四条、每条128字符，它们是检索诊断，不把邻近符号提示升级为该命中的精确身份；没有源码正文，不作为命中参与评分，也不从标题推断符号。需要用 files 的 region/expand 再读取并校验。此实现不生成新的 source slice 或改写原文证明。

packing.partial 表示真正的正文/证据省略，重复装配不会清除旧的省略标记。若连必要状态元数据都装不下，返回明确预算错误，而不是成功的空结果或截断 JSON 前缀。其他非 code_index_context 的旧工具输出仍使用其原有预算策略，不在此模块的整对象保证内。

## 评测与边界

public-v7 识别完整 lanes 或明确省略详情的 lane_receipts，拒绝二者同时出现、重复通道或损坏状态；packing spec 为 v2。通道错误/Partial 和 packing.partial 始终可见，空结果不会据此获得正确无答案分。原评分公式和 gold 不变；历史工件使用对应 runner 回放。强制对象上限可能让旧的不受限响应变为预算 Partial，这个新省略必须独立报告，不得将完整检索 gate 改写为通过。

p5c_selection 使用真实 parser/SQLite 命中及独立区间；p5c_budget 覆盖超大首项、引用优先级、图/metadata 膨胀、完整对象计数和真实 stdio。p5c_hydration 覆盖篡改、跨文件/语言/空范围、UTF-8/CRLF、删文件、换库、取消及最终附加 metadata 的代际校验。合成预算输入不是已验证来源；BudgetPacker 不替代 Hydrator。p5c_cost 的 32/256 文件 release 观察包含来源校验和序列化，不是 100k、峰值 RSS、开放负载吞吐或尾延迟认证。
