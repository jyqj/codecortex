固定 PR110 5ffbadcf 与 PR114 807f471，各唯一一次预注册真实 release MCP stdio 100k，baseline→candidate。两实际 binary 先全部编译，fresh DB/cache/root，输入字节及配置相同，原 PR108 cold→HTTP gate→release、300s ready、200ms poll 和资源/整体界限全部保留。

两版均未在 300s 内 ready：cold 36.066914/29.679855s；实际近边界只读 semantic_manifest 25081/48809（开始 300.000158/300.000121s）；cleanup 尾段 25600/49152 单列。partial count 增加不是通过：本次不接受或集成。1335/1334 次 status 观察，错误均 0；EOF 清理 exit0/未强制，post-ready query/concurrency/normal-exit/reopen 均 not_run。两个失败及原始压缩 RPC/HTTP/resource/input logs 完整保留。

root sampled RSS max 1.942/1.978GB，完整 tree unknown；最后可读累计物理写入 5.035/8.190GB，完成量不同不可把差额全归因于索引。保留原 5k ready 回归和 IO +4.4%。HTTP/server/gap、CPU/proc IO、期限计数/尾段、DB rows/integrity/FK 与同环境/实际 binary hash 均记录。一个配对不作统计显著性或正确性通过结论，旧 PR108 不是本次对照。

证据仅 artifacts/checkpoints/fifo-100k-paired-decision-20261003/。身份/输入/配置/原协议 replay 和压缩后 checksum 核验通过。PR114 新测试 lint head 52a50730 的 382 个生产/构建输入文件与冻结 candidate 字节一致；没有追新 head 编译或重跑，不外推测试身份。正确性另独立 review。

无生产修改、真实 provider/heldout、GC/WAL kill/crash/fault、之前 RO 拒写重试、权限/credential 改动或额外实验。Draft only；不 merge/forcepush/deploy。
