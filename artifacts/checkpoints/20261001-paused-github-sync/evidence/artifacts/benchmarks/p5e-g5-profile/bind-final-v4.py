from pathlib import Path
import json,hashlib
R=Path.cwd();O=R/'artifacts/benchmarks/p5e-g5-profile/final-v4';O.mkdir(exist_ok=False)
def load(p):return json.loads((R/p).read_text())
def row(p):
 q=R/p
 return {'path':p,'bytes':q.stat().st_size,'sha256':hashlib.sha256(q.read_bytes()).hexdigest()}
M='artifacts/benchmarks/p5e-harness-20261001/final-v7/FINAL-HARNESS-INPUT-MANIFEST.json';P='artifacts/benchmarks/p5e-formal-plan-20261001-v3/FINAL-RUN-PLAN.json';m=load(M);plan=load(P);locks=[]
def verify(x):
 if isinstance(x,dict):
  if all(k in x for k in ['path','bytes','sha256']):assert row(x['path'])=={k:x[k] for k in ['path','bytes','sha256']},x['path'];locks.append(x)
  for v in x.values():verify(v)
 elif isinstance(x,list):
  for v in x:verify(v)
verify(m);verify(plan['binaries']);assert len(plan['sequence'])==31
source=load('artifacts/benchmarks/p5e-candidate-20261001/final-source-v4/source-manifest.json')
for x in source['files']:assert row(x['path'])==x
old='artifacts/benchmarks/p5e-g5-profile/final-v3/profile.json';o=load(old);o['schema_version']=9;o['profile']='G5-M2-local-engineering-task-quality-final-v4';o['harness_input_closure']=row(M);o['formal_run_plan']=row(P);o['execution_sequence']=plan['sequence'];o['process_environment']=plan['process_env'];o['failure_policy']=plan['failure_policy'];o['replay_policy']=plan['replay_policy'];o['owned_wrapper']=plan['wrapper'];o['actual_witness_reference']=m['short_control_actual_evidence'];o['premeasurement_compile_reuse']=m['reused_real_fresh_compile_closure'];o['old_premeasurement_failure']=m['original_failure'];o['registered_control_fixture_version']=plan['control_fixture_version']
o['ablation']['build_binding']='Reuse only the independently audited8actual fresh independentemptytarget builds from prior premeasurement attempt/source580 exact3patch/options/binaries. Copy/postcheck immutable compiledclosure without compilation or performance claims; then execute all9 new inlinefileliteral actualwitness/full111candidate equivalence before any measurement. No oldstale/sharedtarget binary or oldfailedemptybody witness accepted.'
o['ablation']['execution_conditions']='Stage0 verifies/copies exactly the correct fresh8compile closed receipts;Stage1 all9 actual3factor/sourceproof/foundation/freshscope/normalwait+full111policy-score-body-proof MUSTPASS beforeStage2 measurements. Any failure stops; previousno-bodyalloff input/wait2 preserved; purpose-only inlinepath functionbody marker731/source added as lex/grep fallback, no production/51gold/facet/guard/budget changes.'
o['pending_before_acceptance']=['explicit independent approval binding final-v4 profileSHA/newv7manifest/v3runplan/newinlineinput/same4binary/source/core/exactreusecompile/wrapper','all31 registered stages: all9 new actualwitness then originalfullscope quality/cost/concurrency/rawreplays/native resources/posthash;no compile overlap/bestof/statefilter','independent110span/noanswer/perquerynegativecause/fullP5-019/020 audit;original strictRED and previousfailure immutable;no automatic completion']
o['registration_amendment_before_measurement']={'prior_profile':row(old),'reason':'stage1 alloff sourcebody NoMatch for path query whiledisabledmechanism correct;actualhardstop before anymeasurement; newpurpose-only samefunctioninlinepath literal permits sourcevalid lexical/grep fallback, allotherinput/model/threshold/control/source/gold unchanged','measurement_started_in_prior_attempt':False,'new_plan_sha256':'2d1131ae0c45d8984335601ea6db38184264ba1e7a31ecea680fe74b41c7f522','prior_compile_reuse_only':True,'all9_actual_witness_rerun_required':True}
o['immutable_lock_occurrences_verified_by_owner']=len(locks);p=O/'profile.json';p.write_text(json.dumps(o,ensure_ascii=False,indent=2)+'\n');print(json.dumps(row(str(p.relative_to(R))),indent=2));print('locks',len(locks))
