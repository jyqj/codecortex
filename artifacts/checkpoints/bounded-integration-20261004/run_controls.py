"""Replay unchanged frozen reviewer controls using exact Cargo-reported artifacts."""
import hashlib,json,subprocess
from pathlib import Path
D=Path(__file__).resolve().parent
repo=Path.cwd()
artifacts={}
for line in (D/'receipts/build.log').read_text().splitlines():
 try: m=json.loads(line)
 except ValueError: continue
 if m.get('reason')=='compiler-artifact' and m['target']['kind']==['lib'] and m['profile']['debuginfo']==2 and not m['profile']['test']:
  rlibs=[p for p in m['filenames'] if p.endswith('.rlib')]
  if rlibs: artifacts[m['target']['name']]=rlibs[0]
print({k:artifacts[k] for k in ['serde_json','cc_model','cc_server','cc_eval']},flush=True)
driver=repo/'artifacts/controls/query-target-independent-20261004/driver.rs'
controls=repo/'artifacts/controls/query-target-independent-20261004/delta-v2'
binary='/tmp/bounded-integration-driver'
argv=['rustc','+1.95.0','--edition=2021',str(driver),'-L','dependency=/workspace/codecortex/target/debug/deps']
for name in ['serde_json','cc_model','cc_server','cc_eval']:argv+=['--extern',name+'='+artifacts[name]]
argv+=['-o',binary]
with (D/'receipts/control-compile.log').open('w') as log:subprocess.run(argv,stdout=log,stderr=subprocess.STDOUT,check=True)
run=[binary,str(controls),'/tmp/bounded-integration-fixture','/tmp/bounded-integration-results']
with (D/'receipts/control-run.log').open('w') as log:subprocess.run(run,stdout=log,stderr=subprocess.STDOUT,check=True)
(D/'results.json').write_bytes(Path('/tmp/bounded-integration-results/results.json').read_bytes())
# Check fresh observations against original review's exact assertions, preserving all old evidence.
checker=(controls/'check.py').read_text()
checker=checker.replace("P = D.parent", "P = Path('artifacts/controls/query-target-independent-20261004').resolve()\nF = P / 'delta-v2'")
checker=checker.replace("(D / 'frozen.sha256')","(F / 'frozen.sha256')").replace("(D / name)","(F / name)")
checker=checker.replace("fixed_source='54b2b92cfca035ef1de2b4d6478f314231fd5511'", "fixed_source='e4a8df4cbc6dfae29af8cb0eac9ee83fbba4d696'")
(D/'check_fresh_controls.py').write_text(checker)
with (D/'receipts/control-check.log').open('w') as log:subprocess.run(['python3',str(D/'check_fresh_controls.py')],stdout=log,stderr=subprocess.STDOUT,check=True)
h=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
binding=dict(product='e4a8df4cbc6dfae29af8cb0eac9ee83fbba4d696',compile_argv=argv,run_argv=run,binary_sha256=h(binary),driver_sha256=h(driver),lock_sha256=h(repo/'Cargo.lock'),matrix_sha256=h(controls/'matrix.json'),fixtures={p.name:h(p) for p in (controls/'fixtures').iterdir()},results_sha256=h(D/'results.json'),check_source='copied exact delta-v2 checker; changed only input/output directory and product SHA',mcp='in-process MCP wire; not subprocess stdio')
(D/'binding.json').write_text(json.dumps(binding,indent=2)+'\n')
print((D/'receipts/control-run.log').read_text())
