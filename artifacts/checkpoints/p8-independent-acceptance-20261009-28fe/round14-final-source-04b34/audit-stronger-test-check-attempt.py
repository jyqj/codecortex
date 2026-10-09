import os,sys,json,hashlib,subprocess,copy,datetime
from pathlib import Path
ROOT=Path.cwd(); META=ROOT/'integration-validation/round14-final-source/integration-objects.git'
os.environ.update(GIT_DIR=str(META),GIT_OPTIONAL_LOCKS='0',GIT_NO_LAZY_FETCH='1',GIT_TERMINAL_PROMPT='0',PYTHONDONTWRITEBYTECODE='1')
sys.path.insert(0,str(ROOT/'integration-validation/round14-final-source/source/scripts'))
import verify_reviewed_source_v15 as guard
import v15_historical_context as history
BASE='7354db236c9d9850a75f31672697ae9eab44565e';PRODUCT='04b34ec76fd33d21fd897fb992d87514f6a2013d';TREE='3d73ccc6685e2f96f74281826707f4adbfab7822';M6='e95fd750d2f5052d0308c970cd5032c48b765cde';LOCAL='4d971d143cd49ae3c1901816c0b7cee0a922977e';D0='d0cb69c601e530dcef738af0c2dffb8d3b8bcf28';MAIN='55902428d49b09bb4a90cee6ccccf985680618bb'
out=ROOT/'runtime-review/round14-actual-source-review';out.mkdir(exist_ok=True)
def run(*args):return subprocess.check_output(['git','--git-dir='+str(META),*args])
def sha(x):return hashlib.sha256(x).hexdigest()
def blob(ref,p):return run('show',ref+':'+p)
def tree(ref,*roots):
 rows={}
 for line in run('ls-tree','-r','-z',ref,'--',*roots).split(b'\0'):
  if not line:continue
  meta,p=line.split(b'\t',1);m,k,o=meta.decode().split();rows[p.decode()]={'mode':m,'kind':k,'oid':o}
 return rows
def hash_blobs(rows):
 unique=sorted({x['oid'] for x in rows.values()});p=subprocess.Popen(['git','--git-dir='+str(META),'cat-file','--batch'],stdin=subprocess.PIPE,stdout=subprocess.PIPE)
 result={}
 for oid in unique:
  p.stdin.write((oid+'\n').encode());p.stdin.flush();header=p.stdout.readline().decode().split();assert len(header)==3 and header[0]==oid and header[1]=='blob'
  data=p.stdout.read(int(header[2]));assert len(data)==int(header[2]) and p.stdout.read(1)==b'\n';result[oid]=sha(data)
 p.stdin.close();assert p.wait()==0
 return {path:result[row['oid']] for path,row in rows.items()}
checks=[]
def check(name,condition,detail=None):
 if not condition:raise AssertionError(name)
 checks.append({'name':name,'passed':True,**({'detail':detail} if detail is not None else {})})
api_path=ROOT/'integration-validation/round14-final-source/api/round14_product_actual_commit_readback.json'
api=json.loads(api_path.read_text());commit=json.loads(api['structuredContent']['content'])
check('official_actual_commit_tree_and_ordered_parents',commit['sha']==PRODUCT and commit['tree']['sha']==TREE and [x['sha'] for x in commit['parents']]==[M6,MAIN])
check('actual_tree_object_available',run('cat-file','-t',TREE).strip()==b'tree')
domains=('crates','Cargo.toml','Cargo.lock')
native=tree(TREE,*domains);base_native=tree(BASE,*domains);local_native=tree(LOCAL,*domains);m6_native=tree(M6,*domains)
check('full_1087_native_domain_same_as_executed4d',len(native)==1087 and native==local_native)
check('native_regular_modes',all(x['kind']=='blob' and x['mode'] in ('100644','100755') for x in native.values()))
check('original_verifier_native_inventory_equal',set(native)==guard.git.inputs(TREE))
check('no_inherited_native_input_deleted',set(base_native)<=set(native))
complete=hash_blobs(native)
d0git=ROOT/'integration-validation/round12-D0-receiver-inputs/source'
d0_rows={}
for line in subprocess.check_output(['git','-C',str(d0git),'ls-tree','-r','-z','HEAD','--',*domains],env={k:v for k,v in os.environ.items() if k!='GIT_DIR'}).split(b'\0'):
 if line:
  md,p=line.split(b'\t',1);m,k,o=md.decode().split();d0_rows[p.decode()]={'mode':m,'kind':k,'oid':o}
d0head=subprocess.check_output(['git','-C',str(d0git),'rev-parse','HEAD'],env={k:v for k,v in os.environ.items() if k!='GIT_DIR'}).decode().strip()
check('full_1087_native_domain_matches_actual_fixed_D0',d0head==D0 and native==d0_rows)
validation=guard.validation_inventory(TREE)
all_val=tree(TREE,*guard.VALIDATION_ROOTS);m6_val=tree(M6,*guard.VALIDATION_ROOTS);local_val=tree(LOCAL,*guard.VALIDATION_ROOTS)
filtered=lambda rows:{p:x for p,x in rows.items() if guard.validation_path(p)}
check('complete_validation_domain_same_as_executed4d',filtered(all_val)==filtered(local_val))
check('complete_validation_domain_same_as_M6',filtered(all_val)==filtered(m6_val))
check('complete_validation_hashes_same_as_original_verifier',validation==hash_blobs(filtered(all_val)))
changes=run('diff','--name-only',BASE,TREE,'--',*domains).decode().splitlines()
delta={}
for p in changes:delta[p]={'before_sha256':sha(blob(BASE,p)) if p in base_native else None,'sha256':complete[p]}
check('cumulative_delta_exact_no_new_exclusions',bool(changes) and set(changes)=={p for p in native if base_native.get(p)!=native[p]})
vs_m6=run('diff','--name-only',M6,TREE,'--',*domains).decode().splitlines()
prior_local=json.loads((ROOT/'runtime-review/round14-final-source/independent-source-review.json').read_text())
check('exact_eight_native_changes_reviewed_on4d',vs_m6==[x['path'] for x in prior_local['changes']] and len(vs_m6)==8)
fixtures=json.loads((ROOT/'integration-validation/round14-final-source/original-fixture-materialization-receipt.json').read_text())
for f in fixtures['files']:check('unchanged_real_test_fixture:'+f['path'],sha(blob(TREE,f['path']))==f['sha256'] and len(blob(TREE,f['path']))==f['bytes'])
check('CI_selector_byte_identical_to_original_guard_expectation',blob(TREE,'.github/workflows/ci.yml').decode()==guard.expected_ci())
check('P7_engineering_workflow_unchanged',blob(TREE,'.github/workflows/p7-engineering.yml')==blob(BASE,'.github/workflows/p7-engineering.yml'))
for p in guard.FROZEN:check('original_v14_frozen:'+p,blob(TREE,p)==blob(BASE,p))
# Derive the original historical protected path set without materializing any archived body.
names=set(tree(BASE,*history.VALIDATION_ROOTS));record_paths=set(history.EXPLICIT_RECORDS)
for p in history.REGISTRIES:
 data=json.loads(blob(BASE,p));record_paths.update(data.get('records',{}));record_paths.update(x['review_path'] for x in data.get('deltas',{}).values())
names.update(record_paths)
protected=record_paths|{p for p in names if p.startswith('tests/source_integrity/') or p.startswith('scripts/source_snapshots/') or (p.startswith('scripts/') and Path(p).name.startswith(('verify_','current-source-','reviewed-source-','v14_historical_'))) or p=='scripts/p0_historical_corpus.py'}
for p in sorted(names):history.relative(p)
full_product=tree(TREE);full_base=tree(BASE)
changed_protected=[p for p in protected if full_product.get(p)!=full_base.get(p)]
check('all_original_historical_protection_blobs_and_modes_unchanged',not changed_protected,{'catalogue_paths':len(names),'protected_paths':len(protected)})
oldpath='artifacts/checkpoints/p8-completion-20261009/independent-source-review.json';oldraw=blob(TREE,oldpath);old=json.loads(oldraw)
check('prior_v15_review_is_original_M6_record',old['source']==guard.PRODUCT and old['base']==BASE and old['tree']==run('rev-parse',guard.PRODUCT+'^{tree}').decode().strip())
check('prior_complete_native_record_matches_original_product',old['complete_inputs']==hash_blobs(tree(guard.PRODUCT,*domains)))
check('prior_validation_record_matches_original_product',old['validation_inputs']==guard.validation_inventory(guard.PRODUCT))
# Verify tests remain actually bound to 4d rather than rewriting the run's commit identity.
coverage=json.loads((ROOT/'runtime-review/round14-final-source/named-test-coverage.json').read_text())
check('409_named_test_coverage_preserved',coverage['actual_source']==LOCAL and coverage['successful_unique_names']==409 and len(coverage['expected_original_test_ids'])==409 and len(coverage['initial_named_ok'])==385 and len(coverage['fixture_completion_ok'])==24 and not set(coverage['initial_named_ok'])&set(coverage['fixture_completion_ok']) and set(coverage['initial_named_ok'])|set(coverage['fixture_completion_ok'])==set(coverage['expected_original_test_ids']) and coverage['first_failure_logs_retained'] is True)
first=json.loads((ROOT/'runtime-review/round14-final-source/all-p8-receipt.json').read_text());completed=json.loads((ROOT/'runtime-review/round14-final-source/fixture-completion-receipt.json').read_text())
check('actual_python_execution_identity_and_input_hashes_preserved',first['actual_HEAD_before']==first['actual_HEAD_after']==completed['actual_HEAD']==completed['actual_HEAD_after']==LOCAL and first['inputs_before']==first['inputs_after']==completed['source_inputs_before']==completed['source_inputs_after'] and all(validation.get(p)==digest for p,digest in first['inputs_before'].items()))
check('initial_sparse_fixture_failure_not_rewritten',first['exit_code']==1 and first['tests_run']==390 and first['standalone_OK'] is False and completed['passed'] is True and [x['tests_run'] for x in completed['commands']]==[5,19] and all(x['exit_code']==0 for x in completed['commands']))
check('original_first_failed_log_exact',sha((ROOT/'runtime-review/round14-final-source/all-p8.log').read_bytes())==first['log_sha256'])
for command in completed['commands']:check('original_followup_log_exact:'+command['log'],sha((ROOT/'runtime-review/round14-final-source'/command['log']).read_bytes())==command['log_sha256'])
binding={'schema_version':1,'reviewer':'/root/runtime_review','source':PRODUCT,'tree':TREE,'base':BASE,'ordered_parents':[M6,MAIN],'at_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'verdict':'accepted_scoped','scope':'independently_reviewed_source_and_validation_inputs','checks':checks,'checks_passed':len(checks),'domains':{'complete_native_inputs':len(native),'cumulative_delta_inputs':len(delta),'complete_validation_inputs':len(validation),'historical_protection_inputs':len(protected)},'actual_API_readback':{'path':str(api_path.relative_to(ROOT)),'bytes':api_path.stat().st_size,'sha256':sha(api_path.read_bytes())},'local_Git_commit_object_present':subprocess.run(['git','--git-dir='+str(META),'cat-file','-e',PRODUCT+'^{commit}'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode==0,'git_read_scope':'Official API binds product commit, ordered parents and tree; local immutable tree/blobs independently read in metadata-only Git view. No branch or worktree change.','actual_tests_source':LOCAL,'source_domain_bridge':'All 1087 native and all original verifier validation inputs are byte/mode identical between actual product tree and tested4d; 3 restored original fixtures identical too. Original first sparse-fixture failure and 24 follow-up executions retained.','old_core_sha256':sha(oldraw),'TODO_completed':0,'TODO_remaining':29}
(out/'final-product-binding-review.json').write_text(json.dumps(binding,indent=2,sort_keys=True)+'\n')
(out/'prior-M6-independent-source-review.json').write_bytes(oldraw)
(out/'product-api-readback.json').write_bytes(api_path.read_bytes())
(out/'calculated-v15-domains.json').write_text(json.dumps({'source':PRODUCT,'tree':TREE,'base':BASE,'paths':delta,'complete_inputs':complete,'validation_inputs':validation},indent=2,sort_keys=True)+'\n')
print(json.dumps({'checks':len(checks),'native':len(native),'delta':len(delta),'validation':len(validation),'protected':len(protected),'old_review_sha256':sha(oldraw),'python_controls':prior_local['python_controls']},indent=2))
