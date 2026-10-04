#!/usr/bin/env python3
"""Fixed TypeScript public DEV paired runner. No product or scoring changes."""
import argparse, hashlib, json, os, shutil, subprocess, sys
from pathlib import Path
from datetime import datetime, timezone
sha=lambda b:hashlib.sha256(b).hexdigest()
canon=lambda x:json.dumps(x,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()
now=lambda:datetime.now(timezone.utc).isoformat()
ROOT=Path('/workspace/ts-candidate')
OUT=ROOT/'artifacts/checkpoints/public-dev-paired-typescript-20261004'
REF=Path('/tmp/ts-pair-reference')
OLD=REF/'artifacts/checkpoints/public-dev-current-group-js-20261003/plan.json'
PROTO=REF/'crates/cc-eval/benchmarks/public-v19/protocol'
ADMISSION=PROTO/'global-dev-review/typescript-extension/admission.json'
RUNTIME=Path('/workspace/ts-pair-runtime')
SHAS={'baseline':'88f2cf099c8b81f3acef485fd5ac9b01c63ce790','candidate':'37dd042eaa1209a86e0cafdcd92ae77e036e76f5'}
TOOL=Path('/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin')
def dump(p,x):p.parent.mkdir(parents=True,exist_ok=True);p.write_text(json.dumps(x,indent=2,ensure_ascii=False)+'\n')
def blob(entry):return subprocess.check_output(['git','show',entry],cwd=ROOT)
def inventory(root):return {p.relative_to(root).as_posix():sha(p.read_bytes()) for p in sorted(root.rglob('*')) if p.is_file()}
def env(arm):
 e=os.environ.copy();e.update(CARGO_HOME=str(RUNTIME/arm/'cargo'),RUSTUP_HOME='/workspace/.rustup',RUSTC=str(TOOL/'rustc'),CARGO_TARGET_DIR=str(RUNTIME/arm/'target'),CARGO_INCREMENTAL='0',CARGO_PROFILE_DEV_DEBUG='0',CARGO_BUILD_JOBS='2');e['PATH']=str(TOOL)+':'+e['PATH'];return e
def product_inventory(tree):
 names=subprocess.check_output(['git','ls-files','crates/*/src/*','crates/*/Cargo.toml','Cargo.toml','Cargo.lock'],cwd=tree,text=True).splitlines()
 return {n:sha((tree/n).read_bytes()) for n in names}
def build(arm):
 tree=Path('/workspace/ts-'+arm);out=OUT/('build-'+arm);out.mkdir(exist_ok=False)
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=tree,text=True).strip()==SHAS[arm]
 assert not subprocess.check_output(['git','diff','HEAD','--','crates','Cargo.toml','Cargo.lock'],cwd=tree)
 before=product_inventory(tree);e=env(arm)
 fetch=[str(TOOL/'cargo'),'fetch','--locked']
 p=subprocess.run(fetch,cwd=tree,env=e,capture_output=True)
 (out/'fetch.stdout').write_bytes(p.stdout);(out/'fetch.stderr').write_bytes(p.stderr)
 dump(out/'fetch-receipt.json',{'command':fetch,'exit_code':p.returncode,'utc':now(),'official_default_registry':True})
 if p.returncode:raise RuntimeError('dependency fetch failed; stop arm')
 command=[str(TOOL/'cargo'),'build','--locked','--offline','-p','cc-eval','--bin','cc-eval','-p','cc-server','--bin','codecortex','--message-format=json']
 start=now();p=subprocess.run(command,cwd=tree,env=e,capture_output=True)
 (out/'cargo-build.jsonl').write_bytes(p.stdout);(out/'cargo-build.stderr').write_bytes(p.stderr)
 artifacts={};bins=RUNTIME/arm/'binaries';bins.mkdir(exist_ok=False)
 if not p.returncode:
  rows=[json.loads(l) for l in p.stdout.splitlines() if l.startswith(b'{')]
  for name in ['cc-eval','codecortex']:
   matches=[r for r in rows if r.get('reason')=='compiler-artifact' and r.get('target',{}).get('name')==name and r.get('executable')]
   assert len(matches)==1
   a=matches[0];assert not {'semantic','eval-http'}&set(a['features'])
   dest=bins/name;shutil.copy2(a['executable'],dest)
   artifacts[name]={'compiler_artifact':a,'copied_binary':str(dest),'binary_sha256':sha(dest.read_bytes()),'features':a['features']}
 assert before==product_inventory(tree)
 receipt={'source_sha':SHAS[arm],'worktree':str(tree),'build_exit_code':p.returncode,'started_utc':start,'finished_utc':now(),'command':command,'environment':{k:e[k] for k in ['CARGO_HOME','RUSTUP_HOME','CARGO_TARGET_DIR','CARGO_INCREMENTAL','CARGO_PROFILE_DEV_DEBUG','CARGO_BUILD_JOBS']},'rustc':subprocess.check_output([str(TOOL/'rustc'),'--version'],text=True).strip(),'cargo':subprocess.check_output([str(TOOL/'cargo'),'--version'],text=True).strip(),'product_file_sha256':before,'product_inventory_sha256':sha(canon(before)),'artifacts':artifacts}
 dump(out/'build-receipt.json',receipt);print(json.dumps({'arm':arm,'build_exit_code':p.returncode,'binaries':{n:r['binary_sha256'] for n,r in artifacts.items()}}),flush=True)
 if p.returncode:raise RuntimeError('build failed; stop arm')
def verify_build(arm):
 p=OUT/('build-'+arm)/'build-receipt.json';r=json.loads(p.read_bytes());assert r['source_sha']==SHAS[arm] and r['build_exit_code']==0
 for a in r['artifacts'].values():assert sha(Path(a['copied_binary']).read_bytes())==a['binary_sha256']
 assert product_inventory(Path(r['worktree']))==r['product_file_sha256'];return r
def prepare():
 assert sha(ADMISSION.read_bytes())=='b4388ca60612f6734719b348bf30e05dd0752b7b754f47cc078b590e35c1e425'
 a=json.loads(ADMISSION.read_bytes());old=json.loads(OLD.read_bytes());assert not a['errors']
 entries=[e for e in old['suite_entries'] if e['repo']=='typescript']
 assert [e['suite_entry'] for e in entries]==a['repo_results']['typescript']['dev_suite_entries']
 assert len(entries)==10 and sum(e['scheduled_rows'] for e in entries)==396
 input_manifests={};builds={}
 for arm in SHAS:
  builds[arm]=verify_build(arm);root=RUNTIME/arm/'inputs02';root.mkdir(exist_ok=False)
  for e in entries:
   suite_raw=blob(e['suite_entry']);qraw=blob(e['query_entry']);assert sha(suite_raw)==e['suite_sha256']==a['inputs_sha256'][e['suite_entry']];assert sha(qraw)==e['query_sha256']==a['inputs_sha256'][e['query_entry']]
   rel=e['suite_entry'].split('/typescript/',1)[1];dest=root/'typescript'/rel;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(suite_raw)
   suite=json.loads(suite_raw);(dest.parent/suite['queries']).write_bytes(qraw)
   assert all(q['split']=='dev' for q in map(json.loads,qraw.splitlines()))
   source=(dest.parent/suite['source']['root']).resolve();assert source.is_relative_to(root)
   author=e['suite_entry'].split(':')[0]
   for path in suite['source']['files']:
    entry=author+':crates/cc-eval/benchmarks/public-v19/typescript/source/'+path;raw=blob(entry);assert sha(raw)==a['inputs_sha256'][entry]
    target=source/path;assert target.resolve().is_relative_to(root);target.parent.mkdir(parents=True,exist_ok=True);target.write_bytes(raw)
  input_manifests[arm]=inventory(root)
 assert input_manifests['baseline']==input_manifests['candidate']
 for path in a['repo_results']['typescript']['license_files_verified']:
  entry=a['repo_results']['typescript']['author_sha']+':crates/cc-eval/benchmarks/public-v19/typescript/'+path['path'];raw=blob(entry);assert sha(raw)==path['sha256'];dest=OUT/'retained-licenses'/path['path'];dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(raw)
 # Source scorer/normalizer/adapter identity from each actual checkout, never overwrite product.
 identities={arm:{n:sha((Path('/workspace/ts-'+arm)/n).read_bytes()) for n in ['crates/cc-eval/src/benchmark/metrics.rs','crates/cc-eval/src/benchmark/span_metrics.rs','crates/cc-eval/src/benchmark/manifest.rs','crates/cc-eval/src/benchmark/schema.rs','crates/cc-eval/src/benchmark/validation.rs','crates/cc-eval/src/benchmark/normalizer.rs','crates/cc-eval/src/benchmark/adapters/mcp_stdio.rs','crates/cc-eval/src/benchmark/runner.rs','crates/cc-parsers/src/lib.rs','crates/cc-parsers/src/jsts/mod.rs'] if (Path('/workspace/ts-'+arm)/n).exists()} for arm in SHAS}
 plan={'created_utc':now(),'scope':'typescript_only_frozen_public_dev_paired_quality','sources':SHAS,'product_candidate':'90858afae647a513537bf118932a7ba5020ee98b','admission_commit':'5385f5a7a2a875c6d5cbd049bdde039bf71bbf32','admission_sha256':sha(ADMISSION.read_bytes()),'original_schedule_commit':'535ff1b13b841af8021346a660c83c57919525e2','original_schedule_plan_sha256':sha(OLD.read_bytes()),'protocol_sha256':{p.name:sha(p.read_bytes()) for p in [PROTO/'PREREGISTRATION.md',PROTO/'preregistration.json',PROTO/'evaluator-query.schema.json']},'suite_entries':entries,'same_input_inventory':input_manifests['baseline'],'stage_source_identity':identities,'build_receipts_sha256':{arm:sha((OUT/('build-'+arm)/'build-receipt.json').read_bytes()) for arm in SHAS},'counts':{'native':73,'compat':59},'scheduled_per_arm':{'native':219,'compat':177},'fixed_order':'original TS suite order; baseline then candidate for each suite; repetitions/order/seed exact original; no extra pilot or best-of','analysis':'same-profile per-query candidate minus baseline; mean repetitions then global-component family means; all unchanged scorer metrics; 10000 paired component bootstrap seed20261003 nearest-rank; no six-repo inference','prior_public_dev_results_exposed':True,'tuning':False,'formal600':0,'clean_holdout':0,'public_dev_pygo_taxonomy_applied':False,'provider_calls':0,'performance_causal_claim':False,'CI_claim':'not_confirmed','old1671':'preserved Partial/qualityFAIL'}
 dump(OUT/'plan.json',plan);print(json.dumps({'prepared':True,'same_inputs':True,'scheduled_per_arm':plan['scheduled_per_arm'],'plan_sha256':sha((OUT/'plan.json').read_bytes())}))
def execute():
 plan=json.loads((OUT/'plan.json').read_bytes());builds={a:verify_build(a) for a in SHAS};runs=OUT/'runs';runs.mkdir(exist_ok=False);commands=[]
 for entry in plan['suite_entries']:
  for arm in SHAS:
   assert inventory(RUNTIME/arm/'inputs02')==plan['same_input_inventory']
   b=builds[arm];suite=RUNTIME/arm/'inputs02/typescript'/entry['suite_entry'].split('/typescript/',1)[1];dest=runs/arm/entry['key'];dest.parent.mkdir(exist_ok=True)
   command=[b['artifacts']['cc-eval']['copied_binary'],'run','--backend','mcp-stdio','--binary',b['artifacts']['codecortex']['copied_binary'],'--suite',str(suite),'--output',str(dest),'--profile','quality']
   e=env(arm);e['CODECORTEX_BENCH_PROCESS_PROBE']='0';start=now();p=subprocess.run(command,env=e,capture_output=True)
   log=dest.parent/entry['key'];Path(str(log)+'.run.stdout').write_bytes(p.stdout);Path(str(log)+'.run.stderr').write_bytes(p.stderr)
   before=inventory(dest) if dest.exists() else {};manifest=json.loads((dest/'manifest.json').read_bytes()) if (dest/'manifest.json').exists() else {};ready=json.loads((dest/'readiness.json').read_bytes()) if (dest/'readiness.json').exists() else None
   replay_command=[b['artifacts']['cc-eval']['copied_binary'],'replay','--run',str(dest)];r=subprocess.run(replay_command,env=e,capture_output=True)
   Path(str(log)+'.replay.stdout').write_bytes(r.stdout);Path(str(log)+'.replay.stderr').write_bytes(r.stderr);after=inventory(dest) if dest.exists() else {}
   rec={'arm':arm,'key':entry['key'],'command':command,'run_exit_code':p.returncode,'started_utc':start,'finished_utc':now(),'replay_command':replay_command,'replay_exit_code':r.returncode,'before_replay_sha256':before,'after_replay_sha256':after,'changed_on_replay':sorted(n for n in before.keys()|after.keys() if before.get(n)!=after.get(n)),'infrastructure_failure':manifest.get('infrastructure_failure'),'readiness':ready}
   commands.append(rec);dump(runs/'commands.json',commands);print(json.dumps({k:rec[k] for k in ['arm','key','run_exit_code','replay_exit_code','changed_on_replay','infrastructure_failure']}),flush=True)
   # Never continue to a next suite after integrity/preparation failure, never impute missing measurements.
   if p.returncode not in [0,1] or manifest.get('infrastructure_failure') or not ready or rec['changed_on_replay'] or p.returncode!=r.returncode:raise RuntimeError('run infrastructure/integrity blocker retained; stop dependent execution')
 for arm in SHAS:verify_build(arm);assert inventory(RUNTIME/arm/'inputs02')==plan['same_input_inventory']
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('action',choices=['build','prepare','execute']);p.add_argument('--arm',choices=list(SHAS));a=p.parse_args()
 if a.action=='build':build(a.arm)
 elif a.action=='prepare':prepare()
 else:execute()
