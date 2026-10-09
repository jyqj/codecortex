from pathlib import Path
import json,hashlib,subprocess,datetime,difflib
P=Path('/workspace/scratch/a217aaae3bde/prospective-combined-tree-prep');W=Path('/dev/shm/a217aaae3bde/prospective-combined-tree-prep/checkout');D=P/'prospective-original-v2-cli-attempt2'
O=Path('/dev/shm/a217aaae3bde/platform-review');sha=lambda x:hashlib.sha256(x).hexdigest()
def ref(path):
 raw=path.read_bytes();return {'path':str(path),'bytes':len(raw),'sha256':sha(raw)}
r=json.loads((D/'execution-review.json').read_bytes());c=json.loads((P/'historical-v2-complete-read-closure.json').read_bytes());b=json.loads((D/'inputs-before.json').read_bytes());a=json.loads((D/'inputs-after.json').read_bytes())
assert (D/'inputs-before.json').read_bytes()==(D/'inputs-after.json').read_bytes() and b==a
for row in list(r['outputs'].values())+[r['runner'],r['input_scope'],r['input_readiness']]:
 raw=Path(row['path']).read_bytes();assert len(raw)==row['bytes'] and sha(raw)==row['sha256'],row['path']
assert len(b['files'])==323 and len(c['current_files'])==323
assert [x['path'] for x in b['files']]==[x['path'] for x in c['current_files']]
for row,expected in zip(b['files'],c['current_files']):
 assert all(row[k]==expected[k] for k in row)
 path=W/row['path'];raw=path.read_bytes();assert len(raw)==row['bytes'] and sha(raw)==row['sha256'] and hashlib.sha1(b'blob '+str(len(raw)).encode()+b'\0'+raw).hexdigest()==row['git_blob']
 assert ('100755' if path.stat().st_mode&0o111 else '100644')==row['mode']
requests=''.join(x['ref']+':'+x['path']+'\n' for x in c['unique_git_blob_reads']).encode()
actual=subprocess.check_output(['git','cat-file','--batch-check'],cwd=W,input=requests)
assert len(actual.splitlines())==2492 and sha(actual)==b['git_pairs_batch_check_sha256']
assert all(len(x.split())==3 and x.split()[1]==b'blob' for x in actual.splitlines())
for commit in c['fixed_provenance_commits']:subprocess.check_call(['git','cat-file','-e',commit+'^{commit}'],cwd=W)
assert len(c['fixed_provenance_commits'])==17
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=W).decode().strip()==r['private_HEAD']=='29682890c89511dd6f477a6bf48bd969aa1537af'
G=Path(subprocess.check_output(['git','rev-parse','--absolute-git-dir'],cwd=W).decode().strip())
assert sha((G/'index').read_bytes())==b['private_index_sha256']
assert not subprocess.check_output(['git','remote'],cwd=W).strip()
refs=subprocess.run(['git','show-ref'],cwd=W,capture_output=True);assert refs.returncode==1 and not refs.stdout and not refs.stderr
lines=[json.loads(x) for x in (D/'stdout.log').read_bytes().splitlines() if x.strip()]
assert len(lines)==2 and lines[0]==r['original_plan_output'] and lines[1]==r['original_guard_output']
assert not (D/'stderr.log').read_bytes()
assert r['actual_exit_code']==0 and lines[0]['status']==lines[1]['status']=='passed'
assert r['command']==['python','-B','scripts/verify_historical_integrations_v2.py'] and r['cwd']==str(W)
source=[]
for path,key in [('scripts/verify_historical_integrations_v2.py','guard_source_sha256'),('scripts/code_index_plan.py','plan_source_sha256')]:
 raw=(W/path).read_bytes();assert sha(raw)==c[key]
 original=subprocess.check_output(['git','show',r['private_HEAD']+':'+path],cwd=W);assert raw==original
 source.append({'path':path,'sha256':sha(raw),'equal_to_commit':r['private_HEAD']})
pre=json.loads((P/'prospective-original-v2-cli/preflight-failure.json').read_bytes());assert sha(Path(pre['original_wrapper_path']).read_bytes())==pre['original_wrapper_sha256'];assert not (P/'prospective-original-v2-cli/stdout.log').exists() and not (P/'prospective-original-v2-cli/stderr.log').exists()
assert pre['original_guard_invocation_count']==0
old=Path(pre['original_wrapper_path']).read_text();new=Path(r['runner']['path']).read_text();delta='\n'.join(difflib.unified_diff(old.splitlines(),new.splitlines(),fromfile='preserved-preflight1',tofile='actual-attempt2'))+'\n'
(O/'prospective-original-v2-wrapper-only-diff.patch').write_text(delta)
limits={'full_P7':'open','V19':'open','quality_and_100k':'not_inherited','fault_runtime_tests':'not_run','evidence_content_acceptance':'not_granted_by_this_verifier'}
assert all(lines[1][k]==v for k,v in limits.items())
assert lines[0]['states']=={'blocked':1,'done':163,'in_progress':16,'todo':12} and lines[0]['task_count']==192
review={'schema':'independent-prospective-original-v2-execution-review-v1','reviewer':'/root/pr_audit','reviewed_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'status':'accepted_scoped_prospective_required_file_projection','method':'Read raw original output and external runner; independently hash all bound records, reread all 323 declared current files/modes and resolve 2492 Git blob reads and 17 fixed commits; no CLI re-execution. Reuse separately independently checked complete static read closure and direct tree FORBIDDEN absence proof.','actual_execution':{'record':ref(D/'execution-review.json'),'exit_code':r['actual_exit_code'],'elapsed_ns':r['elapsed_ns'],'command':r['command'],'cwd':r['cwd'],'stdout':ref(D/'stdout.log'),'stderr':ref(D/'stderr.log')},'source':source,'identity':{'private_HEAD_and_index_base':r['private_HEAD'],'declared_prospective_inputs_tree':r['prospective_inputs_tree'],'prospective_parents':r['prospective_inputs_parents'],'complete_combined_checkout':False,'actual_merged_main':False,'configured_remotes':False},'inputs':{'before':ref(D/'inputs-before.json'),'after':ref(D/'inputs-after.json'),'snapshot_bytes_identical':True,'current_files':323,'git_read_pairs':2492,'fixed_provenance_commits':17,'current_reread_matches_snapshots':True,'current_Git_read_pairs_digest_matches_snapshots':True,'private_index_unchanged':True,'prior_independent_read_closure_review':ref(O/'historical-v2-read-closure-independent-review.json'),'readiness':ref(P/'required-read-projection-readiness.json')},'preserved_scope_limits':limits,'preflight_failure':{'record':ref(P/'prospective-original-v2-cli/preflight-failure.json'),'original_guard_invocations':0,'failure':'External wrapper treated empty detached-repository show-ref exit1 as fatal before guard invocation.','reviewed_wrapper_changes_only':['Change output directory to retain first attempt.','Accept show-ref exit0 or exit1 with empty stderr and record actual exit.'],'diff':ref(O/'prospective-original-v2-wrapper-only-diff.patch'),'guard_plan_or_input_change':False},'review_probe_note':'An initial read-only independent comparison compared entire closure rows to execution rows and stopped because closure additionally contains explanatory reasons. Corrected comparison uses all five actual file identity fields; no guard invocation occurred in either independent review probe.','acceptance_boundaries':['825 new archive paths are not materialized; 696 new archive blobs totalling 88152246 bytes are outside the independently enumerated read closure. This is not a full combined checkout or CI result.','All original guard limitations remain. Historical state_changes_since_baseline does not indicate newly completed TODOs.','After actual PR165 merge, compare fresh actual main and prospective combined tree/read inputs. Equivalent inputs permit reusing this narrowly scoped check; actual normal CI and required merge gates remain required.','No code, workflow, Git refs, root index, task statuses or thresholds modified; no CLI or tests rerun by this reviewer.'],'formal_task_completion':False,'task_counts':{'total':192,'done':163,'remaining':29},'unresolved_blockers_in_reviewed_scope':[],'still_pending_outside_scope':['Actual PR165 outstanding CI/P7 jobs.','296 mechanism regression job.','Fresh actual main and actual PR159 merge eligibility.','Full 599 scale matrix and ten-task original dependency closure.']}
out=O/'prospective-original-v2-execution-independent-review.json';out.write_text(json.dumps(review,ensure_ascii=False,indent=2)+'\n');print(json.dumps(ref(out)))
