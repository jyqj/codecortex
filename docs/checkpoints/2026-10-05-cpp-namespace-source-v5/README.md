# C++ namespace source registry v5

显式版本为 `cpp-namespace-identity-20261005-v5`，仅核验源码完整性。
固定公开提交为 `614da81becf3769ba7f7ccd4eaa07edf1ae879b9`，
固定 tree 为 `9ca8682bebbabb9cef25affd2ee468d982bdc638`，
唯一 parent 为 `1592c7c02ebe88c36ab396b9f705d0541b425383`，
parent tree 为 `905457aee94d7d89e762d02a1deefc8f245d476d`。
本次只新增 v5 verifier、registry、Python 测试和本文；Rust、Cargo、CI、
旧 v1–v4、既有证据、公开结论、能力声明和 TODO 均未修改。

## 固定重建

先执行原 v1 → v2 → v3 → v4 重建链，再将完整 764 个旧输入与固定 parent
逐一比较字节、Git blob 和模式。随后仅应用公开提交中的 11 个 crate 输入差异：
8 个修改、3 个新增。完整结果必须等于固定公开来源的全部 **767** 个输入。
这不是从当前 HEAD、latest、分支或候选提交刷新授权。

公开交付的全部 **15** 个路径均固定 before/after SHA256 和模式，新增文件的
before 必须为空。除 11 个 crate 输入外，另外四个路径为能力声明、公开测试结论
及两个 TODO 文件。能力声明固定为 database schema **26**、project model **3**。
这只是对已发布源码和声明的绑定，不重新执行或继承原运行时评测。

v5 额外核对 **22** 个保留路径，含旧 guard、registry、测试、CI、v4 说明、
两份公开结论、能力声明及 TODO。原重建链继续验证其既有历史证据。
保留路径的字节、文件类型、磁盘 executable 位和 index mode/blob/stage 都必须
匹配固定来源。原 v4 FIFO 失败及修正说明保持原样，不将原失败改写为通过。
没有新增对未公开 C++ 运行时证据或原始验证材料的抓取依赖。

## 当前源码检查

完整 crate/Cargo 清单必须同时匹配固定 Git tree、index 和磁盘字节，拒绝：

- 未授权或缺失输入，包括 ignored 风格的嵌套路径
- index-only blob 或模式变化、未解决合并及重复 stage
- 修改文件和未修改文件的 executable 位漂移
- 同字节文件符号链接、目录符号链接及特殊文件
- registry 序列化、身份、完整清单、模式和公开交付绑定伪造
- 省略 selector、旧版本、HEAD/latest 或未知版本，以及 Python `-O` / `-OO`

沿用修正后的 v4 非目录项枚举，先检查特殊文件清单，再读取输入。
FIFO 使用真实临时文件；socket 仅模拟文件类型，不创建或执行真实 socket。
所有检查均为 Python 源码完整性检查，不是 Rust 或服务器测试。

## 定向验证

显式 v5 命令通过：767 个输入、15 个公开交付路径及 22 个保留文件。
新增 26 项 Python 测试通过，包含真实 Git index 的模式、blob 和冲突 stage 负控；
能力声明检查通过（schema 26 / project model 3）。这些结果不包含 Rust 运行时执行。

```sh
PYTHONDONTWRITEBYTECODE=1 python3 scripts/verify_current_source_v5.py \
  --source-version cpp-namespace-identity-20261005-v5
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover \
  -s tests/source_integrity -p 'test_current_source*.py' -v
```

独立复核另行记录。原 v4 对本次新增输入仍应拒绝；这是固定版本边界。
CI 仍使用原 v3 selector，本次未迁移 CI、触发 CI 或宣称 CI 通过。

原 C++ 修复的有限运行时结论及限制见
[已发布结论](../../reviews/cpp-namespace-function-public-20261005/README.md)。
其中完整 qualified/type owner 解析、保守不绑定造成的召回损失等限制仍在。
本版本不代表新的检索质量、完整 C++ 语义、全工作区、formal DEV/100k、
P7/V19 或 release 验收；source integrity 通过不继承任何这些认证。
