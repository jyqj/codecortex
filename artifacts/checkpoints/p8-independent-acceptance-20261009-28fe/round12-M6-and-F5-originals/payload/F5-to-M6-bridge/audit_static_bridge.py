#!/usr/bin/env python3
"""Read immutable Git objects and already completed controls; no product execution."""
import ast, copy, datetime, hashlib, json, re, subprocess
from pathlib import Path

OUT = Path(__file__).resolve().parent
BASE = OUT.parent.parent
REPO = BASE / 'codecortex'
F5 = 'c2ad27b2b189cbc98775f20a550d718dd4913038'
M6 = 'e95fd750d2f5052d0308c970cd5032c48b765cde'
def git(*args, data=None): return subprocess.check_output(['git', *args], cwd=REPO, input=data)
def blob(commit, path): return git('show', commit + ':' + path)
def sha(data): return hashlib.sha256(data).hexdigest()
def jb(value): return (json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + '\n').encode()
def save(name, value): (OUT / name).write_bytes(jb(value))
def artifact(path):
    data=Path(path).read_bytes()
    return {'path': str(path), 'bytes': len(data), 'sha256': sha(data)}
def tree(commit, *paths):
    entries={}
    for entry in git('ls-tree','-r','-z',commit,'--',*paths).split(b'\0'):
        if entry:
            meta, path=entry.split(b'\t',1); mode,kind,oid=meta.decode().split()
            assert kind=='blob' and mode in ('100644','100755')
            entries[path.decode()]={'mode':mode,'git_blob':oid}
    return entries
checks=[]
def check(name, condition, details=None):
    checks.append({'name':name,'passed':bool(condition),'details':details})
    assert condition, name
for filename, expected in json.loads((OUT/'api-byte-lengths.json').read_text()).items():
    data=(OUT/filename).read_bytes(); length=expected['bytes']
    if len(data)==length+1 and data.endswith(b'\n'): (OUT/filename).write_bytes(data[:-1])
    check('exact API UTF8 bytes: '+filename, (OUT/filename).stat().st_size==length)
run=json.loads((OUT/'run-api.json').read_text())
jobs=json.loads((OUT/'jobs-api.json').read_text())
check('official F5 run and head',run['id']==37858536446 and run['head_sha']==F5 and run['run_attempt']==1)
job=next(j for j in jobs['jobs'] if j['id']==113588467257)
step=next(s for s in job['steps'] if s['name']=='One-hour actual stdio session with Git switches and catalog churn')
check('actual hour step started, not queued timestamp',step['started_at']=='2026-10-09T00:00:32Z' and step['status']=='in_progress')
native_old=tree(F5,'crates','Cargo.toml','Cargo.lock'); native_new=tree(M6,'crates','Cargo.toml','Cargo.lock')
check('all 1087 native inputs byte-identical',len(native_old)==1087 and native_old==native_new)
oids=list(dict.fromkeys(v['git_blob'] for v in native_old.values()))
batch=git('cat-file','--batch',data=('\n'.join(oids)+'\n').encode()); offset=0; hashes={}
for oid in oids:
    end=batch.index(b'\n',offset); actual,kind,size=batch[offset:end].decode().split(); size=int(size)
    check('native batch object '+oid,actual==oid and kind=='blob')
    offset=end+1; data=batch[offset:offset+size]; offset+=size
    assert batch[offset:offset+1]==b'\n'; offset+=1
    hashes[oid]={'sha256':sha(data),'bytes':len(data)}
assert offset==len(batch)
native={p:dict(v,**hashes[v['git_blob']]) for p,v in native_old.items()}
save('identical-native-1087-manifest.json',native)
observer_ast=ast.parse(blob(M6,'scripts/p8_runtime_build.py'))
observer_paths=next(ast.literal_eval(n.value) for n in observer_ast.body if isinstance(n,ast.Assign) and any(isinstance(t,ast.Name) and t.id=='OBSERVER_FILES' for t in n.targets))
observers={}
for path in observer_paths:
    a,b=blob(F5,path),blob(M6,path)
    observers[path]={'F5':{'sha256':sha(a),'bytes':len(a)},'M6':{'sha256':sha(b),'bytes':len(b)},'equal':a==b}
check('only runtime observer differs',len(observers)==9 and [p for p,v in observers.items() if not v['equal']]==['scripts/p8_runtime.py'])
save('runtime-observer-comparison.json',observers)
old_bytes=blob(F5,'scripts/p8_runtime.py'); new_bytes=blob(M6,'scripts/p8_runtime.py')
(OUT/'F5-p8_runtime.py').write_bytes(old_bytes); (OUT/'M6-p8_runtime.py').write_bytes(new_bytes)
(OUT/'runtime-exact.diff').write_bytes(git('diff',F5,M6,'--','scripts/p8_runtime.py'))
old,new=ast.parse(old_bytes),ast.parse(new_bytes)
defs_old={n.name:n for n in old.body if isinstance(n,(ast.FunctionDef,ast.ClassDef))}
defs_new={n.name:n for n in new.body if isinstance(n,(ast.FunctionDef,ast.ClassDef))}
same=lambda a,b:ast.dump(a,include_attributes=False)==ast.dump(b,include_attributes=False)
check('only one new definition',set(defs_new)-set(defs_old)=={'finalize_artifacts'} and not set(defs_old)-set(defs_new))
changed=[name for name in defs_old if not same(defs_old[name],defs_new[name])]
check('only run definition modified',changed==['run'])
old_run=defs_old['run']; final=next(n for n in ast.walk(old_run) if isinstance(n,ast.Try) and any(isinstance(x,ast.Assign) and ast.unparse(x).startswith("report['elapsed_ns']") for x in n.finalbody))
original_tail=final.finalbody[-2:]
check('old tail exactly report write then conditional seal verify',ast.unparse(ast.Module(body=original_tail,type_ignores=[]))=="write_json(out / 'report.json', report)\nif writers_stopped:\n    seal_output(out)\n    verify_output(out)")
normalized=copy.deepcopy(new); replacements=0
for n in ast.walk(normalized):
    if isinstance(n,ast.Try):
        for i,x in enumerate(n.finalbody):
            if isinstance(x,ast.Expr) and isinstance(x.value,ast.Call) and isinstance(x.value.func,ast.Name) and x.value.func.id=='finalize_artifacts':
                check('exact new call arguments',ast.unparse(x)=='finalize_artifacts(out, report, writers_stopped)')
                n.finalbody[i:i+1]=copy.deepcopy(original_tail); replacements+=1
normalized.body=[n for n in normalized.body if not isinstance(n,ast.FunctionDef) or n.name!='finalize_artifacts']
for n in normalized.body:
    if isinstance(n,ast.ImportFrom) and n.module=='p8_cold_build': n.names=[x for x in n.names if x.name!='json_bytes']
check('entire module AST equals F5 after exact finalization substitution',replacements==1 and same(old,normalized))
cold=ast.parse(blob(M6,'scripts/p8_cold_build.py')); cold_defs={n.name:n for n in cold.body if isinstance(n,ast.FunctionDef)}
check('write_json is exactly Path.write_bytes(json_bytes(value))',ast.unparse(cold_defs['write_json'].body[0])=='Path(path).write_bytes(json_bytes(value))')
save('definition-comparison.json',{'unchanged_definitions':sorted(set(defs_old)-{'run'}),'changed':['run'],'added':['finalize_artifacts'],'normalized_entire_module_equal':True,
    'success_effect_order':['serialize report with identical json_bytes','write identical report bytes','if writers stopped: seal_output(output)','if writers stopped: verify_output(output)','return original report'],
    'new_failure_path_only':'seal/verify Exception or KeyboardInterrupt records failed exit2, original pre-seal report and original failed seal without claiming archive success'})
workflow='.github/workflows/p8-runtime.yml'
check('original hour workflow identical',blob(F5,workflow)==blob(M6,workflow))
(OUT/'original-p8-runtime.yml').write_bytes(blob(F5,workflow))
changes=git('diff','--name-status',F5,M6,'--','scripts','crates','Cargo.toml','Cargo.lock','.github/workflows').decode().splitlines()
check('complete runtime/native/workflow change scope',set(line.split('\t')[-1] for line in changes)=={'scripts/p8_runtime.py','scripts/tests/test_p8_runtime_finalization.py','scripts/reviewed-source-registry-v15.json','scripts/verify_reviewed_source_v15.py'})
save('code-scope-diff.json',{'F5':F5,'M6':M6,'changes':changes,'diff':git('diff',F5,M6,'--','scripts/reviewed-source-registry-v15.json','scripts/verify_reviewed_source_v15.py').decode()})
control_dir=BASE/'integration-validation/round9-finalization-m6'
receipt=json.loads((control_dir/'all-p8-receipt.json').read_text()); log=(control_dir/'all-p8.log').read_bytes()
check('existing 409 controls original log digest',len(log)==receipt['log_bytes'] and sha(log)==receipt['log_sha256'])
check('409 controls actual exit and inputs',receipt['exit_code']==0 and receipt['inputs_before']==receipt['inputs_after'] and re.search(rb'^Ran 409 tests in [0-9.]+s$',log,re.M) is not None and re.search(rb'^OK$',log,re.M) is not None)
for path,want in receipt['inputs_before'].items(): check('executed staged control input equals M6 '+path,sha(blob(M6,path))==want)
for path in ['scripts/tests/test_p8_runtime.py','scripts/tests/test_p8_runtime_cache.py']:
    check('old original regression input unchanged '+path,blob(F5,path)==blob(M6,path))
review_dir=BASE/'acceptance-review/round9-seal-finalization/controls'
cr=json.loads((review_dir/'v2-candidate-all-runtime.receipt.json').read_text())
for suffix in ['stdout','stderr']:
    check('independent original runtime control '+suffix,sha((review_dir/('v2-candidate-all-runtime.'+suffix)).read_bytes())==cr[suffix+'_sha256'])
check('independent runtime candidate exit0',cr['exit_code']==0 and cr['runtime_sha256']==sha(new_bytes))
new_tests=ast.parse(blob(M6,'scripts/tests/test_p8_runtime_finalization.py'))
test_names=[n.name for n in ast.walk(new_tests) if isinstance(n,ast.FunctionDef) and n.name.startswith('test_')]
check('seven finalization regressions preserved',len(test_names)==7)
save('existing-control-readback.json',{'new_executions':0,'full_controls':dict(receipt,original_receipt=artifact(control_dir/'all-p8-receipt.json'),original_log=artifact(control_dir/'all-p8.log')),
    'independent_runtime_controls':dict(cr,original_receipt=artifact(review_dir/'v2-candidate-all-runtime.receipt.json'),stderr=artifact(review_dir/'v2-candidate-all-runtime.stderr')),'seven_new_test_names':test_names})
report={'schema_version':1,'kind':'conditional_success_path_evidence_bridge_static_review','created_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),
 'source':{'F5':F5,'F5_tree':git('rev-parse',F5+'^{tree}').decode().strip(),'M6':M6,'M6_tree':git('rev-parse',M6+'^{tree}').decode().strip()},
 'verdict':'ELIGIBLE_CONDITIONAL_SUCCESS_PATH_BRIDGE; no hour acceptance is granted by this static review',
 'basis':{'native_inputs_identical':1087,'observers_identical':8,'observer_changed_only':'scripts/p8_runtime.py finalization failure retention',
          'all_prior_module_AST_identical_after_exact_finalization_substitution':True,'native_binary_equality_claimed':False,'workflow_identical':True,
          'existing_staged_controls_verified':409,'new_finalization_controls':7,'new_executions':0},
 'official_run_snapshot':{'run':37858536446,'attempt':1,'head':F5,'soak_job':113588467257,'job_status':job['status'],
       'hour_step_status':step['status'],'hour_step_started_at':step['started_at'],'terminal_evidence_available_in_this_review':False},
 'bridge_allowed_only_after_originals_review':[
   'Official F5 head/run/job/artifact identity and API ZIP SHA verified; keep source F5, do not relabel execution M6.',
   'Original successful fresh-target build proof: native 1087 and observer 9 source-before/after; actual compiler, feature/profile, three Cargo executable/copy bindings; complete build seal.',
   'Original runtime CLI exit0, passed_observation report exit0, original verify exit0 and complete stable seal all required. A pre-seal report or verify0 alone is insufficient.',
   'Original 3601 offered IDs each terminal exactly once: 1201 build plus 2400 compound reads, four original RPCs per read, 1000 files/C4/1000ms and >=3600s actual offered span; no clipping or pooling.',
   'All original four RPC requests/responses validated against raw and wire logs, PID/generation/preceding mutation/source bytes/currentness; miss/refill/hit and invalidation in all four actual-completion time quarters; same shared query-pool counters, not individual OS-thread identity.',
   'Full mutation/switch/catalog churn, original RSS trend and <=5s complete sample coverage, original raw/process/resource budgets and queue/outcome records required.',
   'Complete no-repair fifteen-table endpoint parity, original Rust statistics raw replay and all-owned-writers stopped before seal must independently pass.'
 ],
 'excluded_claims':[
   'F5 cannot certify the new M6 seal/verify exception retention path; existing seven negative controls and M6 successful mixed/backfill evidence remain distinct.',
   'Not same-checkout/same-job current-build receipt substitution: never feed F5 receipt as M6 receipt or change original source/observer/binary identity.',
   'No claim that compiled binary bytes are equal solely because native source is equal; each build must retain its own exact executable proof.',
   'Local M6 missing session/report/seal remains incomplete, and its observed resource gaps remain failures of that prefix; a separate GHA execution does not erase or average them.',
   'This may support unchanged long-duration/cache behavioral subgates only, not original P8-010 hard dependency P8-009, complete M4-semantic or individual-worker identity, scale study, release certification, or any TODO closure.'
 ],
 'local_M6_loss_report':artifact(BASE/'runtime-review/round12-m6-session-loss/interruption-observation.json'),
 'checks':checks,'checks_passed':len(checks),'checks_failed':0,'todo':{'original_completed_new':0,'remaining':29}}
save('conditional-success-bridge-review.json',report)
save('evidence-manifest.json',{p.name:artifact(p) for p in sorted(OUT.iterdir()) if p.is_file() and p.name!='evidence-manifest.json'})
print(json.dumps({'report':artifact(OUT/'conditional-success-bridge-review.json'),'checks':len(checks),'native_inputs':len(native),'status':report['verdict']}))
