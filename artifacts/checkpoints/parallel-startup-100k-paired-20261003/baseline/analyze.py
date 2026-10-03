#!/usr/bin/env python3
import collections,gzip,hashlib,json,pathlib,statistics,sys
sys.dont_write_bytecode=True
OUT=pathlib.Path(__file__).resolve().parent
CASE=OUT/'live/n100000'
def read(p):
    if p.exists():return json.loads(p.read_text())
    with gzip.open(str(p)+'.gz','rt') as f:return json.load(f)
def rows(p):
    actual=p if p.exists() else pathlib.Path(str(p)+'.gz')
    with (gzip.open(actual,'rt') if actual.suffix=='.gz' else actual.open()) as f:
        for line in f:yield json.loads(line)
def write(p,v):p.write_text(json.dumps(v,ensure_ascii=False,indent=2)+'\n')
s=read(OUT/'summary.json');build=read(OUT/'build-receipt.json')
assert build['source_sha']==s['source_sha']=='098ebd9c08031e0b652e7b021d8abbc9e8b19c3d'
assert s['production_sha']=='098ebd9c08031e0b652e7b021d8abbc9e8b19c3d'
assert build['build_exit_code']==0 and build['guard_stop'] is None
assert sorted(build['compiler_artifact']['features'])==['semantic','semantic-http']
assert build['compiler_artifact']['profile']['opt_level']=='3'
assert not build['compiler_artifact']['profile']['debug_assertions']
assert hashlib.sha256(pathlib.Path(build['compiler_artifact']['executable']).read_bytes()).hexdigest()==s['binary_sha256']==build['binary_sha256']
manifest=list(rows(CASE/'source-inputs.jsonl'));assert len(manifest)==100000
aggregate=hashlib.sha256()
for i,r in enumerate(manifest):
    p=CASE/'repo'/r['path'];data=p.read_bytes()
    assert r['path']==f'src/file_{i:05}.rs' and hashlib.sha256(data).hexdigest()==r['sha256'] and len(data)==r['bytes']
    assert data==f'pub fn resource_{i:05}() -> u32 {{ {i} }}\n'.encode()
    aggregate.update((json.dumps(r,sort_keys=True)+'\n').encode())
assert aggregate.hexdigest()==s['checks'][0]['result']['ordered_manifest_sha256']
phases=collections.defaultdict(lambda:dict(samples=0,root_rss=[],observed_sum=[],cgroup=[],coverage_errors=0,root_unreadable=0,observed_pids=set()))
for r in rows(CASE/'resources.jsonl'):
    d=phases[r['phase']];d['samples']+=1
    p=r.get('product');t=r.get('product_tree',{})
    if p:d['root_rss'].append(p['rss_bytes'])
    else:d['root_unreadable']+=1
    if t.get('sampled_sum_rss_bytes') is not None:d['observed_sum'].append(t['sampled_sum_rss_bytes'])
    d['coverage_errors']+=len(t.get('coverage_errors',[]));d['observed_pids'].update(t.get('observed_pids',[]))
    d['cgroup'].append(r['cgroup_memory_current'])
analysis={}
for phase,d in phases.items():
    analysis[phase]={'samples':d['samples'],'root_rss_max_bytes':max(d['root_rss']) if d['root_rss'] else None,'root_rss_median_bytes':statistics.median(d['root_rss']) if d['root_rss'] else None,'root_unreadable_samples':d['root_unreadable'],'observable_tree_sampled_sum_max_bytes':max(d['observed_sum']) if d['observed_sum'] else None,'complete_tree_rss_max_bytes':None if d['coverage_errors'] else (max(d['observed_sum']) if d['observed_sum'] else None),'tree_coverage_errors':d['coverage_errors'],'observable_pids':sorted(d['observed_pids']),'children_coverage':'unknown' if d['coverage_errors'] else 'observable children files read; transient children may be missed','cgroup_memory_current_max_bytes':max(d['cgroup'])}
write(OUT/'resource-analysis.json',{'phases':analysis,'limits':'non-atomic 20ms sampled RSS, shared-page double counting, not PSS or exact lifetime peak; cgroup includes harness/build/cache and is not product RSS'})
requests={};timings=collections.defaultdict(list);errors=[];statuses=[]
for r in rows(CASE/'rpc.jsonl'):
    p=r['payload'];i=p.get('id')
    if r['event']=='request':requests[i]=r;continue
    req=requests.get(i)
    if not req:continue
    name=req['payload'].get('params',{}).get('name',req['payload']['method']);elapsed=(r['time_ns']-req['sent_ns'])/1e6
    timings[name].append(elapsed)
    result=p.get('result',{});data=result.get('structuredContent',{});data=data.get('result',data)
    if 'error' in p or result.get('isError'):errors.append({'id':i,'tool':name,'payload':p})
    if name=='status' and isinstance(data,dict) and 'retrieval' in data:statuses.append({'time_ns':r['time_ns'],'retrieval':data['retrieval']})
statuserrors=[r for r in statuses if r['retrieval'].get('error') or r['retrieval'].get('index_state')=='error']
stats=lambda a:{'count':len(a),'min_ms':min(a),'p50_ms':statistics.median(a),'p95_ms':sorted(a)[min(len(a)-1,int(len(a)*.95))],'max_ms':max(a)}
http=list(rows(CASE/'http.jsonl'));deadline_ns=s['drain_start_ns']+300_000_000_000
http_counts={}
for period,selected in [('within300s',[r for r in http if r['time_ns']<=deadline_ns]),('cleanup_tail',[r for r in http if r['time_ns']>deadline_ns])]:
    http_counts[period]={event:{'requests':sum(r['event']==event for r in selected),'inputs':sum(r['input_count'] for r in selected if r['event']==event)} for event in ['entered','returned']}
write(OUT/'rpc-http-analysis.json',{'rpc_timings':{k:stats(v) for k,v in timings.items()},'rpc_errors':errors,'status_observations':len(statuses),'status_state_counts':dict(collections.Counter(r['retrieval'].get('semantic_state') for r in statuses)),'status_error_count':len(statuserrors),'status_errors':statuserrors,'http_counts':http_counts,'http_note':'returned HTTP inputs are not committed semantic manifests; deadline DB snapshot actual time is independently recorded','last_status':statuses[-1] if statuses else None})
if s['status']=='failed':
    assert s['failures']
    if any('300s' in r.get('error','') for r in s['failures']):assert 'ready_manifest_integrity_fk' in s['not_run']
    if 'ready_manifest_integrity_fk' in s['not_run']:assert 'bounded_concurrency' in s['not_run'] and 'reopen_stability' in s['not_run']
    assert read(CASE/'cleanup-tail-db.json')['inspection_phase'].startswith('after cleanup')
else:
    assert s['status']=='passed_declared_100k_local_scope'
    assert not s['not_run'] and not s['failures']
    assert read(CASE/'final-db.json')['counts']['semantic_manifest']==100000
write(OUT/'verification.json',{'evidence_identity_manifest_and_retention':'passed','product_outcome':s['status'],'full_V20':False,'quality_gate':False,'independent_correctness_review':'separate; not claimed','source_sha':s['source_sha'],'production_sha':s['production_sha'],'binary_sha256':s['binary_sha256'],'formal_samples':1})
print(json.dumps({'verification':'passed evidence identity/retention','product_outcome':s['status'],'status_observations':len(statuses),'status_errors':len(statuserrors),'http_counts':http_counts,'phases':analysis},ensure_ascii=False))
