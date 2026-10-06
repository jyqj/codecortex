# C++ B1 cv ref FTS source registry v6

显式版本 `cpp-b1-cvref-fts-20261006-v6` 的有界源码集成已完成并通过独立复核，范围仅为 `source_integrity_only`。
测试源码 tree 为 `7c40dfa7bc3f3748e4f02def368c069f24a2f41e`；交付在该 tree 上只新增本文。

## 固定来源与重建

先在隔离的 schema 26 历史 disk/index 中执行未修改的 v1–v5 重建，再核验以下固定公开链：

- v5 SOURCE `614da81becf3769ba7f7ccd4eaa07edf1ae879b9` → v5 交付 `9641add263f18dc840562e46cd388afaea6796a4`：4 个新增 guard/test/说明路径，crate/Cargo 字节、blob 和模式不变。
- B1 `13bd9dbbe3f96b390f6adf34a95bb398da89bf4d`：80 个交付路径，76 个 crate 差异，11 个新增输入。
- cv/ref + FTS `bae3bcc21b89d722d0e676c620f418947b24dc84`：14 个交付路径，11 个 crate 差异，8 个新增输入。

逐步完整输入数为 **767 → 778 → 786**。每个交付路径固定 before/after SHA256 和模式；原始 commit header 核验唯一 parent，shallow 边界不能作为无 parent 的根。缺失精确对象必须失败，不以 HEAD/latest 或其他 tree 代替。

v6 唯一声明例外是能力文件 `database_schema` **27→28** 的逐字替换，SHA256 为 `23314bc80894ba0be913d7c9d53a29cd30150216bf479da73b6430bed0dc3503`。project model 3、rebuild 策略及其他字节保持原样。registry SHA256 为 `86664d754041fbbf28bb292fb597adcdd5e9de78444c8a2654956fe6029e078b`。

## 验证与边界

显式 v6 guard 通过：786 个 crate/Cargo 输入、204 个当前和历史保留文件。磁盘和 Git index 同时核验完整清单、字节、mode/blob/stage，拒绝未知或缺失输入、符号链接、特殊文件、未合并 stage、registry 篡改、未知 selector 及 Python `-O` / `-OO`。

作者 30 项 Python 测试通过；独立复核重跑同一 30 项、历史原样 v1–v5 的 84 项，并通过 9 项额外检查及 392 个交付行字段负控。实际 shallow/no-origin 控制确认仍识别原始 parent，精确对象缺失时失败。这些分组不相加为产品质量测试数量。

```sh
python3 -B scripts/verify_current_source_v6.py --source-version cpp-b1-cvref-fts-20261006-v6
python3 -B -m unittest discover -s tests/source_integrity -p test_current_source_v6.py -v
```

本次只增加 v6 registry、verifier、Python 测试和本文，并修正上述单字段声明。旧 v1–v5、原失败/验收证据、Rust/Cargo/lock 和 CI 保持原样。tasks.json 与其派生 TODO 已被 v6 固定，192 项上级任务状态不变；有界源码集成的完成只记在本文。

CI 仍选择原 v3，本结果不代表 CI、Rust runtime、全 server/workspace、完整 C++/public/NL quality、P7/V19、formal eval/100k 或 release 验收。原修复的运行时结论与限制见 [B1 公开结论](../../reviews/cpp-qualified-owner-public-20261005/README.md) 和 [cv/ref 与 FTS 公开结论](../../reviews/cpp-cvref-identity-public-20261006/README.md)。未新增 private/production、diagnostics 或 DB fault 执行，也未运行已排除的 semantic-runtime paging 测试。
