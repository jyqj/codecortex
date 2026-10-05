# Contextual-member source registry v4

显式版本为 `contextual-member-ranking-20261005-v4`，仅核验源码完整性。
固定公开源码提交为 `ef56a468ae4ee85be172db673c39e32c4f3c06a2`，
固定 tree 为 `c699d92d1845c28b25e58e5a731c7eb7876a37ff`，
唯一 parent 为 `886f90a542a6174a037c79eebbb4f74848fb1f53`。
本次没有修改 Rust 产品源码、Cargo 文件或 lock。

## 可复核范围

原 v1/v2/v3 verifier 和 registry 保持原样。v4 先执行既有独立重建链，
再叠加固定公开提交的五个 crate 文件差异；不是从当前 HEAD 或 latest 生成授权。
完整 764 个 crate/Cargo/lock 输入必须同时匹配固定输入清单、Git blob、
index 和磁盘字节。文件模式、未解决的 index 合并以及文件/目录符号链接也会被拒绝。
Python `-O` / `-OO` 会停在入口，避免禁用历史 guard 的 assert。

v4 固定保留原 guard、原测试、CI、历史验证入口和原公开测试结论。
此前 contextual-member 候选的否定负控失败仍是失败；本版本不将其改写为通过。
本次不发布完整原始验证材料，也不新增对未公开提交或原始证据的抓取依赖。
公开排名结论见 [原测试结论](../../../artifacts/checkpoints/contextual-member-ranking-public-20261004/README.md)。

## 定向验证

- v4 verifier：764 个完整输入与 14 个固定保留文件通过。
- 新增 18 项 Python 测试通过，涵盖未知文件、缺失文件、源码/Cargo 变更、
  index blob/模式变化、未解决合并、文件和目录符号链接、固定身份及清单伪造、
  历史 guard/公开失败结论改写、未知 FIFO/socket、旧版本/latest 选择和 Python 优化模式。
  FIFO 和符号链接使用真实临时文件系统；socket 仅模拟文件类型，未执行真实 socket 测试。
- 原 v3 对新排名源码仍按原规则拒绝。这是预期的版本边界，不把旧版本改为通过。
- 补充检查发现初始 v4 候选会忽略 crate 目录下未跟踪的 FIFO；此前 16 项通过
  未覆盖该边界。修正后枚举所有非目录项并在读取前拒绝未知 FIFO/socket，
  保留原失败结论，未修改 v1/v2/v3。

复核命令：

```sh
PYTHONDONTWRITEBYTECODE=1 python3 scripts/verify_current_source_v4.py \
  --source-version contextual-member-ranking-20261005-v4
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover \
  -s tests/source_integrity -p test_current_source_v4.py -v
```

CI 文件及其 v3 selector 原样保留，v4 此时仅通过上述显式命令使用。
本次没有执行 Rust runtime 测试、全仓测试、正式 DEV/100k 或新的公开质量评测。
192 项 TODO 状态不变；general NL/public quality、P7/V19 和 formal DEV/100k 仍未完成。
source integrity 通过不继承历史质量、规模或 release 认证。
