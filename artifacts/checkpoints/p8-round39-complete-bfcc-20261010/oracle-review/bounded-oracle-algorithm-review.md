固定 G9cc 的双 spool oracle 有一个可独立评估的窄优化点，但没有证据把它认定为当前耗时主因。没有发现值得重新启用旧 witness、PR188 SQL 排除或 PR191 chunk-statement 方案的依据。当前代码、研究与全部验收规则保持不动。

| 项目 | 固定源码与结论 |
|---|---|
| 相同行是否重复 JSON 解析 | `streaming.rs:235–244` 已先比较字节；相同立即返回，不解析任何一侧。只有字节不同才各转 Value，以保留 signed-zero 等原等价语义。待核假设不成立。 |
| 是否重复外部排序 | `streaming.rs:295–319,346–376` 的单个 WITHOUT ROWID 主 B-tree 同时存储/排序，两个游标各读取至 EOF；已有 control 要求无 TEMP B-TREE。未发现第二次外部排序。 |
| 可评估的重复物化 | `streaming.rs:228–232,358–374` 每行从 scratch 取 owned String，随后仅借用 str。未来可保持完整双 spool 和读取顺序，在当前 A/B 两行生命周期内直接借用文本比较/hash，错误/示例/EOF 行为必须逐项保留。 |
| 更小的重复清理 | `oracle.rs:198–247` 纯 streaming 路径的 Map 一直是 Null，仍每行清零；但 `421–451` 明确保留 owned-project 后失败恢复，不能直接删通用 cleanup。仅显式已知 clean 状态可进一步考虑。 |
| 不能随意合并的遍历 | 原输入大小检查先于全部投影。将两个 get_ref 遍历揉成一次可能把“后列超限”与“前列 UTF8 错误”的先后颠倒。完整 integrity/FK、count、full-control/parity 也不是可删的重复门。 |

对已接受的 cold/no-op 两个完整记录，每侧各 6,653,993 行、双方 canonical 共 10,302,684,584 字节。由源码可派生每个完整 parity 的 13,307,986 个 owned 行值物化；这不是实测分配次数、复制带宽或节省时间。未来借游标文本必须在推进/重置前消费完；尤其保持 A step 与其文本转换在 B step 前、原转换错误种类/消息、重复行顺序、双 digest、不同字节的 Value fallback、完整 EOF 和公开字段。

两次 parity 用时分别 921.742980 和 1623.147962 秒，15 表所有行数/digest/结果及已报告排序配置相同。没有序列化、spool、排序读取、hash、完整性检查、I/O 或 cache hit 分项；相邻 build RSS 不能算作 parity RSS。因此既不能把 701.404982 秒差归给某个函数，也不能预测上述候选能让 5 小时内完成。

原 A+empty-B 在 64KiB 下成功、同 A+equal-B 实际 SQLITE_FULL 控制必须保留；完整 B 写入和双 cursor 不得再以相等见证省掉。PR188 原 sparse VM +22.57% 及后续负例、PR191 80 对中的全部 17 个慢样本与 single-plain retained-first 10/10 慢保持原结论。当前 borrowed serializer 已经是被采纳的 PR190 代码，不能当作新优化再次记收益。

此次仅只读源码与已接受报告，没有运行 Rust、ELF、reader、validator、CRC、probe 或任何测量；没有修改源码、计划、预算、研究或状态。建议仅保留上述候选供未来明确授权时评估，不增加 P8-006 门槛，不创建新实验。
