from pathlib import Path
import hashlib,json,os,subprocess,shutil,time
R=Path.cwd();O=R/'artifacts/benchmarks/p5e-grouped-lexical/development';E=os.environ.copy();E['CODECORTEX_BENCH_PROCESS_PROBE']='0'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
paths=[p for d in ['crates','scripts','.github'] for p in (R/d).rglob('*') if p.is_file() and not any(n in p.parts for n in ['target','__pycache__','.codecortex'])]+[R/n for n in ['Cargo.toml','Cargo.lock','README.md']]
rows=[{'path':str(p.relative_to(R)),'sha256':sha(p)} for p in sorted(paths)];digest=hashlib.sha256(json.dumps(rows,sort_keys=True,separators=(',',':')).encode()).hexdigest();assert not (O/'receipt.json').exists()
binary=O/'codecortex';shutil.copy2(R/'target/p0-dev/debug/codecortex',binary);runner=R/'artifacts/benchmarks/p5d-20260930-resume/final-v3/binaries/stable/cc-eval'
DATA={'status':'running_development_not_final_gate','source_digest_sha256':digest,'source_files':rows,'binary_sha256':sha(binary),'runner_sha256':sha(runner),'commands':[],'limits':['development only; not whole P5-019/G5 or performance acceptance','original gold/scorer unchanged; explicit probe0/null','future source changes require final re-freeze']}
(O/'receipt.json').write_text(json.dumps(DATA,indent=2)+'\n')
man=R/'crates/cc-eval/benchmarks/manifests';frozen=R/'artifacts/benchmarks/p0-g0-20260927/frozen-inputs';suites=[('source',frozen/'p0-codecortex-subset/suite.json'),('smoke',frozen/'p0-smoke/suite.json'),('exact',man/'p1b-exact.json'),('intents',man/'p1c-intents.json')]
for name,suite in suites:
 out=O/name;started=time.monotonic();args=[str(runner),'run','--backend','mcp-stdio','--binary',str(binary),'--suite',str(suite),'--output',str(out),'--profile','smoke']
 with (O/(name+'.log')).open('w') as log:p=subprocess.run(args,env=E,stdout=log,stderr=subprocess.STDOUT,timeout=180)
 assert p.returncode in [0,1]
 replay_hashes={n:sha(out/n) for n in ['metrics.json','query-slices.json','costs.jsonl','normalized.jsonl']}
 with (O/(name+'-replay.log')).open('w') as log:q=subprocess.run([str(runner),'replay','--run',str(out)],env=E,stdout=log,stderr=subprocess.STDOUT,timeout=180)
 assert q.returncode==p.returncode and replay_hashes=={n:sha(out/n) for n in replay_hashes}
 DATA['commands'].append({'suite':name,'exit':p.returncode,'replay_exit':q.returncode,'seconds':time.monotonic()-started,'metric_sha256':replay_hashes});(O/'receipt.json').write_text(json.dumps(DATA,indent=2)+'\n');print(name,p.returncode,flush=True)
assert all(sha(R/r['path'])==r['sha256'] for r in rows);DATA.update(status='completed_development_not_final_gate',source_unchanged=True);(O/'receipt.json').write_text(json.dumps(DATA,indent=2)+'\n')
