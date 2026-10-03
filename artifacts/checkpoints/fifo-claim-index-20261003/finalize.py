import pathlib,json,hashlib,gzip,subprocess
O=pathlib.Path(__file__).resolve().parent;R=O.parents[2]
def read(p):
 return p.read_text() if p.exists() else gzip.decompress(pathlib.Path(str(p)+'.gz').read_bytes()).decode()
rows=[]
for label in ['baseline','candidate','candidate-repeat','baseline-repeat']:
 for n in [1000,5000]:
  C=O/f'{label}-n{n}';d=json.loads((C/'summary.json').read_text());p=json.loads((C/'phase-summary.json').read_text())
  assert d['status']=='complete' and d['normal_eof_exit']==0
  assert d['final_status']['spec']=='retrieval-capabilities-v2' and d['final_status']['semantic_state']=='ready'
  assert d['final_status']['semantic_pending']==d['final_status']['semantic_failed']==0
  assert d['db']['counts']['outbox_states']=={'done':n} and d['db']['counts']['attempt_count']==n
  assert d['db']['counts']['semantic_manifest']==n and d['db']['counts']['document_manifest']==n
  assert d['db']['integrity']=='ok' and d['db']['foreign_key_errors']==0
  # deterministic input hashes/config excluding ephemeral port and generated path
  ref=json.loads(read(O/f'baseline-n{n}'/'source-inputs.json'));assert ref==json.loads(read(C/'source-inputs.json'))
  conf=d['config'].copy();conf['semantic']=conf['semantic'].copy();conf['semantic']['endpoint']='http://127.0.0.1:0/v1';assert conf==json.loads((O/'config.json').read_text())
  h=[json.loads(x) for x in read(C/'http.jsonl').splitlines()];assert len([x for x in h if x['event']=='returned'])==n
  assert all(x['input_count']==1 for x in h)
  snap=d['process_cost'];row={'case':label,'n':n,'cold_s':d['cold_s'],'observed_ready_s':d['observed_ready_s'],'claim_s':p['claim']['sum_s'],'claim_count':p['claim']['count'],'round_s':p['round_total_inclusive']['sum_s'],'durable_put_s':p['cache_durable_put']['sum_s'],'publish_cas_s':p['publish_cas']['sum_s'],'process_io':snap['io'],'cpu_user_ticks':snap['cpu_user_ticks'],'cpu_system_ticks':snap['cpu_system_ticks'],'hwm_bytes':snap['vmhwm_bytes'],'db_files_at_ready':d['db']['files']};rows.append(row)
(O/'paired-results.json').write_text(json.dumps({'order':['baseline 1k/5k','candidate 1k/5k','candidate-repeat 1k/5k','baseline-repeat 1k/5k'],'timer_note':'Identical temporary Instant/Mutex Vec diagnostic spans on both binaries. Process IO includes startup/index/drain/status/logging; /proc physical read_bytes=0 can mean cached reads. Two pairs, no statistical significance or 100k claim.','rows':rows},indent=2)+'\n')
identity={'base':'5ffbadcf48e26523b2eb46beda0d187a2e2e29cd','diagnostic_review':'199558c754aae7f8d1dba0ee73256a2ed11a4230','rustc':subprocess.check_output(['/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/rustc','-Vv'],text=True),'binaries':{x:hashlib.sha256(pathlib.Path('/workspace/scratch/fifo',x).read_bytes()).hexdigest() for x in ['baseline','candidate']},'production_files':{x:hashlib.sha256((R/x).read_bytes()).hexdigest() for x in ['crates/cc-db/src/semantic_outbox.rs','crates/cc-db/src/index_migrate.rs','crates/cc-db/src/sql/index_v1.sql']}}
(O/'identity.json').write_text(json.dumps(identity,indent=2)+'\n')
for C in O.glob('*-n*'):
 if C.is_dir():
  for name in ['http.jsonl','rpc.jsonl','resources.jsonl','source-inputs.json','phases-raw.json']:
   f=C/name
   if f.exists():
    with gzip.GzipFile(filename=str(f)+'.gz',mode='wb',mtime=0) as z:z.write(f.read_bytes())
    f.unlink()
print(json.dumps(rows,indent=2))
