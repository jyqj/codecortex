import sys,json,base64,io,zipfile,hashlib,re,ast,yaml,stat,pathlib
# Pure in-memory offline review only: stdin object fields zip(base64), workflow(text), review(JSON text).
fields=json.load(sys.stdin)

raw=base64.b64decode(fields['zip'])
assert len(raw)==127927 and hashlib.sha256(raw).hexdigest()=='2f511b61f5ce369f4aafcf59d8650b67f175b6c3bb25cbac0c0739b931f44701'
z=zipfile.ZipFile(io.BytesIO(raw))
assert z.testzip() is None
members={}
inventory=[]
for item in z.infolist():
 path=pathlib.PurePosixPath(item.filename)
 assert not path.is_absolute() and '..' not in path.parts and '\\' not in item.filename and str(path)==item.filename
 assert not item.is_dir() and not stat.S_ISLNK(item.external_attr>>16) and not (item.flag_bits & 1)
 assert item.filename not in members
 body=z.read(item)
 assert len(body)==item.file_size
 members[item.filename]=body
 inventory.append(dict(path=item.filename,bytes=len(body),sha256=hashlib.sha256(body).hexdigest(),crc=item.CRC))
receipt=json.loads(members['receipt.json'])
plan=json.loads(members['plan.json'])
progress=json.loads(members['progress.json'])
assert set(receipt['files'])==set(members)-{'receipt.json'}
for path,record in receipt['files'].items():
 body=members[path]
 assert record==dict(bytes=len(body),sha256=hashlib.sha256(body).hexdigest())
assert members['source-before.json']==members['source-after.json']
snapshot=json.loads(members['source-before.json'])
review=json.loads(fields['review'])
assert snapshot['source_commit']=='0272a1fb152fd76a7cfb22386a580629d4038a64'
assert snapshot['source_tree']=='85e9f6fb68d7b4ef79e52e23f1ade2e70abca383'
assert snapshot['input_count']==1089==len(snapshot['inputs'])
assert snapshot['inputs']==review['complete_inputs']
source_manifest=hashlib.sha256((json.dumps(snapshot['inputs'],ensure_ascii=False,sort_keys=True,indent=2)+'\n').encode()).hexdigest()
assert source_manifest==snapshot['manifest_sha256']=='691ae804ec606969e70180298407fcacc60b0408ea025eece02c7639b50b94ac'
workflow=yaml.safe_load(fields['workflow'])
step=next(x for x in workflow['jobs']['controls_and_build']['steps'] if x.get('name')=='Capture fixed-source controls and preserve every attempted command')
run=step['run']; py=run.split("python3 - <<'PY'\n",1)[1].rsplit('\nPY',1)[0]
module=ast.parse(py)
constants={}
for node in module.body:
 if isinstance(node,ast.Assign) and len(node.targets)==1 and isinstance(node.targets[0],ast.Name) and node.targets[0].id in ['commands','expected_tests','expected_observers','expected_product_manifest']:
  constants[node.targets[0].id]=ast.literal_eval(node.value)
commands=constants['commands']
tests={str(k):v for k,v in constants['expected_tests'].items()}
assert len(commands)==18 and len(tests)==16 and sum(map(len,tests.values()))==53
assert plan['commands']==receipt['commands']==progress['commands']==commands
assert plan['expected_tests']==receipt['expected_tests']==progress['expected_tests']==tests
for record in [plan,receipt,progress]:
 assert record['expected_source']==snapshot['source_commit']
 assert record['expected_product_input_count']==1089
 assert record['expected_product_manifest_sha256']==source_manifest==constants['expected_product_manifest']
 assert record['expected_test_count']==53
 assert record['cwd']=='/home/runner/work/codecortex/codecortex'
 assert record['environment']==receipt['environment']
assert plan['results']==[] and plan['status']=='running'
assert progress['results']==receipt['results']
assert len(receipt['results'])==18 and receipt['not_run_command_indices']==[]
assert receipt['exit_code']==0 and receipt['status']=='controls_completed'
assert receipt['source_unchanged'] is True and receipt['observers_unchanged'] is True
assert members['observer-before.json']==members['observer-after.json']
observers=json.loads(members['observer-before.json'])
assert set(observers)==set(constants['expected_observers']) and len(observers)==3
for path,r in observers.items():
 assert r['sha256']==constants['expected_observers'][path]==review['validation_inputs'][path]
 assert isinstance(r['bytes'],int) and r['bytes']>0
results=[];actual_names=[]
for i,record in enumerate(receipt['results']):
 assert record['argv']==commands[i] and record['exit_code']==0 and record['accepted'] is True
 assert record['elapsed_ns']>0
 assert record['stdout']==f'{i:02}.stdout' and record['stderr']==f'{i:02}.stderr'
 stdout=members[record['stdout']].decode()
 row=dict(index=i,argv=record['argv'],exit_code=record['exit_code'],elapsed_ns=record['elapsed_ns'],started_utc=record['started_utc'],stdout_sha256=hashlib.sha256(members[record['stdout']]).hexdigest(),stderr_sha256=hashlib.sha256(members[record['stderr']]).hexdigest())
 if str(i) in tests:
  names=[]
  for line in stdout.splitlines():
   if line.startswith('test ') and not line.startswith('test result:'):
    match=re.fullmatch(r'test (.+) \.\.\. ok',line);assert match,line
    names.append(match.group(1))
  assert len(names)==len(set(names)) and sorted(names)==sorted(tests[str(i)])
  assert re.findall(r'^running (\d+) tests?$',stdout,re.M)==[str(len(names))]
  summary=re.findall(r'^test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out; finished in [0-9.]+s$',stdout,re.M)
  assert len(summary)==1 and summary[0][:4]==(str(len(names)),'0','0','0')
  expected_population=dict(actual_names=sorted(names),expected_count=len(names),passed=len(names),failed=0,ignored=0,measured=0)
  assert record['test_population']==expected_population
  row.update(test_population=expected_population,summary=next(x for x in stdout.splitlines() if x.startswith('test result:')))
  actual_names.extend(names)
 results.append(row)
assert len(actual_names)==len(set(actual_names))==53
sql=json.loads(members['observations/p2d-dependency-work.json'])
verified=dict(status='accepted_scoped_original_controls_only',zip_bytes=len(raw),zip_sha256=hashlib.sha256(raw).hexdigest(),zip_git_blob=hashlib.sha1(b'blob '+str(len(raw)).encode()+b'\0'+raw).hexdigest(),member_count=len(members),sealed_file_count=len(receipt['files']),all_members_crc_and_sha_verified=True,complete_seal_exact=True,source_before_after_byte_identical=True,source_commit=snapshot['source_commit'],source_tree=snapshot['source_tree'],product_inputs=1089,product_inputs_equal_canonical_R2=True,source_manifest=source_manifest,observers=observers,observer_before_after_byte_identical=True,commands=results,actual_unique_methods=53,failed=0,ignored=0,not_run_command_indices=[],complete_actual_names=sorted(actual_names),sql_work_original=sql,inventory=inventory,original_receipt_sha256=hashlib.sha256(members['receipt.json']).hexdigest(),original_plan_sha256=hashlib.sha256(members['plan.json']).hexdigest(),original_receipt_exit_code=receipt['exit_code'],local_product_execution=False)
print(json.dumps(verified),flush=True)
