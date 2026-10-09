import ast,hashlib,json,pathlib,subprocess,collections,re
pre='runtime-finalization-controls/'
def body(p):return z.read(pre+p)
def doc(p):return json.loads(body(p))
def sha(b):return hashlib.sha256(b).hexdigest()
def oid(b):return hashlib.sha1(b'blob '+str(len(b)).encode()+b'\0'+b).hexdigest()
def jb(v):return (json.dumps(v,ensure_ascii=False,indent=2,sort_keys=True)+'\n').encode()
rec=doc('receipt.json');exp=doc('test-expectations.json')
assert rec['status']=='commands_completed' and rec['exit_code']==0 and rec['child_process_confirmed_stopped'] is True and rec['not_run_command_indices']==[]
assert rec['failed']==rec['skipped']==0 and rec['actual_methods']==47 and rec['actual_explicit_subtests']==24
assert rec['expected_source']=='c8decec3e74955de089f5fd1effbe985dfcbe6b5'
assert rec['environment']=={'GITHUB_REF':'refs/heads/task/p8-runtime-finalization-controls-20261009','GITHUB_RUN_ATTEMPT':'1','GITHUB_RUN_ID':'37870351197','RUNNER_OS':'Linux'}
actual={k[len(pre):]:{f:v[f] for f in ('bytes','sha256')} for k,v in members.items() if k.startswith(pre) and k!=pre+'receipt.json'}
missing_seal_files=sorted(set(rec['files'])-set(actual))
assert all(actual[k]==rec['files'][k] for k in actual) and not(set(actual)-set(rec['files']))
assert len(missing_seal_files)==630 and all('/.git/' in k for k in missing_seal_files)
missing_seal_bytes=sum(rec['files'][k]['bytes'] for k in missing_seal_files)
assert set(members)=={pre+k for k in actual}|{pre+'receipt.json','p8-runtime-finalization-controller.py'}
assert body('controller.py')==z.read('p8-runtime-finalization-controller.py')
assert sha(body('controller.py'))=='5bb3329b43c7c625800c2a00b744ed48fd6f77c61cc44c34a7ef2d60ecce0100'
assert sha(body('workflow.yml'))=='cdad3f3fd6bc6bf2e8f08b0d6e12849790fd0b9a5b8fca9ef22a4efb4e5a48d3'
assert sha(body('test-expectations.json'))==rec['manifest_sha256']=='d0278ef5a24787aea32f5f95d6892b3aa46d19d01dcd8b10d30456c9665b4db9'
src=doc('source-before.json');obs=doc('observer-before.json');validation=doc('validation-before.json')
for a,b in [('source-before.json','source-after.json'),('observer-before.json','observer-after.json'),('validation-before.json','validation-after.json')]:assert body(a)==body(b)
assert src['source_commit']==rec['expected_source'] and src['source_tree']=='8331b1f00392f574e1ec18c4ef94f18e57ddda2b' and src['input_count']==len(src['inputs'])==1087
assert sha(jb(src['inputs']))==src['manifest_sha256']==exp['source_manifest_sha256']=='7a561d39191708023052b22ef5d626bf26ecf4ca6042eb39dc2f50a82ceb34bf'
repo=pathlib.Path('/workspace/scratch/2eaa00d0f93a/p8-staging-engineering-exact-source')
source_actual={}
for p,expected in src['inputs'].items():
 b=(repo/p).read_bytes();assert sha(b)==expected;source_actual[p]=sha(b)
assert obs['source_commit']==rec['expected_source'] and len(obs['files'])==7 and sha(jb(obs['files']))==obs['manifest_sha256']
fixed={e['path']:e for e in exp['fixed_observer_files']}
assert set(fixed)==set(obs['files'])
draft=pathlib.Path('/dev/shm/p8-runtime-finalization-controls-draft')
for p,e in fixed.items():
 ob=obs['files'][p];assert ob['sha256']==e['sha256'] and ob['git_blob']==e['git_blob'] and ob['git_mode']==e['mode'] and ob['bytes']==e['bytes']
 assert validation[p]==e
 if p=='scripts/p8_runtime.py':b=pathlib.Path('/workspace/scratch/2eaa00d0f93a/p8-main559-python-integration-draft/files/scripts/p8_runtime.py').read_bytes()
 else:b=subprocess.check_output(['git','-C',str(repo),'show','8e542e644b06a83fc82d18d9896cf82f5b2d796e:'+p])
 assert sha(b)==e['sha256'] and oid(b)==e['git_blob']
module_reports=[];all_ids=[];nsub=0
for i,m in enumerate(exp['modules']):
 name=m['module'];ids=[v['id'] for v in m['expected_methods']];sub={v['id']:v['expected_subtests'] for v in m['expected_methods']}
 s=doc(f'module-{i:02d}/summary.json');ev=[json.loads(l) for l in body(f'module-{i:02d}/events.jsonl').splitlines()]
 assert s['status']=='passed_exact_population' and s['tests_run']==m['expected_count']==len(ids) and s['exit_code']==0
 for k in ['errors','failed','expected_failures','skipped','unexpected_successes']:assert s[k]==0
 for k in ['expected_ids','loaded_ids','started_ids','stopped_ids','success_ids']:assert s[k]==ids
 assert s['expected_subtests']==s['actual_subtests']==sub
 assert len(ids)==len(set(ids))
 for kind in ['test_started','test_success','test_stopped']:assert [e['test_id'] for e in ev if e['kind']==kind]==ids
 subactual={k:[] for k in ids}
 state={}
 for event in ev:
  k=event['test_id'];assert k in ids
  if event['kind']=='test_started':assert k not in state;state[k]='started'
  elif event['kind']=='subtest':
   assert state[k]=='started' and event['passed'] is True and event['error'] is None;subactual[k].append(event['parameters'])
  elif event['kind']=='test_success':assert state[k]=='started';state[k]='success'
  elif event['kind']=='test_stopped':assert state[k]=='success';state[k]='stopped'
  else:raise AssertionError(event['kind'])
 assert subactual==sub and all(v=='stopped' for v in state.values())
 argv=['/usr/bin/python3','-B','-u','/home/runner/work/_temp/p8-runtime-finalization-controller.py','--module',name,'--out',f'/home/runner/work/_temp/runtime-finalization-controls/module-{i:02d}']
 assert rec['commands'][i]==rec['results'][i]['argv']==argv and rec['results'][i]['exit_code']==0 and rec['results'][i]['status']=='finished'
 assert re.search(r'Ran '+str(len(ids))+r' tests? in ',body(f'{i:02d}.stderr').decode()) and body(f'{i:02d}.stderr').decode().rstrip().endswith('OK')
 e=m['source'];assert validation[e['path']]==e
 if name in ['test_p8_runtime_cache','test_p8_runtime_finalization']:
  b=pathlib.Path('/workspace/scratch/2eaa00d0f93a/p8-main559-python-integration-draft/files')/e['path'];b=b.read_bytes()
 else:b=subprocess.check_output(['git','-C',str(repo),'show','8e542e644b06a83fc82d18d9896cf82f5b2d796e:'+e['path']])
 assert sha(b)==e['sha256'] and oid(b)==e['git_blob']
 text=b.decode();methods={name+'.'+c.name+'.'+n.name:n for c in ast.parse(text).body if isinstance(c,ast.ClassDef) for n in c.body if isinstance(n,ast.FunctionDef) and n.name.startswith('test_')}
 assert set(methods)==set(ids)
 for v in m['expected_methods']:
  assert sha(ast.get_source_segment(text,methods[v['id']]).encode())==v['source_sha256']
 all_ids+=ids;nsub+=sum(len(v) for v in sub.values())
 module_reports.append({'module':name,'methods':len(ids),'subtests':sum(len(v) for v in sub.values()),'exit_code':0,'ids':ids})
assert len(all_ids)==len(set(all_ids))==47 and nsub==24 and len(rec['results'])==4
result={'status':'actual_c8_python47_results_validated_archive_incomplete','source':src,'observer':obs,'modules':module_reports,'zip_members':len(members),'receipt_sealed_files':len(actual),'full_member_crc_sha_verified':True,'whole_inventory_exact':False,'missing_seal_file_count':len(missing_seal_files),'missing_seal_bytes':missing_seal_bytes,'all_missing_paths_under_fixture_dot_git':True,'raw_receipt_sha256':sha(body('receipt.json')),'actual_explicit_subtests':nsub,'actual_methods':len(all_ids),'actual_all_source_1087_bytes_match_fixed_8e_and_published_c8_bridge':True,'controller_copies_exact':True,'control_stdout_stderr_events_and_summary_concordant':True}
result['source']={k:v for k,v in src.items() if k!='inputs'}

