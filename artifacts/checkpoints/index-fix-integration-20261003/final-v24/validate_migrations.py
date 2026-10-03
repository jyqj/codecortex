import importlib.util,json,pathlib,sqlite3,hashlib,shutil
spec=importlib.util.spec_from_file_location('upgrade','/workspace/codecortex/scripts/index_fix_upgrade_validation.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
base=pathlib.Path('/workspace/codecortex/artifacts/checkpoints/index-fix-integration-20261003');saved=json.loads((base/'intermediate-v23/receipt.json').read_text());out=base/'final-v24';out.mkdir(exist_ok=True)
new=pathlib.Path('/workspace/index-fix-final-default-codecortex');old=pathlib.Path('/workspace/index-fix-old-target/debug/codecortex')
expected={'python-underscore':'_','python-double-underscore':'__','typescript-dollar':'$','python-weierstrass':'℘','python-estimated':'℮'}
def types(root):
 with sqlite3.connect('file:'+str(root/'.codecortex/index.sqlite3')+'?mode=ro',uri=True) as db:
  return dict(edges=list(db.execute("SELECT source_symbol,target_symbol,target_symbol_uid FROM semantic_edges WHERE relation_kind='uses_type'")),buckets=[r[0] for r in db.execute("SELECT key FROM resolution_dependencies WHERE kind='name_bucket' ORDER BY key")])
receipt=dict(new_source_sha=__import__('subprocess').check_output(['git','-C','/workspace/codecortex','rev-parse','HEAD'],text=True).strip(),features='default',binary_sha256=m.digest(new),old101_binary_sha256=m.digest(old),provider_calls=0,search_calls=0,paths=[])
for origin in ('intermediate-v23','pr101-v22'):
 for case in saved['cases']:
  label=case['label'];original=pathlib.Path(case['runtime'])
  if origin=='intermediate-v23':
   root=original
   assert m.digest(root/case['input_file'])==case['input_sha256'] and (root/case['input_file']).stat().st_mtime_ns==case['input_mtime_ns']
   before=m.audit(root);old_result=None;assert before['schema']==23 and before['versions']==[2] and not types(root)['edges']
  else:
   root=pathlib.Path('/workspace/index-fix-pr101-identifier-cache')/label;root.mkdir(parents=True,exist_ok=True)
   shutil.copy2(original/case['input_file'],root/case['input_file']);shutil.copy2(original/'.codecortex.json',root/'.codecortex.json')
   old_result=m.run(old,root,True,out/(origin+'-'+label+'-old.stderr'));before=old_result['after']
   assert 'error' not in old_result['response'] and before['schema']==22 and before['versions']==[1] and len(types(root)['edges'])==1
  before_types=types(root)
  upgraded=m.run(new,root,False,out/(origin+'-'+label+'-upgrade.stderr'));m.success(upgraded)
  after_types=types(root)
  assert upgraded['opened']['schema']==24 and upgraded['opened']['files']==0,upgraded
  assert before['generation']['index_incarnation']!=upgraded['after']['generation']['index_incarnation']
  assert len(after_types['edges'])==1 and after_types['edges'][0][2],after_types
  assert expected[label].lower() in after_types['buckets'],after_types
  assert m.digest(root/case['input_file'])==case['input_sha256']
  noop=m.run(new,root,False,out/(origin+'-'+label+'-noop.stderr'));m.success(noop)
  assert noop['after']==upgraded['after'] and types(root)==after_types
  report=json.loads(noop['response']['result']['content'][0]['text'])['result'];assert report['files_parsed']==0 and report['files_skipped']==1
  receipt['paths'].append(dict(origin=origin,label=label,old=old_result,before=before,before_types=before_types,upgrade=upgraded,after_types=after_types,noop=noop))
  (out/'identifier-migrations.json').write_text(json.dumps(receipt,indent=2,sort_keys=True)+'\n')
print(json.dumps(dict(status='pass',schema=24,manifest=3,paths=len(receipt['paths']))))
