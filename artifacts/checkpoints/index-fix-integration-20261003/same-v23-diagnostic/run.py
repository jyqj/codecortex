import importlib.util,json,pathlib,sqlite3,hashlib,shutil
spec=importlib.util.spec_from_file_location('upgrade','/workspace/codecortex/scripts/index_fix_upgrade_validation.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
base=pathlib.Path('/workspace/codecortex/artifacts/checkpoints/index-fix-integration-20261003');saved=json.loads((base/'intermediate-v23/receipt.json').read_text());out=base/'same-v23-diagnostic';out.mkdir(exist_ok=True)
binary=pathlib.Path('/workspace/index-fix-target-same-v23/debug/codecortex')
def types(root):
 with sqlite3.connect('file:'+str(root/'.codecortex/index.sqlite3')+'?mode=ro',uri=True) as db:
  return dict(edges=list(db.execute("SELECT source_symbol,target_symbol,target_symbol_uid FROM semantic_edges WHERE relation_kind='uses_type'")),buckets=[r[0] for r in db.execute("SELECT key FROM resolution_dependencies WHERE kind='name_bucket' ORDER BY key")])
receipt=dict(source_sha='bf10b6477612f32d991bbfea3ed7791f468acc78',repair_sha='671063b11af8cb40a0d526098de82e684dd24aca',schema=23,manifest=2,features='default',binary_sha256=m.digest(binary),provider_calls=0,search_calls=0,cases=[])
for case in saved['cases']:
 root=pathlib.Path(case['runtime']);source=root/case['input_file']
 assert m.digest(source)==case['input_sha256'] and source.stat().st_mtime_ns==case['input_mtime_ns']
 before=m.audit(root);before_types=types(root);assert not before_types['edges'] and before==case['noop']['after']
 noop=m.run(binary,root,False,out/(case['label']+'-existing.stderr'));after_types=types(root)
 report=json.loads(noop['response']['result']['content'][0]['text'])['result']
 assert report['files_parsed']==0 and report['files_skipped']==1 and not report['parse_errors']
 assert noop['after']==before and not after_types['edges'],noop
 fresh=pathlib.Path('/workspace/index-fix-repaired-v23-fresh')/case['label'];fresh.mkdir(parents=True,exist_ok=True)
 shutil.copy2(source,fresh/case['input_file']);shutil.copy2(root/'.codecortex.json',fresh/'.codecortex.json')
 full=m.run(binary,fresh,True,out/(case['label']+'-fresh.stderr'));fresh_types=types(fresh)
 assert len(fresh_types['edges'])==1 and fresh_types['edges'][0][2],fresh_types
 receipt['cases'].append(dict(label=case['label'],input_sha256=case['input_sha256'],before=before,before_types=before_types,unchanged_noop=noop,after_types=after_types,fresh_full=full,fresh_types=fresh_types))
receipt['conclusion']='same schema23/manifest2 repaired build skips unchanged intermediate rows with missing uses_type; same build fresh full produces expected edge. Explicit semantic version invalidation required.'
(out/'receipt.json').write_text(json.dumps(receipt,indent=2,sort_keys=True)+'\n')
print(json.dumps(dict(status='confirmed',cases=5,same_version_missing_edges=5,fresh_correct_edges=5)))
