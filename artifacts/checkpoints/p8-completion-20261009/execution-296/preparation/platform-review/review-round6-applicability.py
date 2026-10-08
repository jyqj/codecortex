from pathlib import Path
import ast,datetime,hashlib,json,subprocess
ROOT=Path('/dev/shm/a217aaae3bde/codecortex-round6-frozen');OLDROOT=Path('/dev/shm/a217aaae3bde/codecortex-round5-frozen');REPO=Path('/dev/shm/a217aaae3bde/codecortex');OUT=Path('/dev/shm/a217aaae3bde/platform-review/round6-execution-applicability-proof.json')
OLD='599a7050e7d52b5b7b93975c419138e175b3f754';NEW='29682890c89511dd6f477a6bf48bd969aa1537af';PRODUCT='3e3981163efd8def60b89dde150d5bd9e09fccf8';PREFIX='artifacts/checkpoints/p8-completion-20261009';CHANGES={'scripts/p8_runtime.py','scripts/tests/test_p8_runtime.py'}
def git(*a):return subprocess.check_output(['git',*a],cwd=REPO)
def check(x,m):
 if not x:raise AssertionError(m)
def sha(x):return hashlib.sha256(x).hexdigest()
def js(x):return (json.dumps(x,sort_keys=True,ensure_ascii=False,indent=2)+'\n').encode()
def tree(ref):
 d={}
 for line in git('ls-tree','-r','-z',ref).split(b'\0'):
  if line:
   meta,p=line.split(b'\t',1);d[p.decode()]=tuple(x.decode() for x in meta.split())
 return d
old,new=tree(OLD),tree(NEW)
core=json.loads(git('show','f25d0b994edf9c08c5514864701e3b050b317c88:'+PREFIX+'/independent-source-review.json'))
source=core['complete_inputs'];validation=core['validation_inputs'];inputs={**source,**validation};blob={}
for p in inputs:
 check(p in old and p in new,'missing input '+p)
 if p not in CHANGES:check(old[p]==new[p],'unexpected unchanged input '+p)
 raw=(ROOT/p).read_bytes();check(sha(raw)==inputs[p],'frozen disk input '+p);blob[p]=raw
 if p not in CHANGES:check((OLDROOT/p).read_bytes()==raw,'frozen old disk '+p)
check(len(source)==1087 and len(validation)==136,'counts')
check({p for p in inputs if old[p]!=new[p]}==CHANGES,'exact input diff')
canonical=sha(js(source));check(canonical=='ac7421155638a64b3690cb63d5434e562f1b0645f0145d298e9594ef62175a59','source canonical')
# Resolve static local imports from actual Python AST, including imports within functions.
mods={}
for p in validation:
 if p.startswith('scripts/') and p.endswith('.py'):
  name=p[len('scripts/'):-3].replace('/','.')
  if name.endswith('.__init__'):name=name[:-len('.__init__')]
  mods[name]=p
asts={p:ast.parse(raw,filename=p) for p,raw in blob.items() if p.startswith('scripts/') and p.endswith('.py')}
def literal(p,name):
 for n in asts[p].body:
  if isinstance(n,ast.Assign) and any(isinstance(x,ast.Name) and x.id==name for x in n.targets):return ast.literal_eval(n.value)
 raise AssertionError('missing declaration '+name)
def closure(roots):
 seen=set();pending=list(roots)
 while pending:
  p=pending.pop()
  if p in seen:continue
  check(p in blob,'observer not in complete validation '+p);seen.add(p)
  for n in ast.walk(asts[p]):
   names=[]
   if isinstance(n,ast.Import):names=[a.name for a in n.names]
   elif isinstance(n,ast.ImportFrom) and n.level==0 and n.module:names=[n.module]+[n.module+'.'+a.name for a in n.names]
   for name in names:
    if name in mods and mods[name] not in seen:pending.append(mods[name])
 return sorted(seen)
cold=list(literal('scripts/p8_cold_build.py','OBSERVER_MODULES'));full=cold+['scripts/p8_recovery.py','scripts/p7_build_identity.py','scripts/p7_fault_lifecycle_stdio.py'];life=['scripts/'+p for p in literal('scripts/p8_lifecycle.py','OBSERVER_FILES')];scale=list(literal('scripts/p8_scale_matrix.py','DRIVER_FILES'));runtime=list(literal('scripts/p8_runtime_build.py','OBSERVER_FILES'))
groups={
 'scale':dict(declared=scale,entry=['scripts/p8_scale_matrix.py'],workflow='.github/workflows/p8-scale.yml',tasks=['P8-005','P8-006'],status='source_applicable_only_matrix_still_requires_150_actual_original_shards'),
 'platform_cold':dict(declared=cold,entry=['scripts/p8_cold_build.py'],workflow='.github/workflows/p8-platform.yml',tasks=['P8-012'],status='previously_accepted_scoped_599_originals'),
 'recovery_and_version_pair':dict(declared=full,entry=['scripts/p8_recovery.py','scripts/p8_rollback.py'],workflow='.github/workflows/p8-platform.yml',tasks=['P8-011','P8-016'],status='previously_accepted_scoped_599_originals'),
 'lifecycle_resources':dict(declared=life,entry=['scripts/p8_lifecycle.py','scripts/p7_stdio_build_receipt.py'],workflow='.github/workflows/p8-lifecycle.yml',tasks=['P8-008','P8-009'],status='previously_accepted_scoped_599_originals'),
 'gates':dict(declared=[],entry=['scripts/p8_gate_controls.py'],workflow='.github/workflows/p8-gates.yml',tasks=['P8-013'],status='previously_accepted_scoped_599_originals'),
 'p7_engineering':dict(declared=[],entry=['scripts/p7_build_identity.py'],workflow='.github/workflows/p7-engineering.yml',tasks=['P8-011','P8-012','P8-016'],status='old_scoped_originals_retained_new_CI_regression_status_pending'),
 'p7_closeout_offline_mechanism':dict(declared=[],entry=['scripts/p7_fault_lifecycle_stdio.py','scripts/p7_mechanism_build.py','scripts/p7_offline_trace.py','scripts/p7_stdio_build_receipt.py'],workflow='.github/workflows/p7-closeout.yml',tasks=['P8-011','P8-016'],status='old_scoped_originals_retained_new_CI_regression_status_pending')}
for name,g in groups.items():
 paths=sorted(set(closure(g['entry']+g['declared']))|set(g['declared']));check(not CHANGES.intersection(paths),'changed observer in carry scope '+name)
 wf=g['workflow'];check(old[wf]==new[wf] and (OLDROOT/wf).read_bytes()==(ROOT/wf).read_bytes(),'workflow changed '+name)
 g['static_import_closure_and_declared_observers']={p:dict(sha256=sha(blob[p]),git_blob=new[p][2],git_mode=new[p][0],bytes=len(blob[p])) for p in paths};g['workflow_sha256']=sha((ROOT/wf).read_bytes());g['all_actual_declared_and_static_local_observers_byte_identical']=True;g['evidence_identity']=OLD
runtimechanges=[p for p in runtime if old[p]!=new[p]];check(runtimechanges==['scripts/p8_runtime.py'],'runtime9 delta')
originals={}
for name in ['platform-final-task-mapping.json','p7-closeout-originals-independent-review.json','p7-engineering-independent-review.json','recovery-independent-review.json','platform-eight-cells-independent-review.json','platform-collector-equivalence-review.json','lifecycle-independent-review.json','lifecycle-cargo-selection-addendum.json','gates-independent-review.json','fault-recovery-independent-review.json','ci-all-jobs-independent-review.json']:
 p=REPO/PREFIX/'execution-599'/name;raw=p.read_bytes();check(raw==git('show',NEW+':'+p.relative_to(REPO).as_posix()),'original review checkpoint');originals[name]=dict(sha256=sha(raw),bytes=len(raw),path=p.relative_to(REPO).as_posix())
meta=json.loads((OUT.parent/'round6-ci-review/initial-official-metadata.json').read_text());merge=meta['git/commits/452620b3aed27b7b61b56cae19d25ba315437b93'];head=meta['git/commits/'+NEW]
check(merge['tree']['sha']==head['tree']['sha']==git('rev-parse',NEW+'^{tree}').decode().strip(),'merge tree');check([p['sha'] for p in merge['parents']]==['7354db236c9d9850a75f31672697ae9eab44565e',NEW],'merge parents')
for root,ref in [(ROOT,NEW),(OLDROOT,OLD)]:check(subprocess.check_output(['git','rev-parse','HEAD'],cwd=root).decode().strip()==ref,'frozen head drift')
report=dict(schema_version=1,reviewer='/root/pr_audit',created_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),status='accepted_scoped_source_applicability',old_execution_head=OLD,new_product=PRODUCT,new_execution_head=NEW,new_execution_tree=head['tree']['sha'],source_identity=dict(complete_input_count=1087,all_git_modes_blobs_and_disk_bytes_equal=True,canonical_manifest_sha256=canonical,complete_inputs=source),validation_identity=dict(input_count=136,unchanged=134,changed=sorted(CHANGES),complete_new_inputs=validation),carry_forward_groups=groups,old_original_review_records=originals,runtime_and_backfill=dict(observer_count=len(runtime),observer_changes=runtimechanges,required_new_actual_observations=['mixed C1 900 offered','mixed C4 900 offered','mixed C8 900 offered','mixed C16 900 offered','soak 1000 files / 3601 offered / observed work >=3600 seconds','backfill original 3 seeds x C1/4/8/16 held and quiet provider workloads'],must_use_new_strict_build_receipt_source_and_observer=True,old599_receipts_not_valid_as_new_receipts=True),ci_merge_identity=dict(commit=merge['sha'],parents=[p['sha'] for p in merge['parents']],whole_tree=head['tree']['sha'],independent_official_GET=True,actual_job_checkout_log_review='pending'),boundary=['All old observations keep original599 source, producer, binary, runner and raw/seal identities. This proves applicable unchanged source/observer behavior, not a new296 execution.','Original validators remain strict and run against frozen599; never alter old receipts, seals, source_commit, observer snapshots or failure statuses to pass under296.','Platform observer manifest and p7 canonical source manifest use different original formats; their old digests are retained, not replaced.','Source identity cannot create absent samples, replace scale matrix completion, waive original dependencies, or turn an unavailable/failed measurement into success.','The changed runtime observer invalidates a claim of unchanged runtime/backfill execution; full new real runs are required.','This report grants no new benchmark candidate/release certification; any broad source lock remains bound to its original execution.','Current-head CI/P7 failures, if any, must be reviewed separately and cannot be waived by old-source applicability.'],method='Independent Git/disk digest equality of every1087 source and every136 validation input; AST-derived direct and transitive local import closure plus explicit declared observer modules; workflow byte equality; binds already-reviewed599 checkpoint records. No large raw archive re-extraction or product execution.',task_status_changes=[],task_counts=dict(done=163,remaining=29,total=192),unresolved_source_applicability_blockers=[])
OUT.write_bytes(js(report));print(json.dumps(dict(status=report['status'],path=str(OUT),sha256=sha(OUT.read_bytes()),groups={k:len(v['static_import_closure_and_declared_observers']) for k,v in groups.items()},remaining=29)))
