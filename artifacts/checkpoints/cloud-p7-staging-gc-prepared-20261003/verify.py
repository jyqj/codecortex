"""Read-only verification; never rewrites owner artifacts."""
import hashlib,json,pathlib,subprocess
root=pathlib.Path(__file__).resolve().parent
repo=root.parents[2]
manifest=json.loads((root/'artifact-manifest.json').read_text())
for f in manifest:
 assert hashlib.sha256((root/f['path']).read_bytes()).hexdigest()==f['sha256'],f['path']
r=json.loads((root/'receipt.json').read_text())
src='crates/cc-semantic/tests/p7_staging_gc_preparation.rs'
assert subprocess.check_output(['git','show',r['source_sha']+':'+src],cwd=repo)==(repo/src).read_bytes()
rows=json.loads((root/'results.json').read_text())
assert len(rows)==15
assert {(a['seed'],a['point']) for a in rows}=={(s,p) for s in (17,29,43) for p in ('staging-writing','staging-built','staging-swapped','gc-held','gc-committed')}
pids=[p for row in rows for p in row['killed_pids']]
assert len(pids)==len(set(pids))==21
for a in rows:
 assert a['signal']==9 and a['integrity']=='ok' and a['reuse_provider_calls']==0
 if a['point'].startswith('staging'):
  assert a['final_spaces']==1 and a['total_fake_calls']==1
  detail=json.loads((root/'children'/f"{a['seed']}-{a['point']}"/'physical-point.json').read_text())
  if a['point']=='staging-writing': assert detail['inside_transaction'] is True
  elif a['point']=='staging-built': assert detail['staging_exists'] is True
  else: assert detail['staging_exists'] is False and a['old_incarnation']!=a['new_incarnation']
 else:
  assert a['total_fake_calls']==2 and a['final_manifest']==2 and a['active_spaces']==1
  assert a['orphan_control_deleted'] and a['postrestart_gc_deleted']==0
  assert a['gc']['deleted_objects']==1 and a['gc']['kept_fresh']==0
  assert a['gc']['kept_live_task']==int(a['point']=='gc-held')
  assert a['gc']['kept_referenced']==(1 if a['point']=='gc-held' else 2)
c=json.loads((root/'crash-cost-canonical.json').read_text())
assert c['negative_window']['first_attempt_real_billable_tokens'] is None
assert c['negative_window']['per_case_actual_fake_calls']==2
print(json.dumps({'status':'passed','artifact_hashes':len(manifest),'SIGKILL':21,'full_V17':False}))
