#!/usr/bin/env python3
"""Independent frozen-input and existing-control audit; no native execution."""
from pathlib import Path
import datetime,hashlib,importlib.util,json,os,re,subprocess,sys,unittest
OUT=Path(__file__).resolve().parent
BASE=OUT.parent.parent
SOURCE=BASE/'integration-validation/round14-final-source/source'
PRODUCT='4d971d143cd49ae3c1901816c0b7cee0a922977e'
D0='d0cb69c601e530dcef738af0c2dffb8d3b8bcf28'
M6='e95fd750d2f5052d0308c970cd5032c48b765cde'
ENV=os.environ|{'GIT_OPTIONAL_LOCKS':'0'}
def git(*args):return subprocess.check_output(['git','-C',str(SOURCE),*args],env=ENV).decode().strip()
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def read(p):return json.loads(p.read_text())
def identity(p):return {'path':str(p.relative_to(BASE)),'bytes':p.stat().st_size,'sha256':sha(p)}
checks=[]
def check(name,value,detail=None):
    checks.append({'name':name,'passed':bool(value),'detail':detail})
    if not value:raise AssertionError(name)
def write(p,value):p.write_text(json.dumps(value,sort_keys=True,indent=2)+'\n')

check('actual candidate fixed HEAD',git('rev-parse','HEAD')==PRODUCT)
check('candidate single parent is M6',git('show','-s','--format=%P',PRODUCT)==M6)
check('candidate tracked checkout clean',not git('status','--porcelain','--untracked-files=all'))
changed=git('diff-tree','--no-commit-id','--name-only','-r',PRODUCT).splitlines()
proposal=read(BASE/'acceptance-review/round13-final-source-applicability/proposal.json')
check('only eight requested original native files changed',sorted(changed)==sorted(x['path'] for x in proposal['native_differences']))
changes=[]
for name in changed:
    current=sha(SOURCE/name);bound=next(x for x in proposal['native_differences'] if x['path']==name)
    blob=git('rev-parse',PRODUCT+':'+name)
    d0blob=subprocess.check_output(['git','-C',str(BASE/'integration-validation/round12-D0-receiver-inputs/source'),'rev-parse',D0+':'+name],env=ENV,text=True).strip()
    check('exact D0 Git blob and file '+name,blob==d0blob and current==bound['D0_sha256'])
    changes.append({'path':name,'candidate_blob':blob,'D0_blob':d0blob,'sha256':current,'M6_sha256':bound['M6_sha256']})

expected=read(BASE/'acceptance-review/round12-D0-continuation-receiver/candidate-v5/original-registration.json')
helper_path=SOURCE/'scripts/p7_build_identity.py'
check('source helper original registered bytes',sha(helper_path)==expected['scale_observer_inputs']['scripts/p7_build_identity.py'])
spec=importlib.util.spec_from_file_location('round14_original_build_identity',helper_path)
helper=importlib.util.module_from_spec(spec);spec.loader.exec_module(helper)
snapshot=helper.source_snapshot(SOURCE)
check('entire tracked and live 1087 native domain equals D0',snapshot['input_count']==1087 and snapshot['inputs']==expected['complete_source_inputs'])
write(OUT/'candidate-native-source-snapshot.json',snapshot)
for name,digest in expected['scale_observer_inputs'].items():check('D0 scale observer preserved '+name,sha(SOURCE/name)==digest)
observer_paths=git('ls-tree','-r','--name-only',M6,'--','scripts','.github/workflows').splitlines()
rows=[]
for name in observer_paths:
    mode_blob=git('ls-tree',PRODUCT,'--',name).split('\t')[0]
    prior=git('ls-tree',M6,'--',name).split('\t')[0]
    p=SOURCE/name;data=p.read_bytes()
    actual_blob=hashlib.sha1(b'blob '+str(len(data)).encode()+b'\0'+data).hexdigest()
    if mode_blob!=prior or mode_blob.split()[-1]!=actual_blob:raise AssertionError('observer content differs '+name)
    rows.append({'path':name,'git_entry':mode_blob,'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()})
check('all observer, Python tests and workflows retain M6 bytes and modes',True,{'paths':len(rows)})
write(OUT/'candidate-M6-observer-domain.json',{'source_commit':PRODUCT,'original_observer_commit':M6,'files':rows})

oldroot=BASE/'runtime-review/round8-oracle-engineering/11581509438/files'
old=read(oldroot/'receipt.json');prior_review=read(BASE/'runtime-review/round8-oracle-engineering/native-controls-independent-review.json')
check('prior native receipt actual fixed D0 success',old['expected_source']==D0 and old['exit_code']==0 and old['source_unchanged'] is True and not old['not_run_command_indices'] and len(old['results'])==12)
for name,item in old['files'].items():
    p=oldroot/name;check('prior native original '+name,p.stat().st_size==item['bytes'] and sha(p)==item['sha256'])
check('prior native source domain identical to new candidate',read(oldroot/'source-before.json')['inputs']==snapshot['inputs']==read(oldroot/'source-after.json')['inputs'])
check('prior 12 commands are original successful executions',all(x['exit_code']==0 and x['argv']==old['commands'][i] for i,x in enumerate(old['results'])))
for i,command in enumerate(prior_review['commands']):
    check('prior independent command log identity '+str(i),sha(oldroot/old['results'][i]['stdout'])==command['stdout_sha256'] and sha(oldroot/old['results'][i]['stderr'])==command['stderr_sha256'])

first=read(OUT/'all-p8-receipt.json');completion=read(OUT/'fixture-completion-receipt.json')
check('failed sparse-fixture first run retained',first['tests_run']==390 and first['exit_code']==1 and sha(OUT/'all-p8.log')==first['log_sha256'])
check('fixture-only completion succeeded on same fixed inputs',completion['passed'] and completion['inputs_unchanged'] and completion['source_inputs_before']==first['inputs_after'] and completion['actual_HEAD']==completion['actual_HEAD_after']==PRODUCT)
check('only five plus nineteen original tests reran',[x['tests_run'] for x in completion['commands']]==[5,19])
for item in completion['commands']:check('supplement control log '+item['log'],sha(OUT/item['log'])==item['log_sha256'] and item['exit_code']==0 and item['standalone_OK'])
def outcomes(path):
    return [(m[2],m[3]) for m in re.finditer(r'^(test_\w+) \(([^()]+)\) \.\.\. (ok|ERROR)$',path.read_text(),re.M)]
initial=outcomes(OUT/'all-p8.log');supplement=sum((outcomes(OUT/x['log']) for x in completion['commands']),[])
first_ok={name for name,status in initial if status=='ok'};first_bad={name for name,status in initial if status=='ERROR'};supplement_ok={name for name,status in supplement if status=='ok'}
sys.path.insert(0,str(SOURCE/'scripts'))
suite=unittest.TestLoader().discover(str(SOURCE/'scripts/tests'),pattern='test_p8*.py')
def ids(suite):
    for item in suite:
        if isinstance(item,unittest.TestSuite):yield from ids(item)
        else:yield item.id()
expected_ids=list(ids(suite));passed=first_ok|supplement_ok
check('all 409 original named cases eventually passed exactly once',len(expected_ids)==len(set(expected_ids))==409 and len(first_ok)==385 and len(first_bad)==5 and len(supplement_ok)==24 and not first_ok&supplement_ok and passed==set(expected_ids) and first_bad<=supplement_ok)
write(OUT/'named-test-coverage.json',{'actual_source':PRODUCT,'initial_attempt_count':390,'initial_named_ok':sorted(first_ok),'initial_named_errors':sorted(first_bad),'initial_class_setup_error':'test_p8_release_review.ReleaseReviewTests','fixture_completion_ok':sorted(supplement_ok),'expected_original_test_ids':sorted(expected_ids),'all_expected_named_tests_passed':True,'successful_unique_names':len(passed),'first_failure_logs_retained':True})

plan=[
 {'scope':'P8-005/006 scale','reuse':'Exact D0 1087-native plus 2 scale-observer domain; original D0 binaries, plans, attempt ledger and validated raw remain D0.','required':'Finish the registered D0 original/continuation population under its original protocol; no new full scale matrix just for final composite metadata. Current coverage is incomplete, not certified.','limit':'Never validate old D0 binaries as a newly built 4d/final-commit executable or relabel run/source.'},
 {'scope':'P8-007 mixed','workflow':'p8-runtime.yml: mixed[1,4,8,16]','required':'Four actual final-source profiles, each original 900 offers/1000 files/500ms, own fresh receipt/build/CLI and verify, terminal rows, queue, resources, statistics and all15-table endpoint.','reason':'D0 dependency-window changes the incremental runtime path and timing/allocation/lock behavior.'},
 {'scope':'P8-007 held backfill','workflow':'p8-runtime.yml: fake_backfill','required':'Actual final-source semantic fresh-target 768 attempts =3seeds*2phases*4caps*32 with original 5s progress/2s query watchdogs, build receipt, raw and seal/verify.','reason':'Unchanged test invokes changed incremental indexer; old M6 success is historical.'},
 {'scope':'P8-010 one-hour cache soak','workflow':'p8-runtime.yml: soak','required':'One final-source actual 1h/3601 offers/1201 build/2400 compound read/C4/1000ms with original four cache quarters, resources, queue, mutation lineage,15-table parity and seal/verify.','reason':'Prior F5 to M6 success-path bridge cannot cross changed D0 reconcile production behavior; do not claim C4 actual peak4 when measured peak1.'},
 {'scope':'P8-011 incremental faults','workflow':'p8-platform.yml: recovery','required':'Final-source 3 local kill/busy/deletion cases plus original3seed active HTTP/cache/incarnation fault sequence. Existing workflow full-matrix also reruns other phases, without changing predicates.','qualified_reuse':'Seven unchanged lower-level cc-db/cc-semantic faults may retain their original scoped identity after crate/test/dependency proof.'},
 {'scope':'P8-012 platform','workflow':'p8-platform.yml: cold-build8 + complete-matrix','required':'Eight actual final-source Linux/macOS ×1.95/stable ×default/semantic fresh-target builds with real compiler,target,copy/binary,stdio,receipt. Reuse an already available exact D0-native equivalent only after full original receipt/domain verification.','limit':'One Linux D0 scale build or old599 eight-cell compile does not certify new code across all combinations.'},
 {'scope':'P8-008/009 lifecycle/resources','workflow':'p8-lifecycle.yml (existing automatic observation if triggered)','qualified_reuse':'599 cold(full=True), reopen and read/cache strata can support unchanged full-build/read behavior only with original source/binary/sample and numeric labels.','required_if_claiming_final_numbers':'New final-binary lifecycle measurements for numeric latency/RSS equivalence. New current build/runtime provide current executable footprint; old performance is not relabelled.','limit':'No blanket acceptance of old observations as the new binary; external callgraph bridge remains explicit.'},
 {'scope':'P8-013 deterministic gates','workflow':'p8-gates.yml','qualified_reuse':'Existing599 six gate raw results after exact reachable comparator/policy/lock/scorer and nearest-rank equivalence.','fallback':'Use existing final-head gate job to obtain six new cheap controls; no new large native performance run.'},
 {'scope':'P8-016 schema/cache rollback','workflow':'p8-platform.yml: recovery','qualified_reuse':'Existing599 schema25→24→25/backup and cache fallback only if original raw proves unchanged schema/cache/full rebuild paths and no pending dirty frontier on no-change reopen.','limit':'Do not extend bridge to affected incremental restore; run the affected final-source case if that prerequisite is unresolved.'},
 {'scope':'Final source proof and ordinary CI','required':'Actual final product/review/pins v15 source proof and standard CI at the final immutable head/tree; this local4d Python receipt is same-input evidence, not an already-published final-head run.','limit':'D0 exact native choice removes the M6 extra legacy percentile and cancellation-test changes. Same legacy percentile values do not certify the single-owner cleanup of P8-017; no full workspace/server-test pass is claimed from the old12-command subset.'}
]
write(OUT/'minimum-real-execution-plan.json',{'schema':'final-D0-native-M6-observer-required-executions-v1','reviewed_product':PRODUCT,'reviewed_tree':snapshot['source_tree'],'D0_original_source':D0,'M6_original_observer_source':M6,'profiles':plan,'workflows_unchanged':True,'new_runs_started_by_this_review':0,'TODO_newly_completed':0,'TODO_remaining':29})
report={'schema':'composite-source-non-author-integration-review-v1','at_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'verdict':'no_static_integration_blocker_with_explicit_scope_and_pending_final_native_observations','product':PRODUCT,'tree':snapshot['source_tree'],'parent':M6,'source_snapshot':identity(OUT/'candidate-native-source-snapshot.json'),'changes':changes,'observer_domain':identity(OUT/'candidate-M6-observer-domain.json'),'existing_native_evidence':{'execution_source':D0,'run':37846370300,'artifact':11581509438,'receipt':identity(oldroot/'receipt.json'),'commands':prior_review['commands'],'successful_test_executions':49,'unique_test_names':48,'ignored':prior_review['ignored'],'limitations':prior_review['limits'],'reuse_scope':'Identical native/test/Cargo inputs only; no new runtime/scale/platform numerical certification.'},'python_controls':{'source':PRODUCT,'original_first_failed_receipt':identity(OUT/'all-p8-receipt.json'),'fixture_completion':identity(OUT/'fixture-completion-receipt.json'),'coverage':identity(OUT/'named-test-coverage.json'),'successful_unique_original_names':409,'initial_sparse_fixture_errors_retained':True,'same_input_composition_not_fabricated_single_run':True},'execution_plan':identity(OUT/'minimum-real-execution-plan.json'),'code_assessment':{'oracle':'Only bounded INSERT tiers64/8/1 and64KiB pending payload batching; canonical values/projection/order/duplicates/15table and limits unchanged. 64KiB is not RSS bound; deferred SQL can change competing-fault precedence. No measured wall-speedup claimed.','dependency_window':'Initial budget+1 distinct sorted witnesses retained inside prepare, at mostbudget admitted; over-admission falls back to original pending SQL. Existing prepared epoch fence remains. New allocation/read lifetime and skipped SQL require new runtime observations.','statistics':'D0 reintroduces byte-equivalent local nearest-rank formulas at legacy callers; distribution/CI convention unchanged. Shared-owner cleanup and extra M6 tests are intentionally not transplanted.','project_session':'Differences only cfg(test) initializer witness and cancellation tests; release services_for_path path unchanged. Removed tests are not claimed present or passed.'},'checks':checks,'checks_passed':len(checks),'no_native_compilation_or_measurements':True,'new_TODO_completed':0,'remaining_TODO':29}
write(OUT/'independent-source-review.json',report)
print(json.dumps({'checks_passed':len(checks),'report':identity(OUT/'independent-source-review.json'),'plan':identity(OUT/'minimum-real-execution-plan.json'),'native_inputs':snapshot['input_count'],'observer_paths':len(rows),'successful_unique_original_Python_tests':len(passed)}))
