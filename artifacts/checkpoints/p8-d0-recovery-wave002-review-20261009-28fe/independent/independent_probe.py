"""Independent mutations of frozen controller interfaces; no native/network execution."""
import ast,copy,hashlib,json,os,subprocess,sys,tempfile,types
from pathlib import Path
from unittest import mock
ROOT=Path('/workspace/scratch/28fef0db5e01')
P=ROOT/'recovery-audit/round14-wave002/candidate/artifacts/checkpoints/p8-d0-recovery-wave002-20261009-28fe'
module=types.ModuleType('reviewed_author_fixture');module.__file__=str(P/'test_recovery.py')
exec(compile((P/'test_recovery.py').read_bytes(),module.__file__,'exec'),module.__dict__)
r=module.r
checks=[]
def check(name,action,expected_error=None):
 try:
  value=action()
  accepted=True;error=None
 except Exception as e:
  accepted=False;error={'type':type(e).__name__,'message':str(e)}
 passed=accepted if expected_error is None else not accepted and error['type']=='ValueError' and expected_error in error['message']
 checks.append({'name':name,'passed':passed,'expected':expected_error or 'success','actual_error':error})
 if not passed:print('UNEXPECTED',checks[-1])

def observe(change=None,environment=None):
 fixture=module.SyntheticCurrentAPIControls();fixture.setUp()
 old,mapping,api,current=fixture.fixture()
 if change:change(mapping,current,fixture)
 env={'GITHUB_REPOSITORY':r.REPO,'GITHUB_REF_NAME':r.BRANCH,'GITHUB_EVENT_NAME':'push','GITHUB_RUN_ATTEMPT':'1','GITHUB_RUN_ID':str(current['id']),'GITHUB_SHA':current['head_sha']}
 if environment:env.update(environment)
 with mock.patch.dict(os.environ,env):value=r.observe(fixture.reg,fixture.initial,old,api)
 return value,mapping,current,fixture
check('full_actual_originals_and_synthetic_current_observation',lambda:observe())
check('wrong_runtime_repository',lambda:observe(environment={'GITHUB_REPOSITORY':'someone/codecortex'}),'wrong repository/branch/event')
check('workflow_attempt2',lambda:observe(environment={'GITHUB_RUN_ATTEMPT':'2'}),'wrong repository/branch/event')
check('measured_source_cannot_be_controller',lambda:observe(environment={'GITHUB_SHA':r.SOURCE}),'invalid or reused controller')
def unknown_job(m,c,f):
 jobs=m[f'/actions/runs/{r.ORIGINAL_RUN}/attempts/1/jobs']['jobs'];x=copy.deepcopy(jobs[0]);x.update(id=999999,name='unregistered native attempt');jobs.append(x)
check('unknown_original_job_is_not_silently_ignored',lambda:observe(unknown_job),'unknown original workflow job')
def original_identity(m,c,f):m[f'/actions/runs/{r.ORIGINAL_RUN}']['id']=1
check('wrong_original_run_endpoint_body',lambda:observe(original_identity),'original run')
def original_attempt(m,c,f):m[f'/actions/runs/{r.ORIGINAL_RUN}']['run_attempt']=2
check('original_rerun_not_reset',lambda:observe(original_attempt),'original run')
def old_controller(m,c,f):m[f'/actions/runs/{r.WAVE001_RUN}']['head_sha']='0'*40
check('old_wave_head_not_substituted',lambda:observe(old_controller),'run identity/rerun')
def unknown_artifact(m,c,f):
 rows=m[f'/actions/runs/{r.ORIGINAL_RUN}/artifacts']['artifacts'];extra=copy.deepcopy(rows[0]);extra.update(id=987654,name='unregistered-raw');rows.append(extra)
check('unknown_original_artifact',lambda:observe(unknown_artifact),'unknown original artifact')
def build_digest(m,c,f):next(x for x in m[f'/actions/runs/{r.BUILD_RUN}/artifacts']['artifacts'] if x['id']==r.BUILD_ID)['digest']='sha256:'+'0'*64
check('fixed_native_build_digest_changed',lambda:observe(build_digest),'build artifact identity changed')
def build_expired(m,c,f):next(x for x in m[f'/actions/runs/{r.BUILD_RUN}/artifacts']['artifacts'] if x['id']==r.BUILD_ID)['expired']=True
check('fixed_native_build_unavailable',lambda:observe(build_expired),'build artifact identity changed')
def late_worker(m,c,f):m['/actions/jobs/113580044384/logs']+=b'\n{"worker_exit_code":1}\n'
check('late_nonzero_native_log_even_if_regex_incomplete',lambda:observe(late_worker),'late original rep8 log')
# Load the exact separately reviewed receiver bytes and test the real controller
# observation schema against its successful envelope entry point. Phase success
# metadata below is explicit synthetic data, not a fabricated native run.
receiver_dir=ROOT/'acceptance-review/round12-D0-continuation-receiver/candidate-v6'
sys.path.insert(0,str(receiver_dir))
rx=types.ModuleType('receiver_v6_cross_interface');rx.__file__=str(receiver_dir/'receiver.py')
exec(compile((receiver_dir/'receiver.py').read_bytes(),rx.__file__,'exec'),rx.__dict__)
sys.path.pop(0)
value,mapping,current,fixture=observe()
wave={'wave_id':'wave-002','scale':100000,'repetition':8,'ordinal':2,'run_id':current['id'],'controller_commit':current['head_sha'],'registration_sha256':hashlib.sha256((P/'registration.json').read_bytes()).hexdigest(),'reviewed_controller_sha256':hashlib.sha256((P/'recovery.py').read_bytes()).hexdigest(),'reviewed_workflow_sha256':hashlib.sha256((P.parents[2]/'.github/workflows/p8-d0-recovery.yml').read_bytes()).hexdigest()}
with tempfile.TemporaryDirectory(prefix='codecortex-28fe-wave002-envelope-',dir='/dev/shm') as td:
 out=Path(td)
 for folder in ['admission','execution','capacity','recheck']:(out/('p8-d0-recovery-'+folder)).mkdir()
 for folder,phase,status in [('admission','admit','admitted_wave_only'),('execution','execute','supplemental_raw_validated_pending_external_attempt_readback'),('recheck','recheck','predecessor_still_eligible')]:
  record={'phase':phase,'status':status,'exit_code':0,'coverage_accepted':False,'registration_sha256':wave['registration_sha256'],'controller_script_sha256':wave['reviewed_controller_sha256'],'measured_source':r.SOURCE,'wave_id':'wave-002','wave':fixture.reg['wave'],'original_driver_exit_code':0,'original_raw_validation':True,'previous_admission_revoked':False}
  record['pre_execution_observation' if folder=='execution' else 'observation']=value
  (out/('p8-d0-recovery-'+folder)/'receipt.json').write_text(json.dumps(record)+'\n')
 actual={'waves':[{'run':current,'jobs_by_attempt':[{'jobs':mapping[f'/actions/runs/{current["id"]}/attempts/1/jobs']['jobs']}]}]}
 check('actual_controller_observation_is_compatible_with_frozen_receiver_v6',lambda:rx.validate_supplement_envelope(out,wave,fixture.reg,actual))
 path=out/'p8-d0-recovery-execution/receipt.json';bad=json.loads(path.read_text());bad['original_driver_exit_code']=2;path.write_text(json.dumps(bad))
 check('nonzero_native_driver_cannot_be_excused_by_prior_control_class',lambda:rx.validate_supplement_envelope(out,wave,fixture.reg,actual),'original execution/recheck failure')
# Cross-check functions controlling original execution, authentication, original
# artifact validation and raw driver are literally AST-identical to wave001.
old=ast.parse((P/'prior-wave001-controller.py').read_bytes());new=ast.parse((P/'recovery.py').read_bytes())
methods=lambda t:{x.name:ast.dump(x,include_attributes=False) for x in t.body if isinstance(x,(ast.FunctionDef,ast.ClassDef))}
a,b=methods(old),methods(new)
for name in ('main','API','NoRedirect','interruption','validate_original_run','original_job_map','validate_artifacts','initial_map','strict_json'):
 check('unchanged_AST_'+name,lambda name=name:None if a[name]==b[name] else (_ for _ in ()).throw(ValueError('AST changed')))
result={'scope':'Independent API/identity/late-error and exact real-controller-output-to-receiver-v6 interface controls. Future phase success is explicitly synthetic; no API/network/native/SDK deletion executed.','controller_sha256':hashlib.sha256((P/'recovery.py').read_bytes()).hexdigest(),'receiver_v6_sha256':hashlib.sha256((receiver_dir/'receiver.py').read_bytes()).hexdigest(),'results':checks,'passed':sum(x['passed'] for x in checks),'total':len(checks)}
print(json.dumps(result,indent=2))
raise SystemExit(0 if result['passed']==result['total'] else 1)
