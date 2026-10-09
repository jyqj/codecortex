#!/usr/bin/env python3
"""Replay paired evidence. No product runs or changed sampling."""
import collections,gzip,hashlib,json,pathlib,statistics,subprocess
O=pathlib.Path(__file__).resolve().parent
def read(p):
 if p.exists():return json.loads(p.read_text())
 with gzip.open(str(p)+'.gz','rt') as f:return json.load(f)
def rows(p):
 a=p if p.exists() else pathlib.Path(str(p)+'.gz')
 with (gzip.open(a,'rt') if a.suffix=='.gz' else a.open()) as f:
  for l in f:yield json.loads(l)
def write(p,v):p.write_text(json.dumps(v,ensure_ascii=False,indent=2)+'\n')
def stats(a):
 if not a:return {'count':0,'min':None,'p50':None,'p95':None,'max':None}
 return {'count':len(a),'min':min(a),'p50':statistics.median(a),'p95':sorted(a)[min(len(a)-1,int(.95*len(a)))],'max':max(a)}
def counters(x):
 if not x:return None
 return {'pid':x['pid'],'cpu_user_seconds':x['cpu_user_ticks']/x['cpu_ticks_per_second'],'cpu_system_seconds':x['cpu_system_ticks']/x['cpu_ticks_per_second'],'io':x.get('io'),'vmhwm_bytes':x.get('vmhwm_bytes'),'rss_bytes':x.get('rss_bytes')}
allresults={}
for label in ['baseline','candidate']:
 out=O/label;case=out/'live/n100000';s=read(out/'summary.json');build=read(out/'build-receipt.json')
 assert s['source_sha']==build['source_sha']
 assert hashlib.sha256(pathlib.Path(build['compiler_artifact']['executable']).read_bytes()).hexdigest()==s['binary_sha256']
 result=subprocess.run(['python3',str(out/'analyze.py')],stdout=(out/'analyze.log').open('w'),stderr=subprocess.STDOUT)
 assert result.returncode==0,(label,'PR108 replay failed')
 ra=read(out/'resource-analysis.json');rpc=read(out/'rpc-http-analysis.json')
 resources=list(rows(case/'resources.jsonl'));phase_rows=collections.defaultdict(list)
 for x in resources:phase_rows[x['phase']].append(x)
 phases={}
 for phase,rr in phase_rows.items():
  phases[phase]={}
  for actor in ['product','model','runner']:
   observable=[(x['time_ns'],x.get(actor)) for x in rr if x.get(actor)]
   phases[phase][actor]={'samples':len(observable),'unknown_samples':sum(x.get(actor) is None for x in rr),'first':counters(observable[0][1]) if observable else None,'last':counters(observable[-1][1]) if observable else None,'sampled_max_rss_bytes':max(x[1]['rss_bytes'] for x in observable) if observable else None,'counter_note':'Root lifetime cumulative counters; sampled endpoints omit any work after last readable sample. Phase delta is difference of sampled endpoints; absence is unknown.'}
   if observable:
    f,l=observable[0][1],observable[-1][1]
    assert f['pid']==l['pid'] and f['start_time_ticks']==l['start_time_ticks']
    phases[phase][actor]['sampled_delta']={'cpu_user_seconds':(l['cpu_user_ticks']-f['cpu_user_ticks'])/l['cpu_ticks_per_second'],'cpu_system_seconds':(l['cpu_system_ticks']-f['cpu_system_ticks'])/l['cpu_ticks_per_second'],'io':{k:l['io'][k]-f['io'][k] for k in l['io']}}
 entire={}
 for actor in ['product','model','runner']:
  rr=[x[actor] for x in resources if x.get(actor)]
  entire[actor]={'first':counters(rr[0]) if rr else None,'last':counters(rr[-1]) if rr else None,'sampled_max_rss_bytes':max(x['rss_bytes'] for x in rr) if rr else None}
 events=list(rows(case/'rpc.jsonl'));reqs={};statuses=[];pollgaps=[];prior_response=None
 for x in events:
  p=x['payload'];i=p.get('id')
  if x['event']=='request':
   reqs[i]=x
   if p.get('params',{}).get('name')=='status' and x['phase']=='backfill-drain' and prior_response is not None:pollgaps.append((x['sent_ns']-prior_response)/1e6)
  elif i in reqs and reqs[i]['payload'].get('params',{}).get('name')=='status' and x['phase']=='backfill-drain':
   prior_response=x['time_ns'];statuses.append(x)
 assert all(x>=199 for x in pollgaps),('poll gap changed',label,min(pollgaps))
 h=list(rows(case/'http.jsonl'));release=s.get('drain_start_ns');deadline=release+300_000_000_000 if release else None
 hp={};httpcounts={};periods=[('pre_release',lambda t:t<release),('within300s',lambda t:release<=t<=deadline),('cleanup_tail',lambda t:t>deadline)] if release else [('unknown',lambda t:True)]
 for name,select in periods:
  rr=[x for x in h if select(x['time_ns'])];ent=[x for x in rr if x['event']=='entered'];ret=[x for x in rr if x['event']=='returned'];httpcounts[name]={'entered_requests':len(ent),'entered_inputs':sum(x['input_count'] for x in ent),'returned_requests':len(ret),'returned_inputs':sum(x['input_count'] for x in ret),'actual_batch_sizes':dict(collections.Counter(x['input_count'] for x in ent))}
  for x in rr:
   if x['event']=='returned' and x['sequence'] in hp:httpcounts[name].setdefault('server_duration_ms_values',[]).append((x['time_ns']-hp[x['sequence']]['time_ns'])/1e6)
   if x['event']=='entered':hp[x['sequence']]=x
  httpcounts[name]['server_duration_ms']=stats(httpcounts[name].pop('server_duration_ms_values',[]))
  append_gaps=[(b['time_ns']-a['time_ns'])/1e6 for a,b in zip(ent,ent[1:])]
  chronological=sorted(ent,key=lambda x:x['time_ns'])
  httpcounts[name]['request_enter_gap_ms']=stats([(b['time_ns']-a['time_ns'])/1e6 for a,b in zip(chronological,chronological[1:])])
  httpcounts[name]['request_enter_gap_order']='time_ns_sorted'
  httpcounts[name]['request_enter_gap_append_order_diagnostic_ms']=stats(append_gaps)
  httpcounts[name]['append_timestamp_inversion_count']=sum(gap<0 for gap in append_gaps)
  httpcounts[name]['request_enter_gap_note']='Original fixture samples entered time_ns before acquiring its logging lock, then appends under lock. Signed gaps in append order diagnose timestamp/log-order inversions, not negative elapsed arrival intervals. Reported request_enter_gap_ms uses sorted recorded time_ns; body read precedes this timestamp, so it is not socket arrival spacing.'
 cold=next((x['result']['wall_seconds'] for x in s['checks'] if x['name']=='cold_index'),None)
 ready=next((x['result']['drain_wall_seconds'] for x in s['checks'] if x['name']=='ready_manifest_integrity_fk'),None)
 tail=read(case/'cleanup-tail-db.json') if (case/'cleanup-tail-db.json').exists() else None
 boundary=read(case/'deadline-300s-db.json') if (case/'deadline-300s-db.json').exists() else None
 final=read(case/'final-db.json') if (case/'final-db.json').exists() else None
 outcome={'source_sha':s['source_sha'],'production_sha':s['production_sha'],'binary_sha256':s['binary_sha256'],'status':s['status'],'cold_seconds':cold,'ready_seconds':ready,'cold_plus_ready_seconds':cold+ready if cold is not None and ready is not None else None,'readiness_pass':ready is not None,'boundary':boundary,'cleanup_tail':tail,'ready_final':final,'not_run':s['not_run'],'failures':s['failures'],'post_ready_normal_exit_check':any(x['name']=='normal_exit' for x in s['checks']),'cleanup_exit':s.get('failure_cleanup_product_exit'),'process_exits':s.get('product_exits'),'status_observations':rpc['status_observations'],'status_errors':rpc['status_error_count'],'rpc_errors':rpc['rpc_errors'],'status_latency_ms':rpc['rpc_timings'].get('status'),'poll_gap_ms':stats(pollgaps),'http':httpcounts,'resources':{'phase_counters':phases,'root_and_server_observations':entire,'sampled_rss':ra,'complete_process_tree':'unknown if coverage errors; sampled sum is not complete tree or PSS'},'ready_deadline_seconds':300}
 write(out/'extended-analysis.json',outcome);allresults[label]=outcome
# Assert the paired input/config and static environment match exactly.
assert read(O/'baseline/live/n100000/config.json')==read(O/'candidate/live/n100000/config.json')
for key in ['platform','affinity','cpu_max','memory_max','source_generator_sha256']:
 assert read(O/'baseline/environment.json')[key]==read(O/'candidate/environment.json')[key],key
for key in ['files','logical_bytes','ordered_manifest_sha256']:
 assert read(O/'baseline/summary.json')['checks'][0]['result'][key]==read(O/'candidate/summary.json')['checks'][0]['result'][key],key
b,c=allresults['baseline'],allresults['candidate']
decision={'integrate_production':False,'performance_acceptance':False,'reason':'Both fail the unchanged 300s readiness gate; more partial work is not a pass.' if not b['readiness_pass'] and not c['readiness_pass'] else 'Candidate fails unchanged gate.' if not c['readiness_pass'] else 'Candidate completed gate; one pair requires end-to-end and resource judgment, separate correctness review remains.','statistical_significance':'not claimed; exactly one pre-registered sequential pair','historical_PR116_5k_costs':'see preserved reference README; not measured in this task','old_PR108':'not current control','production_changed':False,'correctness_review':'independent; not covered by evidence replay','results':allresults,'verification':{'same_input_bytes':True,'same_config':True,'same_static_environment':True,'original_protocol_identity_retention_replay':'passed both','formal_runs_per_variant':1}}
if c['status']=='passed_declared_100k_local_scope':
 if b['status']=='passed_declared_100k_local_scope':
  decision['candidate_end_to_end_delta_seconds']=c['cold_plus_ready_seconds']-b['cold_plus_ready_seconds']
  decision['performance_acceptance']=decision['candidate_end_to_end_delta_seconds']<0
  decision['reason']='One pair supports lower cold+ready time at 100k; resources reported; separate correctness required.' if decision['performance_acceptance'] else 'No observed paired end-to-end improvement at 100k.'
 elif any('300s' in f.get('error','') for f in b['failures']):
  decision['performance_acceptance']=True
  decision['reason']='Candidate completes declared 100k protocol while this paired baseline times out; single-pair support only, resources reported; separate correctness required.'
 else:
  decision['reason']='Baseline failed outside readiness timeout; no valid paired performance acceptance.'
write(O/'paired-decision.json',decision)
print(json.dumps({'decision':decision['reason'],'outcomes':{k:{p:v[p] for p in ['status','cold_seconds','ready_seconds','status_observations','status_errors']} for k,v in allresults.items()},'identity_replay':'passed'}))
