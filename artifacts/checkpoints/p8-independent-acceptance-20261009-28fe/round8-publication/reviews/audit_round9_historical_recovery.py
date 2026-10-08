from pathlib import Path
import datetime,hashlib,json,subprocess
W=Path('/workspace/scratch/28fef0db5e01'); R=W/'codecortex'; O=W/'integration-validation/round8-pins5-source-proof'; H=W/'integration-validation/round9-historical-environment-recovery'
P='2e2b1e50775fc3df133c7fe72da1f9dc9d6bb55a'; T='fd3406cbd780b012567fc0071aee40059310019b'
def sha(x):return hashlib.sha256(x).hexdigest()
def git(*a,data=None):return subprocess.check_output(['git',*a],cwd=R,input=data)
def read(p):return json.loads(p.read_bytes())
def meta(p):b=p.read_bytes();return {'path':str(p.relative_to(W)),'bytes':len(b),'sha256':sha(b)}
f=read(O/'receipt.json'); m=read(H/'materialization-receipt.json'); q=read(H/'recheck-receipt.json')
assert sha((O/'receipt.json').read_bytes())=='380cf829be0b15f57ae2842e9bf7a8f42ee061aebd5d8d66d8cfa92689c552fc'
assert sha((H/'materialization-receipt.json').read_bytes())=='f670f6339113eae88771d21183924276d13b9079be9a40ac7c478d4570c06c2c'
assert sha((H/'recheck-receipt.json').read_bytes())=='f0d224defc21a56a75816fa994c98920e20f5f29fc4350a07f6eb56d09873aee'
assert f['status']=='failed' and not f['passed'] and f['source_unchanged']
assert f['head_before']==f['head_after']==P and f['tree_before']==f['tree_after']==T
assert f['inputs_before']==f['inputs_after'] and len(f['inputs_before'])==7
assert [c['exit_code'] for c in f['commands']]==[0,0,1,0,0,0]
for c in f['commands']:
 raw=(O/c['log']).read_bytes();assert len(raw)==c['log_bytes'] and sha(raw)==c['log_sha256']
assert 'Ran 181 tests' in (O/'00.log').read_text() and (O/'00.log').read_text().rstrip().endswith('OK')
assert 'No such file or directory' in (O/'02.log').read_text() and 'artifacts/diagnostics' in (O/'02.log').read_text()
identity={'head':git('rev-parse','HEAD').decode().strip(),'tree':git('rev-parse','HEAD^{tree}').decode().strip(),'index_entries_sha256':sha(git('ls-files','--stage','-z')),'inputs':{p:sha((R/p).read_bytes()) for p in f['inputs_before']}}
assert identity==m['source_before']==m['source_after']==q['source_before']==q['source_after']
assert identity['head']==P and identity['tree']==T and identity['inputs']==f['inputs_before']
for p,s in identity['inputs'].items():assert sha(git('show',P+':'+p))==s
refs=[('37dd042eaa1209a86e0cafdcd92ae77e036e76f5','docs/checkpoints/2026-10-03-packing-integration/source-manifest.json','evidence_files'),('88f2cf099c8b81f3acef485fd5ac9b01c63ce790','docs/checkpoints/2026-10-03-fixed-e3-integration/imported-identity.json','files')]
required={}; manifest_refs=[]
for ref,p,key in refs:
 raw=git('show',ref+':'+p);assert raw==git('show',P+':'+p)==(R/p).read_bytes(); j=json.loads(raw)
 manifest_refs.append(dict(path=p,original_ref=ref,bytes=len(raw),sha256=sha(raw),rows=len(j[key])))
 for row in j[key]:
  if row['path'] in required:assert required[row['path']]['sha256']==row['sha256']
  required[row['path']]=row
assert len(required)==m['required_unique_files']==300
entries={}
for raw in git('ls-tree','-r','-z',P,'--',*sorted(required)).split(b'\0'):
 if raw:
  a,p=raw.split(b'\t',1);mode,kind,oid=a.decode().split();entries[p.decode()]=(mode,kind,oid)
assert set(entries)==set(required)
body=git('cat-file','--batch',data=''.join(entries[p][2]+'\n' for p in sorted(required)).encode());pos=0; records=[]
missing={r['path']:r for r in m['missing_files']};assert len(missing)==96
for p in sorted(required):
 mode,kind,oid=entries[p];assert kind=='blob' and mode in ('100644','100755')
 end=body.index(b'\n',pos);a,k,n=body[pos:end].decode().split();n=int(n);raw=body[end+1:end+1+n];pos=end+1+n
 assert body[pos:pos+1]==b'\n';pos+=1
 assert a==oid and k=='blob' and sha(raw)==required[p]['sha256']
 if 'bytes' in required[p]:assert n==required[p]['bytes']
 target=R/p;assert target.is_file() and not target.is_symlink() and target.read_bytes()==raw
 row=dict(path=p,git_blob=oid,git_mode=mode,bytes=n,sha256=sha(raw)); records.append(dict(**row,materialized=p in missing))
 if p in missing:assert row==missing[p]
assert pos==len(body) and set(missing)<=set(required)
assert sum(r['bytes'] for r in records if r['materialized'])==m['materialized_bytes']==55745131
assert m['materialized_files']==96 and m['unchanged_existing_files']==204 and m['status']=='passed'
assert q['argv']==f['commands'][2]['argv']==['python3','-B','scripts/verify_historical_integrations_v2.py']
assert q['status']=='passed' and q['exit_code']==0 and q['cwd']==str(R)
assert q['original_failed_receipt_sha256']==sha((O/'receipt.json').read_bytes()) and q['original_failed_log_sha256']==sha((O/'02.log').read_bytes())
assert q['materialization_receipt_sha256']==sha((H/'materialization-receipt.json').read_bytes())
log=(H/'historical-recheck.log').read_bytes();assert len(log)==q['log_bytes'] and sha(log)==q['log_sha256']=='d1685bf9b6bb10d4c5ca03293983ac66e8cd191efc22fac6a895759d8844ff87'
parsed=[json.loads(x) for x in log.splitlines()];assert len(parsed)==2 and all(x['status']=='passed' for x in parsed)
assert parsed[0]['task_count']==192 and parsed[0]['states']=={'done':163,'blocked':1,'in_progress':16,'todo':12}
last=parsed[-1]; assert last['scope']=='historical_integrity_and_current_task_definitions_only' and last['evidence_content_acceptance']=='not_granted_by_this_verifier' and last['fault_runtime_tests']=='not_run' and last['quality_and_100k']=='not_inherited' and last['V19']==last['full_P7']=='open'
report=dict(schema_version=1,reviewer='/root/pr_audit',reviewed_at_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),verdict='accepted_composite_source_gate_evidence_with_original_failed_execution_preserved',source_identity=identity,original_execution={'receipt':meta(O/'receipt.json'),'status':'failed','exit_vector':[0,0,1,0,0,0],'passed_count':5,'failed_count':1,'source_integrity_tests':181,'all_six_original_log_hashes_verified':True,'failed_log':meta(O/'02.log'),'cause':'Sparse worktree lacked original tracked artifacts/diagnostics; original failure remains failed.'},restoration={'script':meta(H/'materialize_and_recheck.py'),'receipt':meta(H/'materialization-receipt.json'),'original_manifest_refs':manifest_refs,'unique_required':300,'recorded_newly_materialized':96,'recorded_existing_untouched':204,'newly_materialized_bytes':55745131,'all_300_current_bytes_equal_actual_PINS5_git_blobs_and_original_fixed_manifest_sha256':True,'all_96_receipt_paths_modes_blobs_sizes_hashes_independently_match':True,'source_and_index_entries_unchanged':True,'before_state_limit':'The reviewer did not observe the pre-materialization filesystem directly. Original failure, author before/after receipt and fail-on-existing-change script preserve that execution history; all 300 post-state bytes and all 96 recorded materializations were independently checked.','verified_files':records},separate_failed_command_recheck={'receipt':meta(H/'recheck-receipt.json'),'log':meta(H/'historical-recheck.log'),'argv':q['argv'],'exit_code':0,'elapsed_seconds':q['elapsed_seconds'],'started_at_utc':q['started_at_utc'],'finished_at_utc':q['finished_at_utc'],'source_identity_unchanged':True,'original_command_unchanged':True,'log_scope':last['scope']},conclusion={'all_six_source_gate_commands_have_successful_executions_at_identical_source_inputs':True,'single_original_six_command_pipeline_passed':False,'successful_commands_rerun_by_this_reviewer':False,'source_threshold_or_guard_changes':False,'runtime_acceptance':False,'native_soak_or_mixed_or_backfill_completion_not_claimed':True,'new_original_todo_completions':0,'tasks_total':192,'tasks_done':163,'tasks_remaining':29,'quality_and_100k':'not_inherited','fault_runtime_tests':'not_run','V19':'open','full_P7':'open'})
out=W/'pr-audit/round8-pins5-composite-proof-independent-review.json';out.write_text(json.dumps(report,indent=2,sort_keys=True)+'\n');print(json.dumps(meta(out)))
