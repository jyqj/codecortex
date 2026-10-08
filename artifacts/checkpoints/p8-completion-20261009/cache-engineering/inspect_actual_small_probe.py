from pathlib import Path
import json,hashlib,sys,collections
ROOT=Path('/dev/shm/p8-soak-cache-real-probe-root');run=ROOT/'run-01';OUT=Path(__file__).resolve().parent;candidate=Path('/dev/shm/p8-soak-cache-candidate-G2')
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for b in iter(lambda:f.read(65536),b''):h.update(b)
 return h.hexdigest()
def load(name):return json.loads((run/name).read_text())
r=load('receipt.json');assert sha(run/'receipt.json')=='334cb82e9e498ccb518eef3519dbba502269c91619bfcc5dee03902203fe720a'
actual={}
for f in run.rglob('*'):
 assert not f.is_symlink()
 if f.is_file() and f.name!='receipt.json':actual[f.relative_to(run).as_posix()]={'bytes':f.stat().st_size,'sha256':sha(f)}
assert actual==r['files']
assert r['plan']==load('plan.json') and r['initial_full']==load('initial-full.json')
assert r['exit_code']==0 and r['status']=='passed_small_actual_product_probe' and r['errors']==[]
assert r['plan']['product_source']=='bb9a96d71622458c39a143055360cc97f0d11d78'
assert r['plan']['observer_runtime_sha256']=='63ad7e850d5cfeca9830db747fe8137c9200bc0f5d4d446ab678ec96f040617e'
assert r['observer_hashes_after']==r['plan']['observer_hashes_before']
for rel,digest in r['observer_hashes_after'].items():assert sha(candidate/rel)==digest
binary=ROOT/'retained-G-codecortex';original=Path('/workspace/scratch/2eaa00d0f93a/p8-formal-evidence-G/lifecycle/files/product/codecortex');origin=json.loads((original.parent/'build-receipt.json').read_text())
assert sha(binary)==sha(original)==r['plan']['product_sha256']==origin['binary_sha256']
assert origin['source_before']==origin['source_after'] and origin['build_exit_code']==0
assert origin['source_before']['source_commit']==r['plan']['product_source'] and origin['source_before']['input_count']==1087
assert origin['actual_cargo_profile']['opt_level']=='3' and origin['actual_cargo_profile']['test'] is False
sys.path.insert(0,str(candidate/'scripts'));import p8_runtime as runtime
rows=load('reads.json');mutations=load('mutations.json');assert len(rows)==14 and len(mutations)==6
previous=None;states=[];epochs=[];expected_calls=[]
project=run/'project'
expected_calls.append(('index',{'path':str(project),'full':True},r['initial_full']))
for stage in range(7):
 preceding=None
 if stage:
  m=mutations[stage-1];assert m['ordinal']==stage-1 and m['status']=='success'
  assert m['response']['parse_errors']==[] and m['response']['resolution_freshness']['complete'] is True
  expected_calls.append(('index',{'path':str(project),'full':False},m['response']))
  preceding={'action':m['mutation']['action'],'ordinal':m['ordinal'],'operation_id':m['ordinal'],'index_epoch':m['response']['resolution_freshness']['index_epoch']}
 for row in rows[2*stage:2*stage+2]:
  assert row['status']=='success' and row['offered_ns']<=row['started_ns']<=row['call_started_ns']<=row['finished_ns']
  probe=row['cache_probe'];assert probe['preceding_mutation']==preceding
  observed=runtime.validate_cache_probe(probe,previous,project);assert observed==probe['observation'];previous=observed
  assert row['response']==probe['requests'][1]['response']
  assert row['call_started_ns']<=probe['requests'][0]['started_ns']<=probe['requests'][-1]['finished_ns']<=row['finished_ns']
  states.append(observed['lookup']['state']);epochs.append(observed['identity']['generation']['index_epoch'])
  expected_calls.extend((call['name'],call['arguments'],call['response']) for call in probe['requests'])
assert [m['mutation']['action'] for m in mutations]==['bounded_symbol_churn','add','rename','delete','real_git_branch_switch','restore_api']
assert states==r['cache_states']==['miss','hit']*6+['hit','hit']
assert len(expected_calls)==63 and sum(x['cache_probe']['observation']['invalidated'] for x in rows)==r['invalidations']==5
assert len(set(x['cache_probe']['server_pid'] for x in rows))==1 and rows[0]['cache_probe']['server_pid']==r['cleanup']['pid']
raw=[json.loads(l) for l in (run/'product/rpc.jsonl').read_text().splitlines()]
requests=[e['payload'] for e in raw if e['event']=='request'];responses=[e['payload'] for e in raw if e['event']=='response'];wires=[json.loads(e['text']) for e in raw if e['event']=='stdout_wire']
assert len(requests)==len(responses)==len(wires)==65
assert [e['id'] for e in requests]==list(range(1,66)) and responses==wires
assert [e['id'] for e in responses]==list(range(1,66))
assert requests[0]['method']=='initialize' and requests[1]['method']=='tools/list'
for request,response,(name,args,body) in zip(requests[2:],responses[2:],expected_calls):
 assert request['method']=='tools/call' and request['params']=={'name':name,'arguments':args}
 assert 'error' not in response and response['result'].get('isError') is False
 structured=response['result']['structuredContent'];assert structured.get('result',structured)==body
terminal=[e for e in raw if e['event']=='terminal'];assert len(terminal)==2 and terminal[0]['kind']=='stdout_eof' and terminal[1]['kind']=='process_exit' and terminal[1]['exit_code']==0
assert raw[-2:]==terminal and r['cleanup']==load('product/process.json') and r['cleanup']['cleanup']=='completed' and r['cleanup']['exit_code']==0
report={'schema_version':1,'conclusion':'accepted_scoped_small_actual_G_product_with_fixed_v3_observer','reviewer':'/root/pr_audit','product_source':r['plan']['product_source'],'product_sha256':sha(binary),'observer_base':r['plan']['observer_base'],'observer_runtime_sha256':r['plan']['observer_runtime_sha256'],'original_receipt_sha256':sha(run/'receipt.json'),'reviewed_probe_script_sha256':sha(ROOT/'run_probe.py'),'all_original_inventory_members_verified':len(actual),'original_inventory_bytes':sum(v['bytes'] for v in actual.values()),'actual_full_builds':1,'actual_incremental_mutations':6,'actual_compound_reads':14,'actual_read_RPCS':56,'actual_total_tool_calls':63,'actual_total_requests_including_handshake':65,'all_requests_responses_and_stdout_wire_bijective':True,'original_read_mutation_bodies_equal_wire':True,'cache_states':states,'index_epochs':epochs,'invalidations':5,'same_native_pid':r['cleanup']['pid'],'original_stdout_EOF_and_process_exit_zero':True,'loaded_observer_before_after_hashes_verified':True,'binary_original_and_retained_hash_equal':True,'existing_exact_v3_cache_validator_replayed_on_every_original_row':True,'no_product_rerun_by_reviewer':True,'elapsed_ns_original':r['elapsed_ns'],'limits':['Engineering sequential8-file fixture, actual originalG binary with new observer; does not relabel either as newly built final P4.','Does not exercise full formal run queue/sampler/statistics/oracle/quarter/one-hour envelope; those source gates remain intact and formal run pending.','Configured concurrency peak, semantic-worker identity, throughput or latency SLA are not certified.','Probe receipt seals run files but does not itself contain the executor-script SHA; this independent audit binds the reviewed executor bytes and validates all retained wire outputs independently.','TODO remains29; zero new closure.'],'inputs':{'original_build_receipt':{'path':str(original.parent/'build-receipt.json'),'sha256':sha(original.parent/'build-receipt.json')},'original_probe_receipt':{'path':str(run/'receipt.json'),'sha256':sha(run/'receipt.json')},'probe_script':{'path':str(ROOT/'run_probe.py'),'sha256':sha(ROOT/'run_probe.py')}}}
(OUT/'audit.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({'audit_sha256':sha(OUT/'audit.json'),'files':len(actual),'epochs':epochs,'states':states,'elapsed_seconds':r['elapsed_ns']/1e9}))
