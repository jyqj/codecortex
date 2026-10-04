"""Freeze exact Express inputs from original admission; never author gold."""
import hashlib,json,subprocess,platform,os
from pathlib import Path
D=Path(__file__).resolve().parent.parent
R=Path('/workspace/express-reference/artifacts/checkpoints/public-dev-current-group-js-20261003')
P=Path('/workspace/express-protocol/crates/cc-eval/benchmarks/public-v19/protocol')
sha=lambda b:hashlib.sha256(b).hexdigest()
a=json.loads((P/'global-dev-review/typescript-extension/admission.json').read_bytes())
assert sha((P/'global-dev-review/typescript-extension/admission.json').read_bytes())=='b4388ca60612f6734719b348bf30e05dd0752b7b754f47cc078b590e35c1e425'
s=json.loads((R/'plan.json').read_bytes()); es=[e for e in s['suite_entries'] if e['repo']=='express']
assert len(es)==2 and sum(e['scheduled_rows'] for e in es)==387
locks=a['inputs_sha256']; proof={}
for entry,expected in locks.items():
 if '/express/' not in entry: continue
 raw=subprocess.check_output(['git','show',entry]);assert sha(raw)==expected
 proof[entry]=expected
 if entry.endswith('source-manifest.json') or entry.endswith('license/LICENSE'):
  dest=D/('retained-licenses/express/LICENSE' if entry.endswith('license/LICENSE') else 'source-manifest.json');dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(raw)
for arm in ['baseline','candidate']:
 root=Path('/workspace/express-runtime')/arm/'inputs';root.mkdir(parents=True,exist_ok=False)
 for n,expected in s['input_file_sha256'].items():
  if not n.startswith('express/'):continue
  entry='465e9e0bd435e2e30c08de8702f78a0d10c49c8e:crates/cc-eval/benchmarks/public-v19/'+n
  raw=subprocess.check_output(['git','show',entry]);assert sha(raw)==expected==locks[entry]
  dest=root/n;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(raw)
manifest=json.loads((D/'source-manifest.json').read_bytes())
for f in manifest['files']:
 raw=(Path('/workspace/express-runtime/baseline/inputs/express/source')/f['path']).read_bytes()
 assert sha(raw)==f['sha256'] and len(raw)==f['bytes']
 assert hashlib.sha1(b'blob '+str(len(raw)).encode()+b'\0'+raw).hexdigest()==f['git_blob']
plan={'scope':'Express only fixed same-input public DEV paired quality; prior aggregate exposure; not holdout or performance causality',
 'baseline':'88f2cf099c8b81f3acef485fd5ac9b01c63ce790','candidate':'37dd042eaa1209a86e0cafdcd92ae77e036e76f5','candidate_product':'90858afae647a513537bf118932a7ba5020ee98b',
 'original_js_execution_commit':'535ff1b13b841af8021346a660c83c57919525e2','original_js_plan_sha256':sha((R/'plan.json').read_bytes()),
 'admission_commit':'5385f5a7a2a875c6d5cbd049bdde039bf71bbf32','admission_sha256':sha((P/'global-dev-review/typescript-extension/admission.json').read_bytes()),
 'input_file_sha256':{n:h for n,h in s['input_file_sha256'].items() if n.startswith('express/')},'source_and_admission_proof':proof,'suite_entries':es,
 'run_order':['baseline native','baseline compat','candidate native','candidate compat'],'repetitions_are_not_independent_samples':True,
 'analysis':s['analysis_definitions'],'paired_intervals':'10000 random.Random(20261003) fixed global family cluster draws; nearest rank 95%; Express only conditional descriptive; native/compat separate',
 'prior_results':'old1671 allPartial qualityFAIL preserved; JS783 allPartial qualityFAIL; prior exposure disclosed',
 'taxonomy':'original default admission; PYGO optin not applicable','formal_600_accepted':0,'clean_holdout':0,'scheduled_per_arm':387,
 'protocol_file_sha256':s['protocol_file_sha256'],'registry_sha256':s['global_correlation_registry_sha256'],
 'environment':{'os':platform.platform(),'cpu_count':os.cpu_count(),'machine':platform.machine()},
 'prohibited_changes':'product/budget/gold/scorer/normalizer/provider/default knobs unchanged; no post-score tuning',
 'required_preconditions':'actual locked successful build; cc-eval validate; prepare/index and Ready before every suite queries',
 'unsupported':['required facet coverage','graph correctness','Recall20','SymbolAccuracy','DuplicationRate','full freshness','semantic ablation','release performance']}
(D/'plan.json').write_text(json.dumps(plan,indent=2)+'\n')
(D/'instructions-receipt.json').write_text(json.dumps({'AGENTS.md':'absent in workspace and checkouts','.agents/skills':'absent','read':['benchmark README','original PR95/group-js runner','frozen protocol PREREGISTRATION','PYGO registration README (excluded for Express)']},indent=2)+'\n')
print({'source_proof_entries':len(proof),'scheduled_per_arm':387,'sameinput_files':len(plan['input_file_sha256'])})
