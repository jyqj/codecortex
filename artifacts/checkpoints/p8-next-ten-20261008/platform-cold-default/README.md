# P8-012：Linux / MSRV / default 真实冷构建补充

本目录补充原始 **P8-012** 的一个真实构建格，不增加 TODO 计数。
原 `platform/cold-matrix.json` 的 **0 passed / 8 not_run** 保持原样。
这份独立新矩阵为 **1 passed / 0 failed / 7 not_run**，矩阵命令按原协议
返回 exit **2**（不完整）；实际 Cargo 构建 exit **0**，独立 `--verify`
exit **0**。`complete_P8_012=false`、`release_certified=false`。

## 实际格与身份

| 项目 | 原始回执中的值 |
| --- | --- |
| 格 | Linux / Rust 1.95.0 / default |
| profile | dev，关闭调试信息与增量编译 |
| 构建 | 全新 output / target，jobs 2，timeout 1800 秒 |
| Cargo 选项 | `--locked --offline --no-default-features`，显式 `--target x86_64-unknown-linux-gnu` |
| compiler artifact | `fresh=false`，features `[]`，bin `codecortex` |
| 构建时间 | 151.146 秒 |
| 源码 commit | `b69daccaf1d52899d5c963f085ede154782e9828` |
| 绑定源码输入 | 785 个 Cargo/crate 文件，共约 10.1 MB |
| 源码 manifest SHA256 | `19301e929312240239d70396b41e53a6862ade4ef7498a55720a2c253b824ab0` |
| 二进制 SHA256 | `4d9fd5d537c8b49f33b0df88a633c1236296342ae91037f04a2eabb626710f77` |

`source-equivalence.json` 对固定本地构建提交与先前远端源码提交
`78ae91eeae6edae6bea29c27f24b251773341c00` 分别执行 Git tree 枚举，
核对所有 785 个 Cargo/crate 输入的路径、mode 和 blob，结果逐项相同。
它不声称两个提交的整个仓库 tree 相同。

执行使用原 `scripts/p8_cold_build.py`，runner SHA256 为
`1f2572cb8b667899994c722d97ff45e220b07008de102dc9beead7b92d2c98ad`；
本次没有修改 cold runner、验证门或其测试。脚本在固定 detached 源码目录
中构建，编译前后核对源码、实际 Cargo/rustc 字节与配置、唯一成功产物、
日志及二进制。随后原 `--verify` 再独立核验了一次。

## 执行与资源

新运行根为 `/dev/shm/codecortex-p8-cold-default-ebacee`。
使用本地共享 Git 对象的稀疏 clone 固定源码，output 为 `run-01`，新 target
在该 output 下。Cargo registry/source cache 被复用，Cargo 网络访问受
`--offline` 限制；未宣称 hermetic 构建、无依赖源码缓存或发行认证。

启动检查为 3 GiB 可用空间，实际启动前可用 3,375,558,656 bytes；
`preflight.json` 保留原环境、jobs、profile、timeout 与 cgroup 限额。
`resource-observations.jsonl` 记录构建期间共享 tmpfs/cgroup 的实际观测，
其中 `oom` / `oom_kill` 均为 0。其他会话文件与 target 未被改动。

剩余七格保留 `cell_not_selected` / `platform_unavailable`；stable 工具链
缺失细节仍由原始工具链探针记录。只有这一个实际执行的格可以计为通过。
dev 冷构建不替代 release 安装包、semantic 包、macOS 或完整平台矩阵认证。

## 证据

- `matrix.json`、`cell-receipt.json`：新矩阵及原始格回执。
- `compiler-artifact.json`：实际 Cargo compiler artifact，完整内容也在日志及格回执内。
- `source-inputs.json`、`source-equivalence.json`：源码字节 manifest 与固定提交等价检查。
- `cold-build.log`、`verify.log`：两个实际命令的结果；命令、环境与退出码见 `evidence-index.json`。
- `raw-evidence.tar.gz`：完整 stdout JSONL、stderr、工具链探针、格/矩阵回执、
  manifest、资源记录和上述摘要。原始相对目录结构为 `run-01/...`。

不提交大二进制或 target。严格 `--verify` 需要回执绑定的源码、编译器、
配置、日志和二进制仍在相应路径，缺失或改写任何一项都应失败；压缩档案
本身不替代这些 live 输入。归档内绝对路径保存当时 provenance。

```sh
mkdir /tmp/p8-cold-default-evidence
tar -xzf artifacts/checkpoints/p8-next-ten-20261008/platform-cold-default/raw-evidence.tar.gz \
  -C /tmp/p8-cold-default-evidence
```
