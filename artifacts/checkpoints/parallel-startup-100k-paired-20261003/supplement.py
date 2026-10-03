import pathlib,json,gzip,collections,subprocess,hashlib
O=pathlib.Path(__file__).resolve().parent
def write(p,v):p.write_text(json.dumps(v,ensure_ascii=False,indent=2)+'\n')
def rows(p):
 a=p if p.exists() else pathlib.Path(str(p)+'.gz')
 with (gzip.open(a,'rt') if a.suffix=='.gz' else a.open()) as f:
  for l in f:yield json.loads(l)
identity=json.loads((O/'source-identity.json').read_text());out={}
for label in ['baseline','candidate']:
 case=O/label/'live/n100000';s=json.load(gzip.open(O/label/'summary.json.gz','rt')) if not (O/label/'summary.json').exists() else json.loads((O/label/'summary.json').read_text());events=list(rows(case/'http.jsonl'));active=set();peak=0;max_by_period=collections.defaultdict(int);release=s['drain_start_ns'];deadline=release+300_000_000_000
 for e in sorted(events,key=lambda x:x['time_ns']):
  if e['event']=='entered':active.add(e['sequence'])
  else:active.discard(e['sequence'])
  peak=max(peak,len(active));period='pre_release' if e['time_ns']<release else 'within300s' if e['time_ns']<=deadline else 'after_deadline';max_by_period[period]=max(max_by_period[period],len(active))
 histogram=collections.Counter();windows=[];tracked=set();ordered=sorted(events,key=lambda x:x['time_ns'])
 for first,following in zip(ordered,ordered[1:]):
  if first['event']=='entered':tracked.add(first['sequence'])
  else:tracked.discard(first['sequence'])
  duration=following['time_ns']-first['time_ns'];histogram[len(tracked)]+=duration
  if len(tracked)>2:windows.append({'start_ns':first['time_ns'],'end_ns':following['time_ns'],'duration_ns':duration,'active_sequences':sorted(tracked)})
 write(O/label/'http-concurrency-windows.json',{'histogram_ns':dict(histogram),'above_project_cap_observed_log_windows':windows,'note':'Original fixture entered begins after body read; returned logs after response write. Client can observe response and free gate before server appends returned. These intervals are not exact gate in-flight proof or full handler intervals.'})
 root=pathlib.Path(identity[label]['root']);head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip();status=subprocess.check_output(['git','status','--porcelain'],cwd=root,text=True)
 hashes=json.loads((O/label/'source-files.json').read_text());drift=[p for p,h in hashes.items() if hashlib.sha256((root/p).read_bytes()).hexdigest()!=h]
 assert head==identity[label]['head'] and not status and not drift
 rpc=list(rows(case/'rpc.jsonl'));retrieval=[]
 for r in rpc:
  p=r['payload'];d=p.get('result',{}).get('structuredContent',{});d=d.get('result',d)
  if isinstance(d,dict) and 'retrieval' in d:retrieval.append(d['retrieval'])
 gates=[]
 def find(v,path=''):
  if isinstance(v,dict):
   for k,x in v.items():
    if any(t in k for t in ['gate','max_concurrent','in_flight']):gates.append({'path':path+'.'+k,'value':x})
    find(x,path+'.'+k)
  elif isinstance(v,list):
   for x in v:find(x,path)
 if retrieval:find(retrieval[-1])
 out[label]={'http_observed_peak_active_intervals':peak,'http_peak_by_period':dict(max_by_period),'http_end_unmatched_intervals':sorted(active),'http_concurrency_histogram_ns':dict(histogram),'http_above2_window_count':len(windows),'http_above2_total_ns':sum(x['duration_ns'] for x in windows),'http_concurrency_definition':'entered timestamp through returned timestamp from original loopback HTTP fixture; entered occurs after body read and returned after response write; response/permit can complete before returned bookkeeping, so interval peak is not exact gate in-flight; no added instrumentation or delayed responses','source_after_run':{'head':head,'git_status':status,'input_hash_drift':drift},'gate_fields_exposed_in_last_status':gates,'effective_caps_runtime_directly_observable':bool(gates),'configured_caps':{'global':4,'per_project':2},'not_run':s['not_run'],'normal_exits':s.get('product_exits'),'reopen_artifacts':sorted(str(p.relative_to(case)) for p in case.glob('reopen*'))}
write(O/'supplemental-analysis.json',out);print(json.dumps(out,ensure_ascii=False))
