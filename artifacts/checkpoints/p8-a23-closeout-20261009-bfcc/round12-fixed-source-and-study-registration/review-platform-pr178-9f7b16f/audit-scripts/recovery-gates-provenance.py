import sys,os,json,hashlib,subprocess,re
from pathlib import Path
sys.dont_write_bytecode=True
base=Path.cwd();source=(base/'candidate-combined').resolve()
sys.path.insert(0,str(source/'scripts'))
from p7_build_identity import source_snapshot,json_bytes
from p8_cold_build import digest,observer_snapshot,verify_observer_archive
from p8_recovery import full_evidence_manifest,FAULT_TESTS
out=base/'review-platform-pr178-9f7b16f'
commit='9f7b16f0758eb79f306cf44605b84550f02de441'
current=source_snapshot(source);assert current['source_commit']==commit and current['input_count']==1090
env=dict(os.environ,GIT_NO_LAZY_FETCH='1')
def git(*a,data=None):return subprocess.check_output(['git',*a],cwd=source,input=data,env=env)
def historical(commit):
 entries=git('ls-tree','-r','-z',commit,'--','Cargo.toml','Cargo.lock','crates')
 rows={}
 for line in entries.split(b'\0'):
  if not line:continue
  m,p=line.split(b'\t',1);mode,kind,oid=m.decode().split();assert kind=='blob' and mode in ['100644','100755'];rows[p.decode()]=oid
 objects=list(dict.fromkeys(rows.values()));raw=git('cat-file','--batch',data=('\n'.join(objects)+'\n').encode());at=0;hashes={}
 for oid in objects:
  end=raw.index(b'\n',at);actual,kind,size=raw[at:end].decode().split();assert actual==oid and kind=='blob';at=end+1;size=int(size);hashes[oid]=hashlib.sha256(raw[at:at+size]).hexdigest();at+=size;assert raw[at:at+1]==b'\n';at+=1
 assert at==len(raw)
 inputs={p:hashes[oid] for p,oid in sorted(rows.items())}
 return {'source_commit':commit,'source_tree':git('rev-parse',commit+'^{tree}').decode().strip(),'input_count':len(inputs),'manifest_sha256':hashlib.sha256(json_bytes(inputs)).hexdigest(),'inputs':inputs}
previous=historical('277f2490fad3fa30f2812b5547bad033867c9ea5');assert previous['input_count']==704
def artifact(aid,run):
 d=base/'raw-pr178-9f7b16f'/str(aid);m=json.loads((d/'github-metadata.json').read_text())
 assert m['id']==aid and m['workflow_run']['id']==run and m['workflow_run']['head_sha']==commit
 assert (d/'original.zip').stat().st_size==m['size_in_bytes'] and 'sha256:'+digest(d/'original.zip')==m['digest']
 assert not any(p.is_symlink() for p in (d/'extracted').rglob('*'))
 return d/'extracted',m
def observation(record,directory):
 assert record['status']=='passed' and type(record['exit_code']) is int and record['exit_code']==0
 assert json.loads((directory/'command.json').read_text())==record
 for n,h in record['logs_sha256'].items():assert digest(directory/n)==h
 return [json.loads(l) for l in (directory/'stdout.log').read_text().splitlines() if l.strip().startswith('{')]
def fault(record,directory):
 assert record['status']=='passed' and record['source']==current
 assert json.loads((directory/'test-receipt.json').read_text())==record
 messages=observation(record['build'],directory/'build')
 a=record['cargo_artifact'];assert a['reason']=='compiler-artifact' and a['profile']['test'] is True and messages.count(a)==1
 assert all(x in record['build']['command'] for x in ['--locked','--offline','--no-run'])
 assert any(m.get('reason')=='build-finished' and m.get('success') is True for m in messages)
 assert digest(directory/'fault-test')==record['executable_sha256']
 observation(record['execution'],directory/'execution')
 cmd=record['execution']['command'];assert cmd==[record['retained_executable'],'--exact',record['test_name'],'--nocapture','--test-threads=1']
 stdout=(directory/'execution/stdout.log').read_text()
 assert 'test '+record['test_name']+' ... ok' in stdout
 assert re.search(r'test result: ok\. 1 passed; 0 failed; 0 ignored;',stdout)
 return {'test_name':record['test_name'],'package':record['package'],'target':record['target'],'test_binary_sha256':record['executable_sha256'],'cargo_fresh':a['fresh'],'actual_test_exit_code':record['execution']['exit_code'],'stdout_sha256':digest(directory/'execution/stdout.log')}
raw,meta=artifact(11601251523,37896539450);home=raw/'p8-full-recovery';j=json.loads((home/'full-recovery.json').read_text())
assert j['source']==current and j['status']=='passed_declared_fault_matrix'
assert json.loads((home/'source-inputs.json').read_text())==current['inputs']
assert full_evidence_manifest(home)==j['evidence_files_sha256']
actual_observer=observer_snapshot(source,'full');assert j['observer_before']==j['observer_after']
assert dict(j['observer_before'],source_root=actual_observer['source_root'])==actual_observer
assert j['observer_binding']=='required_git_and_loaded_source';verify_observer_archive(home,j['observer_before'])
for n,h in j['runner_files_sha256'].items():assert digest(source/'scripts'/n)==h
products={}
for kind,r in j['products'].items():
 h=home/Path(r['binary_path']).parent.name;s=previous if kind=='previous-default' else current
 assert r['status']=='passed' and r['build_exit_code']==0 and r['source_before']==r['source_after']==s
 assert r['cold_build_claim'] is False and r['release_certified'] is False
 assert json.loads((h/'build-receipt.json').read_text())==r
 assert json.loads((h/'source-inputs.json').read_text())==s['inputs']
 assert digest(h/'codecortex')==r['binary_sha256'] and (h/'codecortex').stat().st_size==r['binary_bytes']
 messages=observation(r['build_observation'],h/'build');a=r['cargo_artifact']
 assert messages.count(a)==1 and a['target']['name']=='codecortex' and a['target']['kind']==['bin'] and a['profile']['test'] is False
 assert a['features']==({'default':[],'previous-default':[],'semantic':['semantic'],'semantic-http':['semantic','semantic-http']}[kind])
 assert any(m.get('reason')=='build-finished' and m.get('success') is True for m in messages)
 assert all(x in r['build_command'] for x in ['--locked','--offline']) and r['build_command']==r['build_observation']['command']
 products[kind]={'source_commit':s['source_commit'],'source_input_count':s['input_count'],'binary_sha256':r['binary_sha256'],'binary_bytes':r['binary_bytes'],'features':a['features'],'build_exit_code':0,'cold_build_claim':False}
assert [(x['package'],x['target'],x['test_name']) for x in j['executions']]==list(FAULT_TESTS)
tests=[fault(r,home/('fault-test-%02d'%i)) for i,r in enumerate(j['executions'])]
result={'schema_version':1,'status':'accepted_source_observers_products_and_original_fault_test_execution','source_commit':commit,'source_tree':current['source_tree'],'input_count':1090,'artifact_id':11601251523,'original_zip_digest':meta['digest'],'evidence_files':len(j['evidence_files_sha256']),'source_observer_inputs':len(actual_observer['inputs']),'products':products,'original_tests':tests,'remaining_scope':['Independent raw RPC/HTTP semantic event checks','Immutable DB/config/rollback snapshot checks'],'limitations':j['limitations'],'todo_status_change':False}
(out/'recovery-provenance-audit.json').write_text(json.dumps(result,indent=2)+'\n')
raw,meta=artifact(11600548850,37896539353);g=json.loads((raw/'report.json').read_text())
assert g['source']==current and g['status']=='passed_original_failure_gates' and g['exit_code']==0
assert {p.relative_to(raw).as_posix():digest(p) for p in raw.rglob('*') if p.is_file() and p!=raw/'report.json'}==g['files']
assert digest(raw/'cc-eval')==g['cli']['executable_sha256']
expected=['unmeasurable_latency_cannot_pass_comparison','zero_measurement_gate_is_invalid','cli_quality_and_latency_failures_are_nonzero_and_keep_raw','cli_inconclusive_is_nonzero','cli_lock_failure_preserves_machine_readable_failure_and_raw','cli_bad_policy_is_recorded_without_overwriting_existing_report']
assert [r['test_name'] for r in g['cases']]==expected
cases=[fault(r,raw/('case-%02d'%i)) for i,r in enumerate(g['cases'])]
result={'schema_version':1,'status':'accepted_original_six_exact_nonignored_fault_controls_cli_replay_pending','source_commit':commit,'source_tree':current['source_tree'],'input_count':1090,'artifact_id':11600548850,'original_zip_digest':meta['digest'],'original_files_verified':len(g['files']),'cli_sha256':g['cli']['executable_sha256'],'cases':cases,'release_approval':g['release_approval'],'task_complete':g['task_complete'],'todo_status_change':False}
(out/'gates-original-case-audit.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({'recovery_products':len(products),'recovery_original_fault_tests':len(tests),'gates_original_fault_tests':len(cases),'source_commit':commit,'status':'accepted_declared_original_provenance_and_test_exits_no_task_closure'}))
