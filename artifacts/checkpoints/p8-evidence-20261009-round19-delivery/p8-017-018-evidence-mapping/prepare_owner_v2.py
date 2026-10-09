#!/usr/bin/env python3
"""Add readable owner navigation without changing the fixed original record."""
import hashlib,json
from pathlib import Path
P=Path(__file__).resolve().parent
old=P/'P8-017-owner-compatibility-regression-inventory.json'
b=old.read_bytes();j=json.loads(b)
def rec(p):
 x=p.read_bytes();return {'path':str(p),'bytes':len(x),'sha256':hashlib.sha256(x).hexdigest(),'git_blob_sha1':hashlib.sha1(b'blob '+str(len(x)).encode()+b'\0'+x).hexdigest()}
j['schema']='P8-017-scoped-owner-compatibility-and-regression-inventory-v2'
j['previous_v1']=rec(old)
j['revision_scope']=['仅修正第一行调用者符号名，并将说明整理为可读中文；源码、实际回归记录、原任务与状态完全不变。', 'v1 的 bench::IncrementalStats::from_durations 是导航笔误；实际类型是 LatencyStats 与 LatencyStatsUs，见固定 E 的 bench.rs:190/197/319/326。']
rows=j['owners']
rows[0]['callers']=['distribution','quantile_interval','bench::run_benchmark_named','bench::LatencyStats::from_durations','bench::LatencyStatsUs::from_durations','report::compute_summary']
translations=[
 ('调用者保留自身 ms/us 单位；旧 wire 的空集合默认 0，版本化统计的空 Option 保持 null。','保留当前 E 实现。旧清理记录中 bench/report 仍有重复的陈述属于历史，不再适用于 E。'),
 ('Python 原始 ns 与旧 ns 摘要不能用 Rust 整数 us 乘 1000 重建；必须保留全部终态分母、空值及 wire 精度。','当前不替换。完整规模验收后，再决定显式兼容适配或同单位算法归并，并独立限定差分回归。'),
 ('该函数只报告 n/min/max/mean，没有承诺 p95/p99 或尾延迟估计。','保留；函数同名不等于重复百分位算法。'),
 ('OceCompat 路径模式与 Native 的分组、替代项、区间、分母具不同语义。','保留语义不同的显式 profile，不按指标名合并。'),
 ('旧 TOML EvalCase 的符号名 Recall@5/MRR 和逐 case 阈值，与版本化 Query 的分组、路径、区间评分不同。','在显式语义适配与旧语料回归明确之前保留。'),
 ('由 Rust 类型生成的 JSON schema 是输出，不是第二份手写事实来源。','保留 Rust owner；盘点其他域后才可声称多 schema 来源已全部清理。'),
 ('该适配层锁定原 schema/scorer 字节并归一化解析后的 Query；外部 wire 可能仍需该适配。','除非具体路径被证明只是临时冗余，否则保留。'),
 ('不同诊断协议的单位、分母和输出形态不能默默替换。','不按名称删除；具体清理需先明确兼容边界及对应回归。'),
]
for row,(compat,action) in zip(rows,translations):
 row['necessary_compatibility']=compat;row['action']=action
rows[1]['callers']=['run report.latency','run report.latency_by_operation']
rows[1]['existing_regression']=['E 六组 runtime/backfill 原件已按各自 scope 独立验收；runtime 原 Rust 统计重放原样保留。这些结果不证明未来 ns 归并与旧 wire 等价。']
rows[2]['callers']=['规模报告的阶段时间聚合']
rows[2]['existing_regression']=['原 b107 工具已经验收部分 shard，完整矩阵仍待完成。本项未提出删除，不能把待验矩阵误称删除回归。']
rows[3]['existing_regression']=['已有 CI benchmark 测试组保留其原范围；本记录没有新增 profile 等价性或删除证明。']
rows[4]['owner']='runner::check_assertions 与旧 expected_symbols 路径'
rows[4]['callers']=['TOML EvalCase runner']
rows[4]['existing_regression']=['本次静态清单没有证明完整 wire 适配或该路径的删除回归。']
rows[5]['existing_regression']=['当前 CI 的版本化 benchmark 契约仍适用；静态 owner 证明不认证仓库中所有 schema 文件。']
rows[6]['callers']=['兼容证据与旧输入归一化']
rows[6]['existing_regression']=['未提出删除；原 source guard 与兼容控制保留各自固定来源和执行范围。']
rows[7]['owner']='p7_worker_contention 与 incremental_write_bench 的局部诊断百分位闭包'
rows[7]['callers']=['测试/基准诊断输出']
rows[7]['existing_regression']=['当前 backfill 实际使用 p7_worker_contention；这不证明两个诊断函数是临时分支。']
j['remaining_scope']='这是已读取路径的精确清单，不是 cc-index/cc-search/cc-eval 全运行分支的穷尽证明。剩余分类与逐删除回归映射仍开放；不据此声称存在更多源码 bug 或需要广泛重构。'
j['decision_timing']='本轮完整规模验收期间不修改固定 E；根代理在独立审查后决定是否需要下一产品变更。'
p=P/'P8-017-owner-compatibility-regression-inventory-v2.json'
with p.open('xb') as f:f.write((json.dumps(j,ensure_ascii=False,sort_keys=True,indent=2)+'\n').encode())
assert old.read_bytes()==b
print(json.dumps(rec(p),ensure_ascii=False))
