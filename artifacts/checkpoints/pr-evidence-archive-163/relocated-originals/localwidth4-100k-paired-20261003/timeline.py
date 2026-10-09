import json,pathlib,gzip,collections
O=pathlib.Path(__file__).resolve().parent
read=lambda p:json.loads(p.read_text()) if p.exists() else json.load(gzip.open(str(p)+'.gz','rt'))
def rows(p):
 with gzip.open(str(p)+'.gz','rt') as f:
  for l in f:yield json.loads(l)
output={}
for name in ['baseline','candidate']:
 p=O/name;case=p/'live/n100000';s=read(p/'summary.json');rs=list(rows(case/'resources.jsonl'));rpc=list(rows(case/'rpc.jsonl'));http=list(rows(case/'http.jsonl'))
 req={x['payload'].get('id'):x for x in rpc if x['event']=='request'};resp={x['payload'].get('id'):x for x in rpc if x['event']=='response'}
 unresolved=[{'id':i,'method':x['payload']['method'],'tool':x['payload'].get('params',{}).get('name'),'phase':x['phase'],'sent_elapsed_drain_seconds':(x['sent_ns']-s['drain_start_ns'])/1e9,'response':'not observed'} for i,x in req.items() if i not in resp]
 above=[x for x in rs if x['cgroup_memory_current']>12*1024**3]
 lastprod=next((x for x in reversed(rs) if x.get('product')),None)
 lastresident=next((x for x in reversed(rs) if x.get('product') and x['product']['rss_bytes']>0),None)
 ended=[x['time_ns'] for x in http if x['event']=='returned'];entered=[x['time_ns'] for x in http if x['event']=='entered']
 output[name]={'guard_stop':s.get('guard_stop'),'first_sample_above_12GiB_elapsed_drain_seconds':(above[0]['time_ns']-s['drain_start_ns'])/1e9 if above else None,'first_sample_above_12GiB_bytes':above[0]['cgroup_memory_current'] if above else None,'cgroup_sampled_max_bytes':max(x['cgroup_memory_current'] for x in rs),'last_nonzero_product_RSS_elapsed_drain_seconds':(lastresident['time_ns']-s['drain_start_ns'])/1e9 if lastresident else None,'readable_zero_RSS_note':'Proc state is unobserved; readable counters do not prove continued HTTP/backfill after server SIGTERM shutdown.','last_readable_product_elapsed_drain_seconds':(lastprod['time_ns']-s['drain_start_ns'])/1e9 if lastprod else None,'last_HTTP_enter_elapsed_drain_seconds':(max(entered)-s['drain_start_ns'])/1e9 if entered else None,'last_HTTP_return_elapsed_drain_seconds':(max(ended)-s['drain_start_ns'])/1e9 if ended else None,'unresponded_rpc_requests':unresolved,'observed_status_response_errors':read(p/'rpc-http-analysis.json')['status_error_count'],'status_rpc_no_response_count':sum(x['tool']=='status' for x in unresolved),'original_rpc_timeout_seconds':300,'latency_denominator':'received responses only; no-response timeout excluded from response p50/p95','event_time_order':'monotonic time_ns sorted; append inversions separately retained','inference_limit':'sampled guard threshold crossing and last readable product bound termination timing; not exact guard signal timestamp; cgroup includes harness/cache/build/preceding run residency, not product RSS'}
 if name=='candidate':
  post=[x['product'] for x in rs if x.get('product') and (x['time_ns']-s['drain_start_ns'])/1e9>183]
  if post:
   a,b=post[0],post[-1]
   output[name]['post183_root_sampled_counter_delta']={'cpu_user_seconds':(b['cpu_user_ticks']-a['cpu_user_ticks'])/b['cpu_ticks_per_second'],'cpu_system_seconds':(b['cpu_system_ticks']-a['cpu_system_ticks'])/b['cpu_ticks_per_second'],'io':{k:b['io'][k]-a['io'][k] for k in a['io']},'first_rss_bytes':a['rss_bytes'],'last_rss_bytes':b['rss_bytes']}
(O/'event-timeline.json').write_text(json.dumps(output,indent=2)+'\n')
(O/'final-outcome.json').write_text(json.dumps({'formal_product_runs':{'baseline':1,'candidate':1},'preflight_complete':True,'candidate_own_real_cache_smoke_complete':True,'hard_gate':{'baseline_ready':False,'candidate_ready':False},'candidate_guard':s.get('guard_stop'),'no_more_product_runs':True,'automatic_accept':False,'production_changed':False,'paired_throughput_comparison':'not established: candidate original resource guard terminated early','independent_correctness_review':'separate session'},indent=2)+'\n')
print(json.dumps(output,indent=2))
