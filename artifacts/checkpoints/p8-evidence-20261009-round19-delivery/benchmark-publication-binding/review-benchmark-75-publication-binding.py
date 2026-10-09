import datetime,hashlib,json
from pathlib import Path
ROOT=Path('/workspace/scratch/a217aaae3bde');B=ROOT/'checkpoint-round19-prep/benchmark-publication-binding';F=Path('/dev/shm/a217aaae3bde/platform-review/round19-full41-actual-tree')
def read(p):return json.loads(p.read_bytes())
def sha(b):return hashlib.sha256(b).hexdigest()
def blob(b):return hashlib.sha1(b'blob '+str(len(b)).encode()+b'\0'+b).hexdigest()
def resolved(p):
 p=Path(p);return p if p.is_absolute() else ROOT/p
def checkrec(r):
 p=resolved(r['path']);b=p.read_bytes();assert len(b)==r['bytes'] and sha(b)==r['sha256'];
 if 'git_blob_sha1' in r:assert blob(b)==r['git_blob_sha1']
 return str(p)
def tree(es):
 es=sorted(es,key=lambda e:(e['path']+('/' if e['type']=='tree' else '')).encode());b=b''.join(e['mode'].lstrip('0').encode()+b' '+e['path'].encode()+b'\0'+bytes.fromhex(e['sha']) for e in es);return hashlib.sha1(b'tree '+str(len(b)).encode()+b'\0'+b).hexdigest()
planp=B/'bound-780296/benchmark-75-upload-plan.json';plan=read(planp);assert sha(planp.read_bytes())=='bbef7d7b5661c284caf4614c6b6dd244c2e4b679be0b268eff0d2a900e3822d2'
proofp=resolved(plan['additional_raw_publication_proof']['path']);proof=read(proofp);checkrec(plan['additional_raw_publication_proof']);assert sha(proofp.read_bytes())=='a6c47163ca8b4231e004ba4e163099a94b3e8ddea4b957121d4a2f39f6ee13f4'
packp=B/'actual-780296-official-tree-pack.json';pack=read(packp)
for r in pack['capture_sources']:checkrec(r)
assert pack['commit']==read(F/'commit.json')==proof['official_objects']['commit']
assert pack['commit']['sha']==proof['actual_publication_commit']=='780296fb5a2f4cfcbda6b3e22075ebb03c373301'
assert pack['commit']['tree']['sha']==proof['actual_publication_tree']=='53d8369455424b09ae4e8117b5ee4967b519659e'
assert pack['trees']==[read(F/(n+'.json')) for n in ['root','artifacts','checkpoints']]
assert pack['recursive_trees']==[read(F/'originals-recursive.json')]
assert pack['ref']['object']['sha']=='780296fb5a2f4cfcbda6b3e22075ebb03c373301' and pack['ref']==proof['official_objects']['ref']
checkrec(proof['publication_receipt']);pub=read(resolved(proof['publication_receipt']['path']));assert pub['commit']=='780296fb5a2f4cfcbda6b3e22075ebb03c373301'
direct={}
for d in pack['trees']:
 assert d['truncated'] is False and tree(d['tree'])==d['sha'];direct[d['sha']]={e['path']:e for e in d['tree']}
for d in pack['recursive_trees']:
 assert not d['truncated']
 for par,oid in [('',d['sha'])]+[(e['path'],e['sha']) for e in d['tree'] if e['type']=='tree']:
  es=[]
  for e in d['tree']:
   pp=Path(e['path']);parent='' if str(pp.parent)=='.' else str(pp.parent)
   if parent==par:es.append({**e,'path':pp.name})
  assert tree(es)==oid;direct[oid]={e['path']:e for e in es}
assert len(direct)==47
bindings=proof['bindings'];assert len(bindings)==46 and len({x['repository_path'] for x in bindings})==46
for b in bindings:
 current=proof['actual_publication_tree'];segments=b['repository_path'].split('/');assert len(b['actual_tree_chain'])==len(segments)
 for i,(seg,link) in enumerate(zip(segments,b['actual_tree_chain'])):
  assert link['tree_sha']==current and link['name']==seg;e=direct[current][seg];assert(e['mode'],e['type'],e['sha'])==(link['mode'],link['type'],link['sha'])
  if i+1<len(segments):assert e['mode']=='040000' and e['type']=='tree';current=e['sha']
  else:assert(e['sha'],e['mode'],e['type'],e['size'])==(b['sha'],b['mode'],b['type'],b['bytes'])
oldplan=read(ROOT/'checkpoint-round17-prep/new-e-original-zip-payload/upload-plan.json');newplan=read(ROOT/'checkpoint-round19-prep/new-two-original-zip-payload/upload-plan.json')
registered={p['repository_path']:p for p in oldplan+newplan};parts=[b for b in bindings if b['role']!='original_zip_manifest'];manifests=[b for b in bindings if b['role']=='original_zip_manifest'];assert len(parts)==44 and len(manifests)==2
for b in parts:
 p=registered[b['repository_path']];assert(b['sha'],b['sha256'],b['bytes'])==(p['git_blob_sha1'],p['sha256'],p['bytes'])
man_expected={
 'artifacts/checkpoints/p8-original-e-a23bb72d-20261009/payload-manifest.json':ROOT/'checkpoint-round17-prep/new-e-original-zip-payload/payload-manifest.json',
 'artifacts/checkpoints/p8-original-e-a23bb72d-20261009/additional-50k-soak/payload-manifest.json':ROOT/'checkpoint-round19-prep/new-two-original-zip-payload/payload-manifest.json'}
for b in manifests:
 raw=man_expected[b['repository_path']].read_bytes();assert(blob(raw),sha(raw),len(raw))==(b['sha'],b['sha256'],b['bytes'])
arts=proof['original_artifacts'];assert {a['artifact_id'] for a in arts}=={11592751905,11591493782,11593165899,11592154271,11594089439,11591821446,11591607348};assert {p for a in arts for p in a['parts']}=={b['repository_path'] for b in parts}
checkrec(plan['original74_index']);old=read(resolved(plan['original74_index']['path']));assert old['file_count']==74;oldrows={r['repository_path']:r for r in old['files']}
assert len(plan['files'])==len({r['repository_path'] for r in plan['files']})==75
byte_rows=[]
for r in plan['files']:
 p=resolved(r['source_path']);raw=p.read_bytes();assert len(raw)==r['bytes'] and sha(raw)==r['sha256'] and blob(raw)==r['git_blob_sha1'] and r['mode']=='100644' and r['type']=='blob'
 if r['repository_path'] in oldrows:
  o=oldrows[r['repository_path']];assert (r['bytes'],r['sha256'],r['git_blob_sha1'],r['mode'])==(o['bytes'],o['sha256'],o['git_blob_sha1'],o['git_mode']);assert p==Path(o['local_path'])
 else:assert p==proofp
 byte_rows.append({'repository_path':r['repository_path'],'source_path':str(p),'bytes':len(raw),'sha256':sha(raw),'git_blob_sha1':blob(raw)})
assert sum(r['bytes'] for r in byte_rows)==plan['total_bytes']==7622127
checkrec(proof['old_pending_guard_preserved']);checkrec(proof['original_member_peer_review']);assert proof['complete_original_raw_publication'] is True and proof['actual_parts_bound']==44 and proof['actual_manifests_bound']==2
assert proof['formal_task_completion'] is False and proof['complete_deliverables_ready'] is False and plan['complete_deliverables_ready'] is False and plan['publication_status']=='upload_pending_for_these_benchmark_files'
receipt=read(B/'bind-780296-execution-receipt.json');assert receipt['actual_execution_exit_code']==0 and receipt['large_zip_reads']==0 and receipt['native_workloads_or_tests']==0
for r in receipt['inputs_outputs']:checkrec(r)
peer=Path('/dev/shm/a217aaae3bde/scale-combined-E-review/publication-binding-helper-scoped-independent-review.json');assert sha(peer.read_bytes())=='d39b3612e24b25715aa417febdbc374e16226706542ebe90e391ea85ec5168c4'
out={'schema':'benchmark75-original-raw-publication-binding-independent-review-v1','reviewer':'/root/pr_audit','reviewed_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'decision':'accepted_scoped_actual44_raw_binding_and_unchanged75_upload_plan','actual_publication_commit':proof['actual_publication_commit'],'actual_publication_tree':proof['actual_publication_tree'],'proof':{'path':str(proofp),'bytes':proofp.stat().st_size,'sha256':sha(proofp.read_bytes()),'git_blob_sha1':blob(proofp.read_bytes())},'upload_plan':{'path':str(planp),'sha256':sha(planp.read_bytes())},'actual_tree_validation':{'reconstructed_tree_count':47,'direct_official_ancestor_GET_captures':3,'official_recursive_subtree_GET_captures':1,'recursive_children_reconstructed_not_independent_GET':True,'part_bindings':44,'manifest_bindings':2,'all46ancestor_mode_type_OID_and_leaf_size_verified':True,'official_commit_matches_prior_independent_Git_object_reconstruction':True,'normal_ref_publication_receipt_bound':True},'original74_local_bytes_unchanged':True,'all75_original_file_bindings':byte_rows,'all75_bytes':7622127,'helper_and16_metadata_controls_peer_review':{'path':str(peer),'sha256':sha(peer.read_bytes())},'actual_binding_helper_execution_receipt':{'path':str(B/'bind-780296-execution-receipt.json'),'sha256':sha((B/'bind-780296-execution-receipt.json').read_bytes()),'exit_code':0,'execution_by_runtime_agent_not_this_reviewer':True},'publication_limits':['Original44raw parts and2manifests are actually published at780296; the75benchmark paths still require actualcandidate/refpublication.','Original74pending-publication guard and prior incomplete snapshots retain exact old bytes; this appended proof doesnotrewritehistory.','No largeZIP read,originalnative replay,source edit or remote mutation by this reviewer.','This review concerns custody/navigation only; original017/018 mapping andten-task harddependencies remain separate.'],'formal_task_completion':False,'task_counts':{'done':163,'remaining':29},'unresolved_scoped_blockers':[]}
p=Path('/dev/shm/a217aaae3bde/platform-review/round19-benchmark75-publication-binding-independent-review.json');p.write_text(json.dumps(out,ensure_ascii=False,indent=2)+'\n');print(json.dumps({'path':str(p),'bytes':p.stat().st_size,'sha256':sha(p.read_bytes())}))
