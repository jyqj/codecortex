#!/usr/bin/env python3
"""Update mutable roadmap summaries after exact accepted evidence and CAS checks."""
from pathlib import Path
import collections,hashlib,json,re,sys
R=Path.cwd();B=R/'artifacts/benchmarks/p5d-20260930-resume';O=B/(sys.argv[1] if len(sys.argv)>1 else 'final-v2');D=R/'docs/roadmap/code-index-v2'
def load(p): return json.loads(p.read_text())
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
A=load(O/'audit.json');V=load(O/'validation.json');M=load(O/'source-manifest.json');A=dict(A,source_bytes=M['total_bytes'],tests={tc:{k:next(c for c in V['commands'] if c['label']==tc+'-'+k)['tests'] for k in ['workspace','http','focused','real-mcp','watcher']} for tc in ['stable','1.95.0']});G=load(D/'P5-D-RUNTIME-GATE.json');T=load(D/'tasks.json');old=load(B/'summary-before.json')
assert A['source_digest_sha256']==G['covered_source_digest_sha256']
assert T['next_task']=='P5-019' and collections.Counter(t['status'] for t in T['tasks'])=={'done':118,'todo':74}
for name,row in old.items(): assert sha(D/name)==row['sha256'],name+' changed; reconcile'
status='118 done / 74 todo，P5 为 18/20；P5-016～018 完成本地实现/契约/回归子集，下一 P5-019。P5-D 整批与 G5/M2 尚未完成。'
links='最新见 [P5-D-RUNTIME-IMPLEMENTATION.md](P5-D-RUNTIME-IMPLEMENTATION.md) 与 [P5-D-RUNTIME-GATE.json](P5-D-RUNTIME-GATE.json)。'
scope='已接入能力状态、查询视图租约、LRU 弱登记与取消安全的冷初始化、非阻塞空闲清理，以及 search/context 的显式策略参数；dense 仍明确 disabled。'
source=f"冻结 {A['source_files']} 文件、{A['source_bytes']} 字节，摘要 `{A['source_digest_sha256']}`；{A['command_count']} 条命令收据、源码归档、日志和不可变二进制一致。证据目录 `{O.relative_to(R)}`。"
validation=[]
for tc,rows in A['tests'].items():
 validation.append(tc+'：'+', '.join(name+' '+str(rows[name]['passed'])+' passed/'+str(rows[name]['ignored'])+' ignored' for name in ['workspace','http','focused','real-mcp','watcher']))
tests='；'.join(validation)+'。各组失败 0，忽略项不计通过，重叠组不相加为唯一测试总数。'
quality='固定 51 题/306 请求无排序负差分、无效源码或新增完整性失败；原 source/intent Partial 和 S11 仍失败，完整检索 gate 保持 not_passed。两个可选 retrieval_strategy 字段以外，14 工具的旧输入属性和必填项保持一致。'
cost='新增 12 组 release 项目清理/租约观测，复跑 90 次旧查询成本和 192 次准入请求；可选 ps 进程树采样在本机停滞后显式关闭，对应 RSS 为 null、原生自身 RSS 单列。核心验证未跳过；不是 P5-019 的完整消融、100k、尾延迟或发行认证。'
publication=f"Git HEAD=`{A['head']}`；未提交、推送、PR 或合并，既有工作与失败证据保留。"
next_section='''## 2. 下一步：P5-019，再进入 P5-020

先读本轮 RUNTIME-IMPLEMENTATION/GATE、[QUERY_LIFECYCLE.md](../../internals/QUERY_LIFECYCLE.md) 与既有 EVIDENCE_ASSEMBLY/QUERY_EXECUTION。P5-016～018 已验收，不重复实现或把三个任务的通过升级为整批通过。

P5-019 必须完成独立 exact/path/selector 等查询质量、成本、并发消融，记录真实混合构建与查询的延迟、线程/资源归属。当前 51 题配对只证明排序和完整性状态未回退，不替代独立消融。保留旧 Partial、S11 与真实预算省略，不能改 gold、按问题编号特判或压掉状态来修绿。

冷路径登记锁属于工作线程直到缓存发布，已打开缓存走不受冷锁影响的快路径；查询 clone 共用租约，在取消后仍运行的 blocking 工作结束前不能释放。保持这些负例，不为吞吐牺牲实例一致性。监听器启动/原生析构不可强制抢占；20 秒启动看门狗不是性能指标，既有 500 毫秒 debug 索引门限未修改。

P5-020/G5 需在当前实现上完成本地增强版整体验收后再判断 M2；真实 provider/vector、持久化 semantic epoch、公开 holdout、100k 和跨平台发行仍未完成。

'''
new={}
text=old['README.md']['text'];tail=text[text.index('\n## 1.'):]
new['README.md']=text.splitlines()[0]+'\n\n> **'+status+'** '+scope+'\n> '+links+'\n\n'+source+tests+quality+'\n\n'+publication+'\n'+tail
# Replace the prior headline progress paragraph without changing the target design.
lines=new['README.md'].splitlines()
for i,line in enumerate(lines):
 if line.startswith('规划规模：') or line.startswith('**10 phases'):
  lines[i]='**10 phases / 39 batches / 192 tasks.** '+status+' tasks.json 为唯一状态源。'
 if line.startswith('**Benchmark 从 P0') or ('schema21' in line and 'public adapter7' in line and '固定51题' in line):
  lines[i]=quality+cost
new['README.md']='\n'.join(lines)+'\n'
for name in ['04-PHASES.md','06-VALIDATION.md','09-BENCHMARK.md']:
 text=old[name]['text'];tail=text[text.index('\n## 1.'):]
 note=''
 if name=='06-VALIDATION.md': note='> Vxx 为验证包，不代表所有未来场景已认证。\n\n'
 if name=='09-BENCHMARK.md': note='> 下文保留目标设计；本轮仅接受运行时与接口子集。\n\n'
 prefix=text.splitlines()[0]+'\n\n'+note+status+links+'\n\n'+source+quality
 if name=='06-VALIDATION.md': prefix+='\n\n'+tests
 if name=='09-BENCHMARK.md': prefix+='\n\n'+cost
 new[name]=prefix+'\n'+tail
 if name=='06-VALIDATION.md':
  new[name]=new[name].replace('artifacts/benchmarks/p5c-20260929-completion/final-v5-20260930/validation.json',str((O/'validation.json').relative_to(R)))
text=old['08-HANDOFF.md']['text'];tail=text[text.index('\n## 3.'):]
new['08-HANDOFF.md']=text.splitlines()[0]+'\n\n## 1. 当前状态\n\n**'+status+'** '+scope+links+'\n\n'+source+tests+'\n\n'+quality+cost+'\n\n'+publication+'\n\n'+next_section+tail
if '--write' not in sys.argv:
 print('VERIFIED_SUMMARY_PLAN',list(new)); raise SystemExit(0)
for name,text in new.items(): (D/name).write_text(text)
print('SUMMARY_DOCS_UPDATED',status)
