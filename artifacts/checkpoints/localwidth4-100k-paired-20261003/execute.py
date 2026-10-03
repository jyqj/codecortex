import pathlib,json,subprocess,shutil,socket,os,time
O=pathlib.Path(__file__).resolve().parent
write=lambda p,x:p.write_text(json.dumps(x,indent=2)+"\n")
receipts={n:json.loads((O/n/'build-receipt.json').read_text()) for n in ['baseline','candidate']}
assert all(r['build_exit_code']==0 and r['guard_stop'] is None for r in receipts.values())
scan=subprocess.check_output(['ps','-eo','args'],text=True)
assert not any(line.lstrip().startswith(('/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/rustc ','/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/cargo build')) for line in scan.splitlines())
write(O/'pair-builds-ready.json',{'time_ns':time.monotonic_ns(),'builds':receipts,'no_compiler_process_observed':True,'free_bytes':shutil.disk_usage(O).free})
for name in ['baseline','candidate']:
 out=O/'preflight'/name;shutil.copyfile(O/name/'build-receipt.json',out/'build-receipt.json')
 with (out/'run.log').open('w') as log:result=subprocess.run(['python3',str(out/'run.py')],stdout=log,stderr=subprocess.STDOUT)
 s=json.loads((out/'summary.json').read_text());write(O/('preflight-'+name+'-result.json'),{'exit_code':result.returncode,'summary':s})
 if result.returncode!=0:
  write(O/'stage-status.json',{'stopped':'correctness_precondition_failed','variant':name,'100k_runs':0,'failures':s['failures']});raise SystemExit(2)
 assert s['files']==32 and not s['not_run'] and not s['failures']
 assert json.loads((out/'sharedgate-preflight.json').read_text())['actual_http_held']==2
with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
env={**os.environ,'P7_RESOURCE_PORT':str(port)}
write(O/'pair-execution.json',{'fixed_port':port,'order':['baseline','candidate'],'formal_samples_per_variant':1,'started_ns':time.monotonic_ns(),'candidate_smoke_complete_before_100k':True})
for name in ['baseline','candidate']:
 with (O/name/'run.log').open('w') as log:result=subprocess.run(['python3',str(O/name/'run.py')],env=env,stdout=log,stderr=subprocess.STDOUT)
 s=json.loads((O/name/'summary.json').read_text())
 print(name,result.returncode,s['status'],s['failures'],flush=True)
 if result.returncode!=0 and not all(f.get('type')=='TimeoutError' and '300s' in f.get('error','') for f in s['failures']):
  write(O/'stage-status.json',{'stopped':'correctness_or_non_readiness_failure','variant':name,'failures':s['failures']});raise SystemExit(3)
write(O/'stage-status.json',{'pair_completed':True,'no_further_product_runs':True,'auto_accept':False})
