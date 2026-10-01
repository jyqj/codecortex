from pathlib import Path
import json,hashlib
R=Path.cwd();O=R/'artifacts/benchmarks/p5e-g5-profile/final-v2';O.mkdir(exist_ok=False)
def load(p):return json.loads((R/p).read_text())
def row(p):
 q=R/p
 return {'path':p,'bytes':q.stat().st_size,'sha256':hashlib.sha256(q.read_bytes()).hexdigest()}
M='artifacts/benchmarks/p5e-harness-20261001/final-v6/FINAL-HARNESS-INPUT-MANIFEST.json';P='artifacts/benchmarks/p5e-formal-plan-20261001-v2/FINAL-RUN-PLAN.json';m=load(M);plan=load(P)
locks=[]
def verify(obj):
 if isinstance(obj,dict):
  if all(k in obj for k in ['path','bytes','sha256']):
   assert row(obj['path'])=={k:obj[k] for k in ['path','bytes','sha256']},obj['path'];locks.append(obj)
  for v in obj.values():verify(v)
 elif isinstance(obj,list):
  for v in obj:verify(v)
verify(m);verify(plan['binaries']);assert len(plan['sequence'])==31
source=load('artifacts/benchmarks/p5e-candidate-20261001/final-source-v4/source-manifest.json')
for x in source['files']:
 assert row(x['path'])==x
core=load('artifacts/benchmarks/p5e-g5-20261001/core-v5/validation.json');a=load('artifacts/benchmarks/p5e-g5-20261001/core-v5/audit.json');t=load('artifacts/benchmarks/p5e-g5-20261001/core-v5/owner-tool-terminal.json')
assert core['status']=='passed_current_engineering_core_only_not_G5' and len(core['commands'])==21 and t['result']['exit_code']==0 and a['status']=='passed_independent_current_engineering_core_only_not_G5'
o=load('artifacts/benchmarks/p5e-g5-profile/proposed-source-v4-profile.json');o['schema_version']=8;o['profile']='G5-M2-local-engineering-task-quality-final-v2';o['status']='immutable_preregistered_pending_explicit_independent_approved_to_execute_not_G5_acceptance';o['registered_before_final_candidate_runs']=True
cr=m['producer_receipts'][0];rec=load(cr['path']);o['candidate']={'source_digest_sha256':source['source_digest_sha256'],'archive_sha256':source['archive_sha256'],'source_files':629,'producer_receipt':cr,'binary':plan['binaries']['candidate'],'options':rec['options'],'profile':rec['profile'],'features':rec['features'],'rustc':rec['rustc'],'cargo':rec['cargo'],'cold_build_claim':False}
o['engineering_core']={k:row('artifacts/benchmarks/p5e-g5-20261001/core-v5/'+p) for k,p in [('validation','validation.json'),('independent_audit','audit.json'),('actual_tool_terminal','owner-tool-terminal.json')]};o['engineering_core']['scope']='21 whole new commands only; accepted_tasks=[]; no old14+7 splice'
o['harness_input_closure']=row(M);o['formal_run_plan']=row(P);o['native_metrics_producer']=m['producer_receipts'][1];o['native_metrics_binary']=plan['binaries']['metrics'];o['harness_producer']=m['producer_receipts'][2];o['harness_binary']=plan['binaries']['harness'];o['execution_sequence']=plan['sequence'];o['process_environment']=plan['process_env'];o['failure_policy']=plan['failure_policy'];o['replay_policy']=plan['replay_policy'];o['missing_offline_obligations']=plan['missing_offline_obligations'];o['owned_wrapper']=plan['wrapper'];o['baseline_cost_rule']=plan['baseline_cost_rule'];o['count_lock']=plan['count_lock'];o['actual_witness_reference']=m['short_control_actual_evidence'];o['source_scoring_versions']={'path':'canonical-scoped-exact-path-domain-token-fallback-v3','query_policy':'query-policy-local-semantic-canonical-path-domain-v4','packing':'whole-json-intent-facets-source-support-before-incidental-v5','cache':'canonical-scoped-exact-path-domain-compact-context-budget-v22','legacy_packing':'v2/v3/v4 readable without changing their status'}
o['ablation']['expected_dataset_edges']=48;o['ablation']['controls']=['path lane','exact-symbol lane','intent-aware facet reservation vs Locate-policy control'];o['ablation']['factor3_scope']=plan['factor3'];o['ablation']['execution_conditions']='Stage0 builds8 new independentlyemptytargets. Stage1 actual eachcell3controls/sourceproof/foundation/freshscope/childexit and full111candidate normalized body/proof/score/policy equality MUSTPASS before Stage2 measurements; any failure stops, all oldraw retained.'
o['pending_before_acceptance']=['explicit independent approval binding exact final-v2 profileSHA/source/binaries/manifest/runplan/wrapper','all31 new registered stage evidence actual fresh8build/witness/quality/facet/graph/fanout/mixed/rawreplay/resources/posthash, no parallelcompile/performance/no bestof','independent110span/noanswer/perquerynegativecause review and wholeP5-019/020 originalscope audit; no automatic done from scripts']
o['immutable_lock_occurrences_verified_by_owner']=len(locks)
p=O/'profile.json';p.write_text(json.dumps(o,ensure_ascii=False,indent=2)+'\n');print(json.dumps(row(str(p.relative_to(R))),indent=2));print('verified_locks',len(locks))
