import importlib.util,json,pathlib,sqlite3,gzip,hashlib,shutil
spec=importlib.util.spec_from_file_location('upgrade','/workspace/codecortex/scripts/index_fix_upgrade_validation.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
out=pathlib.Path('/workspace/codecortex/artifacts/checkpoints/index-fix-integration-20261003/intermediate-v23');runtime=pathlib.Path('/workspace/index-fix-intermediate-cache');runtime.mkdir(exist_ok=True)
binary=pathlib.Path('/workspace/index-fix-intermediate-codecortex')
fixtures={
 'python-underscore':('case.py','class _: pass\ndef consume(item: _):\n    pass\n'),
 'python-double-underscore':('case.py','class __: pass\ndef consume(item: __):\n    pass\n'),
 'typescript-dollar':('case.ts','class $ {}\nfunction consume(item: $): void {}\n'),
 'python-weierstrass':('case.py','class ℘: pass\ndef consume(item: ℘):\n    pass\n'),
 'python-estimated':('case.py','class ℮: pass\ndef consume(item: ℮):\n    pass\n')}
receipt=dict(production_source_sha='57bedbaf193e28be34271a290d28c90dc662b613',features='default; no semantic/semantic-http',binary_sha256=m.digest(binary),provider_calls=0,search_calls=0,cases=[])
for label,(name,body) in fixtures.items():
 root=runtime/label;root.mkdir(exist_ok=True);(root/name).write_text(body);(root/'.codecortex.json').write_text('{"auto_index":{"enabled":false}}')
 full=m.run(binary,root,True,out/(label+'-full.stderr'));assert 'error' not in full['response'],full
 noop=m.run(binary,root,False,out/(label+'-noop.stderr'));assert 'error' not in noop['response'],noop
 assert full['after']==noop['after'],(full,noop)
 db=root/'.codecortex/index.sqlite3'
 with sqlite3.connect('file:'+str(db)+'?mode=ro',uri=True) as conn:
  edges=list(conn.execute("SELECT source_symbol,target_symbol,relation_kind FROM semantic_edges WHERE relation_kind='uses_type' ORDER BY source_symbol,target_symbol"))
  deps=list(conn.execute("SELECT kind,key FROM resolution_dependencies ORDER BY kind,key"))
 snapshot=out/(label+'.sqlite3')
 with sqlite3.connect('file:'+str(db)+'?mode=ro',uri=True) as conn, sqlite3.connect(snapshot) as dest:conn.backup(dest)
 raw=snapshot.read_bytes();(out/(label+'.sqlite3.gz')).write_bytes(gzip.compress(raw,mtime=0));snapshot.unlink()
 (out/(label+'-'+name)).write_text(body)
 receipt['cases'].append(dict(label=label,runtime=str(root),input_file=name,input_sha256=m.digest(root/name),input_mtime_ns=(root/name).stat().st_mtime_ns,full=full,noop=noop,uses_type_edges=edges,dependencies=deps,snapshot_sha256=hashlib.sha256(raw).hexdigest()))
(out/'receipt.json').write_text(json.dumps(receipt,indent=2,sort_keys=True)+'\n')
print(json.dumps(dict(status='captured',schema=23,manifest=2,cases=[dict(label=c['label'],uses_type=len(c['uses_type_edges'])) for c in receipt['cases']])))
