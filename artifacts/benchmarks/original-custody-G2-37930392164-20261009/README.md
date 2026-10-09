# G2 原始工件永久保全候选
本目录逐字节保留 GitHub Actions run 37930392164、attempt 1、固定 G2 4d18dcdb34b5d7a277566b57801cc882d8b1eb61 的完整五个原 ZIP；四份 mixed 原件与 controls 原件分别保持原 SHA。所有 ELF、数据库、raw、Cargo/observer 输入与封存成员都在原 ZIP 字节内，没有筛除或重新压缩。
每个 manifest 按原顺序引用不超过2 MiB的块。相同块可以复用同一 Git 对象，不能把不同原件合并成一个测量结果。四个 mixed ZIP 各自含完整 build；不另造重复 build ZIP。
在完整检出的本目录中，恢复单个原 ZIP：
```sh
python3 restore_original_zip.py manifests/11623110311.json --output /absolute/new/11623110311.zip
```
原冻结恢复器逐块和整件检查字节数、SHA256及Git blob身份，拒绝覆盖已有输出。输出目录须已存在。该操作只重建原 ZIP，不解压、不执行原件、不开新测量。
controls 原ZIP因原本地副本丢失，于14:38 UTC由root恢复性再次取得；旧缺件快照原样保留。R31/R32/R33已公开的小审查记录只按catalog固定commit引用，R34 C16验收记录另行保存。此目录不声称tasks完成、全规模通过或发布批准。尚未创建保全分支时，只是候选。
