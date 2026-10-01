# P8 数据集准备（未认证）

来源：本轮读取 tasks.json P8-001～020 与 09-BENCHMARK.md；这不是任务完成收据。

## 必须保留的认证范围

- D2：至少 6 个真实仓库、Rust/JS/TS/Python/Go 与 mixed monorepo，约 600 个独立审阅问题；当前 51 个反复用于修复的开发题不能转称 holdout。
- D5：以 repository/module/query_family 隔离；同义改写与中英翻译必须同 split。holdout 应由独立审阅者封存，候选代码/预算冻结后一次运行；失败保留，不回写标签保分。
- D4：契约明确允许固定 seed 合成 1k/5k/10k/50k/100k，但须另测真实仓库；不能把合成fixture叫真实语义质量证据。每层报告 files/symbols/chunks/edges/vectors、全部重复、RSS/磁盘与 thread/queue。
- release 输入需锁 binary/config/source/scoring/model/runner/environment；改动后旧输入锁失效。

## 候选真实源码（未下载、未锁定）

候选仅用于讨论，branch URL 不是 pin；正式采集必须 git SHA、tree digest、源码 manifest、license hash、dirty/submodule status、准入与排除原因。

| 仓库 | 语言/用途 | 官方许可入口 |
|---|---|---|
| pallets/flask | Python、已有兼容集对齐 | https://github.com/pallets/flask/blob/main/LICENSE.txt |
| tokio-rs/tokio | Rust、并发/跨模块 | https://github.com/tokio-rs/tokio/blob/master/LICENSE |
| microsoft/TypeScript | TS、大规模模块/编译器 | https://github.com/microsoft/TypeScript/blob/main/LICENSE.txt |
| golang/go | Go、多包/配置 | https://github.com/golang/go/blob/master/LICENSE |
| expressjs/express | JS、路由与错误处理 | 采集时复核官方 LICENSE |
| vercel/next.js | mixed TS/JS monorepo | 采集时复核官方 license 与子树第三方许可 |

前四个官方许可入口本轮网页读取成功，但未完成逐文件第三方许可审计；不得据此声称全仓再分发已认证。

## 接入清单

1. owner 选定 repo/commit 与下载预算后，在独立只读 archive 采集；每仓一份原始许可和 SHA256 文件清单，不覆盖开发 checkout。
2. 审阅者先源码后答案，至少两次独立审阅；gold 不从检索结果生成。争议 quarantine 单独统计。
3. 明确 train/dev/heldout 各 repo/module/family 和重叠判定；答案不进入被测 source manifest 或产品包。
4. 正式 suite 使用现有 cc-eval schema/manifest/adapters/scorer，保持 compat/native 独立。
5. 尚缺：真实源码 pin/hash、约600题独立审阅、sealed holdout、100k release/RSS/尾延迟、跨平台冷构建、真实provider明确授权/预算。P8-014可选LLM旁证与P8-015 live语义不得默认调用付费API。

优先完成 P5-019/020 后再执行昂贵采集与发行认证。本文件不勾任何 done。
