import importlib.util,json,pathlib,subprocess,os,sqlite3
spec=importlib.util.spec_from_file_location('upgrade','/workspace/codecortex/scripts/index_fix_upgrade_validation.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
base=pathlib.Path('/workspace/codecortex/artifacts/checkpoints/index-fix-integration-20261003');out=base/'final-v24';out.mkdir(exist_ok=True)
# The v23 reader is compiled from the isolated repaired-but-unbumped build.
# It opens the original intermediate binary's actual database without rebuilding.
source=(base/'old_pinned_probe.rs').read_text().replace('index.build_index(true).unwrap();','// Preserve the existing actual v23 rows, including missing type evidence.').replace('let mut index =','let index =')
driver_source=out/'old_v23_pinned_probe.rs';driver_source.write_text(source)
artifacts={}
for line in pathlib.Path('/tmp/index-fix-same-v23-build.jsonl').read_text().splitlines():
 d=json.loads(line)
 if d.get('reason')=='compiler-artifact' and d['target']['name'] in ('cc_server','cc_model','tokio','serde_json'):
  artifacts[d['target']['name']]=next(p for p in d['filenames'] if p.endswith('.rlib'))
command=['/workspace/.cargo/bin/rustc','--edition=2021',str(driver_source),'-L','dependency=/workspace/index-fix-target-same-v23/debug/deps','-o','/tmp/index-fix-v23-pinned-probe']
for name,path in artifacts.items():command+=['--extern',name+'='+path]
env=dict(os.environ,RUSTUP_HOME='/workspace/.rustup',CARGO_HOME='/workspace/.cargo')
r=subprocess.run(command,env=env,capture_output=True,text=True);(out/'v23-pinned-compile.stderr').write_text(r.stderr);assert r.returncode==0,r.stderr
receipt=dict(new_source_sha=subprocess.check_output(['git','-C','/workspace/codecortex','rev-parse','HEAD'],text=True).strip(),new_binary_sha256=m.digest(pathlib.Path('/workspace/index-fix-final-default-codecortex')),features='default only',provider_calls=0,local_search_calls=4,readers=[])
for version,driver,oldbinary,oldsource in [(22,'/tmp/index-fix-old-pinned-probe','/workspace/index-fix-old-target/debug/codecortex','574f7598662334c63e020da136c87f4f7281554d'),(23,'/tmp/index-fix-v23-pinned-probe','/workspace/index-fix-intermediate-codecortex','57bedbaf193e28be34271a290d28c90dc662b613')]:
 root=pathlib.Path('/workspace/index-fix-pinned-v24-from-'+str(version));root.mkdir(exist_ok=True)
 (root/'.codecortex.json').write_text('{"auto_index":{"enabled":false}}')
 (root/'main.py').write_text('class _: pass\ndef previous_marker(item: _):\n    pass\n')
 old=m.run(pathlib.Path(oldbinary),root,True,out/('pinned-'+str(version)+'-seed.stderr'));assert old['after']['schema']==version and 'error' not in old['response'],old
 err=(out/('pinned-'+str(version)+'-reader.stderr')).open('w')
 p=subprocess.Popen([driver,str(root)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=err,text=True,env=dict(os.environ,XDG_CONFIG_HOME=str(root/'.config'),XDG_CACHE_HOME=str(root/'.cache')))
 try:
  before=json.loads(p.stdout.readline());assert before['schema']==version and before['pins']==1,before
  (root/'main.py').write_text('class _: pass\ndef corrected_marker(item: _) -> tuple[int, ...]:\n    return ()\n')
  upgraded=m.run(pathlib.Path('/workspace/index-fix-final-default-codecortex'),root,False,out/('pinned-'+str(version)+'-upgrade.stderr'));m.success(upgraded)
  p.stdin.write('after\n');p.stdin.flush();after=json.loads(p.stdout.readline());p.wait(timeout=10)
  assert p.returncode==0 and after['pins']==1 and before['generation']!=after['generation'],after
  assert 'unsupported version' in after['manifest_error'],after
  if after['query'] is not None:assert 'corrected_marker' in after['query'] and 'previous_marker' not in after['query'],after
  receipt['readers'].append(dict(old_schema=version,old_source_sha=oldsource,reader_source_sha=oldsource if version==22 else 'bf10b6477612f32d991bbfea3ed7791f468acc78',reader_binary_sha256=m.digest(pathlib.Path(driver)),old=old,before=before,upgrade=upgraded,after=after))
 finally:
  if p.poll() is None:p.kill();p.wait()
  err.close()
(out/'pinned-boundaries.json').write_text(json.dumps(receipt,indent=2,sort_keys=True)+'\n')
print(json.dumps(dict(status='pass',old_readers=[22,23],new_schema=24,old_manifests_reject_v3=True)))
