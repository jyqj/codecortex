# P8 本地候选与归档工具验证收据

验证源码：`d1bf4b799e52d6367fe4959d08c5a79e63b3bafe`。
`receipt.json` 固定脚本、测试、文档及原始日志的 SHA-256；测试前后这些文件保持相同。

命令：

```sh
python3 -m unittest discover -s scripts/tests -p test_p8_release_evidence.py -v
```

Linux / Python 3.12.14 下 39 项通过，退出码 0。完整输出保留于 `unittest.log`。
夹具使用独立临时 Git 仓库、受控源码和 corpus 文件；不执行夹具 binary。
CLI 子进程和系统 sha256sum 验证了归档格式及退出码，所有临时原始输入删除后仍能验证归档。

新增关键反例包括：源码修改/新增/删除、暂存区/HEAD 变化、被忽略的新源文件、
query/gold 正文变化而 metadata 不变、语料文件增删、没有 corpus-root 时禁止提升 latest、
读取/复制过程中变化、旧 run 和既有 latest 不被失败覆盖，以及 symlink/FIFO/遍历/自包含/大小上限。

推进项为 P8-001 与 P8-019 的可运行 local engineering 前置实现。
两项父任务不因此转 done，P7-020/P8-018/完整 G8 依赖仍需独立闭合。
没有生产 binary 构建来源证明、实际产品新测量、MSRV/跨平台、公开 holdout、100k、soak 或 live 认证。
本目录不是发布候选归档，不应放入发行 latest。最终独立复核与整合由父任务记录。
