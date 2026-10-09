#!/usr/bin/env python3
"""Verify exact object/materialized-read availability without running either CLI."""
import hashlib,json,pathlib,subprocess,stat,datetime
P=pathlib.Path('/workspace/scratch/a217aaae3bde/prospective-combined-tree-prep')
R=pathlib.Path('/dev/shm/a217aaae3bde')
G=R/'prospective-combined-tree-prep/git';W=R/'prospective-combined-tree-prep/checkout'
def git(*args):return subprocess.check_output(['git','--git-dir='+str(G),*args])
def ref(path):
    b=path.read_bytes();return {'path':str(path),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
closure=json.loads((P/'historical-v2-complete-read-closure.json').read_bytes())
frontier=json.loads((P/'historical-provenance-frontier-state.json').read_bytes())
prep=json.loads((P/'private-base-preparation.json').read_bytes())
assert not frontier['frontier'] and not frontier['missing_blobs']
assert len(frontier['resolved'])==len(closure['unique_git_blob_reads'])==2492
commits=[]
for sha in closure['fixed_provenance_commits']:
    raw=git('cat-file','commit',sha)
    assert hashlib.sha1(b'commit '+str(len(raw)).encode()+b'\0'+raw).hexdigest()==sha
    commits.append({'commit':sha,'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest()})
blobs=[]
unique=sorted({e['blob'] for e in frontier['resolved']})
# Sequential requests keep even the largest preserved compressed archive bounded.
proc=subprocess.Popen(['git','--git-dir='+str(G),'cat-file','--batch'],stdin=subprocess.PIPE,stdout=subprocess.PIPE)
for sha in unique:
    proc.stdin.write((sha+'\n').encode());proc.stdin.flush()
    got,kind,size=proc.stdout.readline().decode().strip().split();size=int(size)
    assert got==sha and kind=='blob'
    object_hash=hashlib.sha1(b'blob '+str(size).encode()+b'\0');content_hash=hashlib.sha256();remaining=size
    while remaining:
        block=proc.stdout.read(min(1048576,remaining));assert block
        object_hash.update(block);content_hash.update(block);remaining-=len(block)
    assert proc.stdout.read(1)==b'\n' and object_hash.hexdigest()==sha
    blobs.append({'git_blob':sha,'bytes':size,'sha256':content_hash.hexdigest()})
proc.stdin.close();assert proc.wait()==0

for entry in closure['current_files']:
    path=W/entry['path'];current=W
    for part in pathlib.PurePosixPath(entry['path']).parts:
        current=current/part;mode=current.lstat().st_mode
        assert stat.S_ISREG(mode) if current==path else stat.S_ISDIR(mode)
    raw=path.read_bytes()
    assert len(raw)==entry['bytes'] and hashlib.sha256(raw).hexdigest()==entry['sha256']
    assert bool(path.stat().st_mode&0o111)==(entry['mode']=='100755')
assert not (W/closure['forbidden_path']).exists()
assert not (W/closure['forbidden_path']).is_symlink()
root=R/'codecortex'
actual_root={'HEAD':subprocess.check_output(['git','-C',str(root),'rev-parse','HEAD']).decode().strip(),
 'refs_sha256':hashlib.sha256(subprocess.check_output(['git','-C',str(root),'show-ref'])).hexdigest(),
 'index_sha256':hashlib.sha256((root/'.git/index').read_bytes()).hexdigest()}
assert actual_root==prep['root_fingerprint_before_after']
# Verify no local remote is configured: the prepared fixed reads require no fetch.
remote=subprocess.run(['git','--git-dir='+str(G),'remote'],text=True,capture_output=True,check=True)
assert not remote.stdout.strip()
record={'schema':'prospective-v2-required-read-materialization-readiness-v1',
 'prepared_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
 'status':'prepared_required_read_projection_only_waiting_actual_main_fix',
 'prospective_tree':prep['prospective_tree'],'prospective_parents':prep['prospective_parents'],
 'actual_private_HEAD_and_index_still':prep['private_HEAD_and_index'],
 'worktree_path':str(W),'git_dir':str(G),'root_fingerprint_unchanged':actual_root,
 'current_file_reads_materialized_and_verified':len(closure['current_files']),
 'unique_git_ref_path_reads_resolved':len(frontier['resolved']),
 'required_unique_git_blobs_streamed_and_hash_verified':len(blobs),
 'required_unique_git_blob_bytes':sum(e['bytes'] for e in blobs),
 'fixed_provenance_commits_verified':commits,'original_ensure_will_find_all17_commits':True,
 'remote_configured':False,'runtime_fetch_needed_for_declared_closure':False,
 'required_git_blobs':blobs,
 'original_CLI_executed':False,'original_plan_CLI_executed':False,
 'complete_combined_checkout_materialized':False,'new_archive_blobs_still_missing':696,
 'new_archive_paths_not_materialized':825,
 'untouched_scopes':['root HEAD/refs/index/tracked bytes','all fixed observers','all native measurement originals','all original acceptance budgets and criteria'],
 'source_records':{name:ref(P/name) for name in ['historical-v2-complete-read-closure.json','historical-provenance-frontier-state.json','historical-commit-reconstruction-receipt.json','forbidden-direct-tree-absence-addendum.json','private-base-preparation.json','binary-tool-capability-receipt.json']},
 'eventual_exact_command':['python','-B','scripts/verify_historical_integrations_v2.py'],
 'eventual_command_cwd':str(W),'execution_boundary':'Execute only after root fixes actual merged PR165 fresh main and compares actual combination tree and required reads. Use the script inside this materialized projection so __file__/ROOT bind here. Report projection scope explicitly; full combined checkout and ordinary CI remain external gates.',
 'task_statuses_changed':False,'counts':{'done':163,'remaining':29}}
out=P/'required-read-projection-readiness.json';out.write_text(json.dumps(record,sort_keys=True,indent=2)+'\n')
print(json.dumps(dict(ref(out),git_blobs=len(blobs),blob_bytes=sum(e['bytes'] for e in blobs))))
