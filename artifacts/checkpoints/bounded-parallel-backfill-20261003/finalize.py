from pathlib import Path
import json,hashlib,gzip
E=Path(__file__).resolve().parent
labels=['baseline','candidate','candidate-repeat','baseline-repeat']
rows=[];input_hashes={};body_hashes={}
for label in labels:
 for n in [1000,5000]:
  C=E/f'{label}-n{n}';d=json.loads((C/'summary.json').read_text());p=json.loads((C/'phase-summary.json').read_text())
  assert d['status']=='complete' and d['normal_eof_exit']==0
  assert d['status_poll_gap_seconds']==.2 and d['started_http_without_fixture_hold']
  assert d['final_status']['spec']=='retrieval-capabilities-v2' and d['final_status']['semantic_state']=='ready'
  assert d['final_status']['semantic_pending']==d['final_status']['semantic_failed']==0
  assert d['db']['counts']['outbox_states']=={'done':n} and d['db']['counts']['attempt_count']==n
  assert d['db']['counts']['document_manifest']==d['db']['counts']['semantic_manifest']==n
  assert d['db']['integrity']=='ok' and d['db']['foreign_key_errors']==0
  conf=json.loads(json.dumps(d['config']));conf['semantic']['endpoint']='http://127.0.0.1:0/v1';assert conf==json.loads((E/'config.json').read_text())
  inputs=json.loads((C/'source-inputs.json').read_text())
  if n in input_hashes:assert inputs==input_hashes[n]
  else:input_hashes[n]=inputs
  h=[json.loads(line) for line in (C/'http.jsonl').read_text().splitlines()]
  entered=[x for x in h if x['event']=='entered'];returned=[x for x in h if x['event']=='returned']
  assert len(entered)==len(returned)==n and all(x['input_count']==1 for x in h)
  assert sorted(x['sequence'] for x in entered)==list(range(1,n+1))
  bodies=sorted(x['body_sha256'] for x in entered)
  if n in body_hashes:assert bodies==body_hashes[n]
  else:body_hashes[n]=bodies
  peaks=[json.loads(line.removeprefix('PROVIDER_CONCURRENCY ')) for line in (C/'product-stderr.log').read_text().splitlines() if line.startswith('PROVIDER_CONCURRENCY ')]
  peak=max(x['peak'] for x in peaks);assert peak==(2 if label.startswith('candidate') else 1) and all(x['live']==0 for x in peaks)
  snap=d['process_cost'];samples=[json.loads(x) for x in (C/'resources.jsonl').read_text().splitlines()]
  rss=[]
  for s in samples:
   # Existing Product samples root and model independently; preserve unknown tree.
   root=s.get('product') or s.get('root')
   if isinstance(root,dict) and root.get('rss_bytes') is not None:rss.append(root['rss_bytes'])
  rows.append({'case':label,'n':n,'ready':True,'cold_s':d['cold_s'],'observed_ready_s':d['observed_ready_s'],'index_plus_ready_s':d['cold_s']+d['observed_ready_s'],'actual_claimed_done':n,'attempt_count':n,'claim_call_count_including_empty':p['claim']['count'],'claim_sum_s':p['claim']['sum_s'],'provider_actual_peak':peak,'provider_calls':len(entered),'round_sum_s':p['round_total_inclusive']['sum_s'],'durable_put_sum_s':p['cache_durable_put']['sum_s'],'publish_cas_sum_s':p['publish_cas']['sum_s'],'process_io':snap['io'],'cpu_user_ticks':snap['cpu_user_ticks'],'cpu_system_ticks':snap['cpu_system_ticks'],'rss_hwm_bytes':snap['vmhwm_bytes'],'rss_final_bytes':snap['rss_bytes'],'rss_sampled_max_bytes':max(rss) if rss else None,'full_process_tree':'unknown (owned root separately observed, never zero-filled)','db_files_at_ready':d['db']['files']})
pairs=[]
for n in [1000,5000]:
 for base,cand in [('baseline','candidate'),('baseline-repeat','candidate-repeat')]:
  b=next(x for x in rows if x['case']==base and x['n']==n);c=next(x for x in rows if x['case']==cand and x['n']==n)
  pairs.append({'n':n,'base':base,'candidate':cand,'ready_change_percent':(c['observed_ready_s']/b['observed_ready_s']-1)*100,'physical_write_change_percent':(c['process_io']['write_bytes']/b['process_io']['write_bytes']-1)*100})
report={'order':['baseline1k/5k','candidate1k/5k','candidate-repeat1k/5k','baseline-repeat1k/5k'],'pairs':pairs,'rows':rows,'notes':['Exactly two finite synthetic pairs, no statistical significance or production acceptance.','Original real release MCP/fake loopback HTTP/128 dims/config4+2/claim16/poll.2/90s termination retained.','Source input bytes, sorted real HTTP body hashes, normalized config and n calls/attempts/publications agree across all cases.','Same temporary timing probes and actual admitted-call concurrency counter on both binaries; no probes land in production.','Claim timing sums include per-call DB waits and empty probes; parallel durable-put/provider timing sums overlap and are not wall-time components.','/proc cumulative IO covers startup/index/drain/status/probes; read_bytes=0 may reflect cache, not zero read work.','Root RSS HWM is reported; full process tree is unknown.','Formal PR116 failed 100k pair remains unaccepted; earlier PR114 5k index-alone regression remains retained; no new100k.']}
(E/'paired-results.json').write_text(json.dumps(report,indent=2)+'\n')
# Lossless compression of raw observations, only after all assertions.
for C in E.glob('*-n*'):
 if C.is_dir():
  for name in ['http.jsonl','rpc.jsonl','resources.jsonl','source-inputs.json','phases-raw.json']:
   f=C/name
   if f.exists():
    with gzip.GzipFile(filename=str(f)+'.gz',mode='wb',mtime=0) as z:z.write(f.read_bytes())
    f.unlink()
print(json.dumps(report,indent=2))
