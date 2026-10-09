#!/usr/bin/env python3
"""Run unchanged original CLI against explicitly scoped prospective required files."""
import datetime,hashlib,json,os,pathlib,subprocess,sys,time
P=pathlib.Path('/workspace/scratch/a217aaae3bde/prospective-combined-tree-prep')
W=pathlib.Path('/dev/shm/a217aaae3bde/prospective-combined-tree-prep/checkout')
G=pathlib.Path('/dev/shm/a217aaae3bde/prospective-combined-tree-prep/git')
RUN=P/'prospective-original-v2-cli-attempt2'
RUN.mkdir(exist_ok=False)
def sha(raw):return hashlib.sha256(raw).hexdigest()
def ref(path):
    raw=path.read_bytes();return {'path':str(path),'bytes':len(raw),'sha256':sha(raw)}
def git(*args):return subprocess.check_output(['git',*args],cwd=W)
closure=json.loads((P/'historical-v2-complete-read-closure.json').read_bytes())
ready=json.loads((P/'required-read-projection-readiness.json').read_bytes())
assert not ready['remote_configured'] and not ready['runtime_fetch_needed_for_declared_closure']
assert not git('remote').strip()
for key in ['GIT_DIR','GIT_WORK_TREE','GIT_INDEX_FILE','GIT_OBJECT_DIRECTORY','GIT_ALTERNATE_OBJECT_DIRECTORIES']:
    assert key not in os.environ, ('unexpected ambient Git override',key)
assert not os.environ.get('PYTHONOPTIMIZE')
assert git('rev-parse','HEAD').decode().strip()=='29682890c89511dd6f477a6bf48bd969aa1537af'
assert pathlib.Path(git('rev-parse','--absolute-git-dir').decode().strip())==G
assert (W/'scripts/verify_historical_integrations_v2.py').resolve()==W/'scripts/verify_historical_integrations_v2.py'
assert sha((W/'scripts/verify_historical_integrations_v2.py').read_bytes())==closure['guard_source_sha256']
assert sha((W/'scripts/code_index_plan.py').read_bytes())==closure['plan_source_sha256']

def snapshot():
    rows=[]
    for expected in closure['current_files']:
        path=W/expected['path'];raw=path.read_bytes();mode='100755' if path.stat().st_mode&0o111 else '100644'
        current={'path':expected['path'],'bytes':len(raw),'mode':mode,'sha256':sha(raw),
          'git_blob':hashlib.sha1(b'blob '+str(len(raw)).encode()+b'\0'+raw).hexdigest()}
        assert all(current[k]==expected[k] for k in current)
        rows.append(current)
    requests=''.join(x['ref']+':'+x['path']+'\n' for x in closure['unique_git_blob_reads']).encode()
    result=subprocess.check_output(['git','cat-file','--batch-check'],cwd=W,input=requests)
    lines=result.decode().splitlines();assert len(lines)==2492
    assert all(len(s.split())==3 and s.split()[1]=='blob' for s in lines)
    for commit in closure['fixed_provenance_commits']:
        subprocess.check_call(['git','cat-file','-e',commit+'^{commit}'],cwd=W)
    refs=subprocess.run(['git','show-ref'],cwd=W,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
    assert refs.returncode in (0,1) and not refs.stderr
    return {'files':rows,'git_pairs_batch_check_sha256':sha(result),'git_pairs_resolved_count':len(lines),
      'all17_fixed_provenance_commits_present':True,'private_HEAD':git('rev-parse','HEAD').decode().strip(),
      'private_index_sha256':sha((G/'index').read_bytes()),'private_refs_output_sha256':sha(refs.stdout),
      'private_show_ref_exit':refs.returncode}

before=snapshot();(RUN/'inputs-before.json').write_text(json.dumps(before,indent=2)+'\n')
command=['python','-B','scripts/verify_historical_integrations_v2.py']
started=datetime.datetime.now(datetime.timezone.utc).isoformat();start_ns=time.monotonic_ns()
with (RUN/'stdout.log').open('wb') as stdout,(RUN/'stderr.log').open('wb') as stderr:
    completed=subprocess.run(command,cwd=W,stdout=stdout,stderr=stderr,check=False)
elapsed=time.monotonic_ns()-start_ns;finished=datetime.datetime.now(datetime.timezone.utc).isoformat()
after=snapshot();(RUN/'inputs-after.json').write_text(json.dumps(after,indent=2)+'\n');assert before==after
raw=(RUN/'stdout.log').read_bytes();lines=[json.loads(line) for line in raw.splitlines() if line.strip()]
assert len(lines)==2
plan,guard=lines
expected_limits={'full_P7':'open','V19':'open','quality_and_100k':'not_inherited','fault_runtime_tests':'not_run','evidence_content_acceptance':'not_granted_by_this_verifier'}
assert all(guard.get(k)==v for k,v in expected_limits.items())
record={'schema':'unchanged-historical-v2-prospective-required-projection-execution-v1',
 'scope':'Fixed296+f88 prospective required-file projection only; not actual merged main, not a complete combined checkout, not CI or task completion.',
 'command':command,'cwd':str(W),'actual_script_path':str(W/command[-1]),'environment':'Inherited unchanged; no Python/Git override or guard replacement added.',
 'start_utc':started,'finish_utc':finished,'elapsed_ns':elapsed,'actual_exit_code':completed.returncode,
 'status':'passed_scoped_prospective_required_projection' if completed.returncode==0 and guard.get('status')=='passed' else 'failed',
 'original_plan_output':plan,'original_guard_output':guard,'preserved_original_scope_limits':expected_limits,
 'prospective_inputs_tree':ready['prospective_tree'],'prospective_inputs_parents':ready['prospective_parents'],
 'actual_private_HEAD_and_index_are_base296':True,'private_HEAD':before['private_HEAD'],
 'complete_combined_checkout_materialized':False,'archive_file_paths_not_materialized':825,'missing_new_archive_unique_blobs':696,
 'missing_new_archive_unique_blob_bytes':88152246,'all_actual_declared_reads_available_before_execution':True,
 'current_input_files_before_after_identical':323,'fixed_git_pairs_before_after_identical':2492,
 'all17_fixed_refs_local_before_after':True,'remote_configured':False,'automatic_fetch_possible_via_configured_remote':False,
 'input_scope':ref(P/'historical-v2-complete-read-closure.json'),'input_readiness':ref(P/'required-read-projection-readiness.json'),
 'outputs':{name:ref(RUN/name) for name in ['stdout.log','stderr.log','inputs-before.json','inputs-after.json']},
 'runner':ref(pathlib.Path(__file__)),'original_guard_or_plan_modified':False,'task_statuses_changed':False,
 'counts':{'done':163,'remaining':29},'fresh_actual_main_tree_comparison_required_after_pr165_merge':True}
(RUN/'execution-review.json').write_text(json.dumps(record,sort_keys=True,indent=2)+'\n')
print(json.dumps(dict(ref(RUN/'execution-review.json'),actual_exit_code=completed.returncode,status=record['status'],elapsed_seconds=elapsed/1e9)))
