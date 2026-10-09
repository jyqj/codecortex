# 固定候选组合的历史校验记录

本目录按原字节保留296与PR165固定f88组合的必要文件投影验证。预期组合树为`aa6977a0c4d609ac4409889446a28d0bbfa8c7bf`；私有HEAD与index实际保持296。它不是完整组合checkout、实际合并main或CI通过证明。

原命令`python -B scripts/verify_historical_integrations_v2.py`在投影内实际执行一次，exit0；原有无参plan检查包含四个派生视图。323个当前文件、2492个固定Git读取和17个provenance commit的输入绑定在执行前后保持一致。原脚本与阈值未改。

新增825个档案路径尚未物化，696个唯一档案blob、88,152,246字节不在本地。官方二进制接口拒绝gzip读取的失败原记录也保留。新增路径与完整正向读取集合无交集；直接Git tree检查证明FORBIDDEN旧名在所有mode/type下不存在，包括空tree。

原校验输出仍为`full_P7=open`、`V19=open`、`quality_and_100k=not_inherited`、`fault_runtime_tests=not_run`，并明确不授予证据内容验收。正式任务账本仍163 done / 29 remaining。

从[最终独审追加索引](final-independent-acceptance-index.json)与[原字节保全清单](preservation-manifest.json)开始阅读。旧handoff中的pending是当时历史快照，完整保留；最终独审由新索引另附。第一次外层wrapper在原CLI之前因空refs返回码误报，guard调用次数为0；失败记录、旧wrapper和仅wrapper修复diff均保留。

`preparation/`保存官方新增子树/历史commit元数据、输入闭包、所需对象可读性、前后快照、原stdout/stderr、固定tree差异与外部记录脚本。`official-reference/`保存原官方主分支与PR165树/commit记录。`independent-review/`保存替代方案、读取集合与最终执行独审。

本目录不包含.git、整个checkout或原生二进制库。所有复制文件在保全清单中逐路径记录原位置、字节数与SHA256；临时签名URL或凭据不入库。仓库HEAD、refs、index保持不变，只有此未跟踪目录新增。

PR165实际合并后仍须fresh固定main并比较实际组合树及所需输入。真正完整checkout的普通GitHub CI、合并必要门和十个原任务的验收依赖不由本项替代。

## Large original record delivery

See [LARGE-RECORDS.md](LARGE-RECORDS.md) for the lossless compressed delivery of the two largest preparation JSON records. The original preservation manifest remains unchanged; the exact members and their independent review are indexed alongside the archive.
