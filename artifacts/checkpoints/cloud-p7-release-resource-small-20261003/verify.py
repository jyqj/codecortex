import hashlib,importlib.util,json,pathlib,subprocess,sys
sys.dont_write_bytecode=True
root=pathlib.Path(__file__).resolve().parent;repo=root.parents[2]
manifest=json.loads((root/'artifact-manifest.json').read_text())
for row in manifest:assert hashlib.sha256((root/row['path']).read_bytes()).hexdigest()==row['sha256'],row['path']
s=importlib.util.spec_from_file_location('replay',root/'analyze.py');m=importlib.util.module_from_spec(s);s.loader.exec_module(m);a=m.analyze();assert a==json.loads((root/'measurements.json').read_text());assert len(a['cells'])==8 and sum(c['N'] for c in a['cells'])==256;assert all(c['N']==32 and c['errors']==0 and c['empty_hits']==0 for c in a['cells']);assert a['scales'][0]['product']['sampled_peak_RSS_bytes']>0
r=json.loads((root/'receipt.json').read_text());b=json.loads((root/'build-receipt.json').read_text());assert b['source_sha']==r['build_source_sha'] and b['build_exit_code']==0 and b['guard_stop'] is None;assert b['profile']=='release' and '--release' in b['command'] and b['compiler_artifact']['profile']['opt_level']=='3' and not b['compiler_artifact']['profile']['debug_assertions'];assert sorted(b['compiler_artifact']['features'])==['semantic','semantic-http']
x=json.loads((root/'original-run-summary.json').read_text());assert x['binary_sha256']==b['binary_sha256'] and x['release_build_source_sha']==b['source_sha'];assert all(c['peak_client_inflight']==c['concurrency_cap'] and not c['errors'] for c in x['cells']);assert x['scales'][0]['product_exit_code']==0
f=json.loads((root/'fixed/n1000/final-db.json').read_text());assert f['integrity']=='ok' and f['foreign_key_errors']==0 and f['counts']['files']==f['counts']['semantic_manifest']==1000
assert subprocess.check_output(['git','show',r['source_sha']+':scripts/p7_release_resource_preparation.py'],cwd=repo)==(repo/'scripts/p7_release_resource_preparation.py').read_bytes()
assert not subprocess.check_output(['git','diff',r['build_source_sha'],r['source_sha'],'--','crates/*/src','Cargo.toml','Cargo.lock'],cwd=repo)
print(json.dumps({'status':'passed','hashes':len(manifest),'release_scale':1000,'queries':256,'full_V20':False}))
