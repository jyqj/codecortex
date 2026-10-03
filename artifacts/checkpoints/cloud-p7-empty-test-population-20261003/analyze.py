"""Read-only derivation from retained parsed RPC and PID samples."""
import gzip,json,math,pathlib
root=pathlib.Path(__file__).resolve().parent

def read(path):
 with gzip.open(path,'rt') as f:return [json.loads(l) for l in f]
def quantile(values,q):return sorted(values)[max(0,math.ceil(len(values)*q)-1)] if values else None
def analyze():
 cells=[];scales=[]
 for case in sorted((root/'fixed').glob('n*'),key=lambda p:int(p.name[1:])):
  n=int(case.name[1:]);rpc=read(case/'rpc.jsonl.gz');requests={r['payload']['id']:r for r in rpc if r['event']=='request'}
  groups={}
  for response in (r for r in rpc if r['event']=='response'):
   request=requests.get(response['payload'].get('id'))
   if not request or not request['phase'].startswith(('warm_distinct_query','warm_repeated_query')):continue
   phase=request['phase'];result=response['payload'].get('result',{});value=result.get('structuredContent',{}).get('result',{});err=response['payload'].get('error') or result.get('isError')
   groups.setdefault(phase,[]).append({'id':request['payload']['id'],'queue_ms':(request['sent_ns']-request['offered_ns'])/1e6,'wire_ms':(response['time_ns']-request['sent_ns'])/1e6,'e2e_ms':(response['time_ns']-request['offered_ns'])/1e6,'offered_ns':request['offered_ns'],'response_ns':response['time_ns'],'wire_bytes':response['wire_bytes'],'error':bool(err),'hits':len(value.get('machine_pack',{}).get('hits',[])),'hydration_cache_hits':value.get('evidence_summary',{}).get('retrieval',{}).get('cost',{}).get('hydration',{}).get('text',{}).get('cache_hits'),'packing_used_bytes':value.get('evidence_summary',{}).get('packing',{}).get('used_bytes')})
  for phase,rows in sorted(groups.items()):
   latency=[r['e2e_ms'] for r in rows];duration=(max(r['response_ns'] for r in rows)-min(r['offered_ns'] for r in rows))/1e9
   cells.append({'scale':n,'phase':phase,'N':len(rows),'errors':sum(r['error'] for r in rows),'empty_hits':sum(r['hits']==0 for r in rows),'e2e_ms':{'min':min(latency),'mean':sum(latency)/len(latency),'p50_nearest_rank':quantile(latency,.5),'p95_nearest_rank_descriptive':quantile(latency,.95),'max':max(latency)},'client_queue_ms_max':max(r['queue_ms'] for r in rows),'wire_to_response_ms_max':max(r['wire_ms'] for r in rows),'completed_per_observed_second':len(rows)/duration,'backend_queue_ms':None,'backend_service_ms':None,'tail_certification':False,'samples':rows})
  samples=read(case/'resources.jsonl.gz');live=[r['product'] for r in samples if r.get('product')];runner=[r['runner'] for r in samples if r.get('runner')];model=[r['model'] for r in samples if r.get('model')]
  def resources(rows):
   if not rows:return None
   first,last=rows[0],rows[-1];hz=first['cpu_ticks_per_second']
   return {'pid':first['pid'],'start_time_ticks':first['start_time_ticks'],'samples':len(rows),'sampled_peak_RSS_bytes':max(r['rss_bytes'] for r in rows),'VmHWM_observed_bytes':max(r['vmhwm_bytes'] for r in rows if r['vmhwm_bytes'] is not None),'sampled_peak_threads':max(r['threads'] for r in rows),'observed_cpu_seconds':((last['cpu_user_ticks']+last['cpu_system_ticks'])-(first['cpu_user_ticks']+first['cpu_system_ticks']))/hz,'io_delta':{k:last['io'][k]-first['io'][k] for k in first['io']},'scope':'own root PID sampled, short-lived descendants not measured'}
  scales.append({'scale':n,'product':resources(live),'runner':resources(runner),'model':resources(model),'sample_interval_ms':20,'server_tree_RSS_bytes':None,'db_lock_wait_ms':None,'local_result_cache_hit_count':None,'graph_cache_hit_count':None,'query_jobs_per_stage':None,'provider_worker_threads_per_class':None})
 return {'status':'observed_release_handoff_checkpoint','cells':cells,'scales':scales,'formal_V20':False,'tail_note':'32 samples per profile/C, all retained. Descriptive p95, not >=200-sample formal tail or CI certification.'}
if __name__=='__main__':
 x=analyze();print(json.dumps({'cells':len(x['cells']),'requests':sum(c['N'] for c in x['cells']),'errors':sum(c['errors'] for c in x['cells']),'scales':[(r['scale'],r['product']['sampled_peak_RSS_bytes'] if r['product'] else None) for r in x['scales']],'full_V20':False}))
