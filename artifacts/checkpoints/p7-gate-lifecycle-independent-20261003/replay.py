#!/usr/bin/env python3
"""Finite unchanged-PR58 replay and source-bound new stdio supplement."""
import hashlib,json,os,pathlib,re,subprocess,time
HERE=pathlib.Path(__file__).resolve().parent;ROOT=HERE.parents[2]
BASE='83a6b54ab1e71db033264e3b4e8d4f0a1d5319ad'
SOURCE='ed6663e9af270552d3217e64363416f8f5daa1a5'
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
write=lambda p,r:p.write_text(json.dumps(r,indent=2,sort_keys=True)+'\n')
env=dict(os.environ,CARGO_HOME='/workspace/.cargo',RUSTUP_HOME='/workspace/.rustup',PATH='/workspace/.cargo/bin:'+os.environ['PATH'],CARGO_BUILD_JOBS='5',CARGO_INCREMENTAL='0')
tracked=subprocess.check_output(['git','ls-tree','-r','--name-only',BASE],cwd=ROOT,text=True).splitlines()
proof={}
for path in tracked:
 if ('/src/' in path and path.startswith('crates/')) or path.endswith('Cargo.toml') or path=='Cargo.lock' or path.startswith('crates/cc-server/tests/') or path in ['docs/roadmap/code-index-v2/P7-REMAINING-GATES.md','docs/roadmap/code-index-v2/P7-REMAINING-GATES.json','docs/roadmap/code-index-v2/06-VALIDATION.md']:
  p=ROOT/path;assert p.read_bytes()==subprocess.check_output(['git','show',BASE+':'+path],cwd=ROOT),path;proof[path]=sha(p)
for name in ['audit_scope.py','audit_frozen.py','lifecycle_stdio.py','probe_public_source.py']:
 assert (HERE/name).read_bytes()==subprocess.check_output(['git','show',SOURCE+':'+str((HERE/name).relative_to(ROOT))],cwd=ROOT)
write(HERE/'fixed-source-proof.json',{'baseline_sha':BASE,'review_source_sha':SOURCE,'tracked_sha256':proof,'production_and_main_oracle_ledger_edits':False})
runs=[]
def run(cmd,path,extra=None):
 path.mkdir(parents=True)
 start=time.monotonic()
 with (path/'run.log').open('wb') as log:
  rc=subprocess.run(cmd,cwd=ROOT,env=dict(env,**(extra or {})),stdout=log,stderr=subprocess.STDOUT).returncode
 return rc,time.monotonic()-start

for feature in ['semantic','semantic-http']:
 for rep in range(1,6):
  path=HERE/'matrix'/feature/f'{rep:02}'
  cmd=['cargo','test','-p','cc-server','--test','p7_v05_all_lane_scope','--features',feature,'--locked','--offline','--','--nocapture']
  rc,elapsed=run(cmd,path,{'P7_ALL_LANE_SCOPE_OUTPUT':str(path/'raw')})
  raw=path/'raw/all-lane-scope.json'
  log=(path/'run.log').read_text();m=re.search(r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;',log)
  audit=subprocess.run(['python3',str(HERE/'audit_scope.py'),str(raw)],cwd=ROOT,capture_output=True,text=True) if raw.exists() else None
  if audit:(path/'audit.json').write_text(audit.stdout);(path/'audit.log').write_text(audit.stderr)
  receipt={'baseline_sha':BASE,'test_source_sha':SOURCE,'suite':'unchanged_PR58_V05_L2','profile':feature,'command':cmd,'exit_code':rc,'elapsed_secs':elapsed,'test_result':list(map(int,m.groups())) if m else None,'raw_audit_exit':audit.returncode if audit else None,'product_binary_sha256':sha(ROOT/'target/debug/codecortex'),'oracle_file_sha256':sha(ROOT/'crates/cc-server/tests/p7_v05_all_lane_scope.rs'),'artifact_sha256':{str(p.relative_to(path)):sha(p) for p in path.rglob('*') if p.is_file()}}
  write(path/'receipt.json',receipt);runs.append(receipt)
  assert rc==0 and m and list(map(int,m.groups()))==[1,0,0] and audit.returncode==0,'failure retained'
 print(feature,'five unchanged scope runs passed',flush=True)

binary=ROOT/'target/debug/codecortex'
for rep in range(1,4):
 path=HERE/'matrix'/'lifecycle'/f'{rep:02}'
 cmd=['python3',str(HERE/'lifecycle_stdio.py'),str(binary),str(path/'raw')]
 rc,elapsed=run(cmd,path)
 summary=json.loads((path/'raw/summary.json').read_text())
 receipt={'baseline_sha':BASE,'test_source_sha':SOURCE,'suite':'independent_real_stdio_lifecycle_L3','command':cmd,'exit_code':rc,'elapsed_secs':elapsed,'product_binary_sha256':sha(binary),'summary':summary,'artifact_sha256':{str(p.relative_to(path)):sha(p) for p in path.rglob('*') if p.is_file()}}
 write(path/'receipt.json',receipt);runs.append(receipt)
 assert rc==0 and summary['passed'] is True and len(summary['cases'])==13 and summary['costs']['document_posts']==summary['costs']['query_posts']==4,'failure retained'
 print('lifecycle',rep,'13 checkpoints passed',flush=True)
 path=HERE/'matrix'/'public-source'/f'{rep:02}'
 cmd=['python3',str(HERE/'probe_public_source.py'),str(binary),str(path/'raw')]
 rc,elapsed=run(cmd,path)
 summary=json.loads((path/'raw/coordinate-result.json').read_text())
 receipt={'baseline_sha':BASE,'test_source_sha':SOURCE,'suite':'public_L3_kind_name_source','command':cmd,'exit_code':rc,'elapsed_secs':elapsed,'summary':summary,'artifact_sha256':{str(p.relative_to(path)):sha(p) for p in path.rglob('*') if p.is_file()}}
 write(path/'receipt.json',receipt);runs.append(receipt)
 assert rc==0 and len(summary['rows'])==2 and summary['line_coordinates_closed'],'failure retained'
 print('public source',rep,'two tool responses passed',flush=True)
write(HERE/'matrix/summary.json',{'baseline_sha':BASE,'review_source_sha':SOURCE,'command_runs':len(runs),'unchanged_rust_test_passes':10,'lifecycle_runs':3,'lifecycle_checkpoints':39,'public_source_tool_responses':6,'failed':0,'ignored':0,'distinct_quality_questions':0,'runs':runs,'D1_D2':'unchanged','SIGKILL_P7_016':'not_run'})
