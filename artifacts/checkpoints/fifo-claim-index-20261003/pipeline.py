import pathlib,importlib.util,sys,json,subprocess,time,hashlib,sqlite3,gzip,os
sys.dont_write_bytecode=True
O=pathlib.Path(__file__).resolve().parent;R=pathlib.Path('/workspace/scratch/fifo/source');binary=pathlib.Path(os.environ['FIFO_BINARY'])
spec=importlib.util.spec_from_file_location('driver',R/'scripts/p7_release_resource_preparation.py');d=importlib.util.module_from_spec(spec);spec.loader.exec_module(d)
for n in [1000,5000]:
 case=O/f"{os.environ.get('PHASE_CASE_PREFIX','pipeline')}-n{n}";case.mkdir();repo=case/'repo';(repo/'src').mkdir(parents=True);cache=case/'cache'
 subprocess.run(['git','-c','init.templateDir=','init','--quiet','--initial-branch=synthetic',str(repo)],check=True)
 hashes=[]
 for i in range(n):
  b=d.source(i,value=0).encode();p=repo/'src'/f'file_{i:05}.rs';p.write_bytes(b);hashes.append({'path':str(p.relative_to(repo)),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()})
 d.write(case/'source-inputs.json',hashes)
 # Normal responses from first request. No artificial held provider gate.
 (case/'http-release').touch();mocklog=(case/'model.log').open('wb');mock=subprocess.Popen([sys.executable,str(R/'scripts/p7_release_resource_preparation.py'),'--mock',str(case)],stdout=mocklog,stderr=mocklog)
 d.wait_file(case/'http-port.json');port=json.loads((case/'http-port.json').read_text())['port'];conf=json.loads((O/'config.json').read_text());conf['semantic']['endpoint']=f'http://127.0.0.1:{port}/v1';d.write(repo/'.codecortex.json',conf)
 p=None;res={'n':n,'source_sha':'5ffbadcf48e26523b2eb46beda0d187a2e2e29cd','base_checkout_sha':'5ffbadcf48e26523b2eb46beda0d187a2e2e29cd','instrumentation_patch_sha256':d.digest(O/'instrument.py'),'binary_sha256':d.digest(binary),'status_poll_gap_seconds':.2,'started_http_without_fixture_hold':True,'status':'running'}
 try:
  p=d.Product(binary,repo,case,cache);p.model_pid=mock.pid;p.phase='cold-index';t=time.monotonic();res['index']=p.tool('index',{'path':str(repo)});res['cold_s']=time.monotonic()-t
  assert res['index']['files_scanned']==res['index']['files_parsed']==res['index']['files_added']==n
  p.phase='normal-backfill';start=time.monotonic();polls=[]
  while time.monotonic()-start<90:
   t=time.monotonic();state=p.tool('status',{'aspect':'capabilities'})['retrieval'];polls.append({'elapsed_s':time.monotonic()-start,'latency_ms':(time.monotonic()-t)*1000,'semantic_state':state['semantic_state'],'dense_published':state.get('dense_published'),'semantic_pending':state.get('semantic_pending')})
   if state['semantic_state']=='ready':break
   time.sleep(.2)
  else:raise TimeoutError('bounded 90s diagnostic')
  res['observed_ready_s']=time.monotonic()-start;res['db']=d.db_report(repo);assert res['db']['counts']['semantic_manifest']==n;assert res['db']['integrity']=='ok' and res['db']['foreign_key_errors']==0
  res['status']='complete';res['polls']=polls;res['final_status']=state;res['process_cost']=d.snapshot(p.p.pid);res['model_cost']=d.snapshot(mock.pid);res['config']=conf
 finally:
  if p:res['normal_eof_exit']=p.close()
  mock.terminate();mock.wait(timeout=10);mocklog.close();d.write(case/'summary.json',res)
 phases={};rounds=[]
 for line in (case/'product-stderr.log').read_text().splitlines():
  if line.startswith('PHASE_COST '):
   row=json.loads(line[len('PHASE_COST '):]);rounds.append(row)
   for k,a in row.items():phases.setdefault(k,[]).extend(a)
 d.write(case/'phases-raw.json',rounds)
 def stats(a):
  a=sorted(a);return {'count':len(a),'sum_s':sum(a)/1e9,'mean_ms':sum(a)/len(a)/1e6,'p50_ms':a[len(a)//2]/1e6,'p95_ms':a[int(len(a)*.95)]/1e6,'max_ms':max(a)/1e6}
 summary={k:stats(v) for k,v in phases.items()};d.write(case/'phase-summary.json',summary)
 print(json.dumps({'n':n,'cold_s':res['cold_s'],'observed_ready_s':res.get('observed_ready_s'),'phases':summary}),flush=True)
