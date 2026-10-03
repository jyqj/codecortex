"""Read-only raw replay/hash checks; no benchmark rerun or owner mutation."""
import hashlib,importlib.util,json,pathlib,subprocess,sys
sys.dont_write_bytecode=True
root=pathlib.Path(__file__).resolve().parent
repo=root.parents[2]
files=json.loads((root/'artifact-manifest.json').read_text())
for row in files:assert hashlib.sha256((root/row['path']).read_bytes()).hexdigest()==row['sha256'],row['path']
spec=importlib.util.spec_from_file_location('resource_analysis',root/'analyze.py');module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
actual=module.analyze();assert actual==json.loads((root/'measurements.json').read_text())
assert len(actual['cells'])==24 and sum(c['N'] for c in actual['cells'])==768
assert all(c['N']==32 and c['errors']==0 and c['empty_hits']==0 for c in actual['cells'])
assert all(s['product']['sampled_peak_RSS_bytes']>0 and s['server_tree_RSS_bytes'] is None for s in actual['scales'])
r=json.loads((root/'receipt.json').read_text())
assert subprocess.check_output(['git','show',r['source_sha']+':scripts/p7_resource_preparation.py'],cwd=repo)==(repo/'scripts/p7_resource_preparation.py').read_bytes()
assert r['real_failure']['scale']==50000 and r['real_failure']['rpc_code']==-32603
summary=json.loads((root/'original-run-summary.json').read_text());assert all(c['peak_client_inflight']==c['concurrency_cap'] and not c['errors'] for c in summary['cells'])
for n in (1000,5000,10000):
 case=root/'fixed'/f'n{n}';final=json.loads((case/'final-db.json').read_text());assert final['integrity']=='ok' and final['foreign_key_errors']==0
 assert final['counts']['files']==n and final['counts']['semantic_manifest']==n
assert len({(root/'fixed'/f'n{n}'/'config.json').read_bytes() for n in (1000,5000,10000,50000)})==1
intake=json.loads((root/'public-dev-count-hash-intake.json').read_text());assert intake['formal600accepted']==intake['clean_holdout']==intake['ranking_run_by_main']==0
print(json.dumps({'status':'passed','hashes':len(files),'replayed_requests':768,'completed_debug_scales':[1000,5000,10000],'50k':'failed_sql_bind_limit','release_or_full_V20':False}))
