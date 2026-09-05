from pathlib import Path
import re


def edit(path, transform):
    p=Path(path); old=p.read_text(); new=transform(old)
    if old==new: raise RuntimeError(f'no change in {path}')
    p.write_text(new)


def add_after_line(text, needle, extra):
    lines=text.splitlines(keepends=True)
    indexes=[i for i,line in enumerate(lines) if needle in line]
    if len(indexes)!=1: raise RuntimeError(f'ambiguous documentation link {needle!r}: {indexes}')
    lines.insert(indexes[0]+1,extra+'\n')
    return ''.join(lines)

edit('README.md',lambda s:add_after_line(s,'| [docs/BENCHMARK.md]',
 '| [docs/BENCHMARK_V2.md](docs/BENCHMARK_V2.md) | 身份/区间标签、真实 MCP 原始记录、报告重放、配对比较与增量差分 oracle |'))
edit('docs/README.md',lambda s:add_after_line(s,'| [BENCHMARK.md]',
 '| [BENCHMARK_V2.md](BENCHMARK_V2.md) | 可重放质量协议、错误分母、回归/held-out 隔离与增量全量对照 |'))

p=Path('docs/internals/SEARCH.md');s=p.read_text()
s=s.replace('给候选文件打分，收窄 chunk 级检索范围','给候选文件打分；仅为 grep 提供有界扫描提示')
s=s.replace('chunk 级检索之前先对文件打分收窄范围。','先对文件打分形成排序先验，不将系统猜测写入调用者的硬范围。')
s=s.replace('## 文件预选（PreselectLayer）','''## 文件预选（PreselectLayer）

**范围契约（ADR-0003）**：用户的 `file_paths` / `path_prefix` / `languages`
是硬范围，lexical 和 graph 通道在该范围内独立召回。Preselect 是软提示，
不再覆盖 `SearchRequest.file_paths`；非 fallback 的提示可收窄 grep 扫描，
但 recency fallback 不隐藏全库字面量。候选仍受各通道数量与扫描预算限制。
BM25 文件预选使用 `base + strength/(1+strength)`，其中
`strength=max(-bm25,0)`；排序方向与 SQLite 负分越小越好的约定一致。

图通道的符号投影独立于 SQL：优先最小完整包含 chunk，长符号则取相交的
多个分片；同分项在截断前按稳定身份打破平分。RRF 同一 lane 内的重复
chunk 只投一次票，但仍保留原始位置成本。验证入口见
[Benchmark v2](../BENCHMARK_V2.md)。
''')
p.write_text(s)

p=Path('docs/internals/INDEXING.md');s=p.read_text()
s=s.replace('## dirty closure（脏闭包）','''## dirty closure（脏闭包）

**增量契约补强（ADR-0003）**：只有具备导出摘要的 JS/TS/JSX/TSX 使用
导出指纹证明稳定；其他语言的空摘要不能作为未变证明，源码编辑保守触发
有预算的依赖传播。依赖来源是 imports 加上已持久化调用/引用的实际目标，
避免 import 字符串未解析成功时已有绑定永久陈旧。未绑定引用、全局名字桶
变化与其他尚未覆盖的语义依赖仍不在完备保证内。

Dirty reload 仅保留源码未变文件中 `parser_exact` 的本地 call/ref 绑定及
来源信息；其他 resolver 状态仍清除重算。保守传播可能增加非 JS/TS 的
正文编辑成本；200 文件与 16 轮上限及降级状态不变。三种语言的差分 oracle
比较增量与独立全量构建，见 [Benchmark v2](../BENCHMARK_V2.md)。
''')
p.write_text(s)

p=Path('docs/BENCHMARK.md');s=p.read_text()
s=s.replace('# 基准测试\n','# 基准测试\n\n检索质量新增独立的 [Benchmark v2](BENCHMARK_V2.md)：身份/区间标签、\n原始 MCP 观测与重放、错误分母、配对比较、增量差分 oracle。\n[本轮实测](benchmarks/code_index_quality_round1.md) 与下文历史性能报告分开。\n旧质量/性能数值是原提交的历史记录，不是新作用域/评分契约的重新测量。\n')
s=s.replace('测量取最小值（缓存命中路径）','测量保留全部样本（缓存命中路径，不再取最小值）')
s=s.replace('当前质量基线：24 个 gold 用例','历史质量基线：24 个 gold 用例')
p.write_text(s)

p=Path('docs/TEST_PLAN.md');s=p.read_text()
a=s.index('最近一次 `cargo test --workspace --all-targets`')
b=s.index('## 单元测试',a)
s=s[:a]+'''最近一次 `cargo test --workspace --all-targets`：1326 passed + 16 ignored。
实测实现提交 `dc6b3b3`，逐二进制求和，包含集成与 stdio 测试；详见
[本轮验证记录](benchmarks/code_index_quality_round1.md)。文档基线漂移现在会让
`scripts/update-doc-baselines.sh` 返回失败，而不只是打印提示。

'''+s[b:]
for name,count in [('cc-db','143'),('cc-eval','29 passed + 5 ignored'),('cc-index','363'),('cc-model','62'),('cc-parsers','181'),('cc-search','254'),('cc-server','220')]:
    pattern=r'(\| '+re.escape(name)+r' \| )[^|]+( \|)'
    s,n=re.subn(pattern,lambda m:m.group(1)+count+m.group(2),s,count=1)
    if n!=1:raise RuntimeError(f'missing test row {name}')
s=s.replace('## 基准测试','''## Benchmark v2 正确性轨道

新增 `quality.rs` 独立标签/排名/错误协议测试、10 任务 MCP 回归 manifest、
原始 JSONL 与逐字节报告重放、7 项 Python 配对比较测试，以及
`tests/incremental_oracle.rs` 的 TS/Python/Rust 持久会话差分测试。
所有协议、分母与边界见 [BENCHMARK_V2.md](BENCHMARK_V2.md)。
新 `Code index quality` CI 将这些门与 release 1k 规模 ground-truth 检查常驻化。

## 基准测试''')
p.write_text(s)

p=Path('scripts/update-doc-baselines.sh');s=p.read_text()
old='    echo "All baselines match docs/TEST_PLAN.md"\nfi'
assert old in s
p.write_text(s.replace(old,'    echo "All baselines match docs/TEST_PLAN.md"\nelse\n    exit 1\nfi'))

# Development-only machinery is absent from the final proposed tree.
for name in ['_dev/round_apply.py','_dev/compare_baseline.sh','.github/workflows/round-development.yml','_dev/finalize_docs.py']:
    Path(name).unlink()
print('Synchronized current contracts and removed temporary development runner')
