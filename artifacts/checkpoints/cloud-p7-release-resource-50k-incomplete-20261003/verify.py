import pathlib,json,hashlib,gzip,subprocess,sys,importlib.util
sys.dont_write_bytecode=True
root=pathlib.Path(__file__).resolve().parent;repo=root.parents[2];manifest=json.loads((root/'artifact-manifest.json').read_text())
for x in manifest:assert hashlib.sha256((root/x['path']).read_bytes()).hexdigest()==x['sha256'],x['path']
s=importlib.util.spec_from_file_location('derive',root/'analyze.py');m=importlib.util.module_from_spec(s);s.loader.exec_module(m);a=m.analyze();assert a==json.loads((root/'measurements.json').read_text()) and len(a['cells'])==0 and len(a['scales'])==1 and a['scales'][0]['product']['sampled_peak_RSS_bytes']>0
x=json.loads((root/'summary.json').read_text());assert x['profile']=='release' and not x['cells'] and x['scales'][0]['status']=='failed_or_incomplete' and '_queue.Empty' in x['scales'][0]['traceback'];assert 'TimeoutExpired' in (root/'runner-failure.log').read_text()
with gzip.open(root/'fixed/n50000/source-inputs.json.gz','rt') as f:assert len(json.load(f))==50000
with gzip.open(root/'fixed/n50000/rpc.jsonl.gz','rt') as f:rpc=[json.loads(l) for l in f]
req=[x for x in rpc if x['event']=='request' and x['payload'].get('method')=='tools/call'];assert len(req)==1 and req[0]['payload']['params']['name']=='index';assert not [x for x in rpc if x['event']=='response' and x['payload'].get('id')==req[0]['payload']['id']
r=json.loads((root/'receipt.json').read_text());b=json.loads((root/'build-receipt.json').read_text());assert b['build_exit_code']==0 and b['guard_stop'] is None and b['source_sha']==r['build_source_sha'];assert x['binary_sha256']==b['binary_sha256'];assert b['profile']=='release' and b['compiler_artifact']['profile']['opt_level']=='3'
assert subprocess.check_output(['git','show',r['source_sha']+':scripts/p7_release_resource_preparation.py'],cwd=repo)==(repo/'scripts/p7_release_resource_preparation.py').read_bytes();assert not subprocess.check_output(['git','diff',r['build_source_sha'],r['source_sha'],'--','crates/*/src','Cargo.toml','Cargo.lock'],cwd=repo)
c=json.loads((root/'cleanup-receipt.json').read_text());assert c['owned_processes_only'] and not c['source_or_evidence_deleted'] and all(x['post_cleanup_state'] in ('Z','absent') for x in c['rows'])
print(json.dumps({'status':'passed','hashes':len(manifest),'scale':50000,'outcome':'failed_or_incomplete','query_cells':0,'100k':'not_run','full_V20':False}))
